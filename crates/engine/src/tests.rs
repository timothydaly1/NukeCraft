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

#[test]
fn copy_and_paste_nodes() {
    let mut s = demo_session().unwrap();
    let text = exec(&mut s, "edit.copy", json!({ "nodes": ["Grade1", "Blur1"] }))["text"].as_str().unwrap().to_string();
    let before = s.script.nodes().len();
    let r = exec(&mut s, "edit.paste", json!({ "text": text }));
    assert_eq!(r["names"], json!(["Grade2", "Blur2"]), "taken names are renumbered");
    assert_eq!(s.script.nodes().len(), before + 2);
    let input = |s: &crate::Session, n: &str| s.script.by_name(n).and_then(|n| n.input(0)).and_then(|i| s.script.name_of(i).ok().map(str::to_string));
    assert_eq!(input(&s, "Blur2").as_deref(), Some("Grade2"), "the pipe between pasted nodes");
    assert_eq!(input(&s, "Grade2").as_deref(), Some("Merge2"), "the pipe from outside, by name");
    let (g1, g2) = (s.script.by_name("Grade1").unwrap(), s.script.by_name("Grade2").unwrap());
    assert_eq!(g1.knobs, g2.knobs);
    assert_eq!((g2.x - g1.x, g2.y - g1.y), (40.0, 40.0));
    exec(&mut s, "edit.undo", json!({}));
    assert_eq!(s.script.nodes().len(), before, "one undo step");
    // Positioned paste, then cut.
    exec(&mut s, "edit.paste", json!({ "text": text, "x": 1000, "y": 500 }));
    let g = s.script.by_name("Grade2").unwrap();
    assert_eq!((g.x, g.y), (1000.0, 500.0));
    let cut = exec(&mut s, "edit.cut", json!({ "nodes": ["Grade2", "Blur2"] }));
    assert!(cut["text"].as_str().unwrap().contains("Grade2"));
    assert!(s.script.by_name("Grade2").is_none());
    // Into an empty script: no outside pipes, original names.
    let mut empty = crate::Session::new();
    exec(&mut empty, "edit.paste", json!({ "text": text }));
    assert_eq!(input(&empty, "Grade1"), None);
    assert_eq!(input(&empty, "Blur1").as_deref(), Some("Grade1"));
    // Garbage and hostile fragments are errors that change nothing.
    let snapshot = empty.script.clone();
    for bad in [
        json!({ "text": "not json" }),
        json!({ "text": "{\"nodes\": []}" }),
        json!({ "text": "{\"nukecraft\": \"fragment\", \"nodes\": [{\"class\": \"NoSuchClass\"}]}" }),
        json!({ "text": "{\"nukecraft\": \"fragment\", \"nodes\": [{\"class\": \"Blur\", \"knobs\": 5}]}" }),
        json!({ "text": 3 }),
    ] {
        assert!(empty.execute("edit.paste", &bad).is_err(), "{bad}");
    }
    assert_eq!(empty.script, snapshot);
    // Unknown knobs and ill-typed values are dropped, not fatal.
    exec(
        &mut empty,
        "edit.paste",
        json!({ "text": "{\"nukecraft\": \"fragment\", \"nodes\": [{\"class\": \"Blur\", \"name\": \"B\", \"knobs\": {\"size\": \"big\", \"nope\": 1, \"filter\": \"box\"}}]}" }),
    );
    let b = empty.script.by_name("B").unwrap();
    assert_eq!(b.knobs.len(), 1, "{:?}", b.knobs);
}

/// Red at a pixel of a node at frame 12.
fn red_at(s: &crate::Session, node: &str, x: i32, y: i32) -> f32 {
    let id = s.script.id_of(node).unwrap();
    let (script, reg) = (&s.script, &s.registry);
    let img = nukecraft_render::with_render_stack(|| nukecraft_render::Renderer::new(script, reg).render_format(id, nukecraft_time::FrameTime::from_frame(12)))
        .unwrap()
        .unwrap()
        .1;
    img.get_at(0, x, y)
}

#[test]
fn grouping_keeps_the_picture() {
    let mut s = demo_session().unwrap();
    let before: Vec<f32> = [(100, 100), (320, 180), (500, 300)].iter().map(|(x, y)| red_at(&s, "Write1", *x, *y)).collect();
    let r = exec(&mut s, "node.group", json!({ "nodes": ["Grade1", "Blur1"] }));
    assert_eq!(r["group"], "Group1");
    assert!(s.script.by_name("Group1.Grade1").is_some() && s.script.by_name("Group1.Input1").is_some() && s.script.by_name("Group1.Output1").is_some());
    assert!(s.script.by_name("Grade1").is_none(), "moved inside");
    let input = |s: &crate::Session, n: &str| s.script.by_name(n).and_then(|n| n.input(0)).and_then(|i| s.script.path_of(i).ok());
    assert_eq!(input(&s, "Write1").as_deref(), Some("Group1"));
    assert_eq!(input(&s, "Group1").as_deref(), Some("Merge2"));
    assert_eq!(input(&s, "Group1.Grade1").as_deref(), Some("Group1.Input1"));
    assert_eq!(input(&s, "Group1.Blur1").as_deref(), Some("Group1.Grade1"), "the pipe between grouped nodes survives");
    let grouped: Vec<f32> = [(100, 100), (320, 180), (500, 300)].iter().map(|(x, y)| red_at(&s, "Write1", *x, *y)).collect();
    assert_eq!(grouped, before, "the picture is the same through the group");
    // Knobs of nested nodes by path; nodes made inside by `group`.
    exec(&mut s, "node.setKnob", json!({ "node": "Group1.Grade1", "knob": "white", "value": 1.5 }));
    exec(&mut s, "node.create", json!({ "class": "Saturation", "group": "Group1", "inputs": ["Blur1"] }));
    assert_eq!(input(&s, "Group1.Saturation1").as_deref(), Some("Group1.Blur1"));
    assert!(s.execute("node.connect", &json!({ "node": "Group1.Saturation1", "input": 0, "from": "Merge2" })).is_err(), "pipes stay in their group");
    let nodes = exec(&mut s, "query.nodes", json!({}));
    assert!(nodes.as_array().unwrap().iter().any(|n| n["name"] == "Group1.Grade1" && n["group"] == "Group1"));
    // Copy the whole group and paste it: its contents come along.
    let text = exec(&mut s, "edit.copy", json!({ "nodes": ["Group1"] }))["text"].as_str().unwrap().to_string();
    let r = exec(&mut s, "edit.paste", json!({ "text": text }));
    assert_eq!(r["names"], json!(["Group2"]));
    assert_eq!(input(&s, "Group2.Blur1").as_deref(), Some("Group2.Grade1"));
    assert_eq!(input(&s, "Group2").as_deref(), Some("Merge2"), "outside input by name");
    // Expanding puts things back.
    exec(&mut s, "edit.undo", json!({}));
    exec(&mut s, "edit.undo", json!({}));
    exec(&mut s, "edit.undo", json!({}));
    let r = exec(&mut s, "node.ungroup", json!({ "node": "Group1" }));
    assert_eq!(r["nodes"], json!(["Grade1", "Blur1"]));
    assert_eq!(input(&s, "Write1").as_deref(), Some("Blur1"));
    assert_eq!(input(&s, "Grade1").as_deref(), Some("Merge2"));
    let after: Vec<f32> = [(100, 100), (320, 180), (500, 300)].iter().map(|(x, y)| red_at(&s, "Write1", *x, *y)).collect();
    assert_eq!(after, before);
    assert!(s.execute("node.ungroup", &json!({ "node": "Grade1" })).is_err(), "not a group");
    assert!(s.execute("node.create", &json!({ "class": "Blur", "group": "Grade1" })).is_err(), "not a group");
}
