//! Filter: Sharpen, Erode (fast) (class `Dilate`), Erode (filter) (class `FilterErode`), Glow,
//! Median; Other: NoOp.
//!
//! Like Blur, each processes its `channels`, reads outside the input bbox as the repeated edge, and
//! blends with the input through the mask (input 1) and `mix`.

use nukecraft_color::luminance;
use nukecraft_geom::IRect;
use nukecraft_raster::{Channel, Image};

use crate::blur::{Shape, copy_plane, kernel, put_plane, separable};
use crate::color_nodes::{COLOR_INPUTS, INVERT_MASK, MASK_CHANNEL, MIX, channels};
use crate::{
    Ctx, ImageInfo, Input, InputSpec, KnobDefault, KnobKind, KnobSpec, NodeSpec, Result, engine_passthrough, finish_masked, info_passthrough, input_image,
    input_info, request_same,
};

/// The largest radius a filter here reaches, in pixels.
const MAX_RADIUS: f64 = 1024.0;

const fn knob(name: &'static str, label: &'static str, kind: KnobKind, default: KnobDefault, help: &'static str) -> KnobSpec {
    KnobSpec { name, label, kind, default, help }
}

const SHARPEN_KNOBS: &[KnobSpec] = &[
    channels("rgb"),
    knob("amount", "amount", KnobKind::Float, KnobDefault::Number(1.0), "How much of the detail (the image minus its blur) is added."),
    knob("size", "size", KnobKind::Xy, KnobDefault::Numbers(&[3.0, 3.0]), "Width of the gaussian blur the detail is measured against."),
    MASK_CHANNEL,
    INVERT_MASK,
    MIX,
];

const DILATE_KNOBS: &[KnobSpec] = &[
    channels("rgba"),
    knob("size", "size", KnobKind::Xy, KnobDefault::Numbers(&[0.0, 0.0]), "Pixels to grow by (negative: shrink), rounded to whole pixels."),
    MASK_CHANNEL,
    INVERT_MASK,
    MIX,
];

const FILTER_ERODE_KNOBS: &[KnobSpec] = &[
    channels("rgba"),
    knob("size", "size", KnobKind::Float, KnobDefault::Number(1.0), "Pixels to shrink by (negative: grow); fractions blend."),
    MASK_CHANNEL,
    INVERT_MASK,
    MIX,
];

const GLOW_KNOBS: &[KnobSpec] = &[
    channels("rgb"),
    knob("tolerance", "tolerance", KnobKind::Float, KnobDefault::Number(0.0), "Only the part of each value above this glows."),
    knob("brightness", "brightness", KnobKind::Float, KnobDefault::Number(1.0), "Multiplies the glow."),
    knob("saturation", "saturation", KnobKind::Float, KnobDefault::Number(1.0), "Saturation of the glow."),
    knob("size", "size", KnobKind::Xy, KnobDefault::Numbers(&[10.0, 10.0]), "Width of the glow's gaussian blur."),
    knob("effect_only", "effect only", KnobKind::Bool, KnobDefault::Bool(false), "Output only the glow, not the image plus the glow."),
    knob("crop", "crop to format", KnobKind::Bool, KnobDefault::Bool(true), "Keep the output bbox inside the format."),
    MASK_CHANNEL,
    INVERT_MASK,
    MIX,
];

const MEDIAN_KNOBS: &[KnobSpec] = &[
    channels("rgba"),
    knob("size", "size", KnobKind::Int, KnobDefault::Number(3.0), "Width of the square window, in pixels (rounded up to odd)."),
    MASK_CHANNEL,
    INVERT_MASK,
    MIX,
];

fn spec(
    class: &'static str,
    label: &'static str,
    help: &'static str,
    knobs: &'static [KnobSpec],
    f: (crate::InfoFn, crate::RequestFn, crate::EngineFn),
) -> NodeSpec {
    NodeSpec {
        class,
        aliases: &[],
        label,
        category: "Filter",
        shortcut: None,
        help,
        inputs: COLOR_INPUTS,
        extra_inputs: false,
        knobs,
        info: f.0,
        request: f.1,
        engine: f.2,
        input_time: None,
    }
}

pub fn specs() -> Vec<NodeSpec> {
    vec![
        spec(
            "Sharpen",
            "Sharpen",
            "Adds detail: the image minus a blur of it, times `amount`.",
            SHARPEN_KNOBS,
            (sharpen_info, sharpen_request, sharpen_engine),
        ),
        spec("Dilate", "Erode (fast)", "Grows (or shrinks) bright areas by whole pixels.", DILATE_KNOBS, (dilate_info, dilate_request, dilate_engine)),
        spec(
            "FilterErode",
            "Erode (filter)",
            "Shrinks (or grows) bright areas by any number of pixels.",
            FILTER_ERODE_KNOBS,
            (filter_erode_info, filter_erode_request, filter_erode_engine),
        ),
        spec("Glow", "Glow", "Adds a blurred copy of the bright parts of the image.", GLOW_KNOBS, (glow_info, glow_request, glow_engine)),
        spec(
            "Median",
            "Median",
            "Each pixel becomes the median of its neighbourhood (removes specks).",
            MEDIAN_KNOBS,
            (info_passthrough, median_request, median_engine),
        ),
        NodeSpec {
            class: "NoOp",
            aliases: &[],
            label: "NoOp",
            category: "Other",
            shortcut: None,
            help: "Does nothing; a place for user knobs and expressions.",
            inputs: &[InputSpec { name: "", mask: false }],
            extra_inputs: false,
            knobs: &[],
            info: info_passthrough,
            request: request_same,
            engine: engine_passthrough,
            input_time: None,
        },
    ]
}

/// A finite size within the limits, per axis.
fn size_xy(ctx: &Ctx) -> (f64, f64) {
    let s = ctx.knobs.xy("size");
    let clean = |v: f64| if v.is_finite() { v.clamp(-MAX_RADIUS * 2.0, MAX_RADIUS * 2.0) } else { 0.0 };
    (clean(s.x), clean(s.y))
}

/// Runs `per_channel(src, plane index) -> values over region` on each selected channel, copies
/// the rest, and finishes with the mask and mix.
fn per_channel(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect, f: &dyn Fn(&Image, usize, &Channel) -> Vec<f32>) -> Result<Image> {
    let mut out = Image::new(region, info.channels.clone())?;
    let Some(inp) = input_image(inputs, 0) else { return Ok(out) };
    let src = &inp.image;
    let selected = ctx.channels_knob("channels", &info.channels);
    for (oi, c) in info.channels.iter().enumerate() {
        let Some(si) = src.channels().index_of(c) else { continue };
        if selected.contains(c) {
            let values = f(src, si, c);
            put_plane(&mut out, oi, region, &values);
        } else {
            copy_plane(src, si, &mut out, oi, region);
        }
    }
    Ok(finish_masked(ctx, inputs, 1, src, out, &selected))
}

// Sharpen ---------------------------------------------------------------------------------------

fn sharpen_kernels(ctx: &Ctx) -> ((i32, Vec<f32>), (i32, Vec<f32>)) {
    let (sx, sy) = size_xy(ctx);
    (kernel(Shape::Gaussian, sx.abs()), kernel(Shape::Gaussian, sy.abs()))
}

fn sharpen_info(ctx: &Ctx, inputs: &[Option<ImageInfo>]) -> Result<ImageInfo> {
    // The overshoot reaches as far as the blur, as Blur's bbox does.
    let input = input_info(ctx, inputs, 0);
    let (kx, ky) = sharpen_kernels(ctx);
    Ok(ImageInfo { bbox: input.bbox.expand(kx.0, ky.0), ..input })
}

fn sharpen_request(ctx: &Ctx, _: &ImageInfo, _: &[Option<ImageInfo>], region: IRect, _: usize) -> IRect {
    let (kx, ky) = sharpen_kernels(ctx);
    region.expand(kx.0, ky.0)
}

fn sharpen_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let (kx, ky) = sharpen_kernels(ctx);
    let amount = ctx.knobs.float("amount") as f32;
    per_channel(ctx, info, inputs, region, &|src, si, _| {
        let blurred = separable(src, si, region, &kx, &ky);
        let mut i = 0usize;
        let mut out = Vec::with_capacity(blurred.len());
        for y in region.y0..region.y1 {
            for x in region.x0..region.x1 {
                let v = src.get_at(si, x, y);
                out.push(v + amount * (v - blurred.get(i).copied().unwrap_or(v)));
                i += 1;
            }
        }
        out
    })
}

// Erode / dilate --------------------------------------------------------------------------------

/// The maximum (`grow`) or minimum of channel `si` over a `(2rx+1)×(2ry+1)` box around each pixel
/// of `region`, as two separable passes. Outside `src` its edge repeats, so only the taps inside
/// it are read.
fn box_extreme(src: &Image, si: usize, region: IRect, rx: i32, ry: i32, grow: bool) -> Vec<f32> {
    let sb = src.bbox();
    if sb.is_empty() || region.is_empty() {
        return vec![0.0; region.area() as usize];
    }
    let pick = |a: f32, b: f32| if grow { a.max(b) } else { a.min(b) };
    // The extreme of `get` over `c - r ..= c + r`, clamped into `lo..hi`.
    let line = |get: &dyn Fn(i32) -> f32, lo: i32, hi: i32, c: i32, r: i32| {
        let a = c.saturating_sub(r).clamp(lo, hi - 1);
        let b = c.saturating_add(r).clamp(lo, hi - 1);
        (a..=b).map(get).reduce(pick).unwrap_or(0.0)
    };
    let w = region.width() as usize;
    let mut tmp = vec![0.0f32; sb.height() as usize * w];
    for (row, y) in (sb.y0..sb.y1).enumerate() {
        for (col, x) in (region.x0..region.x1).enumerate() {
            if let Some(t) = tmp.get_mut(row * w + col) {
                *t = line(&|p| src.get_at(si, p, y), sb.x0, sb.x1, x, rx);
            }
        }
    }
    let mut out = Vec::with_capacity(region.area() as usize);
    for y in region.y0..region.y1 {
        for col in 0..w {
            out.push(line(&|p| tmp.get((p - sb.y0) as usize * w + col).copied().unwrap_or(0.0), sb.y0, sb.y1, y, ry));
        }
    }
    out
}

/// Erode (fast): whole-pixel radii per axis and whether it grows.
fn dilate_radii(ctx: &Ctx) -> (i32, i32, bool) {
    let (sx, sy) = size_xy(ctx);
    let grow = sx + sy >= 0.0;
    (sx.abs().round() as i32, sy.abs().round() as i32, grow)
}

fn dilate_info(ctx: &Ctx, inputs: &[Option<ImageInfo>]) -> Result<ImageInfo> {
    let input = input_info(ctx, inputs, 0);
    let (rx, ry, grow) = dilate_radii(ctx);
    Ok(if grow { ImageInfo { bbox: input.bbox.expand(rx, ry), ..input } } else { input })
}

fn dilate_request(ctx: &Ctx, _: &ImageInfo, _: &[Option<ImageInfo>], region: IRect, _: usize) -> IRect {
    let (rx, ry, _) = dilate_radii(ctx);
    region.expand(rx, ry)
}

fn dilate_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let (rx, ry, grow) = dilate_radii(ctx);
    per_channel(ctx, info, inputs, region, &|src, si, _| box_extreme(src, si, region, rx, ry, grow))
}

/// Erode (filter): the whole radius below the size, the fraction above it, and whether it grows.
fn filter_erode_size(ctx: &Ctx) -> (i32, f32, bool) {
    let s = ctx.knobs.float("size");
    let s = if s.is_finite() { s.clamp(-MAX_RADIUS, MAX_RADIUS) } else { 0.0 };
    let r = s.abs().floor();
    (r as i32, (s.abs() - r) as f32, s < 0.0)
}

fn filter_erode_info(ctx: &Ctx, inputs: &[Option<ImageInfo>]) -> Result<ImageInfo> {
    let input = input_info(ctx, inputs, 0);
    let (r, f, grow) = filter_erode_size(ctx);
    let reach = r + i32::from(f > 0.0);
    Ok(if grow { ImageInfo { bbox: input.bbox.expand(reach, reach), ..input } } else { input })
}

fn filter_erode_request(ctx: &Ctx, _: &ImageInfo, _: &[Option<ImageInfo>], region: IRect, _: usize) -> IRect {
    let (r, f, _) = filter_erode_size(ctx);
    let reach = r + i32::from(f > 0.0);
    region.expand(reach, reach)
}

fn filter_erode_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let (r, f, grow) = filter_erode_size(ctx);
    per_channel(ctx, info, inputs, region, &|src, si, _| {
        let near = box_extreme(src, si, region, r, r, grow);
        if f <= 0.0 {
            return near;
        }
        // A fractional size blends the two whole-pixel results around it.
        let far = box_extreme(src, si, region, r + 1, r + 1, grow);
        near.iter().zip(&far).map(|(a, b)| a + (b - a) * f).collect()
    })
}

// Glow ------------------------------------------------------------------------------------------

fn glow_kernels(ctx: &Ctx) -> ((i32, Vec<f32>), (i32, Vec<f32>)) {
    let (sx, sy) = size_xy(ctx);
    (kernel(Shape::Gaussian, sx.abs()), kernel(Shape::Gaussian, sy.abs()))
}

fn glow_info(ctx: &Ctx, inputs: &[Option<ImageInfo>]) -> Result<ImageInfo> {
    let input = input_info(ctx, inputs, 0);
    let (kx, ky) = glow_kernels(ctx);
    let mut bbox = input.bbox.expand(kx.0, ky.0);
    if ctx.knobs.boolean("crop") {
        bbox = bbox.intersect(input.format.rect());
    }
    Ok(ImageInfo { bbox, ..input })
}

fn glow_request(ctx: &Ctx, _: &ImageInfo, _: &[Option<ImageInfo>], region: IRect, _: usize) -> IRect {
    let (kx, ky) = glow_kernels(ctx);
    region.expand(kx.0, ky.0)
}

fn glow_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let Some(inp) = input_image(inputs, 0) else { return Ok(Image::new(region, info.channels.clone())?) };
    // The input over what was rendered of it; reads beyond repeat its edge.
    let src = &inp.image;
    let selected = ctx.channels_knob("channels", &info.channels);
    let (kx, ky) = glow_kernels(ctx);
    let k = ctx.knobs;
    let (tolerance, brightness, saturation) = (k.float("tolerance") as f32, k.float("brightness") as f32, k.float("saturation") as f32);
    let effect_only = k.boolean("effect_only");
    // The glow's source: what lies above the tolerance, its saturation changed about its luminance.
    let mut bright = src.clone();
    let idx = |c: Channel| src.channels().index_of(&c);
    let rgb = [idx(Channel::red()), idx(Channel::green()), idx(Channel::blue())];
    let area = src.bbox();
    for y in area.y0..area.y1 {
        for x in area.x0..area.x1 {
            let above = rgb.map(|p| p.map_or(0.0, |p| (src.get_at(p, x, y) - tolerance).max(0.0)));
            let l = luminance(above[0], above[1], above[2]);
            for (p, v) in rgb.iter().zip(above) {
                if let Some(p) = p {
                    bright.set_at(*p, x, y, l + saturation * (v - l));
                }
            }
            for p in (0..src.channels().len()).filter(|p| !rgb.contains(&Some(*p))) {
                bright.set_at(p, x, y, (src.get_at(p, x, y) - tolerance).max(0.0));
            }
        }
    }
    let mut out = Image::new(region, info.channels.clone())?;
    for (oi, c) in info.channels.iter().enumerate() {
        let Some(si) = src.channels().index_of(c) else { continue };
        if !selected.contains(c) {
            copy_plane(src, si, &mut out, oi, region);
            continue;
        }
        let glow = separable(&bright, si, region, &kx, &ky);
        let mut i = 0usize;
        for y in region.y0..region.y1 {
            for x in region.x0..region.x1 {
                let g = glow.get(i).copied().unwrap_or(0.0) * brightness;
                out.set_at(oi, x, y, if effect_only { g } else { src.get_at(si, x, y) + g });
                i += 1;
            }
        }
    }
    Ok(finish_masked(ctx, inputs, 1, src, out, &selected))
}

// Median ----------------------------------------------------------------------------------------

/// The window radius: `size` rounded up to an odd width.
fn median_radius(ctx: &Ctx) -> i32 {
    let s = ctx.knobs.float("size");
    let s = if s.is_finite() { s.clamp(1.0, 63.0) } else { 1.0 };
    (s.ceil() as i32) / 2
}

fn median_request(ctx: &Ctx, _: &ImageInfo, _: &[Option<ImageInfo>], region: IRect, _: usize) -> IRect {
    let r = median_radius(ctx);
    region.expand(r, r)
}

fn median_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let r = median_radius(ctx);
    per_channel(ctx, info, inputs, region, &|src, si, _| {
        let mut window = Vec::with_capacity(((2 * r + 1) * (2 * r + 1)) as usize);
        let mut out = Vec::with_capacity(region.area() as usize);
        for y in region.y0..region.y1 {
            for x in region.x0..region.x1 {
                window.clear();
                for dy in -r..=r {
                    for dx in -r..=r {
                        window.push(src.get_at(si, x.saturating_add(dx), y.saturating_add(dy)));
                    }
                }
                window.sort_by(f32::total_cmp);
                out.push(window.get(window.len() / 2).copied().unwrap_or(0.0));
            }
        }
        out
    })
}

#[cfg(test)]
mod tests {
    use crate::testutil::*;
    use nukecraft_geom::IRect;
    use nukecraft_graph::Value;
    use nukecraft_raster::Image;

    /// A 16x16 black image with a white 4x4 square at (6, 6)..(10, 10), alpha 1 everywhere.
    fn square() -> Option<Image> {
        let mut img = solid(IRect::new(0, 0, 16, 16), [0.0, 0.0, 0.0, 1.0]);
        for y in 6..10 {
            for x in 6..10 {
                for c in 0..3 {
                    img.set_at(c, x, y, 1.0);
                }
            }
        }
        Some(img)
    }

    #[test]
    fn sharpen_overshoots_edges_and_keeps_flats() {
        let (_, img) = run("Sharpen", vec![], vec![square()], None);
        assert!(img.get_at(0, 6, 8) > 1.0, "inside the edge overshoots: {}", img.get_at(0, 6, 8));
        assert!(img.get_at(0, 5, 8) < 0.0, "outside undershoots");
        assert_eq!(img.get_at(0, 1, 1), 0.0, "flat areas unchanged");
        let border = Some(square().unwrap().reframed(IRect::new(4, 4, 12, 12), &nukecraft_raster::ChannelSet::rgba()).unwrap().with_black_border().unwrap());
        let (info, img) = run("Sharpen", vec![], vec![border], None);
        assert_eq!(info.bbox, IRect::new(1, 1, 15, 15), "grown by the blur's reach");
        assert_eq!(img.get_at(0, 0, 8), 0.0, "no undershoot repeated beyond it");
        assert_eq!(img.get_at(3, 6, 8), 1.0, "alpha untouched (channels rgb)");
        let (_, img) = run("Sharpen", vec![("amount", num(0.0))], vec![square()], None);
        assert_eq!(img.get_at(0, 6, 8), 1.0);
    }

    #[test]
    fn erode_fast_grows_and_shrinks() {
        let (info, img) = run("Dilate", vec![("size", num(2.0))], vec![square()], None);
        assert_eq!(info.bbox, IRect::new(-2, -2, 18, 18));
        assert_eq!((img.get_at(0, 4, 8), img.get_at(0, 3, 8)), (1.0, 0.0), "grown by 2");
        let (_, img) = run("Dilate", vec![("size", num(-1.0))], vec![square()], None);
        assert_eq!((img.get_at(0, 7, 7), img.get_at(0, 6, 8)), (1.0, 0.0), "shrunk by 1");
    }

    #[test]
    fn erode_filter_blends_fractional_sizes() {
        let (_, img) = run("FilterErode", vec![("size", num(1.0))], vec![square()], None);
        assert_eq!((img.get_at(0, 7, 7), img.get_at(0, 6, 7)), (1.0, 0.0), "size 1 shrinks by a pixel");
        let (_, img) = run("FilterErode", vec![("size", num(0.25))], vec![square()], None);
        assert_eq!(img.get_at(0, 6, 7), 0.75, "a quarter of the way to the next pixel");
        let (info, img) = run("FilterErode", vec![("size", num(-1.5))], vec![square()], None);
        assert_eq!(info.bbox, IRect::new(-2, -2, 18, 18));
        assert_eq!((img.get_at(0, 5, 7), img.get_at(0, 4, 7)), (1.0, 0.5), "grows by one and a half");
    }

    #[test]
    fn glow_adds_blurred_highlights() {
        let (_, img) = run("Glow", vec![("size", num(6.0))], vec![square()], None);
        assert!(img.get_at(0, 4, 8) > 0.0, "the glow spreads out");
        assert!(img.get_at(0, 8, 8) > 1.0, "the image plus its glow");
        let (_, img) = run("Glow", vec![("size", num(6.0)), ("tolerance", num(1.0))], vec![square()], None);
        assert_eq!(img.get_at(0, 4, 8), 0.0, "nothing above the tolerance");
        let (_, img) = run("Glow", vec![("size", num(6.0)), ("effect_only", Value::Bool(true))], vec![square()], None);
        assert!(img.get_at(0, 8, 8) < 1.0, "the glow alone");
    }

    #[test]
    fn median_removes_specks() {
        let mut img = solid(IRect::new(0, 0, 8, 8), [0.2, 0.2, 0.2, 1.0]);
        img.set_at(0, 4, 4, 100.0);
        let (_, out) = run("Median", vec![], vec![Some(img)], None);
        assert_eq!(out.get_at(0, 4, 4), 0.2);
    }

    #[test]
    fn noop_and_hostile_sizes() {
        let (_, img) = run("NoOp", vec![], vec![square()], None);
        assert_eq!(img.get_at(0, 7, 7), 1.0);
        for class in ["Sharpen", "Dilate", "FilterErode", "Glow", "Median"] {
            for v in [f64::NAN, f64::INFINITY, -1e300] {
                let (_, img) = run(class, vec![("size", num(v))], vec![square()], Some(IRect::new(0, 0, 4, 4)));
                assert!(img.is_finite(), "{class} {v}");
            }
        }
    }
}
