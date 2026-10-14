//! Transform ▸ Crop, Other ▸ Dot, Merge ▸ Switch.

use nukecraft_geom::{Format, IRect};
use nukecraft_raster::Image;

use crate::{
    Ctx, ImageInfo, Input, InputSpec, KnobDefault, KnobKind, KnobSpec, NodeSpec, Result, engine_passthrough, info_passthrough, input_image, input_info,
    request_same,
};

pub fn specs() -> Vec<NodeSpec> {
    vec![
        NodeSpec {
            class: "Crop",
            aliases: &[],
            label: "Crop",
            category: "Transform",
            shortcut: None,
            help: "Limits the image to `box`, black outside it.",
            inputs: &[InputSpec { name: "Source", mask: false }],
            extra_inputs: false,
            knobs: &[
                KnobSpec { name: "box", label: "box", kind: KnobKind::Box, default: KnobDefault::FormatBox, help: "x, y, r, t of the area kept." },
                KnobSpec {
                    name: "reformat",
                    label: "reformat",
                    kind: KnobKind::Bool,
                    default: KnobDefault::Bool(false),
                    help: "Make the box the new format, moved to the origin.",
                },
                KnobSpec {
                    name: "intersect",
                    label: "intersect",
                    kind: KnobKind::Bool,
                    default: KnobDefault::Bool(false),
                    help: "Also limit the bbox to the input's.",
                },
                KnobSpec {
                    name: "crop",
                    label: "black outside",
                    kind: KnobKind::Bool,
                    default: KnobDefault::Bool(true),
                    help: "Black outside the box instead of repeating its edge.",
                },
            ],
            info: crop_info,
            request: crop_request,
            engine: crop_engine,
            input_time: None,
        },
        NodeSpec {
            class: "Dot",
            aliases: &[],
            label: "Dot",
            category: "Other",
            shortcut: Some("."),
            help: "A bend in a pipe; passes its input through.",
            inputs: &[InputSpec { name: "", mask: false }],
            extra_inputs: false,
            knobs: &[],
            info: info_passthrough,
            request: request_same,
            engine: engine_passthrough,
            input_time: None,
        },
        NodeSpec {
            class: "Switch",
            aliases: &[],
            label: "Switch",
            category: "Merge",
            shortcut: None,
            help: "Outputs input number `which` (the last connected one if `which` is past the end).",
            inputs: &[InputSpec { name: "0", mask: false }, InputSpec { name: "1", mask: false }],
            extra_inputs: true,
            knobs: &[KnobSpec { name: "which", label: "which", kind: KnobKind::Int, default: KnobDefault::Number(0.0), help: "The input to output." }],
            info: switch_info,
            request: switch_request,
            engine: switch_engine,
            input_time: None,
        },
    ]
}

/// The crop box in the input's coordinates, rounded to whole pixels.
fn crop_box(ctx: &Ctx) -> IRect {
    let [x, y, r, t] = ctx.knobs.bbox("box");
    let round = |v: f64| if v.is_finite() { v.round() } else { 0.0 };
    let (x0, x1) = (round(x).min(round(r)), round(x).max(round(r)));
    let (y0, y1) = (round(y).min(round(t)), round(y).max(round(t)));
    IRect::enclosing(x0, y0, x1, y1)
}

/// Where the box's corner moves to: the origin with `reformat`, else nowhere.
fn crop_offset(ctx: &Ctx) -> (i32, i32) {
    let b = crop_box(ctx);
    if ctx.knobs.boolean("reformat") { (b.x0, b.y0) } else { (0, 0) }
}

fn crop_info(ctx: &Ctx, inputs: &[Option<ImageInfo>]) -> Result<ImageInfo> {
    let input = input_info(ctx, inputs, 0);
    let b = crop_box(ctx);
    let mut bbox = if ctx.knobs.boolean("intersect") { b.intersect(input.bbox) } else { b };
    if ctx.knobs.boolean("crop") {
        bbox = bbox.expand(1, 1);
    }
    let (dx, dy) = crop_offset(ctx);
    let bbox = shift(bbox, -dx, -dy);
    let format = if ctx.knobs.boolean("reformat") { Format::new(b.width(), b.height(), input.format.pixel_aspect) } else { input.format.clone() };
    Ok(ImageInfo { format, bbox, ..input })
}

fn shift(r: IRect, dx: i32, dy: i32) -> IRect {
    if r.is_empty() { r } else { IRect::new(r.x0.saturating_add(dx), r.y0.saturating_add(dy), r.x1.saturating_add(dx), r.y1.saturating_add(dy)) }
}

fn crop_request(ctx: &Ctx, _: &ImageInfo, _: &[Option<ImageInfo>], region: IRect, _: usize) -> IRect {
    let (dx, dy) = crop_offset(ctx);
    shift(region, dx, dy).intersect(crop_box(ctx))
}

fn crop_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let mut out = Image::new(region, info.channels.clone())?;
    let Some(inp) = input_image(inputs, 0) else { return Ok(out) };
    let b = crop_box(ctx);
    let black = ctx.knobs.boolean("crop");
    let (dx, dy) = crop_offset(ctx);
    for (oi, c) in info.channels.iter().enumerate() {
        let Some(si) = inp.image.channels().index_of(c) else { continue };
        for y in region.y0..region.y1 {
            for x in region.x0..region.x1 {
                let (sx, sy) = (x.saturating_add(dx), y.saturating_add(dy));
                let v = if black && !b.contains(sx, sy) {
                    0.0
                } else {
                    inp.image.get_at(si, sx.clamp(b.x0, (b.x1 - 1).max(b.x0)), sy.clamp(b.y0, (b.y1 - 1).max(b.y0)))
                };
                out.set_at(oi, x, y, v);
            }
        }
    }
    Ok(out)
}

/// The input `which` selects among `count` inputs.
fn which(ctx: &Ctx, count: usize) -> usize {
    let w = ctx.knobs.int("which").max(0) as usize;
    w.min(count.saturating_sub(1))
}

fn switch_info(ctx: &Ctx, inputs: &[Option<ImageInfo>]) -> Result<ImageInfo> {
    Ok(input_info(ctx, inputs, which(ctx, inputs.len())))
}

fn switch_request(ctx: &Ctx, _: &ImageInfo, inputs: &[Option<ImageInfo>], region: IRect, input: usize) -> IRect {
    if input == which(ctx, inputs.len()) { region } else { IRect::EMPTY }
}

fn switch_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let w = which(ctx, inputs.len());
    match input_image(inputs, w) {
        Some(inp) if inp.image.bbox() == region => Ok(inp.image.clone()),
        Some(inp) => Ok(inp.image.reframed(region, &info.channels)?),
        None => Ok(Image::new(IRect::EMPTY, info.channels.clone())?),
    }
}

#[cfg(test)]
mod tests {
    use crate::testutil::*;
    use nukecraft_geom::IRect;
    use nukecraft_graph::Value;

    fn src() -> Option<nukecraft_raster::Image> {
        Some(solid(IRect::new(0, 0, 64, 32), [1.0, 1.0, 1.0, 1.0]))
    }

    #[test]
    fn crop_blacks_outside_the_box() {
        let (info, img) = run("Crop", vec![("box", nums(&[10.0, 10.0, 20.0, 20.0]))], vec![src()], None);
        assert_eq!(info.bbox, IRect::new(9, 9, 21, 21), "one-pixel black border");
        assert_eq!(img.get_at(0, 10, 10), 1.0);
        assert_eq!(img.get_at(0, 9, 10), 0.0);
        assert_eq!(img.get_at(0, 100, 100), 0.0, "the repeated edge is the black border");
    }

    #[test]
    fn crop_reformat_moves_to_origin() {
        let (info, img) =
            run("Crop", vec![("box", nums(&[10.0, 10.0, 20.0, 30.0])), ("reformat", Value::Bool(true)), ("crop", Value::Bool(false))], vec![src()], None);
        assert_eq!((info.format.width, info.format.height), (10, 20));
        assert_eq!(info.bbox, IRect::new(0, 0, 10, 20));
        assert_eq!(img.get_at(0, 0, 0), 1.0);
    }

    #[test]
    fn switch_picks_an_input() {
        let a = solid(IRect::new(0, 0, 2, 2), [0.25; 4]);
        let b = solid(IRect::new(0, 0, 2, 2), [0.75; 4]);
        let (_, img) = run("Switch", vec![("which", num(1.0))], vec![Some(a.clone()), Some(b.clone())], None);
        assert_eq!(img.get_at(0, 0, 0), 0.75);
        let (_, img) = run("Switch", vec![("which", num(99.0))], vec![Some(a.clone()), Some(b)], None);
        assert_eq!(img.get_at(0, 0, 0), 0.75, "past the end: the last input");
        let (_, img) = run("Switch", vec![("which", num(-5.0))], vec![Some(a)], None);
        assert_eq!(img.get_at(0, 0, 0), 0.25);
    }

    #[test]
    fn dot_passes_through() {
        let (info, img) = run("Dot", vec![], vec![src()], None);
        assert_eq!(info.bbox, IRect::new(0, 0, 64, 32));
        assert_eq!(img.get_at(3, 5, 5), 1.0);
    }
}
