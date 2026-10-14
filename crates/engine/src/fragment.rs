//! Copy, cut and paste of nodes: a script fragment as text.
//!
//! A fragment is JSON (`{"nukecraft": "fragment", "version": 1, "nodes": [...]}`) holding each
//! node's class, name, knobs, position, label, disabled state and inputs **by name**. Pasting
//! renames nodes whose names are taken, keeps the pipes between pasted nodes, and connects pipes
//! to nodes outside the fragment when a node of that name exists (as Nuke does when pasting
//! into the script the nodes came from).

use std::collections::{BTreeMap, HashMap};

use nukecraft_graph::{KnobValue, NodeId};
use serde_json::{Value, json};

use crate::params::{self, bad};
use crate::{Result, Session};

/// The most nodes one paste may create.
const MAX_PASTE: usize = 10_000;
/// The largest fragment text read, in bytes.
const MAX_TEXT: usize = 64 * 1024 * 1024;
/// How far a paste without a position lands from the originals, in graph units.
const OFFSET: f64 = 40.0;

/// The fragment text of `ids` (with the contents of any Groups among them), parents first.
/// Each node carries its `path` within the fragment (`Group1.Blur1`) and its `parent`'s.
pub fn fragment(s: &Session, ids: &[NodeId]) -> Result<String> {
    let mut order: Vec<NodeId> = s.script.nodes().iter().filter(|n| ids.contains(&n.id)).map(|n| n.id).collect();
    let tops = order.clone();
    // Then each group's contents, level by level.
    let mut i = 0;
    while let Some(&id) = order.get(i) {
        if order.len() > MAX_PASTE {
            return Err(bad(format!("a copy holds at most {MAX_PASTE} nodes")));
        }
        for c in s.script.children(Some(id)) {
            if !order.contains(&c.id) {
                order.push(c.id);
            }
        }
        i += 1;
    }
    // Paths relative to the copied nodes' own level.
    let rel = |id: NodeId| -> Option<String> {
        let mut parts = Vec::new();
        let mut cur = Some(id);
        while let Some(c) = cur {
            let n = s.script.node(c)?;
            parts.push(n.name.clone());
            if tops.contains(&c) {
                parts.reverse();
                return Some(parts.join("."));
            }
            cur = n.parent;
        }
        None
    };
    let mut nodes = Vec::new();
    for id in &order {
        let Some(n) = s.script.node(*id) else { continue };
        let inputs: Vec<Value> = n.inputs.iter().map(|i| i.and_then(|i| s.script.name_of(i).ok()).map_or(Value::Null, |name| json!(name))).collect();
        let knobs = serde_json::to_value(&n.knobs).map_err(|e| crate::EngineError::Internal(e.to_string()))?;
        let parent = if tops.contains(id) { Value::Null } else { n.parent.and_then(rel).map_or(Value::Null, Value::String) };
        nodes.push(json!({
            "name": n.name, "path": rel(*id), "parent": parent, "class": n.class, "knobs": knobs, "inputs": inputs,
            "x": n.x, "y": n.y, "disabled": n.disabled, "label": n.label,
        }));
    }
    if nodes.is_empty() {
        return Err(bad("no nodes to copy"));
    }
    Ok(json!({ "nukecraft": "fragment", "version": 1, "nodes": nodes }).to_string())
}

pub(crate) fn edit_copy(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = params::nodes(&s.script, p)?;
    Ok(json!({ "text": fragment(s, &ids)? }))
}

pub(crate) fn edit_cut(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = params::nodes(&s.script, p)?;
    let text = fragment(s, &ids)?;
    crate::commands::node_delete(s, p)?;
    Ok(json!({ "text": text }))
}

/// A name like `wanted` that the group `parent` doesn't use: `Blur1` taken → `Blur2`, `Blur3`…
fn free_name(s: &Session, parent: Option<NodeId>, wanted: &str) -> String {
    if s.script.child_by_name(parent, wanted).is_none() {
        return wanted.to_string();
    }
    let base = wanted.trim_end_matches(|c: char| c.is_ascii_digit());
    s.script.unique_name_in(parent, if base.is_empty() { "Node" } else { base })
}

pub(crate) fn edit_paste(s: &mut Session, p: &Value) -> Result<Value> {
    let text = params::str(p, "text")?;
    if text.len() > MAX_TEXT {
        return Err(bad("the fragment is too large"));
    }
    let target = crate::groups::group_param(s, p)?;
    let doc: Value = serde_json::from_str(text).map_err(|_| bad("the clipboard doesn't hold NukeCraft nodes"))?;
    if doc.get("nukecraft").and_then(Value::as_str) != Some("fragment") {
        return Err(bad("the clipboard doesn't hold NukeCraft nodes"));
    }
    let nodes = doc.get("nodes").and_then(Value::as_array).ok_or_else(|| bad("the fragment has no nodes"))?;
    if nodes.is_empty() || nodes.len() > MAX_PASTE {
        return Err(bad(format!("a paste holds 1 to {MAX_PASTE} nodes")));
    }
    let parent_of = |n: &Value| n.get("parent").and_then(Value::as_str).map(str::to_string);
    // Where the copy goes: its top-left top-level node at (x, y), or next to the originals.
    let tops = || nodes.iter().filter(|n| parent_of(n).is_none());
    let xs = tops().filter_map(|n| n.get("x").and_then(Value::as_f64)).fold(f64::INFINITY, f64::min);
    let ys = tops().filter_map(|n| n.get("y").and_then(Value::as_f64)).fold(f64::INFINITY, f64::min);
    let (dx, dy) = match (params::opt_f64(p, "x")?, params::opt_f64(p, "y")?) {
        (Some(x), Some(y)) if xs.is_finite() && ys.is_finite() => (x - xs, y - ys),
        _ => (OFFSET, OFFSET),
    };
    // Create every node first (parents before children), then connect.
    let mut by_path: HashMap<String, NodeId> = HashMap::new();
    let mut renamed: HashMap<(Option<NodeId>, String), NodeId> = HashMap::new();
    let mut created: Vec<(NodeId, &Value)> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for n in nodes {
        let class = params::str(n, "class")?;
        let spec = s.registry.get(class).ok_or_else(|| bad(format!("unknown node class `{class}`")))?;
        let class = spec.class;
        let knob_specs = spec.knobs;
        let wanted = params::opt_str(n, "name")?.unwrap_or(spec.label);
        let parent = match parent_of(n) {
            None => target,
            Some(path) => Some(*by_path.get(&path).ok_or_else(|| bad(format!("{wanted}: its group `{path}` comes later or is missing")))?),
        };
        let name = free_name(s, parent, wanted);
        let id = s.script.add_node_in(parent, class, Some(&name))?;
        let path = params::opt_str(n, "path")?.unwrap_or(wanted).to_string();
        by_path.insert(path, id);
        renamed.insert((parent, wanted.to_string()), id);
        if parent == target {
            names.push(s.script.path_of(id)?);
        }
        let knobs: BTreeMap<String, KnobValue> = match n.get("knobs") {
            Some(k) if !k.is_null() => serde_json::from_value(k.clone()).map_err(|e| bad(format!("{wanted}: unreadable knobs ({e})")))?,
            _ => BTreeMap::new(),
        };
        let node = s.script.node_mut(id).ok_or_else(|| bad("no such node"))?;
        for (k, v) in knobs {
            // Knobs this class doesn't have, or values that don't fit, are dropped.
            if let Some(ks) = knob_specs.iter().find(|ks| ks.name == k)
                && let Ok(v) = ks.validate(v)
            {
                node.knobs.insert(k, v);
            }
        }
        let num = |key: &str| n.get(key).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(0.0);
        let (ox, oy) = if parent == target { (dx, dy) } else { (0.0, 0.0) };
        node.x = (num("x") + ox).clamp(-1e9, 1e9);
        node.y = (num("y") + oy).clamp(-1e9, 1e9);
        node.disabled = n.get("disabled").and_then(Value::as_bool).unwrap_or(false);
        let label = n.get("label").and_then(Value::as_str).unwrap_or("");
        if label.len() <= nukecraft_graph::MAX_TEXT {
            node.label = label.to_string();
        }
        created.push((id, n));
    }
    for (id, n) in &created {
        let Some(inputs) = n.get("inputs").and_then(Value::as_array) else { continue };
        let parent = s.script.node(*id).and_then(|node| node.parent);
        let max = s.script.node(*id).and_then(|node| s.registry.get(&node.class)).map_or(0, |spec| spec.max_inputs());
        for (i, src) in inputs.iter().enumerate().take(max) {
            let Some(src) = src.as_str() else { continue };
            // A pasted sibling by its (possibly new) name, else an existing node of that name
            // in the same group.
            let from = renamed.get(&(parent, src.to_string())).copied().or_else(|| s.script.child_by_name(parent, src).map(|n| n.id));
            if let Some(from) = from {
                s.script.connect(*id, i, from)?;
            }
        }
    }
    Ok(json!({ "names": names }))
}
