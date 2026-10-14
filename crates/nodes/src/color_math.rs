//! Color: Clamp, Invert, Multiply, Add, Gamma, Exposure (`EXPTool`) and ColorCorrect.
//!
//! All of them run through [`color_op`], so they share Grade's `channels`, `unpremult`, mask and
//! `mix` behaviour.

use nukecraft_color::luminance;
use nukecraft_geom::IRect;
use nukecraft_raster::Image;

use crate::color_nodes::{COLOR_INPUTS, INVERT_MASK, MASK_CHANNEL, MIX, PixelOp, UNPREMULT, channels, color, color_op};
use crate::{Ctx, ImageInfo, Input, KnobDefault, KnobKind, KnobSpec, NodeSpec, Result, info_passthrough, request_same};

const fn flag(name: &'static str, label: &'static str, default: bool, help: &'static str) -> KnobSpec {
    KnobSpec { name, label, kind: KnobKind::Bool, default: KnobDefault::Bool(default), help }
}

const CLAMP_KNOBS: &[KnobSpec] = &[
    channels("rgba"),
    color("minimum", "minimum", 0.0, "Values below this are clamped."),
    flag("minimum_enable", "clamp minimum", true, "Clamp values below `minimum`."),
    color("maximum", "maximum", 1.0, "Values above this are clamped."),
    flag("maximum_enable", "clamp maximum", true, "Clamp values above `maximum`."),
    color("MinClampTo", "min clamp to", 0.0, "What values below the minimum become, when enabled."),
    flag("MinClampTo_enable", "min clamp to enable", false, "Set values below the minimum to `MinClampTo` instead of the minimum."),
    color("MaxClampTo", "max clamp to", 1.0, "What values above the maximum become, when enabled."),
    flag("MaxClampTo_enable", "max clamp to enable", false, "Set values above the maximum to `MaxClampTo` instead of the maximum."),
    UNPREMULT,
    MASK_CHANNEL,
    INVERT_MASK,
    MIX,
];

const INVERT_KNOBS: &[KnobSpec] = &[channels("rgb"), UNPREMULT, MASK_CHANNEL, INVERT_MASK, MIX];

const MULTIPLY_KNOBS: &[KnobSpec] =
    &[channels("rgb"), color("value", "value", 1.0, "Each channel is multiplied by this."), UNPREMULT, MASK_CHANNEL, INVERT_MASK, MIX];

const ADD_KNOBS: &[KnobSpec] = &[channels("rgb"), color("value", "value", 0.0, "Added to each channel."), UNPREMULT, MASK_CHANNEL, INVERT_MASK, MIX];

const GAMMA_KNOBS: &[KnobSpec] =
    &[channels("rgb"), color("value", "value", 1.0, "Positive values are raised to 1 / value (above 1 brightens)."), UNPREMULT, MASK_CHANNEL, INVERT_MASK, MIX];

const fn float(name: &'static str, label: &'static str, default: f64, help: &'static str) -> KnobSpec {
    KnobSpec { name, label, kind: KnobKind::Float, default: KnobDefault::Number(default), help }
}

const EXPOSURE_KNOBS: &[KnobSpec] = &[
    channels("rgb"),
    KnobSpec {
        name: "mode",
        label: "adjust in",
        kind: KnobKind::Choice(&["Stops"]),
        default: KnobDefault::Text("Stops"),
        help: "The unit of the adjustments.",
    },
    float("red", "red", 0.0, "Exposure change of red, in stops (each doubles)."),
    float("green", "green", 0.0, "Exposure change of green, in stops."),
    float("blue", "blue", 0.0, "Exposure change of blue, in stops."),
    UNPREMULT,
    MASK_CHANNEL,
    INVERT_MASK,
    MIX,
];

/// One ColorCorrect range's five controls, in the order they apply.
macro_rules! cc_range {
    ($prefix:literal, $label:literal, $what:literal) => {
        [
            color(concat!($prefix, "saturation"), concat!($label, "saturation"), 1.0, concat!("Saturation of ", $what, " (0 is greyscale).")),
            color(concat!($prefix, "contrast"), concat!($label, "contrast"), 1.0, concat!("Contrast of ", $what, ", about 0.18.")),
            color(concat!($prefix, "gamma"), concat!($label, "gamma"), 1.0, concat!("Gamma of ", $what, " (above 1 brightens).")),
            color(concat!($prefix, "gain"), concat!($label, "gain"), 1.0, concat!("Multiplies ", $what, ".")),
            color(concat!($prefix, "offset"), concat!($label, "offset"), 0.0, concat!("Added to ", $what, ".")),
        ]
    };
}

const CC_MASTER: [KnobSpec; 5] = cc_range!("", "", "the whole image");
const CC_SHADOWS: [KnobSpec; 5] = cc_range!("shadows.", "shadows ", "the shadows");
const CC_MIDTONES: [KnobSpec; 5] = cc_range!("midtones.", "midtones ", "the midtones");
const CC_HIGHLIGHTS: [KnobSpec; 5] = cc_range!("highlights.", "highlights ", "the highlights");

const COLOR_CORRECT_KNOBS: &[KnobSpec] = &[
    channels("rgb"),
    CC_MASTER[0],
    CC_MASTER[1],
    CC_MASTER[2],
    CC_MASTER[3],
    CC_MASTER[4],
    CC_SHADOWS[0],
    CC_SHADOWS[1],
    CC_SHADOWS[2],
    CC_SHADOWS[3],
    CC_SHADOWS[4],
    CC_MIDTONES[0],
    CC_MIDTONES[1],
    CC_MIDTONES[2],
    CC_MIDTONES[3],
    CC_MIDTONES[4],
    CC_HIGHLIGHTS[0],
    CC_HIGHLIGHTS[1],
    CC_HIGHLIGHTS[2],
    CC_HIGHLIGHTS[3],
    CC_HIGHLIGHTS[4],
    UNPREMULT,
    MASK_CHANNEL,
    INVERT_MASK,
    MIX,
];

fn spec(class: &'static str, label: &'static str, help: &'static str, knobs: &'static [KnobSpec], engine: crate::EngineFn) -> NodeSpec {
    NodeSpec {
        class,
        aliases: &[],
        label,
        category: "Color",
        shortcut: None,
        help,
        inputs: COLOR_INPUTS,
        extra_inputs: false,
        knobs,
        info: info_passthrough,
        request: request_same,
        engine,
    }
}

pub fn specs() -> Vec<NodeSpec> {
    vec![
        spec("Clamp", "Clamp", "Limits values to minimum..maximum.", CLAMP_KNOBS, clamp_engine),
        spec("Invert", "Invert", "One minus each channel.", INVERT_KNOBS, invert_engine),
        spec("Multiply", "Multiply", "Multiplies each channel by a value.", MULTIPLY_KNOBS, multiply_engine),
        spec("Add", "Add", "Adds a value to each channel.", ADD_KNOBS, add_engine),
        spec("Gamma", "Gamma", "Raises positive values to the power 1 / value.", GAMMA_KNOBS, gamma_engine),
        spec("EXPTool", "Exposure", "Changes exposure per channel, in stops.", EXPOSURE_KNOBS, exposure_engine),
        spec(
            "ColorCorrect",
            "ColorCorrect",
            "Saturation, contrast, gamma, gain and offset, for the whole image and for shadows, midtones and highlights.",
            COLOR_CORRECT_KNOBS,
            color_correct_engine,
        ),
    ]
}

/// A colour knob's component `ci` (0 red … 3 alpha) as `f32`.
fn comp(v: [f64; 4], ci: usize) -> f32 {
    v.get(ci).copied().unwrap_or(0.0) as f32
}

fn clamp_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let k = ctx.knobs;
    let (lo, hi, lo_to, hi_to) = (k.rgba("minimum", 0.0), k.rgba("maximum", 1.0), k.rgba("MinClampTo", 0.0), k.rgba("MaxClampTo", 1.0));
    let (lo_on, hi_on, lo_to_on, hi_to_on) =
        (k.boolean("minimum_enable"), k.boolean("maximum_enable"), k.boolean("MinClampTo_enable"), k.boolean("MaxClampTo_enable"));
    let f = |ci: usize, v: f32| {
        if lo_on && v < comp(lo, ci) {
            return if lo_to_on { comp(lo_to, ci) } else { comp(lo, ci) };
        }
        if hi_on && v > comp(hi, ci) {
            return if hi_to_on { comp(hi_to, ci) } else { comp(hi, ci) };
        }
        v
    };
    color_op(ctx, info, inputs, region, PixelOp::Channel(&f))
}

fn invert_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    color_op(ctx, info, inputs, region, PixelOp::Channel(&|_, v| 1.0 - v))
}

fn multiply_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let value = ctx.knobs.rgba("value", 1.0);
    color_op(ctx, info, inputs, region, PixelOp::Channel(&|ci, v| v * comp(value, ci)))
}

fn add_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let value = ctx.knobs.rgba("value", 0.0);
    color_op(ctx, info, inputs, region, PixelOp::Channel(&|ci, v| v + comp(value, ci)))
}

/// `v^(1/g)` for positive `v` and `g`; other values pass unchanged.
fn gamma(v: f32, g: f32) -> f32 {
    if v > 0.0 && g > 0.0 && g != 1.0 { v.powf(1.0 / g) } else { v }
}

fn gamma_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let value = ctx.knobs.rgba("value", 1.0);
    color_op(ctx, info, inputs, region, PixelOp::Channel(&|ci, v| gamma(v, comp(value, ci))))
}

fn exposure_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let k = ctx.knobs;
    // Stops: each one doubles. Alpha has no exposure control.
    let gains = [k.float("red"), k.float("green"), k.float("blue")].map(|s| s.clamp(-1000.0, 1000.0).exp2() as f32);
    color_op(ctx, info, inputs, region, PixelOp::Channel(&|ci, v| gains.get(ci).map_or(v, |g| v * g)))
}

/// ColorCorrect's pivot for contrast.
const CONTRAST_PIVOT: f32 = 0.18;

/// The five controls of one ColorCorrect range, per RGBA component.
#[derive(Clone, Copy)]
struct Correction {
    saturation: [f64; 4],
    contrast: [f64; 4],
    gamma: [f64; 4],
    gain: [f64; 4],
    offset: [f64; 4],
}

impl Correction {
    fn read(ctx: &Ctx, prefix: &str) -> Correction {
        let get = |name: &str, alpha3: f64| ctx.knobs.rgba(&format!("{prefix}{name}"), alpha3);
        Correction {
            saturation: get("saturation", 1.0),
            contrast: get("contrast", 1.0),
            gamma: get("gamma", 1.0),
            gain: get("gain", 1.0),
            offset: get("offset", 0.0),
        }
    }

    /// The master controls followed by a range's: factors multiply, offsets add.
    fn then(self, o: Correction) -> Correction {
        let mul = |a: [f64; 4], b: [f64; 4]| std::array::from_fn(|i| a.get(i).copied().unwrap_or(1.0) * b.get(i).copied().unwrap_or(1.0));
        let add = |a: [f64; 4], b: [f64; 4]| std::array::from_fn(|i| a.get(i).copied().unwrap_or(0.0) + b.get(i).copied().unwrap_or(0.0));
        Correction {
            saturation: mul(self.saturation, o.saturation),
            contrast: mul(self.contrast, o.contrast),
            gamma: mul(self.gamma, o.gamma),
            gain: mul(self.gain, o.gain),
            offset: add(self.offset, o.offset),
        }
    }

    fn is_identity(&self) -> bool {
        let all = |v: [f64; 4], x: f64| v.iter().all(|c| *c == x);
        all(self.saturation, 1.0) && all(self.contrast, 1.0) && all(self.gamma, 1.0) && all(self.gain, 1.0) && all(self.offset, 0.0)
    }

    fn apply(&self, [r, g, b, a]: [f32; 4]) -> [f32; 4] {
        let l = luminance(r, g, b);
        let mut px = [r, g, b, a];
        for (ci, v) in px.iter_mut().enumerate() {
            let s = comp(self.saturation, ci);
            if ci < 3 && s != 1.0 {
                *v = l + s * (*v - l);
            }
            let c = comp(self.contrast, ci);
            if *v > 0.0 && c != 1.0 {
                *v = CONTRAST_PIVOT * (*v / CONTRAST_PIVOT).powf(c);
            }
            *v = gamma(*v, comp(self.gamma, ci)) * comp(self.gain, ci) + comp(self.offset, ci);
        }
        px
    }
}

/// `0` at `e0` rising smoothly to `1` at `e1`.
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How much of a pixel of luminance `l` is shadow, midtone and highlight: shadows fade out by
/// 0.09, highlights fade in from 0.5 to 1 (Nuke's default ranges), midtones are the rest.
fn range_weights(l: f32) -> [f32; 3] {
    let s = 1.0 - smoothstep(0.0, 0.09, l);
    let h = smoothstep(0.5, 1.0, l);
    [s, (1.0 - s - h).max(0.0), h]
}

fn color_correct_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let master = Correction::read(ctx, "");
    let ranges = ["shadows.", "midtones.", "highlights."].map(|p| Correction::read(ctx, p));
    let by_range = ranges.iter().any(|r| !r.is_identity());
    let combined = ranges.map(|r| master.then(r));
    let f = |px: [f32; 4]| {
        if !by_range {
            return master.apply(px);
        }
        let w = range_weights(luminance(px[0], px[1], px[2]));
        let mut out = [0.0f32; 4];
        for (c, wi) in combined.iter().zip(w) {
            if wi > 0.0 {
                for (o, v) in out.iter_mut().zip(c.apply(px)) {
                    *o += wi * v;
                }
            }
        }
        out
    };
    color_op(ctx, info, inputs, region, PixelOp::Rgba(&f))
}

#[cfg(test)]
mod tests {
    use crate::testutil::*;
    use nukecraft_geom::IRect;
    use nukecraft_graph::Value;

    fn src() -> Option<nukecraft_raster::Image> {
        Some(solid(IRect::new(0, 0, 4, 4), [0.5, 0.25, -0.1, 0.5]))
    }

    fn rgba(img: &nukecraft_raster::Image) -> [f32; 4] {
        [0, 1, 2, 3].map(|c| img.get_at(c, 1, 1))
    }

    fn close(a: [f32; 4], b: [f32; 4]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5)
    }

    #[test]
    fn clamp_limits_and_replaces() {
        let (_, img) = run("Clamp", vec![("maximum", num(0.4))], vec![src()], None);
        assert_eq!(rgba(&img), [0.4, 0.25, 0.0, 0.4], "rgba by default");
        let (_, img) = run("Clamp", vec![("maximum", num(0.4)), ("MaxClampTo_enable", Value::Bool(true)), ("MaxClampTo", num(0.0))], vec![src()], None);
        assert_eq!(rgba(&img), [0.0, 0.25, 0.0, 0.0]);
        let (_, img) = run("Clamp", vec![("minimum_enable", Value::Bool(false))], vec![src()], None);
        assert_eq!(img.get_at(2, 1, 1), -0.1);
    }

    #[test]
    fn math_nodes() {
        let (_, img) = run("Invert", vec![], vec![src()], None);
        assert!(close(rgba(&img), [0.5, 0.75, 1.1, 0.5]), "alpha untouched");
        let (_, img) = run("Multiply", vec![("value", nums(&[2.0, 4.0, 1.0, 3.0])), ("channels", text("rgba"))], vec![src()], None);
        assert!(close(rgba(&img), [1.0, 1.0, -0.1, 1.5]));
        let (_, img) = run("Add", vec![("value", num(0.25))], vec![src()], None);
        assert!(close(rgba(&img), [0.75, 0.5, 0.15, 0.5]));
        let (_, img) = run("Gamma", vec![("value", num(2.0))], vec![src()], None);
        assert!(close(rgba(&img), [0.5f32.sqrt(), 0.5, -0.1, 0.5]), "negatives pass");
        let (_, img) = run("EXPTool", vec![("red", num(1.0)), ("green", num(-2.0))], vec![src()], None);
        assert!(close(rgba(&img), [1.0, 0.0625, -0.1, 0.5]));
    }

    #[test]
    fn color_correct_master_controls() {
        let (_, img) = run("ColorCorrect", vec![], vec![src()], None);
        assert_eq!(rgba(&img), [0.5, 0.25, -0.1, 0.5], "defaults change nothing");
        let (_, img) = run("ColorCorrect", vec![("gain", num(2.0)), ("offset", num(0.1))], vec![src()], None);
        assert!(close(rgba(&img), [1.1, 0.6, -0.1, 0.5]));
        let (_, img) = run("ColorCorrect", vec![("contrast", num(2.0))], vec![src()], None);
        let c = |v: f32| 0.18 * (v / 0.18f32).powi(2);
        assert!(close(rgba(&img), [c(0.5), c(0.25), -0.1, 0.5]));
        let (_, img) = run("ColorCorrect", vec![("saturation", num(0.0))], vec![src()], None);
        let l = nukecraft_color::luminance(0.5, 0.25, -0.1);
        assert!(close(rgba(&img), [l, l, l, 0.5]));
    }

    #[test]
    fn color_correct_ranges_follow_luminance() {
        let dark = Some(solid(IRect::new(0, 0, 4, 4), [0.01, 0.01, 0.01, 1.0]));
        let bright = Some(solid(IRect::new(0, 0, 4, 4), [2.0, 2.0, 2.0, 1.0]));
        let (_, img) = run("ColorCorrect", vec![("shadows.gain", num(3.0))], vec![dark.clone()], None);
        assert!(img.get_at(0, 1, 1) > 0.02, "shadows lifted");
        let (_, img) = run("ColorCorrect", vec![("shadows.gain", num(3.0))], vec![bright.clone()], None);
        assert_eq!(img.get_at(0, 1, 1), 2.0, "highlights untouched by shadows");
        let (_, img) = run("ColorCorrect", vec![("highlights.gain", num(0.5))], vec![bright], None);
        assert_eq!(img.get_at(0, 1, 1), 1.0, "fully a highlight above 1");
        let (_, img) = run("ColorCorrect", vec![("midtones.gain", num(0.5)), ("gain", num(2.0))], vec![dark], None);
        assert!(img.get_at(0, 1, 1) > 0.019, "master applies everywhere");
        let w = super::range_weights(0.3);
        assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-6 && w[1] == 1.0);
    }

    #[test]
    fn hostile_knob_values_stay_finite_or_pass_through() {
        for (class, knob) in [("Gamma", "value"), ("EXPTool", "red"), ("ColorCorrect", "contrast"), ("ColorCorrect", "gamma"), ("Clamp", "maximum")] {
            for v in [f64::NAN, f64::INFINITY, -1e300, 0.0] {
                let _ = run(class, vec![(knob, num(v))], vec![src()], None);
            }
        }
        let (_, img) = run("Gamma", vec![("value", num(0.0))], vec![src()], None);
        assert_eq!(img.get_at(0, 1, 1), 0.5, "gamma 0 leaves values alone");
    }
}
