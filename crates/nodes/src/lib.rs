//! The NukeCraft node registry and the built-in nodes.
//!
//! A node class is a [`NodeSpec`]: its Nuke class name, menu category, inputs, knobs and three
//! functions that mirror how a Nuke operator works:
//!
//! - **info** (Nuke's validate): from the inputs' info, the output's format, bbox, channels and
//!   frame range;
//! - **request**: the part of an input needed to produce a region of the output;
//! - **engine**: the pixels of a region, from the inputs' pixels.
//!
//! The renderer (`nukecraft-render`) walks the graph and calls these; nodes never see the graph.
//! Nodes that touch files (Read, Write) are registered by `nukecraft-io`.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod blur;
mod channel_nodes;
mod color_math;
mod color_nodes;
mod draw;
pub mod filter;
mod image_nodes;
pub mod knobs;
mod merge;
mod other_nodes;
mod reformat;
mod time_nodes;
mod transform;

use nukecraft_geom::{Format, IRect};
use nukecraft_graph::Root;
use nukecraft_raster::{ChannelSet, Image, RasterError};
use nukecraft_time::{FrameRange, FrameTime};

pub use knobs::{KnobDefault, KnobKind, KnobSpec, Knobs};

/// What a node outputs, before any pixel is computed.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageInfo {
    /// The display window.
    pub format: Format,
    /// The data window: where the node has pixels. Outside it the edge pixels repeat.
    pub bbox: IRect,
    pub channels: ChannelSet,
    pub frames: FrameRange,
}

/// A rendered input: its info and the pixels of the region requested from it.
#[derive(Clone, Debug)]
pub struct Input {
    pub info: ImageInfo,
    pub image: Image,
}

/// What a node's functions see.
pub struct Ctx<'a> {
    pub knobs: &'a Knobs,
    pub root: &'a Root,
    pub time: FrameTime,
    pub node_name: &'a str,
}

pub type InfoFn = fn(&Ctx, &[Option<ImageInfo>]) -> Result<ImageInfo>;
/// `(ctx, own info, input infos, output region, input index)` → the region of that input needed.
pub type RequestFn = fn(&Ctx, &ImageInfo, &[Option<ImageInfo>], IRect, usize) -> IRect;
/// `(ctx, own info, inputs, output region)` → pixels covering the region.
pub type EngineFn = fn(&Ctx, &ImageInfo, &[Option<Input>], IRect) -> Result<Image>;
/// `(ctx at the node's time, input index, that input's frame range)` → the time the input is
/// read at (time nodes: FrameHold, TimeOffset).
pub type InputTimeFn = fn(&Ctx, usize, FrameRange) -> FrameTime;

/// One input pipe.
#[derive(Clone, Copy, Debug)]
pub struct InputSpec {
    /// The label on the pipe (`B`, `A`, `mask`, `Source`).
    pub name: &'static str,
    /// A mask input: unconnected means "no mask" rather than black.
    pub mask: bool,
}

/// A node class.
pub struct NodeSpec {
    /// Nuke's class name (`Merge2`).
    pub class: &'static str,
    /// Other names that create this class (`Merge`).
    pub aliases: &'static [&'static str],
    /// The name in menus (`Merge`).
    pub label: &'static str,
    /// The Nodes toolbar menu (`Image`, `Color`, `Filter`, `Merge`, `Transform`, `Other`).
    pub category: &'static str,
    /// The node graph keyboard shortcut that creates it.
    pub shortcut: Option<&'static str>,
    pub help: &'static str,
    pub inputs: &'static [InputSpec],
    /// Accepts any number of further inputs after `inputs` (Switch).
    pub extra_inputs: bool,
    pub knobs: &'static [KnobSpec],
    pub info: InfoFn,
    pub request: RequestFn,
    pub engine: EngineFn,
    /// When the node reads its inputs at other times than its own; `None` reads them at the
    /// node's time.
    pub input_time: Option<InputTimeFn>,
}

/// The node classes a session knows.
pub struct Registry {
    specs: Vec<NodeSpec>,
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum NodeError {
    #[error(transparent)]
    Raster(#[from] RasterError),
    #[error("{node}: {message}")]
    Knob { node: String, message: String },
    #[error("{node}: {message}")]
    Unsupported { node: String, message: String },
    #[error("{node}: {message}")]
    Io { node: String, message: String },
    #[error("node class `{0}` is already registered")]
    Duplicate(String),
}

pub type Result<T> = std::result::Result<T, NodeError>;

impl Ctx<'_> {
    pub fn knob_error(&self, message: impl Into<String>) -> NodeError {
        NodeError::Knob { node: self.node_name.into(), message: message.into() }
    }

    pub fn unsupported(&self, message: impl Into<String>) -> NodeError {
        NodeError::Unsupported { node: self.node_name.into(), message: message.into() }
    }

    pub fn io_error(&self, message: impl Into<String>) -> NodeError {
        NodeError::Io { node: self.node_name.into(), message: message.into() }
    }

    /// A format knob's value, or the root format when it's empty or unreadable.
    pub fn format_knob(&self, name: &str) -> Format {
        let text = self.knobs.text(name);
        if text.trim().is_empty() { self.root.format.clone() } else { Format::parse(text).unwrap_or_else(|| self.root.format.clone()) }
    }

    /// The channels a `channels` knob selects among `available`; an unknown layer reads as none.
    pub fn channels_knob(&self, name: &str, available: &ChannelSet) -> ChannelSet {
        ChannelSet::parse(self.knobs.text(name), available).unwrap_or_default().intersect(available)
    }
}

impl ImageInfo {
    /// What an unconnected input looks like: black, no data, the root format.
    pub fn black(root: &Root) -> ImageInfo {
        ImageInfo { format: root.format.clone(), bbox: IRect::EMPTY, channels: ChannelSet::rgba(), frames: root.frames }
    }
}

/// The info of input `i`, or black for an unconnected input.
pub fn input_info(ctx: &Ctx, inputs: &[Option<ImageInfo>], i: usize) -> ImageInfo {
    inputs.get(i).cloned().flatten().unwrap_or_else(|| ImageInfo::black(ctx.root))
}

/// Input `i`'s pixels, or an empty black image for an unconnected input.
pub fn input_image(inputs: &[Option<Input>], i: usize) -> Option<&Input> {
    inputs.get(i).and_then(Option::as_ref)
}

/// Request function for nodes whose output pixel `(x, y)` reads only input pixel `(x, y)`.
pub fn request_same(_: &Ctx, _: &ImageInfo, _: &[Option<ImageInfo>], region: IRect, _: usize) -> IRect {
    region
}

/// Request function for nodes without inputs.
pub fn request_none(_: &Ctx, _: &ImageInfo, _: &[Option<ImageInfo>], _: IRect, _: usize) -> IRect {
    IRect::EMPTY
}

/// Info function for nodes that output their first input unchanged.
pub fn info_passthrough(ctx: &Ctx, inputs: &[Option<ImageInfo>]) -> Result<ImageInfo> {
    Ok(input_info(ctx, inputs, 0))
}

/// Engine function for nodes that output their first input unchanged.
pub fn engine_passthrough(_: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    match input_image(inputs, 0) {
        Some(inp) if inp.image.bbox() == region => Ok(inp.image.clone()),
        Some(inp) => Ok(inp.image.reframed(region, &info.channels)?),
        None => Ok(Image::new(IRect::EMPTY, info.channels.clone())?),
    }
}

impl Registry {
    /// An empty registry.
    pub fn new() -> Registry {
        Registry { specs: Vec::new() }
    }

    /// Every built-in node of this crate.
    pub fn builtin() -> Registry {
        let mut r = Registry::new();
        for spec in image_nodes::specs()
            .into_iter()
            .chain(color_nodes::specs())
            .chain(color_math::specs())
            .chain(channel_nodes::specs())
            .chain(draw::specs())
            .chain(merge::specs())
            .chain(transform::specs())
            .chain(reformat::specs())
            .chain(blur::specs())
            .chain(time_nodes::specs())
            .chain(other_nodes::specs())
        {
            // Built-in class names are distinct; a duplicate here is caught by the tests.
            let _ = r.register(spec);
        }
        r
    }

    /// Adds a node class. Refuses a class or alias that is already taken.
    pub fn register(&mut self, spec: NodeSpec) -> Result<()> {
        for name in std::iter::once(spec.class).chain(spec.aliases.iter().copied()) {
            if self.get(name).is_some() {
                return Err(NodeError::Duplicate(name.into()));
            }
        }
        self.specs.push(spec);
        Ok(())
    }

    /// A class by class name or alias (case-sensitive, as Nuke's are).
    pub fn get(&self, class: &str) -> Option<&NodeSpec> {
        self.specs.iter().find(|s| s.class == class || s.aliases.contains(&class))
    }

    pub fn iter(&self) -> impl Iterator<Item = &NodeSpec> {
        self.specs.iter()
    }
}

impl Default for Registry {
    fn default() -> Registry {
        Registry::builtin()
    }
}

impl NodeSpec {
    pub fn knob(&self, name: &str) -> Option<&KnobSpec> {
        self.knobs.iter().find(|k| k.name == name)
    }

    /// The most inputs this class accepts.
    pub fn max_inputs(&self) -> usize {
        if self.extra_inputs { nukecraft_graph::MAX_INPUTS } else { self.inputs.len() }
    }
}

/// The mask of a node over `region`, read from its mask input `index` through the
/// `maskChannelInput` and `invert_mask` knobs: one factor in 0..1 per pixel (rows bottom-up).
/// `None` means no mask: the input is unconnected or the channel is `none`. A channel the mask
/// image lacks reads as black.
pub fn mask_factors(ctx: &Ctx, inputs: &[Option<Input>], index: usize, region: IRect) -> Option<Vec<f32>> {
    mask_factors_by(ctx, inputs, index, region, "maskChannelInput", "invert_mask")
}

/// [`mask_factors`] with other knob names for the channel and the inversion (Keymix's
/// `maskChannel` and `invertMask`).
pub fn mask_factors_by(ctx: &Ctx, inputs: &[Option<Input>], index: usize, region: IRect, channel_knob: &str, invert_knob: &str) -> Option<Vec<f32>> {
    let m = input_image(inputs, index)?;
    let chan = ChannelSet::parse(ctx.knobs.text(channel_knob), m.image.channels()).ok()?.iter().next().cloned()?;
    let ci = m.image.channels().index_of(&chan);
    let invert = ctx.knobs.boolean(invert_knob);
    let mut out = Vec::with_capacity(region.area() as usize);
    for y in region.y0..region.y1 {
        for x in region.x0..region.x1 {
            let v = ci.map_or(0.0, |i| m.image.get_at(i, x, y));
            let v = if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };
            out.push(if invert { 1.0 - v } else { v });
        }
    }
    Some(out)
}

/// `mix` blend of an original and a processed value.
pub fn mix(orig: f32, new: f32, m: f32) -> f32 {
    if m >= 1.0 {
        new
    } else if m <= 0.0 {
        orig
    } else {
        orig + (new - orig) * m
    }
}

/// The RGBA component index of a channel for per-channel colour knobs (others use red's).
pub fn rgba_index(c: &nukecraft_raster::Channel) -> usize {
    match c.name() {
        "rgba.green" => 1,
        "rgba.blue" => 2,
        "rgba.alpha" => 3,
        _ => 0,
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::*;
    use nukecraft_graph::Value;

    pub fn root() -> Root {
        Root { format: Format::new(64, 32, 1.0), ..Root::default() }
    }

    /// Runs one node on inputs (already rendered over their whole bbox), over `region`
    /// (default: its bbox clamped into the format).
    pub fn run(class: &str, knobs: Vec<(&'static str, Value)>, inputs: Vec<Option<Image>>, region: Option<IRect>) -> (ImageInfo, Image) {
        let reg = Registry::builtin();
        let spec = reg.get(class).unwrap();
        let root = root();
        let mut stored = std::collections::BTreeMap::new();
        for (k, v) in knobs {
            let kv = match v {
                Value::Bool(b) => nukecraft_graph::KnobValue::Bool(b),
                Value::Numbers(n) => nukecraft_graph::KnobValue::numbers(&n),
                Value::Text(t) => nukecraft_graph::KnobValue::text(t),
            };
            stored.insert(k.to_string(), kv);
        }
        let knobs = Knobs::evaluate(spec.knobs, &stored, FrameTime::ZERO, &root.format);
        let ctx = Ctx { knobs: &knobs, root: &root, time: FrameTime::ZERO, node_name: "Test1" };
        let infos: Vec<Option<ImageInfo>> = inputs
            .iter()
            .map(|i| i.as_ref().map(|img| ImageInfo { format: root.format.clone(), bbox: img.bbox(), channels: img.channels().clone(), frames: root.frames }))
            .collect();
        let info = (spec.info)(&ctx, &infos).unwrap();
        let region = region.unwrap_or(info.bbox).clamp_into(info.bbox);
        let ins: Vec<Option<Input>> =
            inputs.into_iter().zip(infos.iter()).map(|(img, inf)| img.map(|image| Input { info: inf.clone().unwrap(), image })).collect();
        let img = (spec.engine)(&ctx, &info, &ins, region).unwrap();
        (info, img)
    }

    pub fn solid(bbox: IRect, rgba: [f32; 4]) -> Image {
        Image::filled(bbox, ChannelSet::rgba(), &rgba).unwrap()
    }

    pub fn num(v: f64) -> Value {
        Value::Numbers(vec![v])
    }

    pub fn nums(v: &[f64]) -> Value {
        Value::Numbers(v.to_vec())
    }

    pub fn text(t: &str) -> Value {
        Value::Text(t.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_classes_are_distinct_and_complete() {
        let r = Registry::builtin();
        let n = image_nodes::specs().len()
            + color_nodes::specs().len()
            + color_math::specs().len()
            + channel_nodes::specs().len()
            + draw::specs().len()
            + merge::specs().len()
            + transform::specs().len()
            + reformat::specs().len()
            + blur::specs().len()
            + time_nodes::specs().len()
            + other_nodes::specs().len();
        assert_eq!(r.iter().count(), n, "a built-in class name or alias collides");
        assert!(r.get("Merge").is_some_and(|s| s.class == "Merge2"));
        for spec in r.iter() {
            let mut names: Vec<&str> = spec.knobs.iter().map(|k| k.name).collect();
            names.sort_unstable();
            names.dedup();
            assert_eq!(names.len(), spec.knobs.len(), "{}: duplicate knob", spec.class);
            for k in spec.knobs {
                let v = k.default_value(&Format::default());
                assert!(k.validate(v).is_ok(), "{}.{}: default fails its own validation", spec.class, k.name);
            }
        }
    }

    #[test]
    fn ui_text_is_plain_ascii() {
        // Labels and help show in the UI; its fonts lack many symbols (they draw as boxes).
        for spec in Registry::builtin().iter() {
            for text in [spec.label, spec.help].into_iter().chain(spec.knobs.iter().flat_map(|k| [k.label, k.help])) {
                assert!(text.is_ascii(), "{}: non-ASCII UI text {text:?}", spec.class);
            }
        }
    }

    #[test]
    fn register_refuses_duplicates() {
        let mut r = Registry::builtin();
        let spec = NodeSpec {
            class: "Blur",
            aliases: &[],
            label: "Blur",
            category: "Filter",
            shortcut: None,
            help: "",
            inputs: &[],
            extra_inputs: false,
            knobs: &[],
            info: info_passthrough,
            request: request_same,
            engine: engine_passthrough,
            input_time: None,
        };
        assert_eq!(r.register(spec).unwrap_err(), NodeError::Duplicate("Blur".into()));
    }
}
