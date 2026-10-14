//! Pull-based evaluation of a NukeCraft script.
//!
//! Rendering a node over a region works the way Nuke's does: ask the node what it outputs (its
//! info: format, bbox, channels), clamp the region into its bbox, ask it which region of each
//! input it needs, render those inputs (recursively), then run its engine. Within one
//! [`Renderer`] every `(node, time)` info and every `(node, time, region)` result is computed
//! once, so diamonds in the graph are not rendered twice.
//!
//! Walks are depth-limited and cycle-checked even though the graph refuses cycles: a script is
//! untrusted input.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::collections::HashMap;

use nukecraft_color::Colorspace;
use nukecraft_geom::{Format, IRect};
use nukecraft_graph::{Node, NodeId, Script};
use nukecraft_nodes::{Ctx, ImageInfo, Input, Knobs, NodeError, NodeSpec, Registry};
use nukecraft_raster::{Channel, ChannelSet, Image, RasterError};
use nukecraft_time::FrameTime;

/// The deepest chain of nodes a render may walk.
pub const MAX_DEPTH: usize = 512;

/// The most cached results one renderer keeps before it starts over.
const MAX_CACHED: usize = 4096;

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum RenderError {
    #[error(transparent)]
    Node(#[from] NodeError),
    #[error(transparent)]
    Raster(#[from] RasterError),
    #[error("node `{node}` has unknown class `{class}`")]
    UnknownClass { node: String, class: String },
    #[error("node `{0}` is part of a cycle")]
    Cycle(String),
    #[error("the graph above `{0}` is deeper than {MAX_DEPTH} nodes")]
    TooDeep(String),
    #[error("no node with id {0}")]
    NoSuchNode(u64),
}

pub type Result<T> = std::result::Result<T, RenderError>;

/// Evaluates nodes of one script.
pub struct Renderer<'a> {
    script: &'a Script,
    registry: &'a Registry,
    infos: HashMap<(NodeId, FrameTime), ImageInfo>,
    images: HashMap<(NodeId, FrameTime, IRect), Image>,
    stack: Vec<NodeId>,
}

impl<'a> Renderer<'a> {
    pub fn new(script: &'a Script, registry: &'a Registry) -> Renderer<'a> {
        Renderer { script, registry, infos: HashMap::new(), images: HashMap::new(), stack: Vec::new() }
    }

    /// What `id` outputs at `t`.
    pub fn info(&mut self, id: NodeId, t: FrameTime) -> Result<ImageInfo> {
        if let Some(info) = self.infos.get(&(id, t)) {
            return Ok(info.clone());
        }
        let node = self.enter(id)?;
        let result = self.info_uncached(node, t);
        self.stack.pop();
        let info = result?;
        if self.infos.len() >= MAX_CACHED {
            self.infos.clear();
        }
        self.infos.insert((id, t), info.clone());
        Ok(info)
    }

    /// The pixels of `id` at `t` over `region` clamped into its bbox (see
    /// [`IRect::clamp_into`]): reads outside the returned image repeat its edges, which is
    /// exactly what reading outside the node's bbox means.
    pub fn render(&mut self, id: NodeId, t: FrameTime, region: IRect) -> Result<Image> {
        let info = self.info(id, t)?;
        let region = region.clamp_into(info.bbox);
        if region.is_empty() {
            return Ok(Image::new(IRect::EMPTY, info.channels)?);
        }
        if let Some(img) = self.images.get(&(id, t, region)) {
            return Ok(img.clone());
        }
        let node = self.enter(id)?;
        let result = self.render_uncached(node, &info, t, region);
        self.stack.pop();
        let img = result?;
        if self.images.len() >= MAX_CACHED {
            self.images.clear();
        }
        self.images.insert((id, t, region), img.clone());
        Ok(img)
    }

    /// The pixels of `id` over its format (what a Viewer or Write shows).
    pub fn render_format(&mut self, id: NodeId, t: FrameTime) -> Result<(ImageInfo, Image)> {
        let info = self.info(id, t)?;
        let img = self.render(id, t, info.format.rect())?;
        Ok((info, img))
    }

    /// Pushes `id` on the walk stack, refusing cycles and runaway depth.
    fn enter(&mut self, id: NodeId) -> Result<&'a Node> {
        let node = self.script.node(id).ok_or(RenderError::NoSuchNode(id.0))?;
        if self.stack.contains(&id) {
            return Err(RenderError::Cycle(node.name.clone()));
        }
        if self.stack.len() >= MAX_DEPTH {
            return Err(RenderError::TooDeep(node.name.clone()));
        }
        self.stack.push(id);
        Ok(node)
    }

    fn spec(&self, node: &Node) -> Result<&'a NodeSpec> {
        self.registry.get(&node.class).ok_or_else(|| RenderError::UnknownClass { node: node.name.clone(), class: node.class.clone() })
    }

    /// The input slots the node's class reads.
    fn input_ids(node: &Node, spec: &NodeSpec) -> Vec<Option<NodeId>> {
        let n = if spec.extra_inputs { node.inputs.len().max(spec.inputs.len()) } else { spec.inputs.len() };
        (0..n.min(spec.max_inputs())).map(|i| node.input(i)).collect()
    }

    fn input_infos(&mut self, ids: &[Option<NodeId>], t: FrameTime) -> Result<Vec<Option<ImageInfo>>> {
        ids.iter().map(|id| id.map(|id| self.info(id, t)).transpose()).collect()
    }

    fn info_uncached(&mut self, node: &'a Node, t: FrameTime) -> Result<ImageInfo> {
        let spec = self.spec(node)?;
        let ids = Self::input_ids(node, spec);
        if node.disabled {
            return match ids.first().copied().flatten() {
                Some(first) => self.info(first, t),
                None => Ok(ImageInfo::black(&self.script.root)),
            };
        }
        let infos = self.input_infos(&ids, t)?;
        let knobs = self.knobs(node, spec, t);
        let ctx = Ctx { knobs: &knobs, root: &self.script.root, time: t, node_name: &node.name };
        Ok((spec.info)(&ctx, &infos)?)
    }

    fn render_uncached(&mut self, node: &'a Node, info: &ImageInfo, t: FrameTime, region: IRect) -> Result<Image> {
        let spec = self.spec(node)?;
        let ids = Self::input_ids(node, spec);
        if node.disabled {
            return match ids.first().copied().flatten() {
                Some(first) => self.render(first, t, region),
                None => Ok(Image::new(IRect::EMPTY, info.channels.clone())?),
            };
        }
        let infos = self.input_infos(&ids, t)?;
        let knobs = self.knobs(node, spec, t);
        let ctx = Ctx { knobs: &knobs, root: &self.script.root, time: t, node_name: &node.name };
        let mut inputs = Vec::with_capacity(ids.len());
        for (i, (id, inf)) in ids.iter().zip(&infos).enumerate() {
            let (Some(id), Some(inf)) = (id, inf) else {
                inputs.push(None);
                continue;
            };
            let want = (spec.request)(&ctx, info, &infos, region, i);
            let image = self.render(*id, t, want)?;
            inputs.push(Some(Input { info: inf.clone(), image }));
        }
        let img = (spec.engine)(&ctx, info, &inputs, region)?;
        // An engine must cover exactly the region it was asked for.
        if img.bbox() != region || img.channels() != &info.channels {
            return Ok(img.reframed(region, &info.channels)?);
        }
        Ok(img)
    }

    fn knobs(&self, node: &Node, spec: &NodeSpec, t: FrameTime) -> Knobs {
        Knobs::evaluate(spec.knobs, &node.knobs, t, &self.script.root.format)
    }
}

/// Stack size for render threads: deep graphs recurse once per node.
pub const RENDER_STACK: usize = 64 * 1024 * 1024;

/// Runs `f` (a render) on a thread with [`RENDER_STACK`] bytes of stack, so the deepest graph
/// [`MAX_DEPTH`] allows cannot overflow it. A panic inside `f` comes back as `Err` with its
/// message instead of unwinding into the caller. On targets without threads `f` runs inline.
pub fn with_render_stack<T: Send>(f: impl FnOnce() -> T + Send) -> std::result::Result<T, String> {
    if cfg!(target_arch = "wasm32") {
        return Ok(f());
    }
    std::thread::scope(|scope| {
        let handle = std::thread::Builder::new()
            .name("render".into())
            .stack_size(RENDER_STACK)
            .spawn_scoped(scope, f)
            .map_err(|e| format!("could not start a render thread: {e}"))?;
        handle.join().map_err(|p| {
            let msg = p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "unknown panic".into());
            format!("internal error during render: {msg}")
        })
    })
}

/// An image over `format` as 8-bit RGBA, rows top-down (the order image files and screens
/// use), encoded with `colorspace`. Outside the bbox the pixels are black. Alpha is written
/// as stored (premultiplied colour, unless the caller unpremultiplied first).
pub fn to_rgba8(img: &Image, format: &Format, colorspace: Colorspace) -> Vec<u8> {
    let (w, h) = (format.width as usize, format.height as usize);
    let mut out = vec![0u8; w.saturating_mul(h).saturating_mul(4)];
    let idx: Vec<Option<usize>> = ChannelSet::rgba().iter().map(|c| img.channels().index_of(c)).collect();
    let alpha_only = idx.first().copied().flatten().is_none() && idx.get(3).copied().flatten().is_some();
    for row in 0..h {
        let y = (h - 1 - row) as i32;
        for x in 0..w {
            for (c, i) in idx.iter().enumerate() {
                let v = match i {
                    Some(i) => img.get_or_zero_at(*i, x as i32, y),
                    // An alpha-only image shows its alpha as grey; a missing alpha is opaque.
                    None if alpha_only && c < 3 => idx.get(3).copied().flatten().map_or(0.0, |a| img.get_or_zero_at(a, x as i32, y)),
                    None if c == 3 => 1.0,
                    None => 0.0,
                };
                let v = if c == 3 { v } else { colorspace.from_linear(v) };
                if let Some(px) = out.get_mut((row * w + x) * 4 + c) {
                    *px = quantize8(v);
                }
            }
        }
    }
    out
}

/// 0‥1 to 0‥255 with rounding; NaN is 0.
pub fn quantize8(v: f32) -> u8 {
    if v.is_nan() { 0 } else { (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8 }
}

/// The value of every channel at one pixel, for inspection (`query.pixel`).
pub fn pixel(img: &Image, x: i32, y: i32) -> Vec<(Channel, f32)> {
    img.channels().iter().enumerate().map(|(i, c)| (c.clone(), img.get_or_zero_at(i, x, y))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nukecraft_graph::KnobValue;

    fn script() -> (Script, NodeId, NodeId, NodeId) {
        let mut s = Script::new();
        s.root.format = Format::new(32, 16, 1.0);
        let c = s.add_node("Constant", None).unwrap();
        s.node_mut(c).unwrap().knobs.insert("color".into(), KnobValue::numbers(&[1.0, 0.5, 0.25, 1.0]));
        let g = s.add_node("Grade", None).unwrap();
        s.node_mut(g).unwrap().knobs.insert("white".into(), KnobValue::number(2.0));
        s.connect(g, 0, c).unwrap();
        let m = s.add_node("Merge2", None).unwrap();
        s.connect(m, 0, c).unwrap();
        s.connect(m, 1, g).unwrap();
        s.node_mut(m).unwrap().knobs.insert("operation".into(), KnobValue::text("plus"));
        (s, c, g, m)
    }

    #[test]
    fn renders_a_diamond() {
        let (s, _, _, m) = script();
        let reg = Registry::builtin();
        let mut r = Renderer::new(&s, &reg);
        let (info, img) = r.render_format(m, FrameTime::ZERO).unwrap();
        assert_eq!(info.bbox, IRect::new(0, 0, 32, 16));
        // B (1.0) plus A (graded: 2.0).
        assert_eq!(img.get_at(0, 5, 5), 3.0);
    }

    #[test]
    fn regions_clamp_into_the_bbox() {
        let (s, c, ..) = script();
        let reg = Registry::builtin();
        let mut r = Renderer::new(&s, &reg);
        let img = r.render(c, FrameTime::ZERO, IRect::new(100, 100, 120, 120)).unwrap();
        assert_eq!(img.bbox(), IRect::new(31, 15, 32, 16), "the nearest corner pixel");
        assert_eq!(img.get_at(0, 110, 110), 1.0);
    }

    #[test]
    fn disabled_nodes_pass_through() {
        let (mut s, _, g, _) = script();
        s.node_mut(g).unwrap().disabled = true;
        let reg = Registry::builtin();
        let mut r = Renderer::new(&s, &reg);
        let (_, img) = r.render_format(g, FrameTime::ZERO).unwrap();
        assert_eq!(img.get_at(0, 0, 0), 1.0);
    }

    #[test]
    fn unknown_classes_are_errors() {
        let mut s = Script::new();
        let n = s.add_node("NoSuchNode", None).unwrap();
        let reg = Registry::builtin();
        let err = Renderer::new(&s, &reg).info(n, FrameTime::ZERO).unwrap_err();
        assert!(matches!(err, RenderError::UnknownClass { .. }));
    }

    #[test]
    fn deep_chains_stop_with_an_error() {
        let mut s = Script::new();
        s.root.format = Format::new(4, 4, 1.0);
        let mut prev = s.add_node("Constant", None).unwrap();
        for _ in 0..(MAX_DEPTH + 10) {
            let d = s.add_node("Dot", None).unwrap();
            s.connect(d, 0, prev).unwrap();
            prev = d;
        }
        let reg = Registry::builtin();
        let err = Renderer::new(&s, &reg).info(prev, FrameTime::ZERO).unwrap_err();
        assert!(matches!(err, RenderError::TooDeep(_)));
    }

    #[test]
    fn the_deepest_allowed_chain_renders_on_the_render_stack() {
        let mut s = Script::new();
        s.root.format = Format::new(4, 4, 1.0);
        let mut prev = s.add_node("Constant", None).unwrap();
        s.node_mut(prev).unwrap().knobs.insert("color".into(), KnobValue::number(0.5));
        for _ in 0..(MAX_DEPTH - 1) {
            let d = s.add_node("Grade", None).unwrap();
            s.connect(d, 0, prev).unwrap();
            prev = d;
        }
        let reg = Registry::builtin();
        let img = with_render_stack(|| Renderer::new(&s, &reg).render_format(prev, FrameTime::ZERO)).unwrap().unwrap().1;
        assert_eq!(img.get_at(0, 0, 0), 0.5);
    }

    #[test]
    fn display_conversion_flips_and_encodes() {
        let mut img = Image::new(IRect::new(0, 0, 2, 2), ChannelSet::rgba()).unwrap();
        img.set_at(0, 0, 1, 1.0); // top-left in y-up terms
        img.set_at(3, 0, 1, 1.0);
        let px = to_rgba8(&img, &Format::new(2, 2, 1.0), Colorspace::Srgb);
        assert_eq!(&px[0..4], &[255, 0, 0, 255], "first row written is the top row");
        assert_eq!(&px[8..12], &[0, 0, 0, 0]);
        assert_eq!(quantize8(f32::NAN), 0);
    }
}
