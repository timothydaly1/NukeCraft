//! Draw: Rectangle and Radial.
//!
//! A draw node computes a shape (coverage 0..1 per pixel), scales it by `opacity`, and blends
//! `color` into its `output` channels over the background input by that coverage (or onto black
//! with `replace`), through the usual mask and `mix`.

use nukecraft_geom::{IRect, V2};
use nukecraft_raster::Image;

use crate::color_nodes::{INVERT_MASK, MASK_CHANNEL, MIX};
use crate::{
    Ctx, ImageInfo, Input, InputSpec, KnobDefault, KnobKind, KnobSpec, NodeSpec, Result, input_image, input_info, mask_factors, request_same, rgba_index,
};

const BG: usize = 0;
const MASK: usize = 1;
const INPUTS: &[InputSpec] = &[InputSpec { name: "bg", mask: false }, InputSpec { name: "mask", mask: true }];

const fn knob(name: &'static str, label: &'static str, kind: KnobKind, default: KnobDefault, help: &'static str) -> KnobSpec {
    KnobSpec { name, label, kind, default, help }
}

const OUTPUT: KnobSpec = knob("output", "output", KnobKind::Channels, KnobDefault::Text("rgba"), "Channels the shape is drawn into.");
const REPLACE: KnobSpec = knob("replace", "replace", KnobKind::Bool, KnobDefault::Bool(false), "Draw onto black instead of over the background.");
const INVERT: KnobSpec = knob("invert", "invert", KnobKind::Bool, KnobDefault::Bool(false), "Fill outside the shape instead of inside.");
const OPACITY: KnobSpec = knob("opacity", "opacity", KnobKind::Float, KnobDefault::Number(1.0), "Scales the shape's coverage.");
const AREA: KnobSpec = knob("area", "area", KnobKind::Box, KnobDefault::FormatFraction(&[0.25, 0.25, 0.75, 0.75]), "x, y, r, t of the shape.");
const COLOR: KnobSpec = knob("color", "color", KnobKind::Color, KnobDefault::Number(1.0), "The colour drawn.");

const RECTANGLE_KNOBS: &[KnobSpec] = &[
    OUTPUT,
    REPLACE,
    INVERT,
    OPACITY,
    AREA,
    knob("softness", "softness", KnobKind::Float, KnobDefault::Number(0.0), "Width of the soft edge in pixels, centred on the edge."),
    COLOR,
    MASK_CHANNEL,
    INVERT_MASK,
    MIX,
];

const RADIAL_KNOBS: &[KnobSpec] = &[
    OUTPUT,
    REPLACE,
    INVERT,
    OPACITY,
    AREA,
    knob(
        "softness",
        "softness",
        KnobKind::Float,
        KnobDefault::Number(1.0),
        "Fraction of the radius that fades out (0 is a hard edge, 1 fades from the centre).",
    ),
    knob("plinear", "perceptually linear", KnobKind::Bool, KnobDefault::Bool(false), "Fade linearly instead of smoothly."),
    COLOR,
    MASK_CHANNEL,
    INVERT_MASK,
    MIX,
];

pub fn specs() -> Vec<NodeSpec> {
    vec![
        NodeSpec {
            class: "Rectangle",
            aliases: &[],
            label: "Rectangle",
            category: "Draw",
            shortcut: None,
            help: "Draws a rectangle with optionally soft edges.",
            inputs: INPUTS,
            extra_inputs: false,
            knobs: RECTANGLE_KNOBS,
            info: rectangle_info,
            request: request_same,
            engine: rectangle_engine,
            input_time: None,
        },
        NodeSpec {
            class: "Radial",
            aliases: &[],
            label: "Radial",
            category: "Draw",
            shortcut: None,
            help: "Draws an ellipse filling `area`, fading towards its edge.",
            inputs: INPUTS,
            extra_inputs: false,
            knobs: RADIAL_KNOBS,
            info: radial_info,
            request: request_same,
            engine: radial_engine,
            input_time: None,
        },
    ]
}

/// The `area` knob with its corners in order and finite.
fn area(ctx: &Ctx) -> [f64; 4] {
    let [x, y, r, t] = ctx.knobs.bbox("area").map(|v| if v.is_finite() { v.clamp(-1e7, 1e7) } else { 0.0 });
    [x.min(r), y.min(t), x.max(r), y.max(t)]
}

/// Where a shape can be non-zero: its area grown by `grow` pixels, plus a pixel of zero
/// coverage all round (so the repeated edge outside the bbox is empty).
fn shape_bbox(ctx: &Ctx, grow: f64) -> IRect {
    let [x, y, r, t] = area(ctx);
    if r - x <= 0.0 || t - y <= 0.0 {
        return IRect::EMPTY;
    }
    let g = if grow.is_finite() { grow.clamp(0.0, 1e6) } else { 0.0 };
    IRect::enclosing(x - g, y - g, r + g, t + g).expand(1, 1)
}

/// The background's info with the output channels added and the bbox grown to the shape (the
/// whole format when inverted). `grow`: how far the shape reaches outside `area`.
fn draw_info(ctx: &Ctx, inputs: &[Option<ImageInfo>], grow: f64) -> Result<ImageInfo> {
    let bg = input_info(ctx, inputs, BG);
    let mut channels = bg.channels.clone();
    for c in ctx.channels_knob("output", &nukecraft_raster::ChannelSet::rgba().union(&bg.channels)).iter() {
        channels.insert(c.clone());
    }
    let shape = if ctx.knobs.boolean("invert") { bg.format.rect() } else { shape_bbox(ctx, grow) };
    Ok(ImageInfo { bbox: shape.union(bg.bbox), channels, ..bg })
}

fn rectangle_info(ctx: &Ctx, inputs: &[Option<ImageInfo>]) -> Result<ImageInfo> {
    // The soft edge is centred on the area's edge: half of it lies outside.
    draw_info(ctx, inputs, ctx.knobs.float("softness") / 2.0)
}

fn radial_info(ctx: &Ctx, inputs: &[Option<ImageInfo>]) -> Result<ImageInfo> {
    draw_info(ctx, inputs, 0.0)
}

/// Blends the shape into the background over `region`.
fn draw(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect, coverage: &dyn Fn(i32, i32) -> f32) -> Result<Image> {
    let mut out = match input_image(inputs, BG) {
        Some(inp) if inp.image.bbox() == region && inp.image.channels() == &info.channels => inp.image.clone(),
        Some(inp) => inp.image.reframed(region, &info.channels)?,
        None => Image::new(region, info.channels.clone())?,
    };
    let output = ctx.channels_knob("output", &info.channels);
    let targets: Vec<(usize, usize)> = out.channels().iter().enumerate().filter(|(_, c)| output.contains(c)).map(|(i, c)| (i, rgba_index(c))).collect();
    if targets.is_empty() {
        return Ok(out);
    }
    let color = ctx.knobs.rgba("color", 1.0).map(|v| v as f32);
    let opacity = ctx.knobs.float("opacity") as f32;
    let (invert, replace) = (ctx.knobs.boolean("invert"), ctx.knobs.boolean("replace"));
    let mask = mask_factors(ctx, inputs, MASK, region);
    let m = ctx.knobs.float("mix") as f32;
    let mut i = 0usize;
    for y in region.y0..region.y1 {
        for x in region.x0..region.x1 {
            let k = mask.as_ref().map_or(1.0, |mk| mk.get(i).copied().unwrap_or(0.0)) * m;
            i += 1;
            if k <= 0.0 || k.is_nan() {
                continue;
            }
            let s = coverage(x, y).clamp(0.0, 1.0);
            let a = (if invert { 1.0 - s } else { s }) * opacity;
            for &(p, ci) in &targets {
                let bg = out.get_at(p, x, y);
                let under = if replace { 0.0 } else { bg };
                let v = under * (1.0 - a) + color.get(ci).copied().unwrap_or(0.0) * a;
                out.set_at(p, x, y, crate::mix(bg, v, k));
            }
        }
    }
    Ok(out)
}

/// How much of the pixel `[p, p + 1]` lies in `[lo, hi]`, with a linear soft edge `soft` pixels
/// wide centred on each end (measured at the pixel centre).
fn coverage_1d(lo: f64, hi: f64, soft: f64, p: i32) -> f64 {
    let p = p as f64;
    if soft <= 0.0 {
        return (hi.min(p + 1.0) - lo.max(p)).clamp(0.0, 1.0);
    }
    let c = p + 0.5;
    ((c - (lo - soft / 2.0)) / soft).clamp(0.0, 1.0) * (((hi + soft / 2.0) - c) / soft).clamp(0.0, 1.0)
}

fn rectangle_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let [x0, y0, x1, y1] = area(ctx);
    let soft = ctx.knobs.float("softness");
    let soft = if soft.is_finite() { soft.clamp(0.0, 1e6) } else { 0.0 };
    let cov = |x: i32, y: i32| (coverage_1d(x0, x1, soft, x) * coverage_1d(y0, y1, soft, y)) as f32;
    draw(ctx, info, inputs, region, &cov)
}

/// `0` at `0` rising smoothly to `1` at `1`.
fn smooth(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

fn radial_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let [x0, y0, x1, y1] = area(ctx);
    let (rx, ry) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
    let c = V2::new((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let soft = ctx.knobs.float("softness");
    let soft = if soft.is_finite() { soft.clamp(0.0, 1.0) } else { 0.0 };
    let linear = ctx.knobs.boolean("plinear");
    let cov = |x: i32, y: i32| -> f32 {
        if rx <= 0.0 || ry <= 0.0 {
            return 0.0;
        }
        let (dx, dy) = (x as f64 + 0.5 - c.x, y as f64 + 0.5 - c.y);
        let d = ((dx / rx).powi(2) + (dy / ry).powi(2)).sqrt();
        // The hard edge, antialiased by the distance to it in pixels (d's gradient).
        let grad = if d > 1e-12 { ((dx / (rx * rx)).powi(2) + (dy / (ry * ry)).powi(2)).sqrt() / d } else { 0.0 };
        let edge = if grad > 1e-12 {
            (0.5 + (1.0 - d) / grad).clamp(0.0, 1.0)
        } else if d <= 1.0 {
            1.0
        } else {
            0.0
        };
        if soft <= 0.0 {
            return edge as f32;
        }
        let t = ((1.0 - d) / soft).clamp(0.0, 1.0);
        (if linear { t } else { smooth(t) }) as f32
    };
    draw(ctx, info, inputs, region, &cov)
}

#[cfg(test)]
mod tests {
    use crate::testutil::*;
    use nukecraft_geom::IRect;
    use nukecraft_graph::Value;

    #[test]
    fn rectangle_fills_its_area_with_antialiased_edges() {
        let (info, img) = run("Rectangle", vec![("area", nums(&[4.0, 4.0, 10.5, 8.0]))], vec![None], None);
        assert_eq!(info.bbox, IRect::new(3, 3, 12, 9), "the area, rounded out, plus an empty border");
        assert_eq!(img.get_at(3, 5, 5), 1.0);
        assert_eq!(img.get_at(3, 10, 5), 0.5, "half the pixel covered");
        assert_eq!(img.get_at(3, 3, 5), 0.0);
        assert_eq!(img.get_at(0, 30, 30), 0.0, "outside the bbox: the empty border repeats");
        let (_, img) = run("Rectangle", vec![("area", nums(&[4.0, 4.0, 20.0, 20.0])), ("softness", num(4.0))], vec![None], None);
        assert_eq!(img.get_at(3, 3, 10), 0.375, "a linear ramp from 2 to 6 at the pixel centre 3.5");
        assert_eq!(img.get_at(3, 10, 10), 1.0);
    }

    #[test]
    fn rectangle_over_a_background() {
        let bg = Some(solid(IRect::new(0, 0, 64, 32), [0.0, 0.0, 0.5, 1.0]));
        let knobs = || vec![("area", nums(&[4.0, 4.0, 10.0, 8.0])), ("color", nums(&[1.0, 0.0, 0.0, 1.0])), ("opacity", num(0.5))];
        let (_, img) = run("Rectangle", knobs(), vec![bg.clone()], None);
        assert_eq!([0, 1, 2, 3].map(|c| img.get_at(c, 5, 5)), [0.5, 0.0, 0.25, 1.0]);
        assert_eq!(img.get_at(2, 20, 20), 0.5, "background elsewhere");
        let mut k = knobs();
        k.push(("replace", Value::Bool(true)));
        let (_, img) = run("Rectangle", k, vec![bg.clone()], None);
        assert_eq!([0, 2, 3].map(|c| img.get_at(c, 5, 5)), [0.5, 0.0, 0.5]);
        assert_eq!(img.get_at(2, 20, 20), 0.0, "replace clears the output channels");
        let mut k = knobs();
        k.push(("invert", Value::Bool(true)));
        k.push(("output", text("alpha")));
        let (info, img) = run("Rectangle", k, vec![None], None);
        assert_eq!(info.bbox, IRect::new(0, 0, 64, 32), "inverted: the whole format");
        assert_eq!((img.get_at(3, 5, 5), img.get_at(3, 20, 20), img.get_at(0, 20, 20)), (0.0, 0.5, 0.0));
    }

    #[test]
    fn radial_fades_from_the_centre() {
        let (info, img) = run("Radial", vec![("area", nums(&[0.0, 0.0, 32.0, 32.0]))], vec![None], None);
        assert_eq!(info.bbox, IRect::new(-1, -1, 33, 33));
        assert!(img.get_at(3, 15, 15) > 0.99, "centre");
        assert!((img.get_at(3, 23, 15) - 0.5).abs() < 0.05, "halfway out, smooth: about a half");
        assert!(img.get_at(3, 31, 15) < 0.01, "at the ellipse's edge");
        assert_eq!(img.get_at(3, 32, 15), 0.0, "outside the ellipse");
        // A hard edge through the middle of pixel 31 (centre 16, radius 15.5).
        let (_, img) = run("Radial", vec![("area", nums(&[0.5, 0.5, 31.5, 31.5])), ("softness", num(0.0))], vec![None], None);
        assert_eq!(img.get_at(3, 15, 28), 1.0, "hard edge inside");
        let edge = img.get_at(3, 31, 15);
        assert!((edge - 0.5).abs() < 0.05, "antialiased edge pixel: {edge}");
        let (_, img) = run("Radial", vec![("area", nums(&[0.0, 0.0, 32.0, 16.0])), ("plinear", Value::Bool(true))], vec![None], None);
        let d = ((7.5f32 / 16.0).powi(2) + (0.5f32 / 8.0).powi(2)).sqrt();
        assert!((img.get_at(3, 23, 7) - (1.0 - d)).abs() < 1e-4, "linear in the ellipse's own units");
    }

    #[test]
    fn degenerate_and_hostile_areas() {
        for area in [[5.0, 5.0, 5.0, 9.0], [f64::NAN, 0.0, 1e300, -1e300], [10.0, 10.0, 2.0, 2.0]] {
            for class in ["Rectangle", "Radial"] {
                let (_, img) = run(class, vec![("area", nums(&area)), ("softness", num(f64::INFINITY))], vec![None], Some(IRect::new(0, 0, 16, 16)));
                assert!(img.is_finite(), "{class} {area:?}");
            }
        }
        let (_, img) = run("Rectangle", vec![("area", nums(&[10.0, 10.0, 2.0, 2.0]))], vec![None], None);
        assert_eq!(img.get_at(3, 5, 5), 1.0, "corners in either order");
    }
}
