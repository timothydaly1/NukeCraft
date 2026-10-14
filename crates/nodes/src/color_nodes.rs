//! Color: Grade, Saturation, Premult, Unpremult, and the machinery every colour node shares.
//!
//! These are per-pixel operations: the output bbox is the input's, and output pixel `(x, y)`
//! reads only input pixel `(x, y)` (and the mask's).
//!
//! Like Nuke's colour nodes, they process the channels in their `channels` knob, can divide by an
//! alpha first and multiply back after (`unpremult`), and blend the result with the input by
//! `mix` times an optional mask (input 1).

use nukecraft_color::luminance;
use nukecraft_geom::IRect;
use nukecraft_raster::{Channel, Image};

use crate::{
    Ctx, ImageInfo, Input, InputSpec, KnobDefault, KnobKind, KnobSpec, NodeSpec, Result, info_passthrough, input_image, mask_factors, mix, request_same,
    rgba_index,
};

/// Premult and Unpremult take no mask.
const INPUT: &[InputSpec] = &[InputSpec { name: "Source", mask: false }];

/// A colour node's inputs: the image and a mask (input 1, as in Nuke).
pub(crate) const COLOR_INPUTS: &[InputSpec] = &[InputSpec { name: "Source", mask: false }, InputSpec { name: "mask", mask: true }];
const MASK_INPUT: usize = 1;

pub(crate) const MIX: KnobSpec =
    KnobSpec { name: "mix", label: "mix", kind: KnobKind::Float, default: KnobDefault::Number(1.0), help: "Blend between the input (0) and the result (1)." };

pub(crate) const UNPREMULT: KnobSpec = KnobSpec {
    name: "unpremult",
    label: "(un)premult by",
    kind: KnobKind::Channels,
    default: KnobDefault::Text("none"),
    help: "Divide by this channel before the operation and multiply by it after.",
};

pub(crate) const MASK_CHANNEL: KnobSpec = KnobSpec {
    name: "maskChannelInput",
    label: "mask",
    kind: KnobKind::Channels,
    default: KnobDefault::Text("rgba.alpha"),
    help: "Channel of the mask input that limits the effect (none: no mask).",
};

pub(crate) const INVERT_MASK: KnobSpec =
    KnobSpec { name: "invert_mask", label: "invert mask", kind: KnobKind::Bool, default: KnobDefault::Bool(false), help: "Use one minus the mask." };

pub(crate) const fn channels(default: &'static str) -> KnobSpec {
    KnobSpec {
        name: "channels",
        label: "channels",
        kind: KnobKind::Channels,
        default: KnobDefault::Text(default),
        help: "Channels to process; others pass through.",
    }
}

pub(crate) const fn color(name: &'static str, label: &'static str, default: f64, help: &'static str) -> KnobSpec {
    KnobSpec { name, label, kind: KnobKind::Color, default: KnobDefault::Number(default), help }
}

const GRADE_KNOBS: &[KnobSpec] = &[
    channels("rgb"),
    color("blackpoint", "blackpoint", 0.0, "Input value that maps to `black` (lift)."),
    color("whitepoint", "whitepoint", 1.0, "Input value that maps to `white` (gain)."),
    color("black", "lift", 0.0, "Output value for `blackpoint`."),
    color("white", "gain", 1.0, "Output value for `whitepoint`."),
    color("multiply", "multiply", 1.0, "Multiplies the result."),
    color("add", "offset", 0.0, "Added to the result."),
    color("gamma", "gamma", 1.0, "Gamma applied last, to positive values."),
    KnobSpec { name: "reverse", label: "reverse", kind: KnobKind::Bool, default: KnobDefault::Bool(false), help: "Apply the inverse grade." },
    KnobSpec { name: "black_clamp", label: "black clamp", kind: KnobKind::Bool, default: KnobDefault::Bool(true), help: "Clamp results below 0 to 0." },
    KnobSpec { name: "white_clamp", label: "white clamp", kind: KnobKind::Bool, default: KnobDefault::Bool(false), help: "Clamp results above 1 to 1." },
    UNPREMULT,
    MASK_CHANNEL,
    INVERT_MASK,
    MIX,
];

/// Saturation's luminance formulas.
const LUMA_MODES: &[&str] = &["Rec 709", "Ccir 601", "Average", "Maximum"];

const SATURATION_KNOBS: &[KnobSpec] = &[
    channels("rgb"),
    KnobSpec { name: "saturation", label: "saturation", kind: KnobKind::Float, default: KnobDefault::Number(1.0), help: "0 is greyscale, 1 unchanged." },
    KnobSpec {
        name: "mode",
        label: "luminance math",
        kind: KnobKind::Choice(LUMA_MODES),
        default: KnobDefault::Text("Rec 709"),
        help: "How the grey that saturation moves towards is computed.",
    },
    UNPREMULT,
    MASK_CHANNEL,
    INVERT_MASK,
    MIX,
];

const PREMULT_KNOBS: &[KnobSpec] = &[channels("rgb"), ALPHA];

pub fn specs() -> Vec<NodeSpec> {
    vec![
        NodeSpec {
            class: "Grade",
            aliases: &[],
            label: "Grade",
            category: "Color",
            shortcut: Some("G"),
            help: "Maps blackpoint..whitepoint to lift..gain, then multiply, offset and gamma.",
            inputs: COLOR_INPUTS,
            extra_inputs: false,
            knobs: GRADE_KNOBS,
            info: info_passthrough,
            request: request_same,
            engine: grade_engine,
            input_time: None,
        },
        NodeSpec {
            class: "Saturation",
            aliases: &[],
            label: "Saturation",
            category: "Color",
            shortcut: None,
            help: "Scales colour away from (or towards) its luminance.",
            inputs: COLOR_INPUTS,
            extra_inputs: false,
            knobs: SATURATION_KNOBS,
            info: info_passthrough,
            request: request_same,
            engine: saturation_engine,
            input_time: None,
        },
        NodeSpec {
            class: "Premult",
            aliases: &[],
            label: "Premult",
            category: "Merge",
            shortcut: None,
            help: "Multiplies the channels by the alpha channel.",
            inputs: INPUT,
            extra_inputs: false,
            knobs: PREMULT_KNOBS,
            info: info_passthrough,
            request: request_same,
            engine: premult_engine,
            input_time: None,
        },
        NodeSpec {
            class: "Unpremult",
            aliases: &[],
            label: "Unpremult",
            category: "Merge",
            shortcut: None,
            help: "Divides the channels by the alpha channel where it is not zero.",
            inputs: INPUT,
            extra_inputs: false,
            knobs: PREMULT_KNOBS,
            info: info_passthrough,
            request: request_same,
            engine: unpremult_engine,
            input_time: None,
        },
    ]
}

const ALPHA: KnobSpec =
    KnobSpec { name: "alpha", label: "by", kind: KnobKind::Channels, default: KnobDefault::Text("rgba.alpha"), help: "The channel to multiply or divide by." };

/// Input 0's pixels over `region` (a copy the node can modify), and the channels to process.
fn source(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<(Image, Vec<(usize, Channel)>)> {
    let img = match input_image(inputs, 0) {
        Some(inp) if inp.image.bbox() == region && inp.image.channels() == &info.channels => inp.image.clone(),
        Some(inp) => inp.image.reframed(region, &info.channels)?,
        None => Image::new(IRect::EMPTY, info.channels.clone())?,
    };
    let selected = ctx.channels_knob("channels", &info.channels);
    let chans = img.channels().iter().enumerate().filter(|(_, c)| selected.contains(c)).map(|(i, c)| (i, c.clone())).collect();
    Ok((img, chans))
}

/// How a colour node changes one pixel.
pub(crate) enum PixelOp<'a> {
    /// Each selected channel on its own: `(RGBA component of its knobs, value) -> value`.
    Channel(&'a dyn Fn(usize, f32) -> f32),
    /// Red, green, blue and alpha together (missing ones read 0): selected RGBA channels take
    /// the result, other selected channels pass through.
    Rgba(&'a dyn Fn([f32; 4]) -> [f32; 4]),
}

/// Runs a colour operation on input 0 over `region` the way Nuke's colour nodes do: on the
/// `channels` knob's channels, divided by the `unpremult` channel first (where it isn't 0) and
/// multiplied by it after, blended with the input by `mix` times the mask.
pub(crate) fn color_op(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect, op: PixelOp) -> Result<Image> {
    let (mut img, chans) = source(ctx, info, inputs, region)?;
    if chans.is_empty() || img.bbox().is_empty() {
        return Ok(img);
    }
    let bbox = img.bbox();
    let m = ctx.knobs.float("mix") as f32;
    let mask = mask_factors(ctx, inputs, MASK_INPUT, bbox);
    let by = ctx.channels_knob("unpremult", img.channels()).iter().next().and_then(|c| img.channels().index_of(c));
    let rgba = [Channel::red(), Channel::green(), Channel::blue(), Channel::alpha()].map(|c| img.channels().index_of(&c));
    let n = bbox.area() as usize;
    let at = |img: &Image, p: usize, i: usize| img.plane_at(p).and_then(|pl| pl.get(i)).copied().unwrap_or(0.0);
    let mut out = Vec::with_capacity(chans.len());
    for i in 0..n {
        let k = mask.as_ref().map_or(1.0, |mk| mk.get(i).copied().unwrap_or(0.0)) * m;
        if k <= 0.0 || k.is_nan() {
            continue;
        }
        // The divisor for every channel but the one it comes from.
        let div = by.map(|p| at(&img, p, i)).filter(|a| *a != 0.0);
        let scale = |p: usize| if by == Some(p) { None } else { div };
        out.clear();
        match op {
            PixelOp::Channel(f) => {
                for (p, c) in &chans {
                    let v = at(&img, *p, i);
                    let new = match scale(*p) {
                        Some(a) => f(rgba_index(c), v / a) * a,
                        None => f(rgba_index(c), v),
                    };
                    out.push((*p, mix(v, new, k)));
                }
            }
            PixelOp::Rgba(f) => {
                let px = rgba.map(|p| p.map_or(0.0, |p| at(&img, p, i) / scale(p).unwrap_or(1.0)));
                let res = f(px);
                for (p, _) in &chans {
                    let Some(j) = rgba.iter().position(|q| *q == Some(*p)) else { continue };
                    let new = res.get(j).copied().unwrap_or(0.0) * scale(*p).unwrap_or(1.0);
                    out.push((*p, mix(at(&img, *p, i), new, k)));
                }
            }
        }
        for (p, v) in &out {
            if let Some(px) = img.plane_at_mut(*p).and_then(|pl| pl.get_mut(i)) {
                *px = *v;
            }
        }
    }
    Ok(img)
}

/// Grade's coefficients for one RGBA component: `out = pow(a·in + b, 1/gamma)`.
#[derive(Clone, Copy)]
struct GradeCoeffs {
    a: f64,
    b: f64,
    gamma: f64,
}

fn grade_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let k = ctx.knobs;
    let get = |name: &str| k.rgba(name, 1.0);
    let (bp, wp, lift, gain, mul, add, gamma) = (get("blackpoint"), get("whitepoint"), get("black"), get("white"), get("multiply"), get("add"), get("gamma"));
    let (reverse, bclamp, wclamp) = (k.boolean("reverse"), k.boolean("black_clamp"), k.boolean("white_clamp"));
    let coeffs: [GradeCoeffs; 4] = std::array::from_fn(|ci| {
        let at = |v: [f64; 4]| v.get(ci).copied().unwrap_or(0.0);
        // A = multiply·(gain − lift)/(whitepoint − blackpoint), B = offset + lift − A·blackpoint.
        let range = at(wp) - at(bp);
        let a = if range.abs() < 1e-12 { 0.0 } else { at(mul) * (at(gain) - at(lift)) / range };
        GradeCoeffs { a, b: at(add) + at(lift) - a * at(bp), gamma: at(gamma) }
    });
    let f = |ci: usize, v: f32| {
        let Some(GradeCoeffs { a, b, gamma: g }) = coeffs.get(ci).copied() else { return v };
        let v = v as f64;
        let mut out = if reverse {
            let lin = if g > 0.0 && v > 0.0 { v.powf(g) } else { v };
            if a.abs() < 1e-12 { 0.0 } else { (lin - b) / a }
        } else {
            let lin = a * v + b;
            if g > 0.0 && lin > 0.0 && (g - 1.0).abs() > 1e-12 { lin.powf(1.0 / g) } else { lin }
        };
        if bclamp {
            out = out.max(0.0);
        }
        if wclamp {
            out = out.min(1.0);
        }
        if out.is_finite() { out as f32 } else { 0.0 }
    };
    color_op(ctx, info, inputs, region, PixelOp::Channel(&f))
}

/// The grey Saturation moves towards, by its `mode` knob.
fn luma(mode: &str, r: f32, g: f32, b: f32) -> f32 {
    match mode {
        "Ccir 601" => 0.299 * r + 0.587 * g + 0.114 * b,
        "Average" => (r + g + b) / 3.0,
        "Maximum" => r.max(g).max(b),
        _ => luminance(r, g, b),
    }
}

fn saturation_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let s = ctx.knobs.float("saturation") as f32;
    let mode = ctx.knobs.text("mode");
    let f = |[r, g, b, a]: [f32; 4]| {
        let l = luma(mode, r, g, b);
        [l + s * (r - l), l + s * (g - l), l + s * (b - l), a]
    };
    color_op(ctx, info, inputs, region, PixelOp::Rgba(&f))
}

fn by_alpha(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect, op: fn(f32, f32) -> f32) -> Result<Image> {
    let (mut img, chans) = source(ctx, info, inputs, region)?;
    let Some(alpha_index) = ctx.channels_knob("alpha", &info.channels).iter().next().and_then(|c| img.channels().index_of(c)) else { return Ok(img) };
    let Some(alpha) = img.plane_at(alpha_index).map(<[f32]>::to_vec) else { return Ok(img) };
    for (pi, _) in chans {
        if pi == alpha_index {
            continue;
        }
        let Some(plane) = img.plane_at_mut(pi) else { continue };
        for (px, a) in plane.iter_mut().zip(&alpha) {
            *px = op(*px, *a);
        }
    }
    Ok(img)
}

fn premult_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    by_alpha(ctx, info, inputs, region, |v, a| v * a)
}

fn unpremult_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    by_alpha(ctx, info, inputs, region, |v, a| if a == 0.0 { v } else { v / a })
}

#[cfg(test)]
mod tests {
    use crate::testutil::*;
    use nukecraft_geom::IRect;
    use nukecraft_graph::Value;

    fn src() -> Option<nukecraft_raster::Image> {
        Some(solid(IRect::new(0, 0, 4, 4), [0.5, 0.25, 0.1, 0.5]))
    }

    #[test]
    fn grade_formula() {
        let (_, img) = run("Grade", vec![("white", num(2.0)), ("add", num(0.1))], vec![src()], None);
        assert!((img.get_at(0, 0, 0) - 1.1).abs() < 1e-6);
        assert_eq!(img.get_at(3, 0, 0), 0.5, "alpha untouched (channels rgb)");
        let (_, img) = run("Grade", vec![("gamma", num(2.0))], vec![src()], None);
        assert!((img.get_at(1, 0, 0) - 0.5).abs() < 1e-6, "0.25^(1/2)");
        let (_, img) = run("Grade", vec![("add", num(-1.0))], vec![src()], None);
        assert_eq!(img.get_at(0, 0, 0), 0.0, "black clamp");
        let (_, img) = run("Grade", vec![("add", num(-1.0)), ("black_clamp", Value::Bool(false))], vec![src()], None);
        assert!((img.get_at(0, 0, 0) + 0.5).abs() < 1e-6);
    }

    #[test]
    fn grade_reverse_inverts() {
        let knobs = || vec![("white", num(1.7)), ("black", num(0.05)), ("gamma", num(1.4)), ("multiply", num(0.9))];
        let (_, fwd) = run("Grade", knobs(), vec![src()], None);
        let mut rev_knobs = knobs();
        rev_knobs.push(("reverse", Value::Bool(true)));
        let (_, back) = run("Grade", rev_knobs, vec![Some(fwd)], None);
        assert!((back.get_at(0, 1, 1) - 0.5).abs() < 1e-5);
        // A zero-width range or zero gain divides by nothing.
        let (_, img) = run("Grade", vec![("whitepoint", num(0.0)), ("reverse", Value::Bool(true))], vec![src()], None);
        assert!(img.is_finite());
    }

    #[test]
    fn saturation_zero_is_luminance() {
        let (_, img) = run("Saturation", vec![("saturation", num(0.0))], vec![src()], None);
        let l = nukecraft_color::luminance(0.5, 0.25, 0.1);
        for c in 0..3 {
            assert!((img.get_at(c, 2, 2) - l).abs() < 1e-6);
        }
    }

    #[test]
    fn premult_and_unpremult_round_trip() {
        let (_, p) = run("Premult", vec![], vec![src()], None);
        assert!((p.get_at(0, 0, 0) - 0.25).abs() < 1e-6);
        assert_eq!(p.get_at(3, 0, 0), 0.5);
        let (_, u) = run("Unpremult", vec![], vec![Some(p)], None);
        assert!((u.get_at(0, 0, 0) - 0.5).abs() < 1e-6);
        let zero = Some(solid(IRect::new(0, 0, 1, 1), [0.5, 0.5, 0.5, 0.0]));
        let (_, u) = run("Unpremult", vec![], vec![zero], None);
        assert_eq!(u.get_at(0, 0, 0), 0.5, "zero alpha leaves the value alone");
    }

    #[test]
    fn unconnected_input_is_black_not_an_error() {
        let (info, img) = run("Grade", vec![], vec![None], None);
        assert!(info.bbox.is_empty());
        assert_eq!(img.get_at(0, 10, 10), 0.0);
    }

    /// A 4x4 mask: alpha 1 in the left half, 0 in the right.
    fn half_mask() -> Option<nukecraft_raster::Image> {
        let mut m = solid(IRect::new(0, 0, 4, 4), [0.0; 4]);
        for y in 0..4 {
            for x in 0..2 {
                m.set_at(3, x, y, 1.0);
            }
        }
        Some(m)
    }

    #[test]
    fn the_mask_limits_the_effect() {
        let (_, img) = run("Grade", vec![("white", num(2.0))], vec![src(), half_mask()], None);
        assert_eq!(img.get_at(0, 0, 0), 1.0, "inside the mask");
        assert_eq!(img.get_at(0, 3, 0), 0.5, "outside the mask");
        let (_, img) = run("Grade", vec![("white", num(2.0)), ("invert_mask", Value::Bool(true))], vec![src(), half_mask()], None);
        assert_eq!((img.get_at(0, 0, 0), img.get_at(0, 3, 0)), (0.5, 1.0), "inverted");
        let (_, img) = run("Grade", vec![("white", num(2.0)), ("maskChannelInput", text("none"))], vec![src(), half_mask()], None);
        assert_eq!(img.get_at(0, 3, 0), 1.0, "mask channel none: no mask");
        let (_, img) = run("Grade", vec![("white", num(2.0)), ("maskChannelInput", text("depth.Z"))], vec![src(), half_mask()], None);
        assert_eq!(img.get_at(0, 0, 0), 0.5, "a channel the mask lacks is black");
        let (_, img) = run("Grade", vec![("white", num(2.0)), ("mix", num(0.5))], vec![src(), half_mask()], None);
        assert_eq!((img.get_at(0, 0, 0), img.get_at(0, 3, 0)), (0.75, 0.5), "mix times mask");
    }

    #[test]
    fn unpremult_divides_first_and_multiplies_after() {
        let (_, img) = run("Grade", vec![("add", num(0.1)), ("unpremult", text("rgba.alpha"))], vec![src()], None);
        assert!((img.get_at(0, 0, 0) - 0.55).abs() < 1e-6, "(0.5 / 0.5 + 0.1) * 0.5");
        let zero = Some(solid(IRect::new(0, 0, 1, 1), [0.5, 0.5, 0.5, 0.0]));
        let (_, img) = run("Grade", vec![("add", num(0.1)), ("unpremult", text("rgba.alpha"))], vec![zero], None);
        assert!((img.get_at(0, 0, 0) - 0.6).abs() < 1e-6, "zero alpha: graded as it is");
        let (_, img) = run("Grade", vec![("channels", text("rgba")), ("add", num(0.1)), ("unpremult", text("rgba.alpha"))], vec![src()], None);
        assert!((img.get_at(3, 0, 0) - 0.6).abs() < 1e-6, "the divisor itself is graded undivided");
    }

    #[test]
    fn saturation_luminance_modes() {
        for (mode, grey) in [("Average", (0.5 + 0.25 + 0.1) / 3.0), ("Maximum", 0.5), ("Ccir 601", 0.299 * 0.5 + 0.587 * 0.25 + 0.114 * 0.1)] {
            let (_, img) = run("Saturation", vec![("saturation", num(0.0)), ("mode", text(mode))], vec![src()], None);
            assert!((img.get_at(1, 0, 0) - grey as f32).abs() < 1e-6, "{mode}");
        }
    }
}
