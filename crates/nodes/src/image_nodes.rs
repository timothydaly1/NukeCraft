//! Generators: Image ▸ Constant, CheckerBoard, ColorBars; Draw ▸ Ramp.

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
            input_time: None,
        },
        NodeSpec {
            class: "CheckerBoard2",
            aliases: &["CheckerBoard"],
            label: "CheckerBoard",
            category: "Image",
            shortcut: None,
            help: "A checkerboard of four colours with lines between the boxes, centred on the format.",
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
                KnobSpec {
                    name: "color2",
                    label: "color 2",
                    kind: KnobKind::Color,
                    default: KnobDefault::Numbers(&[0.1, 0.1, 0.1, 1.0]),
                    help: "Third colour (alternate rows' first).",
                },
                KnobSpec {
                    name: "color3",
                    label: "color 3",
                    kind: KnobKind::Color,
                    default: KnobDefault::Numbers(&[0.5, 0.5, 0.5, 1.0]),
                    help: "Fourth colour (alternate rows' second).",
                },
                KnobSpec {
                    name: "linecolor",
                    label: "line color",
                    kind: KnobKind::Color,
                    default: KnobDefault::Numbers(&[1.0, 1.0, 0.0, 1.0]),
                    help: "Colour of the lines between boxes.",
                },
                KnobSpec {
                    name: "linewidth",
                    label: "line width",
                    kind: KnobKind::Float,
                    default: KnobDefault::Number(4.0),
                    help: "Width of the lines between boxes, in pixels (0: none).",
                },
                KnobSpec {
                    name: "centerlinecolor",
                    label: "centerline color",
                    kind: KnobKind::Color,
                    default: KnobDefault::Numbers(&[1.0, 0.0, 0.0, 1.0]),
                    help: "Colour of the lines through the centre of the format.",
                },
                KnobSpec {
                    name: "centerlinewidth",
                    label: "centerline width",
                    kind: KnobKind::Float,
                    default: KnobDefault::Number(3.0),
                    help: "Width of the centre lines, in pixels (0: none).",
                },
            ],
            info: generator_info,
            request: request_none,
            engine: checker_engine,
            input_time: None,
        },
        NodeSpec {
            class: "ColorBars",
            aliases: &[],
            label: "ColorBars",
            category: "Image",
            shortcut: None,
            help: "SMPTE-style colour bars: seven bars, a reversed strip and a strip with a white patch and black levels.",
            inputs: &[],
            extra_inputs: false,
            knobs: &[
                FORMAT,
                KnobSpec {
                    name: "barintensity",
                    label: "bar intensity",
                    kind: KnobKind::Float,
                    default: KnobDefault::Number(0.75),
                    help: "Brightness of the bars (0.75 for 75% bars).",
                },
            ],
            info: generator_info,
            request: request_none,
            engine: colorbars_engine,
            input_time: None,
        },
        NodeSpec {
            class: "Ramp",
            aliases: &[],
            label: "Ramp",
            category: "Draw",
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
            input_time: None,
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

/// How much of the pixel `[p, p + 1]` lies within `w / 2` of a line every `step` pixels from
/// `origin` (the nearest one).
fn grid_line_coverage(p: i32, origin: f64, step: f64, w: f64) -> f64 {
    if w <= 0.0 {
        return 0.0;
    }
    let c = p as f64 + 0.5;
    let line = origin + ((c - origin) / step).round() * step;
    let (lo, hi) = (line - w / 2.0, line + w / 2.0);
    ((p as f64 + 1.0).min(hi) - (p as f64).max(lo)).clamp(0.0, 1.0)
}

/// Coverage of the line through `origin` alone (the centre line), on one axis.
fn line_coverage(p: i32, origin: f64, w: f64) -> f64 {
    if w <= 0.0 {
        return 0.0;
    }
    let (lo, hi) = (origin - w / 2.0, origin + w / 2.0);
    ((p as f64 + 1.0).min(hi) - (p as f64).max(lo)).clamp(0.0, 1.0)
}

fn checker_engine(ctx: &Ctx, info: &ImageInfo, _: &[Option<Input>], region: IRect) -> Result<Image> {
    let k = ctx.knobs;
    let size = k.xy("boxsize");
    let clean = |v: f64| if v.is_finite() { v.abs().clamp(1.0, 1e7) } else { 1.0 };
    let (bw, bh) = (clean(size.x), clean(size.y));
    let width = |name: &str| {
        let w = k.float(name);
        if w.is_finite() { w.clamp(0.0, 1e7) } else { 0.0 }
    };
    let (lw, cw) = (width("linewidth"), width("centerlinewidth"));
    let c = info.format.center();
    let colors = ["color0", "color1", "color2", "color3"].map(|n| rgba32(k.rgba(n, 1.0)));
    let (line, center) = (rgba32(k.rgba("linecolor", 1.0)), rgba32(k.rgba("centerlinecolor", 1.0)));
    let over = |under: [f32; 4], top: [f32; 4], a: f64| -> [f32; 4] {
        let a = a as f32;
        std::array::from_fn(|i| under.get(i).copied().unwrap_or(0.0) * (1.0 - a) + top.get(i).copied().unwrap_or(0.0) * a)
    };
    let mut img = Image::new(region, info.channels.clone())?;
    for y in region.y0..region.y1 {
        let j = ((y as f64 + 0.5 - c.y) / bh).floor() as i64;
        let (ly, cy) = (grid_line_coverage(y, c.y, bh, lw), line_coverage(y, c.y, cw));
        for x in region.x0..region.x1 {
            let i = ((x as f64 + 0.5 - c.x) / bw).floor() as i64;
            // A 2x2 pattern: rows alternate between colours 0/1 and 2/3, shifted by one box.
            let index = (i + j).rem_euclid(2) + 2 * j.rem_euclid(2);
            let mut col = colors.get(index as usize).copied().unwrap_or_default();
            let lx = grid_line_coverage(x, c.x, bw, lw);
            col = over(col, line, 1.0 - (1.0 - lx) * (1.0 - ly));
            let cx = line_coverage(x, c.x, cw);
            col = over(col, center, 1.0 - (1.0 - cx) * (1.0 - cy));
            for (ch, v) in col.iter().enumerate() {
                img.set_at(ch, x, y, *v);
            }
        }
    }
    Ok(img)
}

/// The colour bars' colour at `(u, v)` (0..1 across, 0..1 down from the top), before intensity
/// applies to the bars.
fn colorbar(u: f64, v: f64, bar: f32) -> [f32; 4] {
    let b = bar;
    const TOP: [[f32; 3]; 7] = [[1.0, 1.0, 1.0], [1.0, 1.0, 0.0], [0.0, 1.0, 1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
    const MIDDLE: [[f32; 3]; 7] = [[0.0, 0.0, 1.0], [0.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 0.0], [0.0, 1.0, 1.0], [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]];
    let seventh = ((u * 7.0).floor() as usize).min(6);
    let rgb = if v < 2.0 / 3.0 {
        TOP.get(seventh).map(|c| c.map(|x| x * b))
    } else if v < 0.75 {
        MIDDLE.get(seventh).map(|c| c.map(|x| x * b))
    } else {
        // -I, 100% white, +Q, black, then the pluge (below black, black, above black), black.
        let edges = [5.0 / 28.0, 10.0 / 28.0, 15.0 / 28.0, 20.0 / 28.0, 20.0 / 28.0 + 1.0 / 21.0, 20.0 / 28.0 + 2.0 / 21.0, 20.0 / 28.0 + 3.0 / 21.0];
        let i = edges.iter().position(|e| u < *e).unwrap_or(7);
        Some(match i {
            0 => [0.0, 0.129, 0.298],
            1 => [1.0, 1.0, 1.0],
            2 => [0.196, 0.0, 0.416],
            4 => [-0.04, -0.04, -0.04],
            6 => [0.04, 0.04, 0.04],
            _ => [0.0, 0.0, 0.0],
        })
    };
    let [r, g, bl] = rgb.unwrap_or([0.0; 3]);
    [r, g, bl, 1.0]
}

fn colorbars_engine(ctx: &Ctx, info: &ImageInfo, _: &[Option<Input>], region: IRect) -> Result<Image> {
    let bar = ctx.knobs.float("barintensity");
    let bar = if bar.is_finite() { bar as f32 } else { 0.75 };
    let (w, h) = (info.format.width.max(1) as f64, info.format.height.max(1) as f64);
    let mut img = Image::new(region, info.channels.clone())?;
    for y in region.y0..region.y1 {
        let v = 1.0 - (y as f64 + 0.5) / h;
        for x in region.x0..region.x1 {
            let col = colorbar((x as f64 + 0.5) / w, v, bar);
            for (ch, c) in col.iter().enumerate() {
                img.set_at(ch, x, y, *c);
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
    fn checkerboard_lines_and_four_colours() {
        let knobs = |extra: Vec<(&'static str, nukecraft_graph::Value)>| {
            let mut k = vec![("boxsize", num(16.0)), ("color0", num(0.1)), ("color1", num(0.2)), ("color2", num(0.3)), ("color3", num(0.4))];
            k.extend(extra);
            k
        };
        let (_, img) = run("CheckerBoard", knobs(vec![("linewidth", num(0.0)), ("centerlinewidth", num(0.0))]), vec![], None);
        // Centre (32, 16): boxes (0, 0) color0, (1, 0) color1, (0, 1) color3, (1, 1) color2.
        assert_eq!([img.get_at(0, 34, 18), img.get_at(0, 50, 18), img.get_at(0, 34, 2), img.get_at(0, 50, 2)], [0.1, 0.2, 0.4, 0.3]);
        let (_, img) = run("CheckerBoard", knobs(vec![("centerlinewidth", num(0.0))]), vec![], None);
        assert_eq!(img.get_at(1, 47, 20), 1.0, "a 4-pixel yellow line at x = 48");
        assert_eq!(img.get_at(2, 47, 20), 0.0);
        assert_eq!(img.get_at(1, 45, 20), 0.1, "beyond it, the box");
        let (_, img) = run("CheckerBoard", knobs(vec![("linewidth", num(0.0)), ("centerlinewidth", num(3.0))]), vec![], None);
        assert_eq!([img.get_at(0, 32, 5), img.get_at(1, 32, 5)], [1.0, 0.0], "the red centre line");
        assert!((img.get_at(0, 30, 5) - (0.5 * 1.0 + 0.5 * 0.3)).abs() < 1e-6, "its antialiased edge, half a pixel over color2");
    }

    #[test]
    fn colorbars_have_seven_bars() {
        let (info, img) = run("ColorBars", vec![], vec![], None);
        assert_eq!(info.bbox, IRect::new(0, 0, 64, 32));
        // The top band: white, yellow, cyan, green, magenta, red, blue at 75%.
        let px = |x: i32| [0, 1, 2].map(|c| img.get_at(c, x, 30));
        assert_eq!(px(4), [0.75, 0.75, 0.75]);
        assert_eq!(px(13), [0.75, 0.75, 0.0]);
        assert_eq!(px(59), [0.0, 0.0, 0.75]);
        let (_, img) = run("ColorBars", vec![("barintensity", num(1.0))], vec![], None);
        assert_eq!(img.get_at(1, 13, 30), 1.0);
    }

    #[test]
    fn checkerboard_alternates_around_the_centre() {
        let (_, img) = run(
            "CheckerBoard",
            vec![
                ("boxsize", num(16.0)),
                ("color0", num(0.0)),
                ("color1", num(1.0)),
                ("color2", num(0.0)),
                ("color3", num(1.0)),
                ("linewidth", num(0.0)),
                ("centerlinewidth", num(0.0)),
            ],
            vec![],
            None,
        );
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
