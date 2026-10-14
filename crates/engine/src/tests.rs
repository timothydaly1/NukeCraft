use serde_json::{Value, json};

use crate::demo::demo_session;
use crate::{EngineError, Session, command_specs};

fn exec(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn tmpdir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("nukecraft-engine-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn command_ids_are_unique_and_documented() {
    let mut ids: Vec<&str> = command_specs().iter().map(|c| c.id).collect();
    ids.sort_unstable();
    let n = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), n);
    for c in command_specs() {
        assert!(c.id.contains('.'), "{}", c.id);
        assert!(!c.params.is_empty() && !c.label.is_empty(), "{}", c.id);
    }
}

#[test]
fn create_connect_and_undo() {
    let mut s = Session::new();
    exec(&mut s, "node.create", json!({ "class": "Constant", "knobs": { "color": [1, 0, 0, 1] } }));
    let r = exec(&mut s, "node.create", json!({ "class": "Merge", "inputs": ["Constant1"] }));
    assert_eq!(r, json!({ "name": "Merge1", "class": "Merge2" }));
    assert_eq!(s.history(), ["Create Node", "Create Node"]);
    exec(&mut s, "edit.undo", json!({}));
    assert!(s.script.by_name("Merge1").is_none());
    exec(&mut s, "edit.redo", json!({}));
    assert!(s.script.by_name("Merge1").is_some());
    // Undo with nothing left is "not available", not a crash.
    exec(&mut s, "edit.undo", json!({}));
    exec(&mut s, "edit.undo", json!({}));
    assert!(matches!(s.execute("edit.undo", &json!({})), Err(EngineError::Disabled { .. })));
}

#[test]
fn failing_commands_change_nothing() {
    let mut s = Session::new();
    exec(&mut s, "node.create", json!({ "class": "Blur" }));
    let before = s.script.clone();
    // Second input name is wrong: the node created first must not survive.
    assert!(s.execute("node.create", &json!({ "class": "Merge", "inputs": ["Blur1", "Nope"] })).is_err());
    assert_eq!(s.script, before);
    assert_eq!(s.history().len(), 1);
    assert!(s.execute("node.setKnob", &json!({ "node": "Blur1", "knob": "size", "value": "big" })).is_err());
    assert!(s.execute("node.setKnob", &json!({ "node": "Blur1", "knob": "nosuch", "value": 1 })).is_err());
    assert!(s.execute("node.setKnob", &json!({ "node": "Blur1", "knob": "filter", "value": "sharp" })).is_err());
    assert!(s.execute("node.create", &json!({ "class": "NoSuchClass" })).is_err());
    assert!(s.execute("no.such.command", &json!({})).is_err());
    assert_eq!(s.script, before);
}

#[test]
fn hostile_parameters_are_errors() {
    let mut s = Session::new();
    exec(&mut s, "node.create", json!({ "class": "Transform" }));
    for p in [
        json!(null),
        json!({ "node": 5 }),
        json!({ "node": "Transform1", "knob": "translate", "value": [1, 2, 3, 4, 5, 6, 7] }),
        json!({ "node": "Transform1", "knob": "translate", "value": {} }),
        json!({ "node": "Transform1", "knob": "rotate", "value": 1, "frame": "soon" }),
        json!({ "node": "Transform1", "knob": "rotate", "value": 1, "interp": "bouncy" }),
    ] {
        assert!(s.execute("node.setKnob", &p).is_err(), "{p}");
    }
    assert!(s.execute("node.connect", &json!({ "node": "Transform1", "input": 1e300, "from": "Transform1" })).is_err());
    assert!(s.execute("node.connect", &json!({ "node": "Transform1", "input": -1, "from": "Transform1" })).is_err());
    assert!(s.execute("node.connect", &json!({ "node": "Transform1", "input": 0, "from": "Transform1" })).is_err(), "self-loop");
    assert!(s.execute("root.settings", &json!({ "format": { "width": 0, "height": 10 } })).is_err());
    assert!(s.execute("query.pixel", &json!({ "node": "Transform1", "x": 1e15, "y": 0 })).is_ok(), "far pixels clamp");
    assert!(s.execute("query.pixel", &json!({ "node": "Transform1", "x": 1e18, "y": 0 })).is_err(), "beyond i64 precision is an error");
}

#[test]
fn animation_keys_and_animated_edits() {
    let mut s = Session::new();
    exec(&mut s, "node.create", json!({ "class": "Transform" }));
    exec(&mut s, "node.setKnob", json!({ "node": "Transform1", "knob": "translate", "value": [0, 0], "frame": 1, "interp": "linear" }));
    exec(&mut s, "node.setKnob", json!({ "node": "Transform1", "knob": "translate", "value": [10, 20], "frame": 11, "interp": "linear" }));
    exec(&mut s, "time.goto", json!({ "frame": 6 }));
    let node = exec(&mut s, "query.node", json!({ "node": "Transform1" }));
    let tr = node["knobs"].as_array().unwrap().iter().find(|k| k["name"] == "translate").unwrap().clone();
    assert_eq!(tr["value"], json!([5.0, 10.0]));
    assert_eq!(tr["animated"], json!(true));
    // Editing an animated knob without a frame keys the current frame.
    exec(&mut s, "node.setKnob", json!({ "node": "Transform1", "knob": "translate", "value": 3 }));
    let v = &s.script.by_name("Transform1").unwrap().knobs["translate"];
    assert_eq!(v.components().unwrap().len(), 2, "one number keys both components");
    exec(&mut s, "node.removeKey", json!({ "node": "Transform1", "knob": "translate", "frame": 6 }));
    exec(&mut s, "node.clearAnimation", json!({ "node": "Transform1", "knob": "translate" }));
    assert!(!s.script.by_name("Transform1").unwrap().knobs["translate"].is_animated());
}

#[test]
fn delete_reconnects_the_pipe() {
    let mut s = Session::new();
    exec(&mut s, "node.create", json!({ "class": "Constant" }));
    exec(&mut s, "node.create", json!({ "class": "Grade", "inputs": ["Constant1"] }));
    exec(&mut s, "node.create", json!({ "class": "Blur", "inputs": ["Grade1"] }));
    exec(&mut s, "node.delete", json!({ "node": "Grade1" }));
    let nodes = exec(&mut s, "query.nodes", json!({}));
    let blur = nodes.as_array().unwrap().iter().find(|n| n["name"] == "Blur1").unwrap();
    assert_eq!(blur["inputs"], json!(["Constant1"]));
    // Deleting a whole chain joins its ends.
    exec(&mut s, "node.create", json!({ "class": "Grade", "inputs": ["Constant1"] }));
    exec(&mut s, "node.create", json!({ "class": "Saturation", "inputs": ["Grade1"] }));
    exec(&mut s, "node.connect", json!({ "node": "Blur1", "input": 0, "from": "Saturation1" }));
    exec(&mut s, "node.delete", json!({ "nodes": ["Saturation1", "Grade1"] }));
    let nodes = exec(&mut s, "query.nodes", json!({}));
    let blur = nodes.as_array().unwrap().iter().find(|n| n["name"] == "Blur1").unwrap();
    assert_eq!(blur["inputs"], json!(["Constant1"]));
}

#[test]
fn demo_renders_and_saves() {
    let dir = tmpdir("demo");
    let mut s = demo_session().unwrap();
    assert!(s.history().is_empty());
    let info = exec(&mut s, "query.info", json!({ "node": "Write1" }));
    assert_eq!(info["format"]["width"], json!(640));
    // The ramp's top is blue; the checkerboard sits over the middle.
    let top = exec(&mut s, "query.pixel", json!({ "node": "Merge2", "x": 5, "y": 355 }));
    assert!(top["values"]["rgba.blue"].as_f64().unwrap() > 0.3, "{top}");
    assert_eq!(top["values"]["rgba.alpha"].as_f64(), Some(1.0), "opaque background");
    let mid = exec(&mut s, "query.pixel", json!({ "node": "Merge2", "x": 320, "y": 180 }));
    assert!(mid["values"]["rgba.red"].as_f64().unwrap() > 0.5, "{mid}");

    let png = dir.join("frame.png");
    exec(&mut s, "render.frame", json!({ "node": "Write1", "path": png.display().to_string(), "frame": 12 }));
    assert!(png.exists());
    let pattern = dir.join("out.####.png").display().to_string();
    exec(&mut s, "node.setKnob", json!({ "node": "Write1", "knob": "file", "value": pattern }));
    let r = exec(&mut s, "render.execute", json!({ "frames": "1-3" }));
    assert_eq!(r["written"].as_array().unwrap().len(), 3);

    let script = dir.join("demo.ncs");
    exec(&mut s, "file.save", json!({ "path": script.display().to_string() }));
    let mut t = Session::new();
    exec(&mut t, "file.open", json!({ "path": script.display().to_string() }));
    assert_eq!(t.script, s.script);
    assert!(t.load_warnings.is_empty());
}

#[test]
fn queries_describe_everything() {
    let mut s = Session::new();
    let classes = exec(&mut s, "query.nodeClasses", json!({}));
    let names: Vec<&str> = classes.as_array().unwrap().iter().filter_map(|c| c["class"].as_str()).collect();
    for c in ["Constant", "Merge2", "Transform", "Blur", "Grade", "Read", "Write"] {
        assert!(names.contains(&c), "{c}");
    }
    let cmds = exec(&mut s, "query.commands", json!({}));
    assert_eq!(cmds.as_array().unwrap().len(), command_specs().len());
    // Queries are not undo steps or journal entries.
    assert!(s.history().is_empty());
    assert!(s.journal().is_empty());
}

#[test]
fn undo_groups_make_one_step() {
    let mut s = Session::new();
    exec(&mut s, "node.create", json!({ "class": "Blur" }));
    let rev = s.revision;
    s.begin_group("Drag size");
    for v in [1, 2, 3, 4] {
        exec(&mut s, "node.setKnob", json!({ "node": "Blur1", "knob": "size", "value": v }));
    }
    assert!(s.revision > rev);
    s.end_group();
    assert_eq!(s.history(), ["Create Node", "Drag size"]);
    exec(&mut s, "edit.undo", json!({}));
    assert!(!s.script.by_name("Blur1").unwrap().knobs.contains_key("size"));
    // An empty group adds nothing; undo inside a group closes it first.
    s.begin_group("Nothing");
    s.end_group();
    assert_eq!(s.history(), ["Create Node"]);
    s.begin_group("Move");
    exec(&mut s, "node.move", json!({ "moves": [{ "node": "Blur1", "x": 5, "y": 6 }] }));
    exec(&mut s, "edit.undo", json!({}));
    assert_eq!(s.script.by_name("Blur1").unwrap().x, 0.0);
    assert!(!s.in_group());
}

#[test]
fn creation_fixes_format_and_time_defaults() {
    let mut s = Session::new();
    exec(&mut s, "time.goto", json!({ "frame": 12 }));
    exec(&mut s, "node.create", json!({ "class": "FrameHold" }));
    exec(&mut s, "node.create", json!({ "class": "Rectangle" }));
    let knob = |s: &Session, node: &str, k: &str| {
        s.script.by_name(node).and_then(|n| n.knobs.get(k)).and_then(|v| v.eval(nukecraft_time::FrameTime::ZERO).as_numbers().map(<[f64]>::to_vec))
    };
    assert_eq!(knob(&s, "FrameHold1", "first_frame"), Some(vec![12.0]), "holds the frame it was made on");
    let (w, h) = (s.script.root.format.width as f64, s.script.root.format.height as f64);
    assert_eq!(knob(&s, "Rectangle1", "area"), Some(vec![w / 4.0, h / 4.0, w * 0.75, h * 0.75]));
}

#[test]
fn keys_are_edited_one_component_at_a_time() {
    let mut s = Session::new();
    exec(&mut s, "node.create", json!({ "class": "Transform" }));
    // A component of a static knob becomes a curve on its own.
    exec(
        &mut s,
        "node.editKeys",
        json!({ "node": "Transform1", "knob": "translate", "component": 1, "keys": [{ "frame": 1, "value": 0 }, { "frame": 10, "value": 90, "interp": "linear" }] }),
    );
    let value = |s: &Session, f: f64| {
        let n = s.script.by_name("Transform1").unwrap();
        n.knobs.get("translate").unwrap().eval(nukecraft_time::FrameTime::from_f64(f)).as_numbers().unwrap().to_vec()
    };
    assert_eq!(value(&s, 5.5), vec![0.0, 45.0], "x stays static, y is keyed (linear from frame 1)");
    // Moving a key: remove it and set it elsewhere, in one undoable step.
    exec(&mut s, "node.editKeys", json!({ "node": "Transform1", "knob": "translate", "component": 1, "remove": [10], "keys": [{ "frame": 19, "value": 90 }] }));
    assert_eq!(value(&s, 10.0)[1], 45.0, "the key kept its linear interpolation when moved");
    exec(&mut s, "node.setInterpolation", json!({ "node": "Transform1", "knob": "translate", "frames": [1], "interp": "constant" }));
    assert_eq!(value(&s, 10.0)[1], 0.0, "constant: holds the first key");
    exec(&mut s, "edit.undo", json!({}));
    assert_eq!(value(&s, 10.0)[1], 45.0);
    // Wrong frames, components and knobs are errors that change nothing.
    let before = s.script.clone();
    for p in [
        json!({ "node": "Transform1", "knob": "translate", "component": 1, "remove": [7] }),
        json!({ "node": "Transform1", "knob": "translate", "component": 5, "keys": [{ "frame": 1, "value": 1 }] }),
        json!({ "node": "Transform1", "knob": "filter", "keys": [{ "frame": 1, "value": 1 }] }),
        json!({ "node": "Transform1", "knob": "translate" }),
        json!({ "node": "Transform1", "knob": "translate", "keys": [{ "frame": 1 }] }),
    ] {
        assert!(s.execute("node.editKeys", &p).is_err(), "{p}");
    }
    assert!(s.execute("node.setInterpolation", &json!({ "node": "Transform1", "knob": "translate", "frames": [3], "interp": "linear" })).is_err());
    assert_eq!(s.script, before);
    // Removing the last keys leaves the value it held.
    exec(&mut s, "time.goto", json!({ "frame": 19 }));
    exec(&mut s, "node.editKeys", json!({ "node": "Transform1", "knob": "translate", "component": 1, "remove": [1, 19] }));
    assert_eq!(value(&s, 1.0)[1], 90.0);
    // A colour stored as one number widens to RGB when one channel is keyed.
    exec(&mut s, "node.create", json!({ "class": "Grade" }));
    exec(&mut s, "node.editKeys", json!({ "node": "Grade1", "knob": "white", "component": 1, "keys": [{ "frame": 1, "value": 2 }] }));
    let n = s.script.by_name("Grade1").unwrap();
    assert_eq!(n.knobs.get("white").unwrap().components().unwrap().len(), 3);
}
