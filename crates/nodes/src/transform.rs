//! Transform ▸ Transform: translate, rotate, scale and skew about a centre, with filtering.
//!
//! The matrix is `T(translate) · T(center) · R(rotate) · S(scale) · K(skew) · T(−center)`, so
//! scale, skew and rotation happen about `center`. Each output pixel centre is mapped back into
//! the input and filtered there; the kernel widens with the local minification so downscales
//! average instead of aliasing.

use nukecraft_geom::{Affine, IRect, V2};
use nukecraft_raster::Image;

use crate::filter::{FILTER_NAMES, Filter};
use crate::{Ctx, ImageInfo, Input, InputSpec, KnobDefault, KnobKind, KnobSpec, NodeSpec, Result, input_image, input_info};

/// The most source taps one output pixel may read per axis (bounds the cost of extreme
/// minification).
const MAX_TAPS: f64 = 64.0;

pub fn specs() -> Vec<NodeSpec> {
    vec![NodeSpec {
        class: "Transform",
        aliases: &[],
        label: "Transform",
        category: "Transform",
        shortcut: Some("T"),
        help: "Moves, rotates, scales and skews the image about `center`.",
        inputs: &[InputSpec { name: "Source", mask: false }],
        extra_inputs: false,
        knobs: &[
            KnobSpec { name: "translate", label: "translate", kind: KnobKind::Xy, default: KnobDefault::Numbers(&[0.0, 0.0]), help: "Offset in pixels." },
            KnobSpec { name: "rotate", label: "rotate", kind: KnobKind::Float, default: KnobDefault::Number(0.0), help: "Degrees, counter-clockwise." },
            KnobSpec {
                name: "scale",
                label: "scale",
                kind: KnobKind::Xy,
                default: KnobDefault::Numbers(&[1.0, 1.0]),
                help: "Scale factor; one number scales both axes.",
            },
            KnobSpec { name: "skewX", label: "skew X", kind: KnobKind::Float, default: KnobDefault::Number(0.0), help: "Horizontal shear." },
            KnobSpec { name: "skewY", label: "skew Y", kind: KnobKind::Float, default: KnobDefault::Number(0.0), help: "Vertical shear." },
            KnobSpec { name: "center", label: "center", kind: KnobKind::Xy, default: KnobDefault::FormatCenter, help: "Pivot for rotate, scale and skew." },
            KnobSpec { name: "invert_matrix", label: "invert", kind: KnobKind::Bool, default: KnobDefault::Bool(false), help: "Apply the inverse transform." },
            KnobSpec { name: "filter", label: "filter", kind: KnobKind::Choice(FILTER_NAMES), default: KnobDefault::Text("Cubic"), help: "Resampling filter." },
            KnobSpec {
                name: "black_outside",
                label: "black outside",
                kind: KnobKind::Bool,
                default: KnobDefault::Bool(true),
                help: "Fill outside the transformed image with black instead of repeating its edge.",
            },
        ],
        info: transform_info,
        request: transform_request,
        engine: transform_engine,
        input_time: None,
    }]
}

/// The node's matrix (input space → output space), or `None` when it is singular.
fn matrix(ctx: &Ctx) -> Option<Affine> {
    let k = ctx.knobs;
    let c = k.xy("center");
    let m = Affine::translate(k.xy("translate"))
        .then_after(Affine::translate(c))
        .then_after(Affine::rotate_degrees(k.float("rotate")))
        .then_after(Affine::scale(k.xy("scale")))
        .then_after(Affine::skew(k.float("skewX"), k.float("skewY")))
        .then_after(Affine::translate(-c));
    let m = if k.boolean("invert_matrix") { m.inverse()? } else { m };
    (m.is_finite() && m.inverse().is_some()).then_some(m)
}

fn filter(ctx: &Ctx) -> Filter {
    Filter::from_name(ctx.knobs.text("filter")).unwrap_or(Filter::Cubic { b: 0.0, c: 0.0 })
}

/// The bounding box of `r` mapped through `m`.
pub(crate) fn map_rect(m: Affine, r: IRect) -> IRect {
    if r.is_empty() {
        return IRect::EMPTY;
    }
    let corners = [V2::new(r.x0 as f64, r.y0 as f64), V2::new(r.x1 as f64, r.y0 as f64), V2::new(r.x0 as f64, r.y1 as f64), V2::new(r.x1 as f64, r.y1 as f64)]
        .map(|p| m.apply(p));
    let xs = corners.map(|p| p.x);
    let ys = corners.map(|p| p.y);
    let min = |v: [f64; 4]| v.iter().copied().fold(f64::INFINITY, f64::min);
    let max = |v: [f64; 4]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    IRect::enclosing(min(xs), min(ys), max(xs), max(ys))
}

/// Input pixels per output pixel along each input axis (≥ 1), from the inverse matrix.
pub(crate) fn footprint(inv: Affine) -> (f64, f64) {
    (inv.a.hypot(inv.b).clamp(1.0, MAX_TAPS), inv.d.hypot(inv.e).clamp(1.0, MAX_TAPS))
}

fn transform_info(ctx: &Ctx, inputs: &[Option<ImageInfo>]) -> Result<ImageInfo> {
    let input = input_info(ctx, inputs, 0);
    let Some(m) = matrix(ctx) else { return Ok(ImageInfo { bbox: IRect::EMPTY, ..input }) };
    let src = if ctx.knobs.boolean("black_outside") { input.bbox.expand(1, 1) } else { input.bbox };
    let margin = filter(ctx).support().ceil() as i32 + 1;
    Ok(ImageInfo { bbox: map_rect(m, src).expand(margin, margin), ..input })
}

fn transform_request(ctx: &Ctx, _: &ImageInfo, _: &[Option<ImageInfo>], region: IRect, _: usize) -> IRect {
    let Some(inv) = matrix(ctx).and_then(Affine::inverse) else { return IRect::EMPTY };
    let (fx, fy) = footprint(inv);
    let s = filter(ctx).support();
    map_rect(inv, region).expand((s * fx).ceil() as i32 + 1, (s * fy).ceil() as i32 + 1)
}

fn transform_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let Some(inv) = matrix(ctx).and_then(Affine::inverse) else { return Ok(Image::new(region, info.channels.clone())?) };
    let (fx, fy) = footprint(inv);
    let map = |p: V2| Some((inv.apply(p), fx, fy));
    resample(info, input_image(inputs, 0), region, filter(ctx), ctx.knobs.boolean("black_outside"), &map)
}

/// Where an output pixel centre reads from in the input, and the input pixels per output pixel
/// along each input axis there (≥ 1; the kernel widens by them). `None`: nothing (black).
pub(crate) type WarpFn<'a> = &'a dyn Fn(V2) -> Option<(V2, f64, f64)>;

/// Fills `region` by mapping each output pixel centre back into `input` with `map` and filtering
/// there. Outside the input's bbox the source is black (`black_outside`) or its edge repeated.
pub(crate) fn resample(info: &ImageInfo, input: Option<&Input>, region: IRect, f: Filter, black_outside: bool, map: WarpFn) -> Result<Image> {
    let mut out = Image::new(region, info.channels.clone())?;
    let Some(inp) = input else { return Ok(out) };
    let src_bbox = inp.info.bbox;
    let src = &inp.image;
    let chans: Vec<Option<usize>> = info.channels.iter().map(|c| src.channels().index_of(c)).collect();
    // Source value with Nuke's outside rule: black beyond the input bbox, or its edge repeated.
    let read = |i: usize, x: i32, y: i32| if black_outside && !src_bbox.contains(x, y) { 0.0 } else { src.get_at(i, x, y) };
    let mut wx = Vec::new();
    let mut wy = Vec::new();
    for y in region.y0..region.y1 {
        for x in region.x0..region.x1 {
            let Some((p, fx, fy)) = map(V2::new(x as f64 + 0.5, y as f64 + 0.5)) else { continue };
            if !p.is_finite() {
                continue;
            }
            let (fx, fy) = (clamp_footprint(fx), clamp_footprint(fy));
            if f == Filter::Impulse {
                let (sx, sy) = (clamp_coord(p.x.floor()), clamp_coord(p.y.floor()));
                for (oi, si) in chans.iter().enumerate() {
                    if let Some(si) = si {
                        out.set_at(oi, x, y, read(*si, sx, sy));
                    }
                }
                continue;
            }
            let (rx, ry) = (f.support() * fx, f.support() * fy);
            // Taps: source pixel centres within the kernel's reach.
            let (x0, x1) = (clamp_coord((p.x - rx - 0.5).ceil()), clamp_coord((p.x + rx - 0.5).floor()));
            let (y0, y1) = (clamp_coord((p.y - ry - 0.5).ceil()), clamp_coord((p.y + ry - 0.5).floor()));
            wx.clear();
            wy.clear();
            wx.extend((x0..=x1).map(|sx| f.weight((sx as f64 + 0.5 - p.x) / fx)));
            wy.extend((y0..=y1).map(|sy| f.weight((sy as f64 + 0.5 - p.y) / fy)));
            let total: f64 = wx.iter().sum::<f64>() * wy.iter().sum::<f64>();
            if total.abs() < 1e-12 {
                continue;
            }
            for (oi, si) in chans.iter().enumerate() {
                let Some(si) = *si else { continue };
                let mut acc = 0.0f64;
                for (sy, wyv) in (y0..=y1).zip(&wy) {
                    let mut row = 0.0f64;
                    for (sx, wxv) in (x0..=x1).zip(&wx) {
                        row += read(si, sx, sy) as f64 * wxv;
                    }
                    acc += row * wyv;
                }
                out.set_at(oi, x, y, (acc / total) as f32);
            }
        }
    }
    Ok(out)
}

/// A footprint kept within 1..[`MAX_TAPS`] (and finite).
pub(crate) fn clamp_footprint(v: f64) -> f64 {
    if v.is_finite() { v.clamp(1.0, MAX_TAPS) } else { MAX_TAPS }
}

pub(crate) fn clamp_coord(v: f64) -> i32 {
    v.clamp(-(nukecraft_geom::MAX_COORD as f64), nukecraft_geom::MAX_COORD as f64) as i32
}

#[cfg(test)]
mod tests {
    use crate::testutil::*;
    use nukecraft_geom::IRect;
    use nukecraft_graph::Value;

    fn square() -> Option<nukecraft_raster::Image> {
        Some(solid(IRect::new(0, 0, 8, 8), [1.0, 1.0, 1.0, 1.0]))
    }

    #[test]
    fn integer_translate_moves_pixels_exactly() {
        let (info, img) = run("Transform", vec![("translate", nums(&[10.0, 5.0])), ("filter", text("Cubic"))], vec![square()], None);
        assert!(info.bbox.contains(17, 12));
        assert!((img.get_at(0, 10, 5) - 1.0).abs() < 1e-6);
        assert!((img.get_at(0, 17, 12) - 1.0).abs() < 1e-6);
        assert_eq!(img.get_at(0, 9, 5), 0.0, "black outside");
        assert_eq!(img.get_at(0, 18, 5), 0.0);
    }

    #[test]
    fn rotate_about_center() {
        // 90° about (4, 4) maps the 8×8 square onto itself.
        let (_, img) = run("Transform", vec![("rotate", num(90.0)), ("center", nums(&[4.0, 4.0])), ("filter", text("Impulse"))], vec![square()], None);
        assert_eq!(img.get_at(0, 0, 0), 1.0);
        assert_eq!(img.get_at(0, 7, 7), 1.0);
        assert_eq!(img.get_at(0, 8, 4), 0.0);
    }

    #[test]
    fn downscale_averages() {
        let mut stripes = solid(IRect::new(0, 0, 16, 16), [0.0, 0.0, 0.0, 1.0]);
        for y in 0..16 {
            for x in (0..16).step_by(2) {
                stripes.set_at(0, x, y, 1.0);
            }
        }
        let (_, img) = run("Transform", vec![("scale", num(0.25)), ("center", nums(&[0.0, 0.0]))], vec![Some(stripes)], Some(IRect::new(1, 1, 3, 3)));
        assert!((img.get_at(0, 1, 1) - 0.5).abs() < 0.05, "stripes average to grey, got {}", img.get_at(0, 1, 1));
    }

    #[test]
    fn edge_repeats_without_black_outside() {
        let (_, img) =
            run("Transform", vec![("translate", nums(&[4.0, 0.0])), ("black_outside", Value::Bool(false))], vec![square()], Some(IRect::new(0, 0, 4, 4)));
        assert!((img.get_at(0, 0, 0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn singular_and_hostile_matrices_give_empty_output() {
        let (info, img) = run("Transform", vec![("scale", num(0.0))], vec![square()], None);
        assert!(info.bbox.is_empty());
        assert!(img.is_finite());
        let (_, img) = run("Transform", vec![("scale", num(1e-300)), ("translate", num(1e300))], vec![square()], None);
        assert!(img.is_finite());
        let (_, img) = run("Transform", vec![("scale", num(1e-9))], vec![square()], None);
        assert!(img.is_finite());
    }
}
