//! Transform ▸ Reformat, Mirror (class `Mirror2`), Position and CornerPin2D.
//!
//! All of them map output pixels back into the input; Reformat and CornerPin2D filter there with
//! Transform's resampler, Mirror2 and Position move whole pixels exactly.

use nukecraft_geom::{Affine, Format, Homography, IRect, V2};
use nukecraft_raster::Image;

use crate::filter::{FILTER_NAMES, Filter};
use crate::transform::{clamp_footprint, footprint, map_rect, resample};
use crate::{Ctx, ImageInfo, Input, InputSpec, KnobDefault, KnobKind, KnobSpec, NodeSpec, Result, input_image, input_info};

const SOURCE: &[InputSpec] = &[InputSpec { name: "Source", mask: false }];

const fn knob(name: &'static str, label: &'static str, kind: KnobKind, default: KnobDefault, help: &'static str) -> KnobSpec {
    KnobSpec { name, label, kind, default, help }
}

const fn flag(name: &'static str, label: &'static str, default: bool, help: &'static str) -> KnobSpec {
    knob(name, label, KnobKind::Bool, KnobDefault::Bool(default), help)
}

const FILTER: KnobSpec = knob("filter", "filter", KnobKind::Choice(FILTER_NAMES), KnobDefault::Text("Cubic"), "Resampling filter.");
const BLACK_OUTSIDE: KnobSpec = flag("black_outside", "black outside", true, "Fill outside the input with black instead of repeating its edge.");

const REFORMAT_KNOBS: &[KnobSpec] = &[
    knob(
        "type",
        "type",
        KnobKind::Choice(&["to format", "to box", "scale"]),
        KnobDefault::Text("to format"),
        "Resize to a format, to a box in pixels, or by a factor.",
    ),
    knob("format", "output format", KnobKind::Format, KnobDefault::Text(""), "The output format (type: to format); empty uses the root format."),
    knob("box_width", "width", KnobKind::Int, KnobDefault::Number(200.0), "Output width (type: to box)."),
    knob("box_height", "height", KnobKind::Int, KnobDefault::Number(200.0), "Output height (type: to box) when `box_fixed` is on."),
    flag("box_fixed", "force this shape", false, "Use `box_height`; otherwise the height keeps the input's shape."),
    knob("box_pixel_aspect", "pixel aspect", KnobKind::Float, KnobDefault::Number(1.0), "Pixel aspect of the box (type: to box)."),
    knob("scale", "scale", KnobKind::Xy, KnobDefault::Numbers(&[1.0, 1.0]), "Scale factor (type: scale); one number scales both axes."),
    knob(
        "resize",
        "resize type",
        KnobKind::Choice(&["none", "width", "height", "fit", "fill", "distort"]),
        KnobDefault::Text("width"),
        "How the image fills the new format: match its width or height, fit inside, fill it, or stretch to it.",
    ),
    flag("center", "center", true, "Centre the image in the new format (else align its bottom-left corners)."),
    flag("flip", "flip", false, "Upside down."),
    flag("flop", "flop", false, "Left to right."),
    flag("turn", "turn", false, "Rotate 90 degrees counter-clockwise first."),
    FILTER,
    BLACK_OUTSIDE,
    flag("pbb", "preserve bounding box", false, "Keep the data outside the new format."),
];

const MIRROR_KNOBS: &[KnobSpec] =
    &[flag("flip", "flip", false, "Mirror vertically (upside down)."), flag("flop", "flop", false, "Mirror horizontally (left to right).")];

const POSITION_KNOBS: &[KnobSpec] =
    &[knob("translate", "translate", KnobKind::Xy, KnobDefault::Numbers(&[0.0, 0.0]), "Offset in pixels, rounded to whole pixels (no filtering).")];

const fn corner(name: &'static str, label: &'static str, at: &'static [f64], help: &'static str) -> KnobSpec {
    knob(name, label, KnobKind::Xy, KnobDefault::FormatFraction(at), help)
}

const CORNER_PIN_KNOBS: &[KnobSpec] = &[
    corner("to1", "to1", &[0.0, 0.0], "Where the first corner goes."),
    flag("enable1", "enable1", true, "Use to1 (off: the corner stays at from1)."),
    corner("to2", "to2", &[1.0, 0.0], "Where the second corner goes."),
    flag("enable2", "enable2", true, "Use to2."),
    corner("to3", "to3", &[1.0, 1.0], "Where the third corner goes."),
    flag("enable3", "enable3", true, "Use to3."),
    corner("to4", "to4", &[0.0, 1.0], "Where the fourth corner goes."),
    flag("enable4", "enable4", true, "Use to4."),
    flag("invert", "invert", false, "Map the to corners onto the from corners instead."),
    corner("from1", "from1", &[0.0, 0.0], "The first corner in the input."),
    corner("from2", "from2", &[1.0, 0.0], "The second corner in the input."),
    corner("from3", "from3", &[1.0, 1.0], "The third corner in the input."),
    corner("from4", "from4", &[0.0, 1.0], "The fourth corner in the input."),
    FILTER,
    BLACK_OUTSIDE,
];

pub fn specs() -> Vec<NodeSpec> {
    vec![
        NodeSpec {
            class: "Reformat",
            aliases: &[],
            label: "Reformat",
            category: "Transform",
            shortcut: None,
            help: "Resizes the image to another format, a box or by a factor.",
            inputs: SOURCE,
            extra_inputs: false,
            knobs: REFORMAT_KNOBS,
            info: reformat_info,
            request: reformat_request,
            engine: reformat_engine,
        },
        NodeSpec {
            class: "Mirror2",
            aliases: &[],
            label: "Mirror",
            category: "Transform",
            shortcut: None,
            help: "Flips the image about the centre of its format.",
            inputs: SOURCE,
            extra_inputs: false,
            knobs: MIRROR_KNOBS,
            info: mirror_info,
            request: mirror_request,
            engine: mirror_engine,
        },
        NodeSpec {
            class: "Position",
            aliases: &[],
            label: "Position",
            category: "Transform",
            shortcut: None,
            help: "Moves the image by whole pixels.",
            inputs: SOURCE,
            extra_inputs: false,
            knobs: POSITION_KNOBS,
            info: position_info,
            request: position_request,
            engine: position_engine,
        },
        NodeSpec {
            class: "CornerPin2D",
            aliases: &[],
            label: "CornerPin",
            category: "Transform",
            shortcut: None,
            help: "Moves the four corners of the image (from) to new places (to): a perspective warp.",
            inputs: SOURCE,
            extra_inputs: false,
            knobs: CORNER_PIN_KNOBS,
            info: cornerpin_info,
            request: cornerpin_request,
            engine: cornerpin_engine,
        },
    ]
}

fn filter(ctx: &Ctx) -> Filter {
    Filter::from_name(ctx.knobs.text("filter")).unwrap_or(Filter::Cubic { b: 0.0, c: 0.0 })
}

/// How far a filtered image reaches past its mapped bbox.
fn margin(f: Filter) -> i32 {
    f.support().ceil() as i32 + 1
}

/// A finite, positive number of pixels, or `fallback`.
fn dim(v: f64, fallback: u32) -> u32 {
    if v.is_finite() && v >= 1.0 { v.round().min(nukecraft_geom::MAX_COORD as f64) as u32 } else { fallback }
}

/// Reformat's output format and matrix (input → output).
fn reformat_plan(ctx: &Ctx, input: &ImageInfo) -> Option<(Format, Affine)> {
    let k = ctx.knobs;
    let inf = &input.format;
    let ipa = if inf.pixel_aspect.is_finite() && inf.pixel_aspect > 0.0 { inf.pixel_aspect } else { 1.0 };
    let turn = k.boolean("turn");
    let (iw, ih) = if turn { (inf.height as f64, inf.width as f64) } else { (inf.width as f64, inf.height as f64) };
    if iw <= 0.0 || ih <= 0.0 {
        return None;
    }
    let turned = if turn { Affine { a: 0.0, b: -1.0, c: inf.height as f64, d: 1.0, e: 0.0, f: 0.0 } } else { Affine::IDENTITY };
    let (format, sx, sy, centred) = match k.text("type") {
        "scale" => {
            let s = k.xy("scale");
            if !(s.x.is_finite() && s.y.is_finite() && s.x > 0.0 && s.y > 0.0) {
                return None;
            }
            (Format::new(dim(iw * s.x, 1), dim(ih * s.y, 1), ipa), s.x, s.y, false)
        }
        kind => {
            let format = if kind == "to box" {
                let opa = k.float("box_pixel_aspect");
                let opa = if opa.is_finite() && opa > 0.0 { opa } else { 1.0 };
                let w = dim(k.float("box_width"), 1);
                let h = if k.boolean("box_fixed") { dim(k.float("box_height"), 1) } else { dim(w as f64 * ih * opa / (iw * ipa), 1) };
                Format::new(w, h, opa)
            } else {
                ctx.format_knob("format")
            };
            let opa = if format.pixel_aspect.is_finite() && format.pixel_aspect > 0.0 { format.pixel_aspect } else { 1.0 };
            let (ow, oh) = (format.width as f64, format.height as f64);
            // Keeping the picture's shape: an input pixel is `ipa` wide, an output pixel `opa`.
            let by_width = (ow / iw, ow / iw * opa / ipa);
            let by_height = (oh / ih * ipa / opa, oh / ih);
            let fits = ih * by_width.1 <= oh;
            let (sx, sy) = match k.text("resize") {
                "none" => (ipa / opa, 1.0),
                "height" => by_height,
                "fit" => {
                    if fits {
                        by_width
                    } else {
                        by_height
                    }
                }
                "fill" => {
                    if fits {
                        by_height
                    } else {
                        by_width
                    }
                }
                "distort" => (ow / iw, oh / ih),
                _ => by_width,
            };
            (format, sx, sy, k.boolean("center"))
        }
    };
    let (ow, oh) = (format.width as f64, format.height as f64);
    let offset = if centred { V2::new((ow - iw * sx) / 2.0, (oh - ih * sy) / 2.0) } else { V2::ZERO };
    let mut m = Affine::translate(offset).then_after(Affine::scale(V2::new(sx, sy))).then_after(turned);
    if k.boolean("flop") {
        m = Affine { a: -1.0, c: ow, ..Affine::IDENTITY }.then_after(m);
    }
    if k.boolean("flip") {
        m = Affine { e: -1.0, f: oh, ..Affine::IDENTITY }.then_after(m);
    }
    (m.is_finite() && m.inverse().is_some()).then_some((format, m))
}

fn reformat_info(ctx: &Ctx, inputs: &[Option<ImageInfo>]) -> Result<ImageInfo> {
    let input = input_info(ctx, inputs, 0);
    let Some((format, m)) = reformat_plan(ctx, &input) else { return Ok(ImageInfo { bbox: IRect::EMPTY, ..input }) };
    let src = if ctx.knobs.boolean("black_outside") { input.bbox.expand(1, 1) } else { input.bbox };
    let mut bbox = map_rect(m, src).expand(margin(filter(ctx)), margin(filter(ctx)));
    if !ctx.knobs.boolean("pbb") {
        bbox = bbox.intersect(format.rect());
    }
    Ok(ImageInfo { format, bbox, ..input })
}

fn reformat_request(ctx: &Ctx, _: &ImageInfo, inputs: &[Option<ImageInfo>], region: IRect, _: usize) -> IRect {
    let input = input_info(ctx, inputs, 0);
    let Some(inv) = reformat_plan(ctx, &input).and_then(|(_, m)| m.inverse()) else { return IRect::EMPTY };
    let (fx, fy) = footprint(inv);
    let s = filter(ctx).support();
    map_rect(inv, region).expand((s * fx).ceil() as i32 + 1, (s * fy).ceil() as i32 + 1)
}

fn reformat_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let input = input_image(inputs, 0);
    let in_info = input.map_or_else(|| ImageInfo::black(ctx.root), |i| i.info.clone());
    let Some(inv) = reformat_plan(ctx, &in_info).and_then(|(_, m)| m.inverse()) else { return Ok(Image::new(region, info.channels.clone())?) };
    let (fx, fy) = footprint(inv);
    let map = |p: V2| Some((inv.apply(p), fx, fy));
    resample(info, input, region, filter(ctx), ctx.knobs.boolean("black_outside"), &map)
}

/// Mirror2's matrix: an exact flip about the format's centre.
fn mirror_matrix(ctx: &Ctx, format: &Format) -> Affine {
    let mut m = Affine::IDENTITY;
    if ctx.knobs.boolean("flop") {
        m = Affine { a: -1.0, c: format.width as f64, ..m };
    }
    if ctx.knobs.boolean("flip") {
        m = Affine { e: -1.0, f: format.height as f64, ..m };
    }
    m
}

fn mirror_info(ctx: &Ctx, inputs: &[Option<ImageInfo>]) -> Result<ImageInfo> {
    let input = input_info(ctx, inputs, 0);
    let m = mirror_matrix(ctx, &input.format);
    Ok(ImageInfo { bbox: map_rect(m, input.bbox), ..input })
}

fn mirror_request(ctx: &Ctx, info: &ImageInfo, _: &[Option<ImageInfo>], region: IRect, _: usize) -> IRect {
    // A mirror is its own inverse.
    map_rect(mirror_matrix(ctx, &info.format), region)
}

fn mirror_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let m = mirror_matrix(ctx, &info.format);
    let map = |p: V2| Some((m.apply(p), 1.0, 1.0));
    resample(info, input_image(inputs, 0), region, Filter::Impulse, false, &map)
}

/// Position's offset in whole pixels.
fn position_offset(ctx: &Ctx) -> (i32, i32) {
    let t = ctx.knobs.xy("translate");
    let r = |v: f64| if v.is_finite() { v.round().clamp(-(nukecraft_geom::MAX_COORD as f64), nukecraft_geom::MAX_COORD as f64) as i32 } else { 0 };
    (r(t.x), r(t.y))
}

fn shift(r: IRect, dx: i32, dy: i32) -> IRect {
    if r.is_empty() { r } else { IRect::new(r.x0.saturating_add(dx), r.y0.saturating_add(dy), r.x1.saturating_add(dx), r.y1.saturating_add(dy)) }
}

fn position_info(ctx: &Ctx, inputs: &[Option<ImageInfo>]) -> Result<ImageInfo> {
    let input = input_info(ctx, inputs, 0);
    let (dx, dy) = position_offset(ctx);
    Ok(ImageInfo { bbox: shift(input.bbox, dx, dy), ..input })
}

fn position_request(ctx: &Ctx, _: &ImageInfo, _: &[Option<ImageInfo>], region: IRect, _: usize) -> IRect {
    let (dx, dy) = position_offset(ctx);
    shift(region, dx.saturating_neg(), dy.saturating_neg())
}

fn position_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let mut out = Image::new(region, info.channels.clone())?;
    let Some(inp) = input_image(inputs, 0) else { return Ok(out) };
    let (dx, dy) = position_offset(ctx);
    for (oi, c) in info.channels.iter().enumerate() {
        let Some(si) = inp.image.channels().index_of(c) else { continue };
        for y in region.y0..region.y1 {
            for x in region.x0..region.x1 {
                out.set_at(oi, x, y, inp.image.get_at(si, x.saturating_sub(dx), y.saturating_sub(dy)));
            }
        }
    }
    Ok(out)
}

/// CornerPin2D's homography (input → output).
fn cornerpin(ctx: &Ctx) -> Option<Homography> {
    let k = ctx.knobs;
    let from = [1, 2, 3, 4].map(|i| k.xy(&format!("from{i}")));
    let to = [1, 2, 3, 4].map(|i| if k.boolean(&format!("enable{i}")) { k.xy(&format!("to{i}")) } else { k.xy(&format!("from{i}")) });
    let h = Homography::from_quads(from, to)?;
    if k.boolean("invert") { h.inverse() } else { Some(h) }
}

/// The bbox of `r` mapped through `h`, or `None` when part of it crosses the horizon.
fn map_rect_h(h: Homography, r: IRect) -> Option<IRect> {
    if r.is_empty() {
        return Some(IRect::EMPTY);
    }
    let corners = [V2::new(r.x0 as f64, r.y0 as f64), V2::new(r.x1 as f64, r.y0 as f64), V2::new(r.x0 as f64, r.y1 as f64), V2::new(r.x1 as f64, r.y1 as f64)];
    let mut pts = Vec::with_capacity(4);
    for c in corners {
        pts.push(h.apply(c)?);
    }
    let fold = |f: fn(f64, f64) -> f64, init: f64, g: fn(&V2) -> f64| pts.iter().map(g).fold(init, f);
    Some(IRect::enclosing(
        fold(f64::min, f64::INFINITY, |p| p.x),
        fold(f64::min, f64::INFINITY, |p| p.y),
        fold(f64::max, f64::NEG_INFINITY, |p| p.x),
        fold(f64::max, f64::NEG_INFINITY, |p| p.y),
    ))
}

fn cornerpin_info(ctx: &Ctx, inputs: &[Option<ImageInfo>]) -> Result<ImageInfo> {
    let input = input_info(ctx, inputs, 0);
    let Some(h) = cornerpin(ctx) else { return Ok(ImageInfo { bbox: IRect::EMPTY, ..input }) };
    let src = if ctx.knobs.boolean("black_outside") { input.bbox.expand(1, 1) } else { input.bbox };
    // Past the horizon the image is unbounded: keep the format.
    let bbox = map_rect_h(h, src).unwrap_or_else(|| input.format.rect());
    Ok(ImageInfo { bbox: bbox.expand(margin(filter(ctx)), margin(filter(ctx))), ..input })
}

/// Where an output point reads from, and the input footprint there.
fn cornerpin_source(inv: Homography, p: V2) -> Option<(V2, f64, f64)> {
    let src = inv.apply(p)?;
    let j = inv.jacobian(p)?;
    Some((src, j[0][0].hypot(j[0][1]), j[1][0].hypot(j[1][1])))
}

fn cornerpin_request(ctx: &Ctx, _: &ImageInfo, inputs: &[Option<ImageInfo>], region: IRect, _: usize) -> IRect {
    let input = input_info(ctx, inputs, 0);
    let Some(inv) = cornerpin(ctx).and_then(Homography::inverse) else { return IRect::EMPTY };
    let corners = [(region.x0, region.y0), (region.x1, region.y0), (region.x0, region.y1), (region.x1, region.y1)];
    let mut reach = 1.0f64;
    for (x, y) in corners {
        match cornerpin_source(inv, V2::new(x as f64, y as f64)) {
            Some((_, fx, fy)) => reach = reach.max(clamp_footprint(fx)).max(clamp_footprint(fy)),
            None => return input.bbox,
        }
    }
    let m = (filter(ctx).support() * reach).ceil() as i32 + 1;
    map_rect_h(inv, region).map_or(input.bbox, |r| r.expand(m, m))
}

fn cornerpin_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let Some(inv) = cornerpin(ctx).and_then(Homography::inverse) else { return Ok(Image::new(region, info.channels.clone())?) };
    let map = |p: V2| cornerpin_source(inv, p);
    resample(info, input_image(inputs, 0), region, filter(ctx), ctx.knobs.boolean("black_outside"), &map)
}

#[cfg(test)]
mod tests {
    use crate::testutil::*;
    use nukecraft_geom::{Format, IRect};
    use nukecraft_graph::Value;
    use nukecraft_raster::Image;

    /// The 64x32 test format with red = x and green = y at each pixel.
    fn coords() -> Option<Image> {
        let mut img = solid(IRect::new(0, 0, 64, 32), [0.0, 0.0, 0.0, 1.0]);
        for y in 0..32 {
            for x in 0..64 {
                img.set_at(0, x, y, x as f32);
                img.set_at(1, x, y, y as f32);
            }
        }
        Some(img)
    }

    #[test]
    fn reformat_to_a_box_by_width_and_fit() {
        let box_knobs = |resize: &str| {
            vec![("type", text("to box")), ("box_width", num(32.0)), ("box_height", num(32.0)), ("box_fixed", Value::Bool(true)), ("resize", text(resize))]
        };
        let (info, img) = run("Reformat", box_knobs("width"), vec![coords()], None);
        assert_eq!((info.format.width, info.format.height), (32, 32));
        assert_eq!(info.bbox, IRect::new(0, 4, 32, 28), "half size, centred: rows 8..24, plus the black border and filter reach");
        let v = img.get_at(0, 10, 16);
        assert!((v - 20.5).abs() < 0.3, "x 10 reads around input x 20.5: {v}");
        let (info, _) = run("Reformat", box_knobs("fill"), vec![coords()], None);
        assert_eq!(info.bbox, IRect::new(0, 0, 32, 32), "fill covers the box");
        let (_, img) = run("Reformat", box_knobs("distort"), vec![coords()], None);
        assert!((img.get_at(1, 16, 8) - 8.0).abs() < 1e-3, "distort: y scales by 1");
        let (info, _) = run("Reformat", vec![("type", text("to box")), ("box_width", num(32.0))], vec![coords()], None);
        assert_eq!(info.format.height, 16, "the height keeps the input's shape unless fixed");
    }

    #[test]
    fn reformat_scale_flip_flop_and_turn() {
        let (info, img) = run("Reformat", vec![("type", text("scale")), ("scale", num(2.0)), ("filter", text("Impulse"))], vec![coords()], None);
        assert_eq!((info.format.width, info.format.height), (128, 64));
        assert_eq!(img.get_at(0, 21, 3), 10.0);
        let (_, img) = run(
            "Reformat",
            vec![("type", text("scale")), ("flop", Value::Bool(true)), ("flip", Value::Bool(true)), ("filter", text("Impulse"))],
            vec![coords()],
            None,
        );
        assert_eq!((img.get_at(0, 0, 0), img.get_at(1, 0, 0)), (63.0, 31.0));
        let (info, img) = run("Reformat", vec![("type", text("scale")), ("turn", Value::Bool(true)), ("filter", text("Impulse"))], vec![coords()], None);
        assert_eq!((info.format.width, info.format.height), (32, 64));
        // Turned 90 degrees counter-clockwise: the input's bottom-right corner is now at the top right.
        assert_eq!((img.get_at(0, 31, 63), img.get_at(1, 31, 63)), (63.0, 0.0));
        assert_eq!((img.get_at(0, 0, 0), img.get_at(1, 0, 0)), (0.0, 31.0));
    }

    #[test]
    fn reformat_keeps_the_bbox_only_when_asked() {
        let wide = Some(solid(IRect::new(-10, -10, 74, 42), [1.0; 4]));
        let (info, _) = run("Reformat", vec![("type", text("scale"))], vec![wide.clone()], None);
        assert_eq!(info.bbox, IRect::new(0, 0, 64, 32));
        let (info, _) = run("Reformat", vec![("type", text("scale")), ("pbb", Value::Bool(true))], vec![wide], None);
        assert!(info.bbox.x0 < -10);
        let (info, _) = run("Reformat", vec![("format", text("HD_1080"))], vec![coords()], None);
        assert_eq!(info.format, Format::named("HD_1080").unwrap());
    }

    #[test]
    fn mirror_and_position_move_whole_pixels() {
        let (info, img) = run("Mirror2", vec![("flop", Value::Bool(true))], vec![coords()], None);
        assert_eq!(info.bbox, IRect::new(0, 0, 64, 32));
        assert_eq!((img.get_at(0, 0, 5), img.get_at(0, 63, 5), img.get_at(1, 0, 5)), (63.0, 0.0, 5.0));
        let (_, img) = run("Mirror2", vec![("flip", Value::Bool(true))], vec![coords()], None);
        assert_eq!(img.get_at(1, 3, 0), 31.0);
        let (info, img) = run("Position", vec![("translate", nums(&[10.4, -3.6]))], vec![coords()], None);
        assert_eq!(info.bbox, IRect::new(10, -4, 74, 28));
        assert_eq!((img.get_at(0, 20, 10), img.get_at(1, 20, 10)), (10.0, 14.0));
    }

    #[test]
    fn cornerpin_maps_corners() {
        // Defaults (corners of the format) change nothing.
        let (info, img) = run("CornerPin2D", vec![], vec![coords()], None);
        assert!(info.bbox.contains(0, 0) && info.bbox.contains(63, 31));
        assert!((img.get_at(0, 10, 10) - 10.0).abs() < 1e-4);
        // Halve the image towards the origin: to = from / 2.
        let knobs = vec![("to2", nums(&[32.0, 0.0])), ("to3", nums(&[32.0, 16.0])), ("to4", nums(&[0.0, 16.0])), ("filter", text("Impulse"))];
        let (info, img) = run("CornerPin2D", knobs.clone(), vec![coords()], None);
        assert!(info.bbox.x1 <= 36 && info.bbox.y1 <= 20, "{:?}", info.bbox);
        assert_eq!((img.get_at(0, 5, 5), img.get_at(1, 5, 5)), (11.0, 11.0));
        // Disabled corners stay put; invert undoes the warp.
        let mut off = knobs.clone();
        off.extend([("enable2", Value::Bool(false)), ("enable3", Value::Bool(false)), ("enable4", Value::Bool(false))]);
        let (_, img) = run("CornerPin2D", off, vec![coords()], None);
        assert_eq!(img.get_at(0, 7, 7), 7.0);
        let mut inv = knobs;
        inv.push(("invert", Value::Bool(true)));
        let (_, img) = run("CornerPin2D", inv, vec![coords()], Some(IRect::new(0, 0, 32, 32)));
        assert_eq!(img.get_at(0, 20, 6), 10.0, "inverted: doubled");
    }

    #[test]
    fn degenerate_corners_and_formats() {
        let line = vec![("to1", nums(&[0.0, 0.0])), ("to2", nums(&[1.0, 0.0])), ("to3", nums(&[2.0, 0.0])), ("to4", nums(&[3.0, 0.0]))];
        let (info, img) = run("CornerPin2D", line, vec![coords()], None);
        assert!(info.bbox.is_empty() && img.is_finite());
        // A quad that folds past the horizon keeps a bounded bbox.
        let fold = vec![("to3", nums(&[-1000.0, -1000.0]))];
        let (info, img) = run("CornerPin2D", fold, vec![coords()], Some(IRect::new(0, 0, 16, 16)));
        assert!(info.bbox.width() < 100_000 && img.is_finite());
        for v in [0.0, f64::NAN, -2.0, 1e300] {
            let (_, img) = run("Reformat", vec![("type", text("scale")), ("scale", num(v))], vec![coords()], Some(IRect::new(0, 0, 8, 8)));
            assert!(img.is_finite());
            let (_, img) = run(
                "Reformat",
                vec![("type", text("to box")), ("box_width", num(v)), ("box_pixel_aspect", num(v))],
                vec![coords()],
                Some(IRect::new(0, 0, 8, 8)),
            );
            assert!(img.is_finite());
        }
    }
}
