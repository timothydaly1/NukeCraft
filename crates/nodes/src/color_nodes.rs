//! Color: Grade, Saturation, Premult, Unpremult.
//!
//! These are per-pixel operations: the output bbox is the input's, and output pixel `(x, y)`
//! reads only input pixel `(x, y)`.

use nukecraft_color::luminance;
use nukecraft_geom::IRect;
use nukecraft_raster::{Channel, Image};

use crate::{Ctx, ImageInfo, Input, KnobDefault, KnobKind, KnobSpec, NodeSpec, Result, info_passthrough, input_image, mix, request_same, rgba_index};

const INPUT: &[crate::InputSpec] = &[crate::InputSpec { name: "Source", mask: false }];

const MIX: KnobSpec =
    KnobSpec { name: "mix", label: "mix", kind: KnobKind::Float, default: KnobDefault::Number(1.0), help: "Blend between the input (0) and the result (1)." };

const fn channels(default: &'static str) -> KnobSpec {
    KnobSpec {
        name: "channels",
        label: "channels",
        kind: KnobKind::Channels,
        default: KnobDefault::Text(default),
        help: "Channels to process; others pass through.",
    }
}

const fn color(name: &'static str, label: &'static str, default: f64, help: &'static str) -> KnobSpec {
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
    MIX,
];

const SATURATION_KNOBS: &[KnobSpec] = &[
    channels("rgb"),
    KnobSpec { name: "saturation", label: "saturation", kind: KnobKind::Float, default: KnobDefault::Number(1.0), help: "0 is greyscale, 1 unchanged." },
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
            inputs: INPUT,
            extra_inputs: false,
            knobs: GRADE_KNOBS,
            info: info_passthrough,
            request: request_same,
            engine: grade_engine,
        },
        NodeSpec {
            class: "Saturation",
            aliases: &[],
            label: "Saturation",
            category: "Color",
            shortcut: None,
            help: "Scales colour away from (or towards) Rec. 709 luminance.",
            inputs: INPUT,
            extra_inputs: false,
            knobs: SATURATION_KNOBS,
            info: info_passthrough,
            request: request_same,
            engine: saturation_engine,
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

fn grade_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let (mut img, chans) = source(ctx, info, inputs, region)?;
    let k = ctx.knobs;
    let get = |name: &str| k.rgba(name, 1.0);
    let (bp, wp, lift, gain, mul, add, gamma) = (get("blackpoint"), get("whitepoint"), get("black"), get("white"), get("multiply"), get("add"), get("gamma"));
    let (reverse, bclamp, wclamp) = (k.boolean("reverse"), k.boolean("black_clamp"), k.boolean("white_clamp"));
    let m = k.float("mix") as f32;
    for (pi, c) in chans {
        let ci = rgba_index(&c);
        let at = |v: [f64; 4]| v.get(ci).copied().unwrap_or(0.0);
        // out = pow(A·in + B, 1/gamma); A = multiply·(gain − lift)/(whitepoint − blackpoint),
        // B = offset + lift − A·blackpoint.
        let range = at(wp) - at(bp);
        let a = if range.abs() < 1e-12 { 0.0 } else { at(mul) * (at(gain) - at(lift)) / range };
        let b = at(add) + at(lift) - a * at(bp);
        let g = at(gamma);
        let Some(plane) = img.plane_at_mut(pi) else { continue };
        for px in plane.iter_mut() {
            let v = *px as f64;
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
            let out = if out.is_finite() { out as f32 } else { 0.0 };
            *px = mix(*px, out, m);
        }
    }
    Ok(img)
}

fn saturation_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let (mut img, chans) = source(ctx, info, inputs, region)?;
    let s = ctx.knobs.float("saturation") as f32;
    let m = ctx.knobs.float("mix") as f32;
    let idx = |c: Channel| img.channels().index_of(&c);
    let (Some(ri), Some(gi), Some(bi)) = (idx(Channel::red()), idx(Channel::green()), idx(Channel::blue())) else { return Ok(img) };
    let n = img.bbox().area() as usize;
    let lum: Vec<f32> = (0..n)
        .map(|i| {
            let at = |p: usize| img.plane_at(p).and_then(|pl| pl.get(i)).copied().unwrap_or(0.0);
            luminance(at(ri), at(gi), at(bi))
        })
        .collect();
    for (pi, _) in chans {
        if ![ri, gi, bi].contains(&pi) {
            continue;
        }
        let Some(plane) = img.plane_at_mut(pi) else { continue };
        for (px, l) in plane.iter_mut().zip(&lum) {
            *px = mix(*px, l + s * (*px - l), m);
        }
    }
    Ok(img)
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
}
