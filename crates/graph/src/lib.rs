//! The NukeCraft script: a directed acyclic graph of nodes, their knobs and the root settings.
//!
//! The graph knows nothing about what a node *does*; that lives in `nukecraft-nodes`. Nodes are
//! addressed by a stable [`NodeId`] (connections use ids, so renaming never breaks a pipe) and by
//! a unique name (`Blur1`), which is what people and agents type.
//!
//! A script file (`.ncs`) is the script as readable JSON headed by `savedBy` and `schema`; see
//! `docs/script-format.md`. Loading is lenient: dangling inputs, duplicate names and connections
//! that would close a cycle are repaired and reported, never trusted.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::collections::{BTreeMap, HashMap, HashSet};

use nukecraft_curve::{Curve, Interp, Key};
use nukecraft_geom::Format;
use nukecraft_time::{Fps, FrameRange, FrameTime};
use serde::{Deserialize, Serialize};

/// The schema this version writes. Files with a newer schema are refused rather than misread.
pub const SCHEMA: u32 = 1;

/// The NukeCraft version, written into `savedBy`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The most nodes a script may hold.
pub const MAX_NODES: usize = 100_000;

/// The most inputs a node may have.
pub const MAX_INPUTS: usize = 256;

/// The most components a knob may have.
pub const MAX_COMPONENTS: usize = 64;

/// The longest text a knob may hold, in bytes.
pub const MAX_TEXT: usize = 64 * 1024;

/// A node's stable identity within a script.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub u64);

/// One animatable number: a constant or a curve.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Anim {
    Static(f64),
    Curve { curve: Curve },
}

/// A stored knob value. Numbers may be one component or several (`translate`, `color`), each
/// animated on its own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum KnobValue {
    Bool(bool),
    Number(Anim),
    Vector(Vec<Anim>),
    Text(String),
}

/// A knob value evaluated at one time.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Numbers(Vec<f64>),
    Text(String),
}

/// Script-wide settings (Nuke's Root node).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Root {
    #[serde(default)]
    pub format: Format,
    #[serde(default)]
    pub fps: Fps,
    #[serde(default)]
    pub frames: FrameRange,
}

/// A node in the graph.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub name: String,
    pub class: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub knobs: BTreeMap<String, KnobValue>,
    /// Input `i` connected to a node, or `None` when the pipe is empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<Option<NodeId>>,
    /// Position in the node graph (DAG) panel.
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// The script.
#[derive(Clone, Debug, PartialEq)]
pub struct Script {
    pub root: Root,
    nodes: Vec<Node>,
    next_id: u64,
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum GraphError {
    #[error("no node named `{0}`")]
    NoSuchNode(String),
    #[error("no node with id {0}")]
    NoSuchId(u64),
    #[error("a node named `{0}` already exists")]
    NameTaken(String),
    #[error("`{0}` is not a valid node name (letters, digits and _, not starting with a digit)")]
    BadName(String),
    #[error("connecting {from} into {node} would make a cycle")]
    Cycle { node: String, from: String },
    #[error("input {0} is out of range (at most {MAX_INPUTS} inputs)")]
    BadInput(usize),
    #[error("a script holds at most {MAX_NODES} nodes")]
    TooManyNodes,
    #[error("{0}")]
    BadValue(String),
    #[error("this script was saved by a newer NukeCraft (schema {0}, this version reads {SCHEMA})")]
    NewerSchema(u32),
    #[error("script file: {0}")]
    Parse(String),
}

pub type Result<T> = std::result::Result<T, GraphError>;

/// The on-disk form.
#[derive(Serialize, Deserialize)]
struct FileDoc {
    #[serde(rename = "savedBy", default)]
    saved_by: String,
    schema: u32,
    #[serde(default)]
    root: Root,
    #[serde(default)]
    nodes: Vec<Node>,
}

impl Anim {
    pub fn eval(&self, t: FrameTime) -> f64 {
        match self {
            Anim::Static(v) => *v,
            Anim::Curve { curve } => curve.eval(t),
        }
    }

    pub fn is_animated(&self) -> bool {
        matches!(self, Anim::Curve { .. })
    }
}

impl KnobValue {
    /// A single number.
    pub fn number(v: f64) -> KnobValue {
        KnobValue::Number(Anim::Static(v))
    }

    /// Several numbers.
    pub fn numbers(vs: &[f64]) -> KnobValue {
        KnobValue::Vector(vs.iter().map(|&v| Anim::Static(v)).collect())
    }

    pub fn text(s: impl Into<String>) -> KnobValue {
        KnobValue::Text(s.into())
    }

    pub fn eval(&self, t: FrameTime) -> Value {
        match self {
            KnobValue::Bool(b) => Value::Bool(*b),
            KnobValue::Number(a) => Value::Numbers(vec![a.eval(t)]),
            KnobValue::Vector(v) => Value::Numbers(v.iter().map(|a| a.eval(t)).collect()),
            KnobValue::Text(s) => Value::Text(s.clone()),
        }
    }

    pub fn is_animated(&self) -> bool {
        match self {
            KnobValue::Number(a) => a.is_animated(),
            KnobValue::Vector(v) => v.iter().any(Anim::is_animated),
            _ => false,
        }
    }

    /// The numeric components, if the value is numeric.
    pub fn components(&self) -> Option<Vec<&Anim>> {
        match self {
            KnobValue::Number(a) => Some(vec![a]),
            KnobValue::Vector(v) => Some(v.iter().collect()),
            _ => None,
        }
    }

    /// Sets numeric components from `values`. With `at = Some(t)` each component gets a key at
    /// `t` (a static component becomes a curve with that one key, as in Nuke); with
    /// `None` the components become static. The value's arity follows `values`.
    pub fn set_numbers(&mut self, values: &[f64], at: Option<(FrameTime, Interp)>) -> Result<()> {
        if values.is_empty() || values.len() > MAX_COMPONENTS {
            return Err(GraphError::BadValue(format!("a numeric knob takes 1 to {MAX_COMPONENTS} numbers")));
        }
        if let Some(v) = values.iter().find(|v| !v.is_finite()) {
            return Err(GraphError::BadValue(format!("{v} is not a finite number")));
        }
        let old: Vec<Anim> = match self {
            KnobValue::Number(a) => vec![a.clone()],
            KnobValue::Vector(v) => v.clone(),
            _ => Vec::new(),
        };
        let mut out = Vec::with_capacity(values.len());
        for (i, &v) in values.iter().enumerate() {
            let prev = old.get(i).cloned().or_else(|| old.last().cloned());
            let anim = match at {
                None => Anim::Static(v),
                Some((t, interp)) => {
                    let mut curve = match prev {
                        Some(Anim::Curve { curve }) => curve,
                        _ => Curve::new(),
                    };
                    curve.set_key(Key { time: t, value: v, interp }).map_err(GraphError::BadValue)?;
                    Anim::Curve { curve }
                }
            };
            out.push(anim);
        }
        *self = if out.len() == 1 { KnobValue::Number(out.remove(0)) } else { KnobValue::Vector(out) };
        Ok(())
    }

    /// Removes the key at `t` from every component; a curve left without keys becomes the
    /// constant it last held. Returns whether any key was removed.
    pub fn remove_key(&mut self, t: FrameTime) -> bool {
        let fix = |a: &mut Anim| -> bool {
            let Anim::Curve { curve } = a else { return false };
            let held = curve.eval(t);
            let removed = curve.remove_key(t);
            if curve.is_empty() {
                *a = Anim::Static(held);
            }
            removed
        };
        match self {
            KnobValue::Number(a) => fix(a),
            KnobValue::Vector(v) => v.iter_mut().map(fix).fold(false, |x, y| x | y),
            _ => false,
        }
    }

    /// Replaces animation with the values at `t`.
    pub fn clear_animation(&mut self, t: FrameTime) {
        match self {
            KnobValue::Number(a) => *a = Anim::Static(a.eval(t)),
            KnobValue::Vector(v) => v.iter_mut().for_each(|a| *a = Anim::Static(a.eval(t))),
            _ => {}
        }
    }

    /// Whether every number in the value is finite and texts are within [`MAX_TEXT`].
    fn is_valid(&self) -> bool {
        let anim_ok = |a: &Anim| match a {
            Anim::Static(v) => v.is_finite(),
            Anim::Curve { curve } => curve.keys().iter().all(|k| k.value.is_finite()),
        };
        match self {
            KnobValue::Bool(_) => true,
            KnobValue::Number(a) => anim_ok(a),
            KnobValue::Vector(v) => v.len() <= MAX_COMPONENTS && v.iter().all(anim_ok),
            KnobValue::Text(s) => s.len() <= MAX_TEXT,
        }
    }
}

impl Value {
    pub fn as_numbers(&self) -> Option<&[f64]> {
        match self {
            Value::Numbers(v) => Some(v),
            _ => None,
        }
    }
}

impl Node {
    pub fn input(&self, i: usize) -> Option<NodeId> {
        self.inputs.get(i).copied().flatten()
    }
}

/// Whether `name` is a valid node name: ASCII letters, digits and `_`, not starting with a
/// digit, at most 128 bytes.
pub fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else { return false };
    name.len() <= 128 && (first.is_ascii_alphabetic() || first == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl Default for Script {
    fn default() -> Script {
        Script::new()
    }
}

impl Script {
    pub fn new() -> Script {
        Script { root: Root::default(), nodes: Vec::new(), next_id: 1 }
    }

    /// Nodes in creation order.
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    pub fn by_name(&self, name: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.name == name)
    }

    /// The id of a node by name, or an error naming it.
    pub fn id_of(&self, name: &str) -> Result<NodeId> {
        self.by_name(name).map(|n| n.id).ok_or_else(|| GraphError::NoSuchNode(name.into()))
    }

    /// The name of a node by id.
    pub fn name_of(&self, id: NodeId) -> Result<&str> {
        self.node(id).map(|n| n.name.as_str()).ok_or(GraphError::NoSuchId(id.0))
    }

    /// `prefix` followed by the smallest number ≥ 1 that makes an unused name (`Blur1`, `Blur2`…).
    pub fn unique_name(&self, prefix: &str) -> String {
        let taken: HashSet<&str> = self.nodes.iter().map(|n| n.name.as_str()).collect();
        let prefix = if is_valid_name(prefix) { prefix } else { "Node" };
        (1..).map(|i| format!("{prefix}{i}")).find(|n| !taken.contains(n.as_str())).unwrap_or_else(|| format!("{prefix}_"))
    }

    /// Adds a node of `class`, named `name` or `<class>N`. Returns its id.
    pub fn add_node(&mut self, class: &str, name: Option<&str>) -> Result<NodeId> {
        if self.nodes.len() >= MAX_NODES {
            return Err(GraphError::TooManyNodes);
        }
        let name = match name {
            Some(n) => {
                if !is_valid_name(n) {
                    return Err(GraphError::BadName(n.into()));
                }
                if self.by_name(n).is_some() {
                    return Err(GraphError::NameTaken(n.into()));
                }
                n.to_string()
            }
            None => self.unique_name(class),
        };
        let id = NodeId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        self.nodes.push(Node {
            id,
            name,
            class: class.into(),
            knobs: BTreeMap::new(),
            inputs: Vec::new(),
            x: 0.0,
            y: 0.0,
            disabled: false,
            label: String::new(),
        });
        Ok(id)
    }

    /// Removes a node and disconnects everything it fed. Returns the node and the inputs
    /// `(node, input)` it was connected to.
    pub fn remove_node(&mut self, id: NodeId) -> Result<(Node, Vec<(NodeId, usize)>)> {
        let pos = self.nodes.iter().position(|n| n.id == id).ok_or(GraphError::NoSuchId(id.0))?;
        let consumers = self.consumers(id);
        for (c, i) in &consumers {
            if let Some(slot) = self.node_mut(*c).and_then(|n| n.inputs.get_mut(*i)) {
                *slot = None;
            }
        }
        let node = self.nodes.remove(pos);
        Ok((node, consumers))
    }

    pub fn rename(&mut self, id: NodeId, name: &str) -> Result<()> {
        if !is_valid_name(name) {
            return Err(GraphError::BadName(name.into()));
        }
        if self.by_name(name).is_some_and(|n| n.id != id) {
            return Err(GraphError::NameTaken(name.into()));
        }
        let node = self.node_mut(id).ok_or(GraphError::NoSuchId(id.0))?;
        node.name = name.into();
        Ok(())
    }

    /// Every `(node, input)` connected to `id`'s output.
    pub fn consumers(&self, id: NodeId) -> Vec<(NodeId, usize)> {
        let mut out = Vec::new();
        for n in &self.nodes {
            for (i, inp) in n.inputs.iter().enumerate() {
                if *inp == Some(id) {
                    out.push((n.id, i));
                }
            }
        }
        out
    }

    /// Whether `target` is `start` or one of its upstream nodes.
    pub fn is_upstream(&self, start: NodeId, target: NodeId) -> bool {
        let mut stack = vec![start];
        let mut seen = HashSet::new();
        while let Some(id) = stack.pop() {
            if id == target {
                return true;
            }
            if !seen.insert(id) {
                continue;
            }
            if let Some(n) = self.node(id) {
                stack.extend(n.inputs.iter().flatten().copied());
            }
        }
        false
    }

    /// Connects input `input` of `node` to the output of `from`, refusing cycles.
    pub fn connect(&mut self, node: NodeId, input: usize, from: NodeId) -> Result<()> {
        if input >= MAX_INPUTS {
            return Err(GraphError::BadInput(input));
        }
        self.node(from).ok_or(GraphError::NoSuchId(from.0))?;
        if self.is_upstream(from, node) {
            return Err(GraphError::Cycle { node: self.name_of(node)?.into(), from: self.name_of(from)?.into() });
        }
        let n = self.node_mut(node).ok_or(GraphError::NoSuchId(node.0))?;
        if n.inputs.len() <= input {
            n.inputs.resize(input + 1, None);
        }
        if let Some(slot) = n.inputs.get_mut(input) {
            *slot = Some(from);
        }
        Ok(())
    }

    /// Empties input `input` of `node` (trailing empty inputs are dropped).
    pub fn disconnect(&mut self, node: NodeId, input: usize) -> Result<()> {
        let n = self.node_mut(node).ok_or(GraphError::NoSuchId(node.0))?;
        if let Some(slot) = n.inputs.get_mut(input) {
            *slot = None;
        }
        while n.inputs.last() == Some(&None) {
            n.inputs.pop();
        }
        Ok(())
    }

    /// The script as `.ncs` JSON. Checks that the text reads back before returning it.
    pub fn to_json(&self) -> Result<String> {
        let doc = FileDoc { saved_by: format!("NukeCraft {VERSION}"), schema: SCHEMA, root: self.root.clone(), nodes: self.nodes.clone() };
        let text = serde_json::to_string_pretty(&doc).map_err(|e| GraphError::Parse(e.to_string()))?;
        let (back, _) = Script::from_json(&text)?;
        if back.nodes != self.nodes {
            return Err(GraphError::Parse("the script does not read back the same (a value JSON cannot hold?)".into()));
        }
        Ok(text)
    }

    /// Reads `.ncs` JSON. Returns the script and the repairs made while loading.
    pub fn from_json(text: &str) -> Result<(Script, Vec<String>)> {
        let doc: FileDoc = serde_json::from_str(text).map_err(|e| GraphError::Parse(e.to_string()))?;
        if doc.schema > SCHEMA {
            return Err(GraphError::NewerSchema(doc.schema));
        }
        if doc.nodes.len() > MAX_NODES {
            return Err(GraphError::TooManyNodes);
        }
        let mut warnings = Vec::new();
        let mut script = Script { root: doc.root, nodes: Vec::with_capacity(doc.nodes.len()), next_id: 1 };
        script.root.frames = FrameRange::new(script.root.frames.first, script.root.frames.last);
        script.root.format = Format {
            name: script.root.format.name.clone(),
            ..Format::new(script.root.format.width, script.root.format.height, script.root.format.pixel_aspect)
        };
        script.root.fps = script.root.fps.sanitized();

        // Pass 1: nodes without connections; unique ids and names.
        let mut ids = HashSet::new();
        let mut pending: Vec<(NodeId, Vec<Option<NodeId>>)> = Vec::new();
        let mut id_map: HashMap<u64, NodeId> = HashMap::new();
        for mut n in doc.nodes {
            let old_id = n.id.0;
            if ids.contains(&old_id) || old_id == 0 {
                warnings.push(format!("node `{}`: duplicate id {old_id}, given a new one", n.name));
                n.id = NodeId(0);
            }
            if !is_valid_name(&n.name) || script.by_name(&n.name).is_some() {
                let fresh = script.unique_name(if is_valid_name(&n.class) { &n.class } else { "Node" });
                warnings.push(format!("node `{}`: invalid or duplicate name, renamed to `{fresh}`", n.name));
                n.name = fresh;
            }
            if n.inputs.len() > MAX_INPUTS {
                warnings.push(format!("node `{}`: more than {MAX_INPUTS} inputs, extra inputs dropped", n.name));
                n.inputs.truncate(MAX_INPUTS);
            }
            n.knobs.retain(|k, v| {
                let ok = v.is_valid();
                if !ok {
                    warnings.push(format!("node `{}`: knob `{k}` has an invalid value, reset to its default", n.name));
                }
                ok
            });
            if !n.x.is_finite() || !n.y.is_finite() {
                (n.x, n.y) = (0.0, 0.0);
            }
            if n.id.0 != 0 {
                ids.insert(old_id);
                id_map.entry(old_id).or_insert(n.id);
            }
            pending.push((n.id, std::mem::take(&mut n.inputs)));
            script.nodes.push(n);
        }
        script.next_id = ids.iter().max().map_or(1, |m| m.saturating_add(1));
        for (i, n) in script.nodes.iter_mut().enumerate() {
            if n.id.0 == 0 {
                n.id = NodeId(script.next_id);
                script.next_id = script.next_id.saturating_add(1);
                if let Some(p) = pending.get_mut(i) {
                    p.0 = n.id;
                }
            }
        }

        // Pass 2: connections, refusing dangling references and cycles.
        for (id, inputs) in pending {
            for (i, inp) in inputs.into_iter().enumerate() {
                let Some(from) = inp else { continue };
                let name = script.name_of(id).unwrap_or("?").to_string();
                let Some(&from) = id_map.get(&from.0) else {
                    warnings.push(format!("node `{name}`: input {i} pointed at missing node {}, disconnected", from.0));
                    continue;
                };
                if let Err(e) = script.connect(id, i, from) {
                    warnings.push(format!("node `{name}`: input {i} dropped ({e})"));
                }
            }
        }
        Ok((script, warnings))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain() -> (Script, NodeId, NodeId, NodeId) {
        let mut s = Script::new();
        let a = s.add_node("Constant", None).unwrap();
        let b = s.add_node("Blur", None).unwrap();
        let c = s.add_node("Grade", None).unwrap();
        s.connect(b, 0, a).unwrap();
        s.connect(c, 0, b).unwrap();
        (s, a, b, c)
    }

    #[test]
    fn names_are_unique_and_valid() {
        let mut s = Script::new();
        s.add_node("Blur", None).unwrap();
        s.add_node("Blur", None).unwrap();
        assert!(s.by_name("Blur2").is_some());
        assert_eq!(s.add_node("Blur", Some("Blur1")), Err(GraphError::NameTaken("Blur1".into())));
        assert!(matches!(s.add_node("Blur", Some("1bad")), Err(GraphError::BadName(_))));
        assert!(is_valid_name("_x9"));
        assert!(!is_valid_name("a b"));
    }

    #[test]
    fn cycles_are_refused() {
        let (mut s, a, _, c) = chain();
        assert!(matches!(s.connect(a, 0, c), Err(GraphError::Cycle { .. })));
        assert!(matches!(s.connect(a, 0, a), Err(GraphError::Cycle { .. })));
        assert!(s.connect(c, 1, a).is_ok(), "a diamond is fine");
    }

    #[test]
    fn remove_disconnects_consumers() {
        let (mut s, _, b, c) = chain();
        let (_, consumers) = s.remove_node(b).unwrap();
        assert_eq!(consumers, vec![(c, 0)]);
        assert_eq!(s.node(c).unwrap().input(0), None);
    }

    #[test]
    fn knob_values_animate() {
        let mut v = KnobValue::number(1.0);
        v.set_numbers(&[2.0, 3.0], Some((FrameTime::from_frame(10), Interp::Linear))).unwrap();
        v.set_numbers(&[4.0, 5.0], Some((FrameTime::from_frame(20), Interp::Linear))).unwrap();
        assert_eq!(v.eval(FrameTime::from_frame(15)), Value::Numbers(vec![3.0, 4.0]));
        assert!(v.is_animated());
        assert!(v.remove_key(FrameTime::from_frame(20)));
        v.clear_animation(FrameTime::ZERO);
        assert_eq!(v, KnobValue::numbers(&[2.0, 3.0]));
        assert!(v.set_numbers(&[f64::NAN], None).is_err());
        assert!(v.set_numbers(&[], None).is_err());
    }

    #[test]
    fn json_round_trips() {
        let (mut s, a, ..) = chain();
        s.node_mut(a).unwrap().knobs.insert("color".into(), KnobValue::numbers(&[1.0, 0.5, 0.25, 1.0]));
        let mut anim = KnobValue::number(0.0);
        anim.set_numbers(&[3.0], Some((FrameTime::from_frame(1), Interp::CatmullRom))).unwrap();
        s.node_mut(a).unwrap().knobs.insert("mix".into(), anim);
        s.node_mut(a).unwrap().knobs.insert("label".into(), KnobValue::text("hi"));
        s.node_mut(a).unwrap().knobs.insert("invert".into(), KnobValue::Bool(true));
        let text = s.to_json().unwrap();
        assert!(text.contains("\"savedBy\""));
        let (back, warnings) = Script::from_json(&text).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(back.nodes(), s.nodes());
    }

    #[test]
    fn hostile_files_are_repaired_not_trusted() {
        let text = r#"{"schema":1,"nodes":[
            {"id":1,"name":"A","class":"Constant","inputs":[3]},
            {"id":1,"name":"A","class":"Blur","inputs":[1]},
            {"id":3,"name":"bad name","class":"Grade","inputs":[1, 99]},
            {"id":4,"name":"Pos","class":"Dot","x":1e400}
        ]}"#;
        // 1e400 is not valid JSON for f64 in serde_json: the whole file is an error, not a crash.
        assert!(Script::from_json(text).is_err());
        let text = text.replace("1e400", "5");
        let (s, warnings) = Script::from_json(&text).unwrap();
        assert_eq!(s.nodes().len(), 4);
        let names: HashSet<&str> = s.nodes().iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names.len(), 4, "names made unique: {names:?}");
        assert!(warnings.iter().any(|w| w.contains("missing node 99")), "{warnings:?}");
        assert!(warnings.iter().any(|w| w.contains("cycle")), "the A↔Grade loop is broken: {warnings:?}");
        // The result is a DAG.
        for n in s.nodes() {
            for &inp in n.inputs.iter().flatten() {
                assert!(!s.is_upstream(inp, n.id));
            }
        }
    }

    #[test]
    fn newer_schema_is_refused() {
        assert_eq!(Script::from_json(r#"{"schema":99}"#).unwrap_err(), GraphError::NewerSchema(99));
        assert!(Script::from_json("not json").is_err());
        assert!(Script::from_json(r#"{"schema":1,"root":{"format":{"width":0,"height":0,"pixel_aspect":-1}}}"#).is_ok());
    }
}
