//! Filter ▸ Blur: a separable blur with box, triangle, quadratic or gaussian kernels.
//!
//! `size` is the kernel's full width in pixels on each axis. Reads outside the input bbox repeat
//! its edge (so a blurred Constant stays flat to its border, as in Nuke), and the output bbox
//! grows by half the kernel unless `crop` limits it to the format. The size → kernel mapping is
//! NukeCraft's and is listed for verification in `docs/fidelity/README.md`.

use nukecraft_geom::IRect;
use nukecraft_raster::Image;

use crate::{Ctx, ImageInfo, Input, InputSpec, KnobDefault, KnobKind, KnobSpec, NodeSpec, Result, input_image, input_info, mix};

/// The largest blur size, in pixels.
const MAX_SIZE: f64 = 4096.0;

pub fn specs() -> Vec<NodeSpec> {
    vec![NodeSpec {
        class: "Blur",
        aliases: &[],
        label: "Blur",
        category: "Filter",
        shortcut: Some("B"),
        help: "Blurs the image by `size` pixels.",
        inputs: &[InputSpec { name: "Source", mask: false }],
        extra_inputs: false,
        knobs: &[
            KnobSpec {
                name: "channels",
                label: "channels",
                kind: KnobKind::Channels,
                default: KnobDefault::Text("rgba"),
                help: "Channels to blur; others pass through.",
            },
            KnobSpec {
                name: "size",
                label: "size",
                kind: KnobKind::Xy,
                default: KnobDefault::Numbers(&[0.0, 0.0]),
                help: "Kernel width in pixels; one number blurs both axes.",
            },
            KnobSpec {
                name: "filter",
                label: "filter",
                kind: KnobKind::Choice(&["box", "triangle", "quadratic", "gaussian"]),
                default: KnobDefault::Text("gaussian"),
                help: "Kernel shape.",
            },
            KnobSpec {
                name: "crop",
                label: "crop to format",
                kind: KnobKind::Bool,
                default: KnobDefault::Bool(true),
                help: "Keep the output bbox inside the format.",
            },
            KnobSpec {
                name: "mix",
                label: "mix",
                kind: KnobKind::Float,
                default: KnobDefault::Number(1.0),
                help: "Blend between the input (0) and the result (1).",
            },
        ],
        info: blur_info,
        request: blur_request,
        engine: blur_engine,
        input_time: None,
    }]
}

#[derive(Clone, Copy, PartialEq)]
enum Shape {
    Box,
    Triangle,
    Quadratic,
    Gaussian,
}

fn shape(ctx: &Ctx) -> Shape {
    match ctx.knobs.text("filter") {
        "box" => Shape::Box,
        "triangle" => Shape::Triangle,
        "quadratic" => Shape::Quadratic,
        _ => Shape::Gaussian,
    }
}

fn sizes(ctx: &Ctx) -> (f64, f64) {
    let s = ctx.knobs.xy("size");
    let clean = |v: f64| if v.is_finite() { v.abs().min(MAX_SIZE) } else { 0.0 };
    (clean(s.x), clean(s.y))
}

/// Normalised weights for offsets `-r..=r` (r = returned radius).
fn kernel(shape: Shape, size: f64) -> (i32, Vec<f32>) {
    if size < 1e-3 {
        return (0, vec![1.0]);
    }
    let half = size / 2.0;
    let r = half.ceil() as i32;
    let w: Vec<f64> = (-r..=r)
        .map(|d| {
            let d = d as f64;
            match shape {
                // Overlap of the pixel [d−½, d+½] with the box [−half, half].
                Shape::Box => ((d + 0.5).min(half) - (d - 0.5).max(-half)).max(0.0),
                Shape::Triangle => (1.0 - d.abs() / half.max(0.5)).max(0.0),
                Shape::Quadratic => {
                    let u = d.abs() * 1.5 / half.max(0.5);
                    if u < 0.5 {
                        0.75 - u * u
                    } else if u < 1.5 {
                        0.5 * (1.5 - u) * (1.5 - u)
                    } else {
                        0.0
                    }
                }
                Shape::Gaussian => {
                    let sigma = (size / 6.0).max(1e-3);
                    (-(d * d) / (2.0 * sigma * sigma)).exp()
                }
            }
        })
        .collect();
    let sum: f64 = w.iter().sum();
    if sum <= 0.0 {
        return (0, vec![1.0]);
    }
    (r, w.iter().map(|v| (v / sum) as f32).collect())
}

fn radii(ctx: &Ctx) -> (i32, i32) {
    let (sx, sy) = sizes(ctx);
    let s = shape(ctx);
    (kernel(s, sx).0, kernel(s, sy).0)
}

fn blur_info(ctx: &Ctx, inputs: &[Option<ImageInfo>]) -> Result<ImageInfo> {
    let input = input_info(ctx, inputs, 0);
    let (rx, ry) = radii(ctx);
    let mut bbox = input.bbox.expand(rx, ry);
    if ctx.knobs.boolean("crop") {
        bbox = bbox.intersect(input.format.rect());
    }
    Ok(ImageInfo { bbox, ..input })
}

fn blur_request(ctx: &Ctx, _: &ImageInfo, _: &[Option<ImageInfo>], region: IRect, _: usize) -> IRect {
    let (rx, ry) = radii(ctx);
    region.expand(rx, ry)
}

fn blur_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let mut out = Image::new(region, info.channels.clone())?;
    let Some(inp) = input_image(inputs, 0) else { return Ok(out) };
    let src = &inp.image;
    let (sx, sy) = sizes(ctx);
    let s = shape(ctx);
    let (rx, kx) = kernel(s, sx);
    let (ry, ky) = kernel(s, sy);
    let selected = ctx.channels_knob("channels", &info.channels);
    let m = ctx.knobs.float("mix") as f32;
    // Horizontal pass over the rows the vertical pass needs.
    let rows = IRect::new(region.x0, region.y0.saturating_sub(ry), region.x1, region.y1.saturating_add(ry));
    for (oi, c) in info.channels.iter().enumerate() {
        let Some(si) = src.channels().index_of(c) else { continue };
        if !selected.contains(c) {
            for y in region.y0..region.y1 {
                for x in region.x0..region.x1 {
                    out.set_at(oi, x, y, src.get_at(si, x, y));
                }
            }
            continue;
        }
        let w = rows.width() as usize;
        let mut tmp = vec![0.0f32; rows.area() as usize];
        for (row, y) in (rows.y0..rows.y1).enumerate() {
            for (col, x) in (region.x0..region.x1).enumerate() {
                let mut acc = 0.0f32;
                for (k, d) in kx.iter().zip(-rx..=rx) {
                    acc += k * src.get_at(si, x.saturating_add(d), y);
                }
                if let Some(t) = tmp.get_mut(row * w + col) {
                    *t = acc;
                }
            }
        }
        for (row, y) in (region.y0..region.y1).enumerate() {
            for (col, x) in (region.x0..region.x1).enumerate() {
                let mut acc = 0.0f32;
                for (j, k) in ky.iter().enumerate() {
                    acc += k * tmp.get((row + j) * w + col).copied().unwrap_or(0.0);
                }
                out.set_at(oi, x, y, mix(src.get_at(si, x, y), acc, m));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;
    use nukecraft_graph::Value;

    #[test]
    fn kernels_are_normalised() {
        for s in [Shape::Box, Shape::Triangle, Shape::Quadratic, Shape::Gaussian] {
            for size in [0.0, 0.5, 1.0, 3.0, 10.5] {
                let (r, k) = kernel(s, size);
                assert_eq!(k.len(), (2 * r + 1) as usize);
                assert!((k.iter().sum::<f32>() - 1.0).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn flat_input_stays_flat_to_its_edges() {
        let src = solid(IRect::new(0, 0, 64, 32), [0.5, 0.5, 0.5, 1.0]);
        let (info, img) = run("Blur", vec![("size", num(10.0))], vec![Some(src)], None);
        assert_eq!(info.bbox, IRect::new(0, 0, 64, 32), "cropped to format");
        assert!((img.get_at(0, 0, 0) - 0.5).abs() < 1e-5);
        assert!((img.get_at(0, 63, 31) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn a_point_spreads_and_keeps_its_energy() {
        let mut src = solid(IRect::new(0, 0, 21, 21), [0.0; 4]);
        src.set_at(0, 10, 10, 1.0);
        let src = src.with_black_border().unwrap();
        let (info, img) = run("Blur", vec![("size", num(6.0)), ("crop", Value::Bool(false)), ("filter", text("box"))], vec![Some(src)], None);
        let mut total = 0.0;
        for y in info.bbox.y0..info.bbox.y1 {
            for x in info.bbox.x0..info.bbox.x1 {
                total += img.get_at(0, x, y);
            }
        }
        assert!((total - 1.0).abs() < 1e-4, "energy {total}");
        assert!(img.get_at(0, 12, 10) > 0.0);
        assert_eq!(img.get_at(0, 14, 10), 0.0);
    }

    #[test]
    fn hostile_sizes_are_bounded() {
        let src = solid(IRect::new(0, 0, 4, 4), [1.0; 4]);
        let (_, img) = run("Blur", vec![("size", num(1e300))], vec![Some(src.clone())], Some(IRect::new(0, 0, 1, 1)));
        assert!(img.is_finite());
        let (_, img) = run("Blur", vec![("size", num(-3.0))], vec![Some(src)], None);
        assert!(img.is_finite());
    }
}
