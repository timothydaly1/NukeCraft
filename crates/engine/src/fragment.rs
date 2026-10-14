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

/// The fragment text of `ids`, in script order.
pub fn fragment(s: &Session, ids: &[NodeId]) -> Result<String> {
    let mut nodes = Vec::new();
    for n in s.script.nodes().iter().filter(|n| ids.contains(&n.id)) {
        let inputs: Vec<Value> = n.inputs.iter().map(|i| i.and_then(|i| s.script.name_of(i).ok()).map_or(Value::Null, |name| json!(name))).collect();
        let knobs = serde_json::to_value(&n.knobs).map_err(|e| crate::EngineError::Internal(e.to_string()))?;
        nodes.push(json!({
            "name": n.name, "class": n.class, "knobs": knobs, "inputs": inputs,
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

/// A name like `base` that the script doesn't use: `Blur1` taken → `Blur2`, `Blur3`…
fn free_name(s: &Session, wanted: &str, taken: &[String]) -> String {
    if s.script.by_name(wanted).is_none() && !taken.iter().any(|t| t == wanted) {
        return wanted.to_string();
    }
    let base = wanted.trim_end_matches(|c: char| c.is_ascii_digit());
    let base = if base.is_empty() { "Node" } else { base };
    (1..).map(|i| format!("{base}{i}")).find(|n| s.script.by_name(n).is_none() && !taken.contains(n)).unwrap_or_else(|| s.script.unique_name(base))
}

pub(crate) fn edit_paste(s: &mut Session, p: &Value) -> Result<Value> {
    let text = params::str(p, "text")?;
    if text.len() > MAX_TEXT {
        return Err(bad("the fragment is too large"));
    }
    let doc: Value = serde_json::from_str(text).map_err(|_| bad("the clipboard doesn't hold NukeCraft nodes"))?;
    if doc.get("nukecraft").and_then(Value::as_str) != Some("fragment") {
        return Err(bad("the clipboard doesn't hold NukeCraft nodes"));
    }
    let nodes = doc.get("nodes").and_then(Value::as_array).ok_or_else(|| bad("the fragment has no nodes"))?;
    if nodes.is_empty() || nodes.len() > MAX_PASTE {
        return Err(bad(format!("a paste holds 1 to {MAX_PASTE} nodes")));
    }
    // Where the copy goes: its top-left node at (x, y), or next to the originals.
    let xs = nodes.iter().filter_map(|n| n.get("x").and_then(Value::as_f64)).fold(f64::INFINITY, f64::min);
    let ys = nodes.iter().filter_map(|n| n.get("y").and_then(Value::as_f64)).fold(f64::INFINITY, f64::min);
    let (dx, dy) = match (params::opt_f64(p, "x")?, params::opt_f64(p, "y")?) {
        (Some(x), Some(y)) if xs.is_finite() && ys.is_finite() => (x - xs, y - ys),
        _ => (OFFSET, OFFSET),
    };
    // Create every node first, then connect, so pipes between pasted nodes find their targets.
    let mut renamed: HashMap<String, String> = HashMap::new();
    let mut created: Vec<(NodeId, &Value)> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for n in nodes {
        let class = params::str(n, "class")?;
        let spec = s.registry.get(class).ok_or_else(|| bad(format!("unknown node class `{class}`")))?;
        let class = spec.class;
        let knob_specs = spec.knobs;
        let wanted = params::opt_str(n, "name")?.unwrap_or(spec.label);
        let name = free_name(s, wanted, &names);
        let id = s.script.add_node(class, Some(&name))?;
        renamed.insert(wanted.to_string(), name.clone());
        names.push(name);
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
        node.x = (num("x") + dx).clamp(-1e9, 1e9);
        node.y = (num("y") + dy).clamp(-1e9, 1e9);
        node.disabled = n.get("disabled").and_then(Value::as_bool).unwrap_or(false);
        let label = n.get("label").and_then(Value::as_str).unwrap_or("");
        if label.len() <= nukecraft_graph::MAX_TEXT {
            node.label = label.to_string();
        }
        created.push((id, n));
    }
    for (id, n) in &created {
        let Some(inputs) = n.get("inputs").and_then(Value::as_array) else { continue };
        let max = s.script.node(*id).and_then(|node| s.registry.get(&node.class)).map_or(0, |spec| spec.max_inputs());
        for (i, src) in inputs.iter().enumerate().take(max) {
            let Some(src) = src.as_str() else { continue };
            // A pasted node by its new name, else an existing node of that name.
            let target = renamed.get(src).map_or(src, String::as_str);
            if let Ok(from) = s.script.id_of(target) {
                s.script.connect(*id, i, from)?;
            }
        }
    }
    Ok(json!({ "names": names }))
}
