//! Image files for NukeCraft: PNG and OpenEXR in and out, and the Read and Write nodes.
//!
//! Files are converted to NukeCraft's working space on the way in (linear light, y-up rows) and
//! back on the way out. File names may carry a frame number pattern: `####` (padded to the
//! number of `#`), `%04d` or `%d`.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod exr_io;
mod png_io;

use std::path::{Path, PathBuf};

use nukecraft_color::Colorspace;
use nukecraft_geom::{Format, IRect};
use nukecraft_graph::{NodeId, Script};
use nukecraft_nodes::{
    Ctx, ImageInfo, Input, InputSpec, KnobDefault, KnobKind, KnobSpec, NodeError, NodeSpec, Registry, info_passthrough, request_none, request_same,
};
use nukecraft_raster::{Channel, ChannelSet, Image};
use nukecraft_render::{RenderError, Renderer};
use nukecraft_time::FrameTime;

/// A decoded file in working space.
#[derive(Clone, Debug)]
pub struct Decoded {
    pub format: Format,
    pub image: Image,
}

/// What a file holds, from its header alone.
#[derive(Clone, Debug, PartialEq)]
pub struct FileInfo {
    pub format: Format,
    pub bbox: IRect,
    pub channels: ChannelSet,
}

/// How to write a file.
#[derive(Clone, Debug, PartialEq)]
pub struct WriteOptions {
    pub channels: ChannelSet,
    pub colorspace: Colorspace,
    /// Sample size: 8 or 16 (PNG), 16 (half) or 32 (float) for EXR.
    pub bits: u8,
    /// Unpremultiply the colour channels by alpha before converting (and premultiply after).
    pub premultiplied: bool,
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum IoError {
    #[error("{path}: {message}")]
    File { path: String, message: String },
    #[error("{0}: unsupported file type (NukeCraft reads and writes .png and .exr)")]
    UnsupportedType(String),
    #[error(transparent)]
    Render(#[from] RenderError),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, IoError>;

/// The file types NukeCraft reads and writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileType {
    Png,
    Exr,
}

impl FileType {
    pub fn from_path(path: &Path) -> Option<FileType> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "png" => Some(FileType::Png),
            "exr" => Some(FileType::Exr),
            _ => None,
        }
    }

    /// The colour space files of this type hold unless a knob says otherwise.
    pub fn default_colorspace(self) -> Colorspace {
        match self {
            FileType::Png => Colorspace::Srgb,
            FileType::Exr => Colorspace::Linear,
        }
    }
}

pub(crate) fn file_error(path: &Path, message: impl std::fmt::Display) -> IoError {
    IoError::File { path: path.display().to_string(), message: message.to_string() }
}

/// Replaces a frame-number pattern in `pattern` with `frame`: `####` (zero-padded to the number
/// of `#`), `%0Nd` or `%d`. A name without a pattern is returned unchanged.
pub fn frame_path(pattern: &str, frame: i64) -> String {
    let pad = |width: usize| if frame < 0 { format!("-{:0w$}", frame.unsigned_abs(), w = width.saturating_sub(1)) } else { format!("{frame:0width$}") };
    if let Some(start) = pattern.find('#') {
        let len = pattern.get(start..).map_or(0, |rest| rest.chars().take_while(|c| *c == '#').count());
        return format!("{}{}{}", pattern.get(..start).unwrap_or(""), pad(len), pattern.get(start + len..).unwrap_or(""));
    }
    if let Some(start) = pattern.find('%') {
        let rest = pattern.get(start + 1..).unwrap_or("");
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if rest.get(digits.len()..).is_some_and(|r| r.starts_with('d')) {
            let width = digits.trim_start_matches('0').parse::<usize>().unwrap_or(0).min(32);
            let end = start + 1 + digits.len() + 1;
            return format!("{}{}{}", pattern.get(..start).unwrap_or(""), pad(width), pattern.get(end..).unwrap_or(""));
        }
    }
    pattern.to_string()
}

/// Reads a file's header.
pub fn read_info(path: &Path) -> Result<FileInfo> {
    match FileType::from_path(path) {
        Some(FileType::Png) => png_io::read_info(path),
        Some(FileType::Exr) => exr_io::read_info(path),
        None => Err(IoError::UnsupportedType(path.display().to_string())),
    }
}

/// Reads a file into working space: linear (converted from `colorspace`, or the type's
/// default), y-up. With `premultiplied`, colour is unpremultiplied around the conversion.
pub fn read_image(path: &Path, colorspace: Option<Colorspace>, premultiplied: bool) -> Result<Decoded> {
    let ty = FileType::from_path(path).ok_or_else(|| IoError::UnsupportedType(path.display().to_string()))?;
    let mut d = match ty {
        FileType::Png => png_io::read(path)?,
        FileType::Exr => exr_io::read(path)?,
    };
    let cs = colorspace.unwrap_or(ty.default_colorspace());
    convert(&mut d.image, |v| cs.to_linear(v), premultiplied);
    Ok(d)
}

/// Encodes 8-bit RGBA pixels (rows top-down) as a PNG in memory.
pub fn encode_png_rgba8(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    png_io::encode_rgba8(width, height, rgba)
}

/// Writes `image` (working space) to `path` over `format`, atomically: the data goes to a
/// temporary file next to it that is renamed into place once complete.
pub fn write_image(path: &Path, image: &Image, format: &Format, opts: &WriteOptions) -> Result<()> {
    let ty = FileType::from_path(path).ok_or_else(|| IoError::UnsupportedType(path.display().to_string()))?;
    let mut img = image.reframed(image.bbox(), &opts.channels).map_err(|e| file_error(path, e))?;
    let cs = opts.colorspace;
    convert(&mut img, |v| cs.from_linear(v), opts.premultiplied);
    let tmp = temp_path(path);
    let result = match ty {
        FileType::Png => png_io::write(&tmp, &img, format, opts.bits),
        FileType::Exr => exr_io::write(&tmp, &img, format, opts.bits),
    };
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        file_error(path, e)
    })
}

fn temp_path(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    path.with_file_name(format!(".{name}.nukecraft-tmp"))
}

/// Applies `f` to red, green and blue (not alpha, depth or other layers), unpremultiplying
/// around it when asked.
fn convert(img: &mut Image, f: impl Fn(f32) -> f32, premultiplied: bool) {
    let alpha = img.plane(&Channel::alpha()).map(<[f32]>::to_vec);
    for c in [Channel::red(), Channel::green(), Channel::blue()] {
        let Some(plane) = img.plane_mut(&c) else { continue };
        for (k, px) in plane.iter_mut().enumerate() {
            let a = if premultiplied { alpha.as_ref().and_then(|a| a.get(k)).copied().unwrap_or(1.0) } else { 1.0 };
            *px = if a != 0.0 && a != 1.0 { f(*px / a) * a } else { f(*px) };
        }
    }
}

/// Registers Read and Write.
pub fn register(registry: &mut Registry) -> std::result::Result<(), NodeError> {
    registry.register(read_spec())?;
    registry.register(write_spec())
}

/// The built-in nodes plus Read and Write.
pub fn full_registry() -> Registry {
    let mut r = Registry::builtin();
    // Read and Write are not built-in names, so this cannot collide.
    let _ = register(&mut r);
    r
}

const COLORSPACES: &[&str] = &["default", "linear", "sRGB", "rec709", "Gamma1.8", "Gamma2.2", "Cineon"];

const FILE: KnobSpec =
    KnobSpec { name: "file", label: "file", kind: KnobKind::File, default: KnobDefault::Text(""), help: "Path; #### or %04d stands for the frame number." };
const COLORSPACE: KnobSpec = KnobSpec {
    name: "colorspace",
    label: "colorspace",
    kind: KnobKind::Choice(COLORSPACES),
    default: KnobDefault::Text("default"),
    help: "The file's colour space; default is sRGB for PNG and linear for EXR.",
};
const PREMULTIPLIED: KnobSpec = KnobSpec {
    name: "premultiplied",
    label: "premultiplied",
    kind: KnobKind::Bool,
    default: KnobDefault::Bool(false),
    help: "The colour is premultiplied: divide by alpha before converting colour and multiply after.",
};

fn read_spec() -> NodeSpec {
    NodeSpec {
        class: "Read",
        aliases: &[],
        label: "Read",
        category: "Image",
        shortcut: Some("R"),
        help: "Reads an image or image sequence (PNG, OpenEXR).",
        inputs: &[],
        extra_inputs: false,
        knobs: &[
            FILE,
            COLORSPACE,
            PREMULTIPLIED,
            KnobSpec { name: "raw", label: "raw data", kind: KnobKind::Bool, default: KnobDefault::Bool(false), help: "No colour conversion." },
        ],
        info: read_node_info,
        request: request_none,
        engine: read_node_engine,
        input_time: None,
    }
}

fn write_spec() -> NodeSpec {
    NodeSpec {
        class: "Write",
        aliases: &[],
        label: "Write",
        category: "Image",
        shortcut: Some("W"),
        help: "Writes its input to files when rendered (render.execute); passes it through otherwise.",
        inputs: &[InputSpec { name: "", mask: false }],
        extra_inputs: false,
        knobs: &[
            FILE,
            KnobSpec { name: "channels", label: "channels", kind: KnobKind::Channels, default: KnobDefault::Text("rgb"), help: "Channels to write." },
            COLORSPACE,
            PREMULTIPLIED,
            KnobSpec {
                name: "datatype",
                label: "datatype",
                kind: KnobKind::Choice(&["auto", "8 bit", "16 bit", "16 bit half", "32 bit float"]),
                default: KnobDefault::Text("auto"),
                help: "Sample size: auto is 8 bit for PNG and 16 bit half for EXR.",
            },
            KnobSpec {
                name: "create_directories",
                label: "create directories",
                kind: KnobKind::Bool,
                default: KnobDefault::Bool(false),
                help: "Create missing folders in the path.",
            },
        ],
        info: info_passthrough,
        request: request_same,
        engine: nukecraft_nodes::engine_passthrough,
        input_time: None,
    }
}

/// The file a Read/Write knob names at `t`.
fn knob_path(ctx: &Ctx) -> std::result::Result<PathBuf, NodeError> {
    let pattern = ctx.knobs.text("file");
    if pattern.trim().is_empty() {
        return Err(ctx.io_error("no file name"));
    }
    Ok(PathBuf::from(frame_path(pattern, ctx.time.floor_frame())))
}

fn knob_colorspace(ctx: &Ctx) -> Option<Colorspace> {
    if ctx.knobs.boolean("raw") {
        return Some(Colorspace::Linear);
    }
    Colorspace::from_name(ctx.knobs.text("colorspace"))
}

fn read_node_info(ctx: &Ctx, _: &[Option<ImageInfo>]) -> std::result::Result<ImageInfo, NodeError> {
    let path = knob_path(ctx)?;
    let fi = read_info(&path).map_err(|e| ctx.io_error(e.to_string()))?;
    Ok(ImageInfo { format: fi.format, bbox: fi.bbox, channels: fi.channels, frames: ctx.root.frames })
}

fn read_node_engine(ctx: &Ctx, info: &ImageInfo, _: &[Option<Input>], region: IRect) -> std::result::Result<Image, NodeError> {
    let path = knob_path(ctx)?;
    let d = read_image(&path, knob_colorspace(ctx), ctx.knobs.boolean("premultiplied")).map_err(|e| ctx.io_error(e.to_string()))?;
    Ok(d.image.reframed(region, &info.channels)?)
}

/// Renders a Write node at each frame and writes the files. Returns the paths written.
pub fn execute_write(script: &Script, registry: &Registry, write: NodeId, frames: &[i64]) -> Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    for &frame in frames {
        written.extend(execute_write_frame(script, registry, write, frame)?);
    }
    Ok(written)
}

/// Renders a Write node at one frame and writes its file; `None` for a disabled Write.
pub fn execute_write_frame(script: &Script, registry: &Registry, write: NodeId, frame: i64) -> Result<Option<PathBuf>> {
    let node = script.node(write).ok_or_else(|| IoError::Other(format!("no node with id {}", write.0)))?;
    let spec = registry.get(&node.class).ok_or_else(|| IoError::Other(format!("unknown class {}", node.class)))?;
    if spec.class != "Write" {
        return Err(IoError::Other(format!("`{}` is a {}, not a Write node", node.name, node.class)));
    }
    if node.disabled {
        return Ok(None);
    }
    let t = FrameTime::from_frame(frame);
    let knobs = nukecraft_nodes::Knobs::evaluate(spec.knobs, &node.knobs, t, &script.root.format);
    let ctx = Ctx { knobs: &knobs, root: &script.root, time: t, node_name: &node.name };
    let path = knob_path(&ctx).map_err(|e| IoError::Other(e.to_string()))?;
    let ty = FileType::from_path(&path).ok_or_else(|| IoError::UnsupportedType(path.display().to_string()))?;
    if knobs.boolean("create_directories")
        && let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty())
    {
        std::fs::create_dir_all(dir).map_err(|e| file_error(dir, e))?;
    }
    let mut renderer = Renderer::new(script, registry);
    let info = renderer.info(write, t)?;
    let region = match ty {
        FileType::Png => info.format.rect(),
        FileType::Exr => info.bbox,
    };
    let image = renderer.render(write, t, region)?;
    let opts = write_options(&ctx, ty, &info.channels);
    write_image(&path, &image, &info.format, &opts)?;
    log::info!("{}: wrote {}", node.name, path.display());
    Ok(Some(path))
}

fn write_options(ctx: &Ctx, ty: FileType, available: &ChannelSet) -> WriteOptions {
    let bits = match (ctx.knobs.text("datatype"), ty) {
        ("16 bit", FileType::Png) => 16,
        ("32 bit float", FileType::Exr) => 32,
        (_, FileType::Png) => 8,
        (_, FileType::Exr) => 16,
    };
    let wanted = ChannelSet::parse(ctx.knobs.text("channels"), available).unwrap_or_else(|_| ChannelSet::rgb());
    WriteOptions {
        channels: wanted,
        colorspace: knob_colorspace(ctx).unwrap_or(ty.default_colorspace()),
        bits,
        premultiplied: ctx.knobs.boolean("premultiplied"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nukecraft_graph::KnobValue;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nukecraft-io-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn frame_patterns() {
        assert_eq!(frame_path("a.####.exr", 7), "a.0007.exr");
        assert_eq!(frame_path("a.#.exr", 123), "a.123.exr");
        assert_eq!(frame_path("a.%04d.png", 12), "a.0012.png");
        assert_eq!(frame_path("a.%d.png", 12), "a.12.png");
        assert_eq!(frame_path("a.####.exr", -3), "a.-003.exr");
        assert_eq!(frame_path("plain.png", 5), "plain.png");
        assert_eq!(frame_path("100%.png", 5), "100%.png");
    }

    fn gradient() -> Image {
        let mut img = Image::new(IRect::new(0, 0, 8, 4), ChannelSet::rgba()).unwrap();
        for y in 0..4 {
            for x in 0..8 {
                img.set_at(0, x, y, x as f32 / 7.0);
                img.set_at(1, x, y, y as f32 / 3.0);
                img.set_at(2, x, y, 0.25);
                img.set_at(3, x, y, 1.0);
            }
        }
        img
    }

    #[test]
    fn png_round_trip() {
        let dir = tmpdir("png");
        let path = dir.join("g.png");
        let img = gradient();
        let fmt = Format::new(8, 4, 1.0);
        for bits in [8, 16] {
            let opts = WriteOptions { channels: ChannelSet::rgba(), colorspace: Colorspace::Srgb, bits, premultiplied: false };
            write_image(&path, &img, &fmt, &opts).unwrap();
            let info = read_info(&path).unwrap();
            assert_eq!(info.bbox, IRect::new(0, 0, 8, 4));
            let back = read_image(&path, None, false).unwrap().image;
            let tol = if bits == 8 { 0.01 } else { 1e-4 };
            for (x, y) in [(0, 0), (7, 3), (3, 1)] {
                for c in 0..4 {
                    assert!((back.get_at(c, x, y) - img.get_at(c, x, y)).abs() < tol, "{bits} bit ({x},{y}) c{c}");
                }
            }
        }
        assert!(!dir.join(".g.png.nukecraft-tmp").exists());
    }

    #[test]
    fn exr_round_trip_keeps_floats_and_bbox() {
        let dir = tmpdir("exr");
        let path = dir.join("g.exr");
        let mut img = Image::new(IRect::new(-2, 3, 6, 9), ChannelSet::rgba()).unwrap();
        img.set_at(0, -2, 8, 7.5);
        img.set_at(3, 5, 3, 0.5);
        let fmt = Format::new(16, 16, 1.0);
        let opts = WriteOptions { channels: ChannelSet::rgba(), colorspace: Colorspace::Linear, bits: 32, premultiplied: false };
        write_image(&path, &img, &fmt, &opts).unwrap();
        let info = read_info(&path).unwrap();
        assert_eq!(info.bbox, img.bbox());
        assert_eq!(info.format.width, 16);
        let back = read_image(&path, None, false).unwrap().image;
        assert_eq!(back.get_at(0, -2, 8), 7.5);
        assert_eq!(back.get_at(3, 5, 3), 0.5);
    }

    #[test]
    fn broken_files_are_errors() {
        let dir = tmpdir("broken");
        for name in ["x.png", "x.exr"] {
            let p = dir.join(name);
            std::fs::write(&p, b"definitely not an image").unwrap();
            assert!(read_info(&p).is_err());
            assert!(read_image(&p, None, false).is_err());
        }
        assert!(matches!(read_info(&dir.join("x.tif")), Err(IoError::UnsupportedType(_))));
        assert!(read_info(&dir.join("missing.png")).is_err());
    }

    #[test]
    fn read_and_write_nodes() {
        let dir = tmpdir("nodes");
        let reg = full_registry();
        let mut s = Script::new();
        s.root.format = Format::new(16, 8, 1.0);
        let c = s.add_node("Constant", None).unwrap();
        s.node_mut(c).unwrap().knobs.insert("color".into(), KnobValue::numbers(&[0.5, 0.25, 0.125, 1.0]));
        let w = s.add_node("Write", None).unwrap();
        s.connect(w, 0, c).unwrap();
        let pattern = dir.join("out.####.exr").display().to_string();
        s.node_mut(w).unwrap().knobs.insert("file".into(), KnobValue::text(&pattern));
        s.node_mut(w).unwrap().knobs.insert("channels".into(), KnobValue::text("rgba"));
        let written = execute_write(&s, &reg, w, &[1, 2]).unwrap();
        assert_eq!(written.len(), 2);
        assert!(dir.join("out.0002.exr").exists());

        let r = s.add_node("Read", None).unwrap();
        s.node_mut(r).unwrap().knobs.insert("file".into(), KnobValue::text(&pattern));
        let mut renderer = Renderer::new(&s, &reg);
        let (info, img) = renderer.render_format(r, FrameTime::from_frame(2)).unwrap();
        assert_eq!(info.format.width, 16);
        assert!((img.get_at(1, 3, 3) - 0.25).abs() < 1e-3);
        // A frame that doesn't exist is an error naming the node, not a crash.
        let err = Renderer::new(&s, &reg).info(r, FrameTime::from_frame(99)).unwrap_err();
        assert!(err.to_string().contains("Read1"), "{err}");
    }
}
