//! Image ▸ generators: Constant, CheckerBoard, Ramp.

use nukecraft_geom::{IRect, V2};
use nukecraft_raster::{ChannelSet, Image};

use crate::{Ctx, ImageInfo, Input, KnobDefault, KnobKind, KnobSpec, NodeSpec, Result, request_none};

const FORMAT: KnobSpec =
    KnobSpec { name: "format", label: "format", kind: KnobKind::Format, default: KnobDefault::Text(""), help: "Output format; empty uses the root format." };

pub fn specs() -> Vec<NodeSpec> {
    vec![
        NodeSpec {
            class: "Constant",
            aliases: &[],
            label: "Constant",
            category: "Image",
            shortcut: None,
            help: "A solid colour over the whole format.",
            inputs: &[],
            extra_inputs: false,
            knobs: &[
                KnobSpec { name: "color", label: "color", kind: KnobKind::Color, default: KnobDefault::Numbers(&[0.0, 0.0, 0.0, 0.0]), help: "RGBA colour." },
                FORMAT,
            ],
            info: generator_info,
            request: request_none,
            engine: constant_engine,
        },
        NodeSpec {
            class: "CheckerBoard2",
            aliases: &["CheckerBoard"],
            label: "CheckerBoard",
            category: "Image",
            shortcut: None,
            help: "A checkerboard of two colours, centred on the format.",
            inputs: &[],
            extra_inputs: false,
            knobs: &[
                FORMAT,
                KnobSpec {
                    name: "boxsize",
                    label: "box size",
                    kind: KnobKind::Xy,
                    default: KnobDefault::Numbers(&[64.0, 64.0]),
                    help: "Square size in pixels.",
                },
                KnobSpec {
                    name: "color0",
                    label: "color 0",
                    kind: KnobKind::Color,
                    default: KnobDefault::Numbers(&[0.1, 0.1, 0.1, 1.0]),
                    help: "First colour.",
                },
                KnobSpec {
                    name: "color1",
                    label: "color 1",
                    kind: KnobKind::Color,
                    default: KnobDefault::Numbers(&[0.5, 0.5, 0.5, 1.0]),
                    help: "Second colour.",
                },
            ],
            info: generator_info,
            request: request_none,
            engine: checker_engine,
        },
        NodeSpec {
            class: "Ramp",
            aliases: &[],
            label: "Ramp",
            category: "Image",
            shortcut: None,
            help: "A gradient from black at p0 to `color` at p1.",
            inputs: &[],
            extra_inputs: false,
            knobs: &[
                FORMAT,
                KnobSpec { name: "p0", label: "p0", kind: KnobKind::Xy, default: KnobDefault::Numbers(&[100.0, 100.0]), help: "Start point (black)." },
                KnobSpec { name: "p1", label: "p1", kind: KnobKind::Xy, default: KnobDefault::Numbers(&[100.0, 400.0]), help: "End point (`color`)." },
                KnobSpec { name: "color", label: "color", kind: KnobKind::Color, default: KnobDefault::Numbers(&[1.0, 1.0, 1.0, 1.0]), help: "Colour at p1." },
                KnobSpec {
                    name: "type",
                    label: "type",
                    kind: KnobKind::Choice(&["linear", "smooth0", "smooth1", "smooth"]),
                    default: KnobDefault::Text("linear"),
                    help: "Profile: linear, eased at p0, eased at p1, or eased at both ends.",
                },
            ],
            info: generator_info,
            request: request_none,
            engine: ramp_engine,
        },
    ]
}

fn generator_info(ctx: &Ctx, _: &[Option<ImageInfo>]) -> Result<ImageInfo> {
    let format = ctx.format_knob("format");
    Ok(ImageInfo { bbox: format.rect(), format, channels: ChannelSet::rgba(), frames: ctx.root.frames })
}

fn rgba32(v: [f64; 4]) -> [f32; 4] {
    v.map(|x| x as f32)
}

fn constant_engine(ctx: &Ctx, info: &ImageInfo, _: &[Option<Input>], region: IRect) -> Result<Image> {
    Ok(Image::filled(region, info.channels.clone(), &rgba32(ctx.knobs.rgba("color", 1.0)))?)
}

fn checker_engine(ctx: &Ctx, info: &ImageInfo, _: &[Option<Input>], region: IRect) -> Result<Image> {
    let size = ctx.knobs.xy("boxsize");
    let (bw, bh) = (size.x.abs().max(1.0), size.y.abs().max(1.0));
    let c = info.format.center();
    let colors = [rgba32(ctx.knobs.rgba("color0", 1.0)), rgba32(ctx.knobs.rgba("color1", 1.0))];
    let mut img = Image::new(region, info.channels.clone())?;
    for y in region.y0..region.y1 {
        let j = ((y as f64 + 0.5 - c.y) / bh).floor() as i64;
        for x in region.x0..region.x1 {
            let i = ((x as f64 + 0.5 - c.x) / bw).floor() as i64;
            let col = colors.get((i + j).rem_euclid(2) as usize).copied().unwrap_or_default();
            for (ch, v) in col.iter().enumerate() {
                img.set_at(ch, x, y, *v);
            }
        }
    }
    Ok(img)
}

fn ramp_engine(ctx: &Ctx, info: &ImageInfo, _: &[Option<Input>], region: IRect) -> Result<Image> {
    let (p0, p1) = (ctx.knobs.xy("p0"), ctx.knobs.xy("p1"));
    let color = rgba32(ctx.knobs.rgba("color", 1.0));
    let d = p1 - p0;
    let len2 = d.dot(d);
    let profile: fn(f64) -> f64 = match ctx.knobs.text("type") {
        "smooth0" => |t: f64| -t * t * t + 2.0 * t * t,
        "smooth1" => |t: f64| -t * t * t + t * t + t,
        "smooth" => |t: f64| t * t * (3.0 - 2.0 * t),
        _ => |t: f64| t,
    };
    let mut img = Image::new(region, info.channels.clone())?;
    for y in region.y0..region.y1 {
        for x in region.x0..region.x1 {
            let p = V2::new(x as f64 + 0.5, y as f64 + 0.5);
            // Coincident points: a hard edge at p0, as a degenerate gradient.
            let t = if len2 > 1e-12 { ((p - p0).dot(d) / len2).clamp(0.0, 1.0) } else { 1.0 };
            let f = profile(t) as f32;
            for (ch, v) in color.iter().enumerate() {
                img.set_at(ch, x, y, v * f);
            }
        }
    }
    Ok(img)
}

#[cfg(test)]
mod tests {
    use crate::testutil::*;
    use nukecraft_geom::IRect;

    #[test]
    fn constant_fills_the_format() {
        let (info, img) = run("Constant", vec![("color", nums(&[0.5, 0.25, 0.125, 1.0]))], vec![], None);
        assert_eq!(info.bbox, IRect::new(0, 0, 64, 32));
        assert_eq!(img.get_at(2, 63, 31), 0.125);
        let (info, _) = run("Constant", vec![("format", text("square_256"))], vec![], None);
        assert_eq!(info.format.width, 256);
    }

    #[test]
    fn checkerboard_alternates_around_the_centre() {
        let (_, img) = run("CheckerBoard", vec![("boxsize", num(16.0)), ("color0", num(0.0)), ("color1", num(1.0))], vec![], None);
        // Centre (32, 16): the pixel just up-right of it is in box (0,0) → color0.
        assert_eq!(img.get_at(0, 32, 16), 0.0);
        assert_eq!(img.get_at(0, 31, 16), 1.0);
        assert_eq!(img.get_at(0, 31, 15), 0.0);
    }

    #[test]
    fn ramp_runs_from_black_to_colour() {
        let (_, img) = run("Ramp", vec![("p0", nums(&[0.0, 0.0])), ("p1", nums(&[64.0, 0.0]))], vec![], None);
        assert!(img.get_at(0, 0, 0) < 0.01);
        assert!(img.get_at(0, 63, 0) > 0.99);
        assert!((img.get_at(0, 31, 5) - 0.5).abs() < 0.01);
        // Coincident points don't divide by zero.
        let (_, img) = run("Ramp", vec![("p0", num(5.0)), ("p1", num(5.0))], vec![], None);
        assert!(img.is_finite());
    }
}
