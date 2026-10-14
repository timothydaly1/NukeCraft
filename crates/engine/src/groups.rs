//! Groups: collapsing nodes into a Group (`node.group`) and expanding one (`node.ungroup`).
//!
//! A Group's inputs are its Input nodes (`number` 0, 1, …) and its output is its Output node; the
//! renderer reads through them. Pipes connect nodes of the same group only.

use nukecraft_graph::{KnobValue, NodeId};
use serde_json::{Value, json};

use crate::params::{self, bad};
use crate::{Result, Session};

/// Rows apart, in Node Graph units, for the Input and Output nodes placed around the contents.
const ROW: f64 = 60.0;

/// The group a command's `group` parameter names (`None`: the top level).
pub(crate) fn group_param(s: &Session, p: &Value) -> Result<Option<NodeId>> {
    let Some(path) = params::opt_str(p, "group")? else { return Ok(None) };
    if path.is_empty() {
        return Ok(None);
    }
    let id = s.script.id_of(path)?;
    if s.script.node(id).is_none_or(|n| n.class != "Group") {
        return Err(bad(format!("`{path}` is not a Group")));
    }
    Ok(Some(id))
}

pub(crate) fn node_group(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = params::nodes(&s.script, p)?;
    let first = ids.first().copied().ok_or_else(|| bad("no nodes to group"))?;
    let parent = s.script.node(first).and_then(|n| n.parent);
    if ids.iter().any(|id| s.script.node(*id).map(|n| n.parent) != Some(parent)) {
        return Err(bad("the nodes to group must be in the same group"));
    }
    if ids.iter().any(|id| s.script.node(*id).is_some_and(|n| matches!(n.class.as_str(), "Input" | "Output"))) {
        return Err(bad("Input and Output nodes belong to their group; group the nodes around them"));
    }
    let inside = |id: &NodeId| ids.contains(id);
    // Outside sources, in order of first use: each becomes one Input.
    let mut sources: Vec<NodeId> = Vec::new();
    let mut fed: Vec<(NodeId, usize, NodeId)> = Vec::new();
    for &id in &ids {
        let Some(n) = s.script.node(id) else { continue };
        for (i, src) in n.inputs.iter().enumerate() {
            if let Some(src) = src.filter(|src| !inside(src)) {
                if !sources.contains(&src) {
                    sources.push(src);
                }
                fed.push((id, i, src));
            }
        }
    }
    // The output: the node whose result is used outside (the last such), else the last node
    // that nothing in the group reads.
    let used_outside: Vec<NodeId> = ids.iter().copied().filter(|id| s.script.consumers(*id).iter().any(|(c, _)| !inside(c))).collect();
    let terminal = ids.iter().copied().rev().find(|id| !s.script.consumers(*id).iter().any(|(c, _)| inside(c)));
    let output = used_outside.last().copied().or(terminal).or_else(|| ids.last().copied());
    let consumers: Vec<(NodeId, usize)> = output.map(|o| s.script.consumers(o).into_iter().filter(|(c, _)| !inside(c)).collect()).unwrap_or_default();

    // The group sits where the nodes were.
    let (xs, ys): (Vec<f64>, Vec<f64>) = ids.iter().filter_map(|id| s.script.node(*id)).map(|n| (n.x, n.y)).unzip();
    let (cx, cy) = (xs.iter().sum::<f64>() / xs.len().max(1) as f64, ys.iter().sum::<f64>() / ys.len().max(1) as f64);
    let (top, bottom) = (ys.iter().copied().fold(f64::INFINITY, f64::min), ys.iter().copied().fold(f64::NEG_INFINITY, f64::max));
    let group_name = s.script.unique_name_in(parent, "Group");
    let group = s.script.add_node_in(parent, "Group", Some(&group_name))?;
    if let Some(g) = s.script.node_mut(group) {
        (g.x, g.y) = (cx.round(), cy.round());
    }
    // Pipes between the grouped nodes: moving them one by one cuts these, so they are restored.
    let inner: Vec<(NodeId, usize, NodeId)> = ids
        .iter()
        .filter_map(|id| s.script.node(*id).map(|n| (n.id, n.inputs.clone())))
        .flat_map(|(id, inputs)| inputs.into_iter().enumerate().filter_map(move |(i, f)| f.map(|f| (id, i, f))))
        .filter(|(_, _, f)| inside(f))
        .collect();
    for &id in &ids {
        s.script.set_parent(id, Some(group))?;
    }
    for (node, i, from) in inner {
        s.script.connect(node, i, from)?;
    }
    for (k, src) in sources.iter().enumerate() {
        let input = s.script.add_node_in(Some(group), "Input", Some(&format!("Input{}", k + 1)))?;
        // Above the first node it feeds.
        let below = fed.iter().find(|(_, _, s2)| s2 == src).and_then(|(n, ..)| s.script.node(*n)).map_or((cx, top), |n| (n.x, n.y));
        if let Some(n) = s.script.node_mut(input) {
            n.knobs.insert("number".into(), KnobValue::number(k as f64));
            (n.x, n.y) = (below.0, (top - ROW).min(below.1 - ROW));
        }
        s.script.connect(group, k, *src)?;
        for (node, i, _) in fed.iter().filter(|(_, _, s2)| s2 == src) {
            s.script.connect(*node, *i, input)?;
        }
    }
    if let Some(o) = output {
        let out = s.script.add_node_in(Some(group), "Output", Some("Output1"))?;
        let x = s.script.node(o).map_or(cx, |n| n.x);
        if let Some(n) = s.script.node_mut(out) {
            (n.x, n.y) = (x, bottom + ROW);
        }
        s.script.connect(out, 0, o)?;
        for (c, i) in consumers {
            s.script.connect(c, i, group)?;
        }
    }
    Ok(json!({ "group": s.script.path_of(group)? }))
}

pub(crate) fn node_ungroup(s: &mut Session, p: &Value) -> Result<Value> {
    let group = params::node(&s.script, p, "node")?;
    let g = s.script.node(group).ok_or_else(|| bad("no such node"))?;
    if g.class != "Group" {
        return Err(bad(format!("`{}` is not a Group", g.name)));
    }
    let (parent, gx, gy) = (g.parent, g.x, g.y);
    let group_inputs = g.inputs.clone();
    let children: Vec<NodeId> = s.script.children(Some(group)).map(|n| n.id).collect();
    let is = |s: &Session, id: NodeId, class: &str| s.script.node(id).is_some_and(|n| n.class == class);
    // Where each Input's consumers reconnect, and what the Output shows.
    let mut rewire: Vec<(NodeId, usize, Option<NodeId>)> = Vec::new();
    for &c in children.iter().filter(|c| is(s, **c, "Input")) {
        let number = s
            .script
            .node(c)
            .and_then(|n| n.knobs.get("number"))
            .map(|v| v.eval(s.time))
            .and_then(|v| v.as_numbers().and_then(|n| n.first().copied()))
            .unwrap_or(0.0);
        let number = if number.is_finite() { number.round().max(0.0) as usize } else { 0 };
        let src = group_inputs.get(number).copied().flatten();
        for (node, i) in s.script.consumers(c) {
            rewire.push((node, i, src));
        }
    }
    let shown = children.iter().copied().find(|c| is(s, *c, "Output")).and_then(|o| s.script.node(o).and_then(|n| n.input(0)));
    let consumers = s.script.consumers(group);
    // The contents move out, centred where the group was, renamed where their names are taken.
    let moving: Vec<NodeId> = children.iter().copied().filter(|c| !is(s, *c, "Input") && !is(s, *c, "Output")).collect();
    let (xs, ys): (Vec<f64>, Vec<f64>) = moving.iter().filter_map(|id| s.script.node(*id)).map(|n| (n.x, n.y)).unzip();
    let (cx, cy) = (xs.iter().sum::<f64>() / xs.len().max(1) as f64, ys.iter().sum::<f64>() / ys.len().max(1) as f64);
    // Inner pipes to be restored after the move (set_parent cuts pipes to the Input nodes).
    let inner: Vec<(NodeId, usize, NodeId)> = moving
        .iter()
        .filter_map(|id| s.script.node(*id).map(|n| (n.id, n.inputs.clone())))
        .flat_map(|(id, inputs)| inputs.into_iter().enumerate().filter_map(move |(i, f)| f.map(|f| (id, i, f))))
        .filter(|(_, _, f)| moving.contains(f))
        .collect();
    for &id in &moving {
        let name = s.script.name_of(id)?.to_string();
        if s.script.child_by_name(parent, &name).is_some_and(|n| n.id != group) || name == s.script.name_of(group)? {
            let base = name.trim_end_matches(|c: char| c.is_ascii_digit());
            let fresh = s.script.unique_name_in(parent, if base.is_empty() { "Node" } else { base });
            s.script.rename(id, &fresh)?;
        }
        s.script.set_parent(id, parent)?;
        if let Some(n) = s.script.node_mut(id) {
            (n.x, n.y) = ((n.x - cx + gx).round(), (n.y - cy + gy).round());
        }
    }
    for (node, i, from) in inner {
        s.script.connect(node, i, from)?;
    }
    for (node, i, src) in rewire {
        if let Some(src) = src
            && moving.contains(&node)
        {
            s.script.connect(node, i, src)?;
        }
    }
    let shown = shown.filter(|n| moving.contains(n));
    s.script.remove_node(group)?;
    for (c, i) in consumers {
        if let Some(src) = shown {
            s.script.connect(c, i, src)?;
        }
    }
    let names: Vec<String> = moving.iter().filter_map(|id| s.script.path_of(*id).ok()).collect();
    Ok(json!({ "nodes": names }))
}
