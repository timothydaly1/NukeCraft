//! The command registry. Ids are `area.verb` (`node.create`, `file.save`); menu paths and
//! shortcuts follow Nuke's where it has them. Every menu item, node graph gesture, properties
//! panel edit, CLI call and MCP tool call maps to one of these.

use std::collections::HashSet;
use std::path::PathBuf;

use nukecraft_curve::Interp;
use nukecraft_geom::{Format, IRect};
use nukecraft_graph::{KnobValue, NodeId, Script};
use nukecraft_nodes::{KnobDefault, KnobKind, NodeSpec};
use nukecraft_render::{Renderer, with_render_stack};
use nukecraft_time::{Fps, FrameRange, FrameTime, parse_frames};
use serde_json::{Map, Value, json};

use crate::params::{self, bad};
use crate::{EngineError, Result, Session};

pub type Run = fn(&mut Session, &Value) -> Result<Value>;
pub type Enabled = fn(&Session) -> std::result::Result<(), String>;

/// Metadata and implementation of one command.
pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    /// Menu placement (`["File"]`); empty = not in menus.
    pub menu: &'static [&'static str],
    /// Default shortcut (`Cmd` is ⌘ on macOS, Ctrl elsewhere).
    pub shortcut: Option<&'static str>,
    /// Parameters, for people and agents (published in the MCP tool description).
    pub params: &'static str,
    pub enabled: Enabled,
    pub run: Run,
    /// Changes to the script become an undo step.
    pub undoable: bool,
    /// Recorded in the session journal (false for pure queries).
    pub journal: bool,
}

/// The largest script file `file.open` reads.
const MAX_SCRIPT_BYTES: u64 = 256 * 1024 * 1024;

fn always(_: &Session) -> std::result::Result<(), String> {
    Ok(())
}

fn can_undo(s: &Session) -> std::result::Result<(), String> {
    if s.can_undo() { Ok(()) } else { Err("nothing to undo".into()) }
}

fn can_redo(s: &Session) -> std::result::Result<(), String> {
    if s.can_redo() { Ok(()) } else { Err("nothing to redo".into()) }
}

const fn edit(
    id: &'static str,
    label: &'static str,
    menu: &'static [&'static str],
    shortcut: Option<&'static str>,
    params: &'static str,
    run: Run,
) -> CommandSpec {
    CommandSpec { id, label, menu, shortcut, params, enabled: always, run, undoable: true, journal: true }
}

const fn action(
    id: &'static str,
    label: &'static str,
    menu: &'static [&'static str],
    shortcut: Option<&'static str>,
    params: &'static str,
    run: Run,
) -> CommandSpec {
    CommandSpec { id, label, menu, shortcut, params, enabled: always, run, undoable: false, journal: true }
}

const fn query(id: &'static str, label: &'static str, params: &'static str, run: Run) -> CommandSpec {
    CommandSpec { id, label, menu: &[], shortcut: None, params, enabled: always, run, undoable: false, journal: false }
}

static SPECS: &[CommandSpec] = &[
    // File
    action("file.new", "New Script", &["File"], Some("Cmd+N"), "{format?: name or WxH} — replaces the open script and clears undo", file_new),
    action("file.open", "Open Script…", &["File"], Some("Cmd+O"), "{path} — a .ncs script", file_open),
    action("file.save", "Save Script", &["File"], Some("Cmd+S"), "{path?} — defaults to the script's own path", file_save),
    action("file.saveAs", "Save Script As…", &["File"], Some("Cmd+Shift+S"), "{path}", file_save),
    // Edit
    CommandSpec {
        id: "edit.undo",
        label: "Undo",
        menu: &["Edit"],
        shortcut: Some("Cmd+Z"),
        params: "{}",
        enabled: can_undo,
        run: edit_undo,
        undoable: false,
        journal: true,
    },
    CommandSpec {
        id: "edit.redo",
        label: "Redo",
        menu: &["Edit"],
        shortcut: Some("Cmd+Shift+Z"),
        params: "{}",
        enabled: can_redo,
        run: edit_redo,
        undoable: false,
        journal: true,
    },
    action(
        "edit.copy",
        "Copy",
        &["Edit"],
        Some("Cmd+C"),
        "{nodes: [name] | node} — returns {text}: the nodes as a script fragment",
        crate::fragment::edit_copy,
    ),
    edit("edit.cut", "Cut", &["Edit"], Some("Cmd+X"), "{nodes: [name] | node} — copies, then deletes; returns {text}", crate::fragment::edit_cut),
    edit(
        "edit.paste",
        "Paste",
        &["Edit"],
        Some("Cmd+V"),
        "{text: fragment from edit.copy, group?: Group path, x?, y?} — renames taken names, keeps pipes (to outside nodes by name); returns {names}",
        crate::fragment::edit_paste,
    ),
    edit(
        "node.delete",
        "Delete",
        &["Edit"],
        Some("Delete"),
        "{nodes: [name] | node: name, reconnect?: bool = true} — reconnect joins input 0's node to what the deleted node fed",
        node_delete,
    ),
    edit(
        "node.disable",
        "Disable/Enable",
        &["Edit", "Node"],
        Some("D"),
        "{nodes: [name] | node, disabled?: bool} — toggles when `disabled` is absent",
        node_disable,
    ),
    edit(
        "root.settings",
        "Project Settings",
        &["Edit"],
        Some("S"),
        "{format?: name, WxH or {width,height,pixel_aspect}, fps?: number, first?: int, last?: int}",
        root_settings,
    ),
    // Nodes
    edit(
        "node.create",
        "Create Node",
        &[],
        None,
        "{class, name?, group?: Group path, knobs?: {knob: value}, inputs?: [name|null], x?, y?} — class names or menu names (Merge → Merge2); inside a group, inputs name siblings; returns {name: path, class}",
        node_create,
    ),
    edit("node.rename", "Rename", &[], None, "{node, name}", node_rename),
    edit(
        "node.group",
        "Collapse to Group",
        &["Edit"],
        Some("Cmd+G"),
        "{nodes} — moves the nodes into a new Group, with an Input per outside source and an Output; returns {group}",
        crate::groups::node_group,
    ),
    edit(
        "node.ungroup",
        "Expand Group",
        &["Edit"],
        Some("Cmd+Alt+G"),
        "{node: a Group} — moves its nodes out and deletes it, keeping the pipes",
        crate::groups::node_ungroup,
    ),
    edit("node.connect", "Connect", &[], None, "{node, input: index, from: name} — refuses cycles", node_connect),
    edit("node.disconnect", "Disconnect", &[], None, "{node, input: index}", node_disconnect),
    edit(
        "node.setKnob",
        "Set Knob",
        &[],
        None,
        "{node, knob, value: number | [numbers] | bool | text, frame?: number, interp?: constant|linear|horizontal|catmull-rom} — with `frame` (or on an animated knob) sets a key",
        node_set_knob,
    ),
    edit("node.resetKnob", "Reset Knob to Default", &[], None, "{node, knob}", node_reset_knob),
    edit("node.removeKey", "Delete Key", &[], None, "{node, knob, frame}", node_remove_key),
    edit("node.clearAnimation", "No Animation", &[], None, "{node, knob} — keeps the value at the current frame", node_clear_animation),
    edit(
        "node.editKeys",
        "Edit Keys",
        &[],
        None,
        "{node, knob, component?: 0, remove?: [frames], keys?: [{frame, value, interp?}]} — one component's curve: removes keys, then sets keys (moving a key is both); a curve left without keys keeps its value",
        node_edit_keys,
    ),
    edit(
        "node.setInterpolation",
        "Set Interpolation",
        &[],
        None,
        "{node, knob, component?, frames: [frames], interp: constant|linear|horizontal|catmull-rom} — of the keys at those frames (every component unless `component`)",
        node_set_interpolation,
    ),
    edit("node.move", "Move Node", &[], None, "{node, x, y} | {moves: [{node, x, y}]} — positions in the node graph, one undo step", node_move),
    edit("node.label", "Set Label", &[], None, "{node, label}", node_label),
    // Time and rendering
    // View state, not an edit: not journalled, so playback doesn't flood the journal.
    CommandSpec {
        id: "time.goto",
        label: "Go to Frame",
        menu: &[],
        shortcut: None,
        params: "{frame}",
        enabled: always,
        run: time_goto,
        undoable: false,
        journal: false,
    },
    action(
        "render.execute",
        "Render…",
        &["Render"],
        Some("F7"),
        "{nodes?: [Write names] (default: every enabled Write), frames?: \"1-10\", \"1-10x2\", \"1,5,9\"} — returns {written}",
        render_execute,
    ),
    action(
        "render.frame",
        "Render Frame to File",
        &["Render"],
        None,
        "{node, path: .png or .exr, frame?} — renders any node over its format, as the Viewer shows it",
        render_frame,
    ),
    // Queries
    query("query.script", "Script", "{} — the whole script as .ncs JSON", q_script),
    query("query.nodes", "Nodes", "{} — every node with its class and inputs", q_nodes),
    query("query.node", "Node", "{node} — class, knobs (value, kind, default, animated) and inputs", q_node),
    query("query.nodeClasses", "Node Classes", "{} — every node class with its category, inputs and knobs", q_node_classes),
    query("query.commands", "Commands", "{} — every command with its label, menu, shortcut and parameters", q_commands),
    query("query.info", "Image Info", "{node, frame?} — format, bbox and channels", q_info),
    query("query.pixel", "Pixel", "{node, x, y, frame?} — every channel's value at a pixel (y up from the bottom)", q_pixel),
    query("query.history", "History", "{} — undo step labels and the journal length", q_history),
];

/// Every command.
pub fn command_specs() -> &'static [CommandSpec] {
    SPECS
}

pub fn find_command(id: &str) -> Option<&'static CommandSpec> {
    SPECS.iter().find(|c| c.id == id)
}

// ── File ─────────────────────────────────────────────────────────────────────────────────

fn file_new(s: &mut Session, p: &Value) -> Result<Value> {
    let mut script = Script::new();
    if let Some(f) = params::opt_str(p, "format")? {
        script.root.format = Format::parse(f).ok_or_else(|| bad(format!("unknown format `{f}`")))?;
    }
    s.script = script;
    s.path = None;
    s.dirty = false;
    s.load_warnings.clear();
    s.clear_history();
    Ok(json!({}))
}

fn file_open(s: &mut Session, p: &Value) -> Result<Value> {
    let path = PathBuf::from(params::str(p, "path")?);
    let meta = std::fs::metadata(&path).map_err(|e| EngineError::Io(format!("{}: {e}", path.display())))?;
    if meta.len() > MAX_SCRIPT_BYTES {
        return Err(EngineError::Io(format!("{}: larger than {MAX_SCRIPT_BYTES} bytes", path.display())));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| EngineError::Io(format!("{}: {e}", path.display())))?;
    let (script, warnings) = Script::from_json(&text)?;
    for w in &warnings {
        log::warn!("{}: {w}", path.display());
    }
    s.script = script;
    s.time = FrameTime::from_frame(s.script.root.frames.first);
    s.path = Some(path);
    s.dirty = false;
    s.load_warnings = warnings.clone();
    s.clear_history();
    Ok(json!({ "warnings": warnings }))
}

fn file_save(s: &mut Session, p: &Value) -> Result<Value> {
    let path = match params::opt_str(p, "path")? {
        Some(p) => PathBuf::from(p),
        None => s.path.clone().ok_or_else(|| bad("the script has no path yet: give `path`"))?,
    };
    let text = s.script.to_json()?;
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.nukecraft-tmp"));
    std::fs::write(&tmp, text.as_bytes()).map_err(|e| EngineError::Io(format!("{}: {e}", tmp.display())))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        EngineError::Io(format!("{}: {e}", path.display()))
    })?;
    s.path = Some(path.clone());
    s.dirty = false;
    Ok(json!({ "path": path.display().to_string() }))
}

// ── Edit ─────────────────────────────────────────────────────────────────────────────────

fn edit_undo(s: &mut Session, _: &Value) -> Result<Value> {
    let label = s.undo_step().ok_or_else(|| bad("nothing to undo"))?;
    Ok(json!({ "undone": label }))
}

fn edit_redo(s: &mut Session, _: &Value) -> Result<Value> {
    let label = s.redo_step().ok_or_else(|| bad("nothing to redo"))?;
    Ok(json!({ "redone": label }))
}

pub(crate) fn node_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = params::nodes(&s.script, p)?;
    let reconnect = params::opt_bool(p, "reconnect")?.unwrap_or(true);
    let doomed: HashSet<NodeId> = ids.iter().copied().collect();
    // Where each deleted node's consumers reconnect: the nearest upstream node (through input 0)
    // that survives, worked out before anything is removed so deleting a chain keeps the pipe.
    let plan: Vec<(NodeId, Option<NodeId>)> = ids
        .iter()
        .map(|&id| {
            let mut upstream = s.script.node(id).and_then(|n| n.input(0));
            for _ in 0..=doomed.len() {
                match upstream {
                    Some(u) if doomed.contains(&u) => upstream = s.script.node(u).and_then(|n| n.input(0)),
                    _ => break,
                }
            }
            (id, upstream.filter(|u| !doomed.contains(u)))
        })
        .collect();
    let mut deleted = Vec::new();
    let mut rewire = Vec::new();
    for (id, upstream) in plan {
        if s.script.node(id).is_none() {
            continue;
        }
        let (node, consumers) = s.script.remove_node(id)?;
        if let Some(up) = upstream {
            rewire.extend(consumers.into_iter().filter(|(c, _)| !doomed.contains(c)).map(|(c, i)| (c, i, up)));
        }
        deleted.push(node.name);
    }
    if reconnect {
        for (c, i, up) in rewire {
            s.script.connect(c, i, up)?;
        }
    }
    Ok(json!({ "deleted": deleted }))
}

fn node_disable(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = params::nodes(&s.script, p)?;
    let want = params::opt_bool(p, "disabled")?;
    for id in ids {
        if let Some(n) = s.script.node_mut(id) {
            n.disabled = want.unwrap_or(!n.disabled);
        }
    }
    Ok(json!({}))
}

fn root_settings(s: &mut Session, p: &Value) -> Result<Value> {
    match p.get("format") {
        None | Some(Value::Null) => {}
        Some(Value::String(name)) => s.script.root.format = Format::parse(name).ok_or_else(|| bad(format!("unknown format `{name}`")))?,
        Some(obj @ Value::Object(_)) => {
            let w = params::usize(obj, "width")?;
            let h = params::usize(obj, "height")?;
            let pa = params::opt_f64(obj, "pixel_aspect")?.unwrap_or(1.0);
            if w == 0 || h == 0 || w > nukecraft_geom::MAX_COORD as usize || h > nukecraft_geom::MAX_COORD as usize || pa <= 0.0 {
                return Err(bad("format width, height and pixel_aspect must be positive"));
            }
            s.script.root.format = Format::new(w as u32, h as u32, pa);
        }
        Some(_) => return Err(bad("`format` is a name, WxH, or {width, height, pixel_aspect}")),
    }
    if let Some(fps) = params::opt_f64(p, "fps")? {
        s.script.root.fps = Fps::from_f64(fps).ok_or_else(|| bad(format!("{fps} is not a frame rate")))?;
    }
    let first = params::opt_i64(p, "first")?.unwrap_or(s.script.root.frames.first);
    let last = params::opt_i64(p, "last")?.unwrap_or(s.script.root.frames.last);
    s.script.root.frames = FrameRange::new(first, last);
    Ok(json!({}))
}

// ── Nodes ────────────────────────────────────────────────────────────────────────────────

fn class_spec<'a>(s: &'a Session, class: &str) -> Result<&'a NodeSpec> {
    s.registry.get(class).ok_or_else(|| bad(format!("unknown node class `{class}` (query.nodeClasses lists them)")))
}

fn node_spec(s: &Session, id: NodeId) -> Result<&NodeSpec> {
    let node = s.script.node(id).ok_or_else(|| bad("no such node"))?;
    class_spec(s, &node.class)
}

fn node_create(s: &mut Session, p: &Value) -> Result<Value> {
    let spec = class_spec(s, params::str(p, "class")?)?;
    let (class, label, knob_specs, max_inputs) = (spec.class, spec.label, spec.knobs, spec.max_inputs());
    let parent = crate::groups::group_param(s, p)?;
    let name = match params::opt_str(p, "name")? {
        Some(n) => n.to_string(),
        None => s.script.unique_name_in(parent, label),
    };
    let id = s.script.add_node_in(parent, class, Some(&name))?;
    // Format- and time-dependent defaults are fixed when the node is made, as Nuke does.
    let format = s.script.root.format.clone();
    let frame = s.time.floor_frame() as f64;
    for k in knob_specs {
        let value = match k.default {
            KnobDefault::CurrentFrame => KnobValue::number(frame),
            d if d.is_format_dependent() => k.default_value(&format),
            _ => continue,
        };
        if let Some(n) = s.script.node_mut(id) {
            n.knobs.insert(k.name.into(), value);
        }
    }
    if let Some(knobs) = p.get("knobs") {
        let obj = knobs.as_object().ok_or_else(|| bad("`knobs` must be an object {knob: value}"))?;
        for (k, v) in obj {
            set_knob(s, id, k, v, None, Interp::default())?;
        }
    }
    if let Some(inputs) = p.get("inputs") {
        let arr = inputs.as_array().ok_or_else(|| bad("`inputs` must be a list of node names (or null)"))?;
        if arr.len() > max_inputs {
            return Err(bad(format!("{class} takes at most {max_inputs} inputs")));
        }
        for (i, v) in arr.iter().enumerate() {
            match v {
                Value::Null => {}
                Value::String(from) => {
                    // A sibling by name, else a path from the top.
                    let from = match s.script.child_by_name(parent, from) {
                        Some(n) => n.id,
                        None => s.script.id_of(from)?,
                    };
                    s.script.connect(id, i, from)?;
                }
                _ => return Err(bad("`inputs` must be a list of node names (or null)")),
            }
        }
    }
    let (x, y) = match (params::opt_f64(p, "x")?, params::opt_f64(p, "y")?) {
        (Some(x), Some(y)) => (x, y),
        _ => auto_position(&s.script, id),
    };
    if let Some(n) = s.script.node_mut(id) {
        (n.x, n.y) = (x, y);
    }
    Ok(json!({ "name": s.script.path_of(id)?, "class": class }))
}

/// Below input 0 if connected, else to the right of everything.
fn auto_position(script: &Script, id: NodeId) -> (f64, f64) {
    if let Some(up) = script.node(id).and_then(|n| n.input(0)).and_then(|u| script.node(u)) {
        return (up.x, up.y + 80.0);
    }
    let max_x = script.nodes().iter().filter(|n| n.id != id).map(|n| n.x).fold(f64::NEG_INFINITY, f64::max);
    (if max_x.is_finite() { max_x + 120.0 } else { 0.0 }, 0.0)
}

fn node_rename(s: &mut Session, p: &Value) -> Result<Value> {
    let id = params::node(&s.script, p, "node")?;
    s.script.rename(id, params::str(p, "name")?)?;
    Ok(json!({}))
}

fn node_connect(s: &mut Session, p: &Value) -> Result<Value> {
    let id = params::node(&s.script, p, "node")?;
    let input = params::usize(p, "input")?;
    let from = params::node(&s.script, p, "from")?;
    let max = node_spec(s, id)?.max_inputs();
    if input >= max {
        return Err(bad(format!("input {input} is out of range: this node has {max} inputs")));
    }
    s.script.connect(id, input, from)?;
    Ok(json!({}))
}

fn node_disconnect(s: &mut Session, p: &Value) -> Result<Value> {
    let id = params::node(&s.script, p, "node")?;
    s.script.disconnect(id, params::usize(p, "input")?)?;
    Ok(json!({}))
}

fn parse_interp(p: &Value) -> Result<Interp> {
    Ok(match params::opt_str(p, "interp")? {
        None => Interp::default(),
        Some("constant") => Interp::Constant,
        Some("linear") => Interp::Linear,
        Some("horizontal") => Interp::Horizontal,
        Some("catmull-rom") => Interp::CatmullRom,
        Some(other) => return Err(bad(format!("unknown interpolation `{other}` (constant, linear, horizontal, catmull-rom)"))),
    })
}

fn node_set_knob(s: &mut Session, p: &Value) -> Result<Value> {
    let id = params::node(&s.script, p, "node")?;
    let knob = params::str(p, "knob")?.to_string();
    let value = p.get("value").ok_or_else(|| bad("`value` is required"))?;
    let frame = params::opt_frame(p, "frame")?;
    let interp = parse_interp(p)?;
    set_knob(s, id, &knob, value, frame, interp)?;
    Ok(json!({}))
}

/// Sets a knob, validating it against the node's class. With `frame` (or when the knob is
/// already animated, at the current frame) numeric values become keys.
fn set_knob(s: &mut Session, id: NodeId, knob: &str, value: &Value, frame: Option<FrameTime>, interp: Interp) -> Result<()> {
    let spec = node_spec(s, id)?;
    let ks = *spec.knob(knob).ok_or_else(|| {
        let names: Vec<&str> = spec.knobs.iter().map(|k| k.name).collect();
        bad(format!("{} has no knob `{knob}` (knobs: {})", spec.class, names.join(", ")))
    })?;
    let incoming = ks.validate(params::knob_value(value)?).map_err(bad)?;
    let format = s.script.root.format.clone();
    let time = s.time;
    let node = s.script.node_mut(id).ok_or_else(|| bad("no such node"))?;
    let current = node.knobs.get(knob).cloned().unwrap_or_else(|| ks.default_value(&format));
    let key_at = frame.or_else(|| current.is_animated().then_some(time));
    let new = match (key_at, &incoming) {
        (Some(t), KnobValue::Number(_) | KnobValue::Vector(_)) => {
            let mut nums: Vec<f64> = incoming.components().unwrap_or_default().iter().map(|a| a.eval(t)).collect();
            // One number keys every component of a multi-component knob.
            let arity = current.components().map_or(1, |c| c.len());
            if let [single] = nums[..] {
                nums = vec![single; arity.max(1)];
            }
            let mut v = current;
            v.set_numbers(&nums, Some((t, interp)))?;
            v
        }
        (Some(_), _) if frame.is_some() => return Err(bad(format!("`{knob}` is not numeric and cannot be animated"))),
        _ => incoming,
    };
    node.knobs.insert(knob.into(), new);
    Ok(())
}

fn knob_of(s: &Session, p: &Value) -> Result<(NodeId, String)> {
    let id = params::node(&s.script, p, "node")?;
    let knob = params::str(p, "knob")?;
    let spec = node_spec(s, id)?;
    spec.knob(knob).ok_or_else(|| bad(format!("{} has no knob `{knob}`", spec.class)))?;
    Ok((id, knob.to_string()))
}

fn node_reset_knob(s: &mut Session, p: &Value) -> Result<Value> {
    let (id, knob) = knob_of(s, p)?;
    if let Some(n) = s.script.node_mut(id) {
        n.knobs.remove(&knob);
    }
    Ok(json!({}))
}

fn node_remove_key(s: &mut Session, p: &Value) -> Result<Value> {
    let (id, knob) = knob_of(s, p)?;
    let t = params::opt_frame(p, "frame")?.ok_or_else(|| bad("`frame` is required"))?;
    let removed = s.script.node_mut(id).and_then(|n| n.knobs.get_mut(&knob)).is_some_and(|v| v.remove_key(t));
    if !removed {
        return Err(bad(format!("`{knob}` has no key at frame {t}")));
    }
    Ok(json!({}))
}

fn node_clear_animation(s: &mut Session, p: &Value) -> Result<Value> {
    let (id, knob) = knob_of(s, p)?;
    let t = s.time;
    if let Some(v) = s.script.node_mut(id).and_then(|n| n.knobs.get_mut(&knob)) {
        v.clear_animation(t);
    }
    Ok(json!({}))
}

/// The most keys one key command may touch.
const MAX_KEY_EDITS: usize = 100_000;

fn frame_list(p: &Value, key: &str) -> Result<Vec<FrameTime>> {
    let Some(v) = p.get(key).filter(|v| !v.is_null()) else { return Ok(Vec::new()) };
    let arr = v.as_array().ok_or_else(|| bad(format!("`{key}` must be a list of frame numbers")))?;
    if arr.len() > MAX_KEY_EDITS {
        return Err(bad(format!("`{key}` lists more than {MAX_KEY_EDITS} frames")));
    }
    arr.iter()
        .map(|f| f.as_f64().filter(|f| f.is_finite()).map(FrameTime::from_f64).ok_or_else(|| bad(format!("`{key}` must be a list of frame numbers"))))
        .collect()
}

fn component_of(p: &Value) -> Result<Option<usize>> {
    match p.get("component") {
        None | Some(Value::Null) => Ok(None),
        Some(_) => Ok(Some(params::usize(p, "component")?)),
    }
}

fn node_edit_keys(s: &mut Session, p: &Value) -> Result<Value> {
    let (id, knob) = knob_of(s, p)?;
    let comp = component_of(p)?.unwrap_or(0);
    let removes = frame_list(p, "remove")?;
    let mut sets = Vec::new();
    if let Some(v) = p.get("keys").filter(|v| !v.is_null()) {
        let arr = v.as_array().ok_or_else(|| bad("`keys` must be a list of {frame, value, interp?}"))?;
        if arr.len() > MAX_KEY_EDITS {
            return Err(bad(format!("`keys` lists more than {MAX_KEY_EDITS} keys")));
        }
        for k in arr {
            let t = params::opt_frame(k, "frame")?.ok_or_else(|| bad("each key needs a `frame`"))?;
            let interp = if k.get("interp").is_some_and(|i| !i.is_null()) { Some(parse_interp(k)?) } else { None };
            sets.push((t, params::f64(k, "value")?, interp));
        }
    }
    if removes.is_empty() && sets.is_empty() {
        return Err(bad("nothing to do: give `remove` and/or `keys`"));
    }
    let spec = node_spec(s, id)?;
    let ks = *spec.knob(&knob).ok_or_else(|| bad("no such knob"))?;
    if !ks.kind.is_numeric() {
        return Err(bad(format!("`{knob}` is not numeric and has no keys")));
    }
    let format = s.script.root.format.clone();
    let now = s.time;
    let node = s.script.node_mut(id).ok_or_else(|| bad("no such node"))?;
    let current = node.knobs.get(&knob).cloned().unwrap_or_else(|| ks.default_value(&format));
    let arity = current.components().map_or(0, |c| c.len());
    // A component past the stored ones widens the value to the knob's next valid size.
    let mut value = if comp < arity {
        current
    } else {
        (comp + 1..=nukecraft_graph::MAX_COMPONENTS)
            .filter_map(|n| current.widened(n))
            .find(|v| ks.validate(v.clone()).is_ok())
            .ok_or_else(|| bad(format!("`{knob}` has no component {comp}")))?
    };
    let anim = value.anim_mut(comp).ok_or_else(|| bad(format!("`{knob}` has no component {comp}")))?;
    let held = anim.eval(now);
    let mut curve = match anim {
        nukecraft_graph::Anim::Curve { curve } => curve.clone(),
        nukecraft_graph::Anim::Static(_) => nukecraft_curve::Curve::new(),
    };
    for t in &removes {
        if !curve.remove_key(*t) {
            return Err(bad(format!("`{knob}` has no key at frame {t}")));
        }
    }
    for (t, v, interp) in sets {
        let interp = interp.or_else(|| curve.keys().iter().find(|k| k.time == t).map(|k| k.interp)).unwrap_or_default();
        curve.set_key(nukecraft_curve::Key { time: t, value: v, interp }).map_err(bad)?;
    }
    *anim = if curve.is_empty() { nukecraft_graph::Anim::Static(held) } else { nukecraft_graph::Anim::Curve { curve } };
    let value = ks.validate(value).map_err(bad)?;
    node.knobs.insert(knob, value);
    Ok(json!({}))
}

fn node_set_interpolation(s: &mut Session, p: &Value) -> Result<Value> {
    let (id, knob) = knob_of(s, p)?;
    let comp = component_of(p)?;
    let frames = frame_list(p, "frames")?;
    if frames.is_empty() {
        return Err(bad("`frames` (the keys to change) is required"));
    }
    let interp = parse_interp(p)?;
    let node = s.script.node_mut(id).ok_or_else(|| bad("no such node"))?;
    let value = node.knobs.get_mut(&knob).ok_or_else(|| bad(format!("`{knob}` is not animated")))?;
    let arity = value.components().map_or(0, |c| c.len());
    let comps: Vec<usize> = match comp {
        Some(c) if c < arity => vec![c],
        Some(c) => return Err(bad(format!("`{knob}` has no component {c}"))),
        None => (0..arity).collect(),
    };
    for t in frames {
        let mut found = false;
        for &c in &comps {
            if let Some(nukecraft_graph::Anim::Curve { curve }) = value.anim_mut(c)
                && let Some(k) = curve.keys().iter().find(|k| k.time == t).copied()
            {
                curve.set_key(nukecraft_curve::Key { interp, ..k }).map_err(bad)?;
                found = true;
            }
        }
        if !found {
            return Err(bad(format!("`{knob}` has no key at frame {t}")));
        }
    }
    Ok(json!({}))
}

fn node_move(s: &mut Session, p: &Value) -> Result<Value> {
    let moves: Vec<&Value> = match p.get("moves") {
        Some(list) => list.as_array().ok_or_else(|| bad("`moves` must be a list of {node, x, y}"))?.iter().collect(),
        None => vec![p],
    };
    for m in moves {
        let id = params::node(&s.script, m, "node")?;
        let (x, y) = (params::f64(m, "x")?, params::f64(m, "y")?);
        if let Some(n) = s.script.node_mut(id) {
            (n.x, n.y) = (x, y);
        }
    }
    Ok(json!({}))
}

fn node_label(s: &mut Session, p: &Value) -> Result<Value> {
    let id = params::node(&s.script, p, "node")?;
    let label = params::str(p, "label")?;
    if label.len() > nukecraft_graph::MAX_TEXT {
        return Err(bad("label is too long"));
    }
    if let Some(n) = s.script.node_mut(id) {
        n.label = label.into();
    }
    Ok(json!({}))
}

// ── Time and rendering ───────────────────────────────────────────────────────────────────

fn time_goto(s: &mut Session, p: &Value) -> Result<Value> {
    s.time = params::opt_frame(p, "frame")?.ok_or_else(|| bad("`frame` is required"))?;
    Ok(json!({ "frame": s.time.as_f64() }))
}

fn frame_of(s: &Session, p: &Value) -> Result<FrameTime> {
    Ok(params::opt_frame(p, "frame")?.unwrap_or(s.time))
}

/// What `render.execute` with these parameters renders: the Write nodes (all enabled ones by
/// default) and the frames (`frames`, else the script's range). The desktop app renders the
/// same plan on a background thread.
pub fn render_plan(s: &Session, p: &Value) -> Result<(Vec<NodeId>, Vec<i64>)> {
    let writes: Vec<NodeId> = match p.get("nodes") {
        Some(_) => params::nodes(&s.script, p)?,
        None => s.script.nodes().iter().filter(|n| n.class == "Write" && !n.disabled).map(|n| n.id).collect(),
    };
    if writes.is_empty() {
        return Err(bad("no Write nodes to render"));
    }
    let frames = match params::opt_str(p, "frames")? {
        Some(expr) => parse_frames(expr).map_err(|e| bad(e.to_string()))?,
        None => {
            let r = s.script.root.frames;
            if r.len() > nukecraft_time::MAX_RANGE_FRAMES as u64 {
                return Err(bad("the frame range is too long; give `frames`"));
            }
            (r.first..=r.last).collect()
        }
    };
    Ok((writes, frames))
}

fn render_execute(s: &mut Session, p: &Value) -> Result<Value> {
    let (writes, frames) = render_plan(s, p)?;
    let (script, registry) = (&s.script, &s.registry);
    let mut written = Vec::new();
    for w in writes {
        let paths = with_render_stack(|| nukecraft_io::execute_write(script, registry, w, &frames)).map_err(EngineError::Internal)??;
        written.extend(paths.into_iter().map(|p| p.display().to_string()));
    }
    Ok(json!({ "written": written }))
}

fn render_frame(s: &mut Session, p: &Value) -> Result<Value> {
    let id = params::node(&s.script, p, "node")?;
    let path = PathBuf::from(params::str(p, "path")?);
    let t = frame_of(s, p)?;
    let ty = nukecraft_io::FileType::from_path(&path).ok_or_else(|| bad("`path` must end in .png or .exr"))?;
    let (script, registry) = (&s.script, &s.registry);
    let (info, img) = with_render_stack(|| Renderer::new(script, registry).render_format(id, t)).map_err(EngineError::Internal)??;
    let channels =
        if img.channels().contains(&nukecraft_raster::Channel::alpha()) { nukecraft_raster::ChannelSet::rgba() } else { nukecraft_raster::ChannelSet::rgb() };
    let opts = nukecraft_io::WriteOptions {
        channels,
        colorspace: ty.default_colorspace(),
        bits: if ty == nukecraft_io::FileType::Exr { 32 } else { 8 },
        premultiplied: false,
    };
    nukecraft_io::write_image(&path, &img, &info.format, &opts)?;
    Ok(json!({ "path": path.display().to_string(), "width": info.format.width, "height": info.format.height }))
}

// ── Queries ──────────────────────────────────────────────────────────────────────────────

fn q_script(s: &mut Session, _: &Value) -> Result<Value> {
    let text = s.script.to_json()?;
    serde_json::from_str(&text).map_err(|e| EngineError::Internal(e.to_string()))
}

fn input_names(s: &Session, id: NodeId) -> Vec<Value> {
    s.script.node(id).map_or_else(Vec::new, |n| n.inputs.iter().map(|i| i.and_then(|i| s.script.name_of(i).ok()).map_or(Value::Null, |n| json!(n))).collect())
}

fn q_nodes(s: &mut Session, _: &Value) -> Result<Value> {
    let list: Vec<Value> = s
        .script
        .nodes()
        .iter()
        .map(|n| {
            let path = s.script.path_of(n.id).unwrap_or_else(|_| n.name.clone());
            let group = n.parent.and_then(|g| s.script.path_of(g).ok());
            json!({ "name": path, "class": n.class, "group": group, "inputs": input_names(s, n.id), "disabled": n.disabled, "x": n.x, "y": n.y })
        })
        .collect();
    Ok(json!(list))
}

fn knob_value_json(v: &KnobValue, t: FrameTime) -> Value {
    match v.eval(t) {
        nukecraft_graph::Value::Bool(b) => json!(b),
        nukecraft_graph::Value::Numbers(n) if n.len() == 1 => json!(n.first()),
        nukecraft_graph::Value::Numbers(n) => json!(n),
        nukecraft_graph::Value::Text(t) => json!(t),
    }
}

fn kind_json(kind: KnobKind) -> Value {
    match kind {
        KnobKind::Choice(options) => json!({ "kind": "choice", "choices": options }),
        k => json!({ "kind": k.name() }),
    }
}

fn q_node(s: &mut Session, p: &Value) -> Result<Value> {
    let id = params::node(&s.script, p, "node")?;
    let node = s.script.node(id).ok_or_else(|| bad("no such node"))?;
    let spec = class_spec(s, &node.class)?;
    let t = s.time;
    let format = &s.script.root.format;
    let knobs: Vec<Value> = spec
        .knobs
        .iter()
        .map(|k| {
            let stored = node.knobs.get(k.name);
            let current = stored.cloned().unwrap_or_else(|| k.default_value(format));
            let mut o = Map::new();
            o.insert("name".into(), json!(k.name));
            o.insert("label".into(), json!(k.label));
            if let Value::Object(kind) = kind_json(k.kind) {
                o.extend(kind);
            }
            o.insert("value".into(), knob_value_json(&current, t));
            o.insert("default".into(), knob_value_json(&k.default_value(format), t));
            o.insert("set".into(), json!(stored.is_some()));
            o.insert("animated".into(), json!(current.is_animated()));
            Value::Object(o)
        })
        .collect();
    let inputs: Vec<Value> = (0..node.inputs.len().max(spec.inputs.len()))
        .map(|i| json!({ "input": i, "label": spec.inputs.get(i).map_or("", |s| s.name), "from": node.input(i).and_then(|f| s.script.name_of(f).ok()) }))
        .collect();
    Ok(
        json!({ "name": node.name, "class": node.class, "label": node.label, "disabled": node.disabled, "x": node.x, "y": node.y, "knobs": knobs, "inputs": inputs }),
    )
}

fn q_node_classes(s: &mut Session, _: &Value) -> Result<Value> {
    let format = &s.script.root.format;
    let list: Vec<Value> = s
        .registry
        .iter()
        .map(|c| {
            let knobs: Vec<Value> = c
                .knobs
                .iter()
                .map(|k| {
                    let mut o =
                        json!({ "name": k.name, "label": k.label, "default": knob_value_json(&k.default_value(format), FrameTime::ZERO), "help": k.help });
                    if let (Value::Object(o), Value::Object(kind)) = (&mut o, kind_json(k.kind)) {
                        o.extend(kind);
                    }
                    o
                })
                .collect();
            json!({
                "class": c.class, "label": c.label, "aliases": c.aliases, "category": c.category, "shortcut": c.shortcut, "help": c.help,
                "inputs": c.inputs.iter().map(|i| i.name).collect::<Vec<_>>(), "extraInputs": c.extra_inputs, "knobs": knobs,
            })
        })
        .collect();
    Ok(json!(list))
}

fn q_commands(_: &mut Session, _: &Value) -> Result<Value> {
    let list: Vec<Value> = SPECS
        .iter()
        .map(|c| json!({ "id": c.id, "label": c.label, "menu": c.menu, "shortcut": c.shortcut, "params": c.params, "undoable": c.undoable }))
        .collect();
    Ok(json!(list))
}

fn rect_json(r: IRect) -> Value {
    json!({ "x": r.x0, "y": r.y0, "r": r.x1, "t": r.y1 })
}

fn q_info(s: &mut Session, p: &Value) -> Result<Value> {
    let id = params::node(&s.script, p, "node")?;
    let t = frame_of(s, p)?;
    let (script, registry) = (&s.script, &s.registry);
    let info = with_render_stack(|| Renderer::new(script, registry).info(id, t)).map_err(EngineError::Internal)??;
    Ok(json!({
        "format": { "width": info.format.width, "height": info.format.height, "pixel_aspect": info.format.pixel_aspect, "name": info.format.name },
        "bbox": rect_json(info.bbox),
        "channels": info.channels.iter().map(|c| c.name()).collect::<Vec<_>>(),
        "frame": t.as_f64(),
    }))
}

fn q_pixel(s: &mut Session, p: &Value) -> Result<Value> {
    let id = params::node(&s.script, p, "node")?;
    let t = frame_of(s, p)?;
    let to_i32 = |v: i64| v.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    let x = to_i32(params::opt_i64(p, "x")?.ok_or_else(|| bad("`x` is required"))?);
    let y = to_i32(params::opt_i64(p, "y")?.ok_or_else(|| bad("`y` is required"))?);
    let (script, registry) = (&s.script, &s.registry);
    let img = with_render_stack(|| Renderer::new(script, registry).render(id, t, IRect::new(x, y, x.saturating_add(1), y.saturating_add(1))))
        .map_err(EngineError::Internal)??;
    let inside = img.bbox().contains(x, y);
    let mut values = Map::new();
    for (c, v) in nukecraft_render::pixel(&img, x, y) {
        values.insert(c.name().into(), json!(v));
    }
    Ok(json!({ "x": x, "y": y, "frame": t.as_f64(), "inBbox": inside, "values": values }))
}

fn q_history(s: &mut Session, _: &Value) -> Result<Value> {
    Ok(json!({ "undo": s.history(), "journal": s.journal().len(), "dirty": s.dirty }))
}
