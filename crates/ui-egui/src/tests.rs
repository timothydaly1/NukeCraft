use std::time::Duration;

use egui::{Modifiers, Vec2};
use nukecraft_engine::Session;
use nukecraft_engine::demo::demo_session;

use crate::snapshot::Headless;

fn demo() -> Headless {
    let mut h = Headless::new(demo_session().unwrap(), Vec2::new(1280.0, 800.0), 1.0);
    h.settle(Duration::from_secs(20));
    h
}

#[test]
fn the_shell_draws_and_registers_its_menus() {
    let mut h = demo();
    for id in ["menu.File", "menu.Edit", "menu.Render", "menu.Nodes", "menu.Help"] {
        assert!(h.app.automation.find(id).is_some(), "{id}");
    }
    let snap = h.snapshot();
    assert_eq!((snap.width, snap.height), (1280, 800));
    // Not a blank frame: the menu bar text differs from the panel colour.
    let distinct: std::collections::HashSet<[u8; 4]> = (0..snap.width).step_by(3).filter_map(|x| snap.pixel(x, 10)).collect();
    assert!(distinct.len() > 3, "menu bar looks empty");
    h.click("menu.Help", 1, Modifiers::NONE).unwrap();
    assert!(h.app.automation.find("menu.Help").is_some());
}

#[test]
fn shortcuts_run_commands() {
    let mut h = Headless::new(Session::new(), Vec2::new(1024.0, 700.0), 1.0);
    h.frames(2);
    h.app.create_node("Constant", false);
    assert_eq!(h.app.session.history().len(), 1);
    h.key(egui::Key::Z, Modifiers::COMMAND);
    assert!(h.app.session.script.nodes().is_empty(), "Cmd+Z undid the creation");
    h.key(egui::Key::Z, Modifiers::COMMAND | Modifiers::SHIFT);
    assert_eq!(h.app.session.script.nodes().len(), 1, "Cmd+Shift+Z redid it");
}

#[test]
fn creating_with_a_selection_inserts_one_undo_step() {
    let mut h = demo();
    let grade = h.app.session.script.id_of("Grade1").unwrap();
    h.app.selection = vec![grade];
    let new = h.app.create_node("Saturation", false).unwrap();
    let script = &h.app.session.script;
    assert_eq!(script.node(new).unwrap().input(0), Some(grade));
    let blur = script.by_name("Blur1").unwrap();
    assert_eq!(blur.input(0), Some(new), "Blur1 now reads the inserted node");
    assert_eq!(h.app.session.history(), ["Create Saturation"]);
    // Shift (branch): the downstream pipes stay where they are.
    h.app.selection = vec![grade];
    h.app.create_node("Saturation", true).unwrap();
    assert_eq!(h.app.session.script.by_name("Blur1").unwrap().input(0), Some(new));
}

mod graph {
    use super::*;
    use egui::{Key, PointerButton, Pos2};

    fn center(h: &Headless, id: &str) -> Pos2 {
        h.app.automation.center(id).unwrap_or_else(|| panic!("no element {id}"))
    }

    fn input_of(h: &Headless, node: &str, i: usize) -> Option<String> {
        let s = &h.app.session.script;
        s.by_name(node).and_then(|n| n.input(i)).and_then(|u| s.name_of(u).ok()).map(str::to_string)
    }

    #[test]
    fn dragging_a_node_moves_it_in_one_step() {
        let mut h = demo();
        let before = h.app.session.script.by_name("Grade1").map(|n| (n.x, n.y)).unwrap();
        let from = center(&h, "graph.node.Grade1");
        h.drag_at(from, from + egui::Vec2::new(-120.0, 0.0), PointerButton::Primary, Modifiers::NONE);
        let after = h.app.session.script.by_name("Grade1").map(|n| (n.x, n.y)).unwrap();
        assert!(after.0 < before.0 - 50.0, "{before:?} → {after:?}");
        assert_eq!(after.1, before.1);
        assert_eq!(h.app.session.history(), ["Move Node"]);
    }

    #[test]
    fn pipes_connect_and_disconnect_by_dragging() {
        let mut h = demo();
        // Ramp1's output onto Merge2: its first empty non-mask input is... none (B and A are
        // full), so the mask.
        h.drag("graph.node.Ramp1.output", "graph.node.Merge2").unwrap();
        assert_eq!(input_of(&h, "Merge2", 2).as_deref(), Some("Ramp1"));
        // Pull Merge2's A input off into empty space: disconnected.
        let a = center(&h, "graph.node.Merge2.input.1");
        h.drag_at(a, a + egui::Vec2::new(-30.0, -200.0), PointerButton::Primary, Modifiers::NONE);
        assert_eq!(input_of(&h, "Merge2", 1), None);
        // And drag that input onto CheckerBoard1.
        h.drag("graph.node.Merge2.input.1", "graph.node.CheckerBoard1").unwrap();
        assert_eq!(input_of(&h, "Merge2", 1).as_deref(), Some("CheckerBoard1"));
        // A pipe that would close a loop is refused with a message, not a crash.
        h.drag("graph.node.Write1.output", "graph.node.Grade1").unwrap();
        assert_eq!(input_of(&h, "Grade1", 0).as_deref(), Some("Merge2"));
    }

    #[test]
    fn box_select_then_delete() {
        let mut h = demo();
        let a = center(&h, "graph.node.Grade1");
        let b = center(&h, "graph.node.Blur1");
        h.drag_at(a + egui::Vec2::new(-80.0, -20.0), b + egui::Vec2::new(80.0, 20.0), PointerButton::Primary, Modifiers::NONE);
        let mut names = h.app.selected_names();
        names.sort();
        assert_eq!(names, ["Blur1", "Grade1"]);
        h.key(Key::Delete, Modifiers::NONE);
        assert!(h.app.session.script.by_name("Grade1").is_none());
        // Deleting reconnected the pipe: Write1 now reads Merge2.
        assert_eq!(input_of(&h, "Write1", 0).as_deref(), Some("Merge2"));
    }

    #[test]
    fn keys_create_insert_and_branch() {
        let mut h = demo();
        h.click("graph.node.Grade1", 1, Modifiers::NONE).unwrap();
        h.hover(center(&h, "graph.canvas"));
        h.key(Key::B, Modifiers::NONE);
        assert_eq!(input_of(&h, "Blur2", 0).as_deref(), Some("Grade1"));
        assert_eq!(input_of(&h, "Blur1", 0).as_deref(), Some("Blur2"), "inserted");
        h.click("graph.node.Grade1", 1, Modifiers::NONE).unwrap();
        h.key(Key::G, Modifiers::SHIFT);
        assert_eq!(input_of(&h, "Grade2", 0).as_deref(), Some("Grade1"));
        assert_eq!(input_of(&h, "Blur2", 0).as_deref(), Some("Grade1"), "branched: Blur2 still reads Grade1");
        assert_eq!(h.app.session.history(), ["Create Blur", "Create Grade"]);
        // K over the graph is Copy (over the Viewer it stops playback).
        h.click("graph.node.Grade1", 1, Modifiers::NONE).unwrap();
        h.key(Key::K, Modifiers::NONE);
        assert_eq!(input_of(&h, "Copy1", 0).as_deref(), Some("Grade1"));
        // 1 views the selection.
        h.click("graph.node.Merge1", 1, Modifiers::NONE).unwrap();
        h.key(Key::Num1, Modifiers::NONE);
        assert_eq!(h.app.viewed, h.app.session.script.id_of("Merge1").ok());
    }

    #[test]
    fn tab_search_creates_the_chosen_class() {
        let mut h = demo();
        h.app.selection.clear();
        h.hover(center(&h, "graph.canvas"));
        h.key(Key::Tab, Modifiers::NONE);
        assert!(h.app.automation.find("graph.search").is_some());
        h.text("prem");
        h.key(Key::Enter, Modifiers::NONE);
        assert!(h.app.session.script.by_name("Premult1").is_some());
        h.frames(2);
        assert!(h.app.automation.find("graph.search").is_none(), "the popup closed");
        // Escape closes without creating.
        h.key(Key::Tab, Modifiers::NONE);
        h.text("blur");
        h.key(Key::Escape, Modifiers::NONE);
        assert!(h.app.session.script.by_name("Blur2").is_none());
    }

    #[test]
    fn double_click_opens_properties() {
        let mut h = demo();
        h.click("graph.node.Transform1", 2, Modifiers::NONE).unwrap();
        assert_eq!(h.app.open_props.first().copied(), h.app.session.script.id_of("Transform1").ok());
    }
}

mod viewer {
    use super::*;
    use egui::Key;
    use serde_json::json;

    /// The colour at the centre of the Viewer's image area.
    fn viewer_center(h: &Headless) -> [u8; 4] {
        let c = h.app.automation.center("viewer.image").expect("viewer");
        h.snapshot().pixel(c.x as usize, c.y as usize).expect("pixel")
    }

    #[test]
    fn shows_the_viewed_node_and_follows_edits() {
        let mut h = demo();
        h.app.session.execute("time.goto", &json!({ "frame": 12 })).unwrap();
        h.settle(Duration::from_secs(20));
        let before = viewer_center(&h);
        // The centre of frame 12 is the orange/cream checkerboard, not the dark background.
        assert!(before[0] > 150, "{before:?}");
        // Turn the Grade down: the Viewer re-renders.
        h.app.session.execute("node.setKnob", &json!({ "node": "Grade1", "knob": "white", "value": 0.2 })).unwrap();
        h.settle(Duration::from_secs(20));
        let after = viewer_center(&h);
        assert!(after[0] < before[0] - 40, "{before:?} → {after:?}");
    }

    #[test]
    fn channel_keys_switch_the_display() {
        let mut h = demo();
        h.app.session.execute("time.goto", &json!({ "frame": 12 })).unwrap();
        h.settle(Duration::from_secs(20));
        let c = h.app.automation.center("viewer.image").unwrap();
        h.hover(c);
        h.key(Key::A, Modifiers::NONE);
        h.settle(Duration::from_secs(5));
        let p = viewer_center(&h);
        assert_eq!((p[0], p[1], p[2]), (255, 255, 255), "alpha is 1 everywhere: white");
        h.key(Key::A, Modifiers::NONE);
        h.frames(2);
        assert_ne!(viewer_center(&h)[2], 255, "back to RGB");
    }

    #[test]
    fn timeline_steps_and_plays() {
        let mut h = demo();
        h.click("viewer.next", 1, Modifiers::NONE).unwrap();
        assert_eq!(h.app.session.time.floor_frame(), 2);
        h.click("viewer.last", 1, Modifiers::NONE).unwrap();
        assert_eq!(h.app.session.time.floor_frame(), 24);
        h.click("viewer.play", 1, Modifiers::NONE).unwrap();
        // Playing wraps from the last frame to the first and keeps going.
        for _ in 0..200 {
            h.settle(Duration::from_millis(200));
            if (2..24).contains(&h.app.session.time.floor_frame()) {
                break;
            }
        }
        assert!((1..24).contains(&h.app.session.time.floor_frame()), "playing moved on from frame 24");
        h.key(Key::K, Modifiers::NONE);
        assert!(!h.app.viewer.playing);
        // Time changes are not undo steps or journal entries.
        assert!(h.app.session.history().is_empty());
    }

    /// A session with a small Constant feeding a Write into a fresh temp directory.
    fn writer(name: &str, frames: i64) -> (Headless, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("nukecraft-ui-render-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = Session::new();
        s.execute("root.settings", &json!({ "format": { "width": 32, "height": 16 }, "first": 1, "last": frames })).unwrap();
        s.execute("node.create", &json!({ "class": "Constant", "knobs": { "color": [0.5, 0.25, 0.1, 1] } })).unwrap();
        let file = dir.join("out.####.png").display().to_string();
        s.execute("node.create", &json!({ "class": "Write", "inputs": ["Constant1"], "knobs": { "file": file } })).unwrap();
        let mut h = Headless::new(s, Vec2::new(1024.0, 700.0), 1.0);
        h.frames(2);
        (h, dir)
    }

    #[test]
    fn render_runs_in_the_background_and_reports() {
        let (mut h, dir) = writer("bg", 3);
        h.app.invoke("render.execute");
        assert!(h.app.busy(), "the job runs while the window keeps drawing");
        h.settle(Duration::from_secs(20));
        assert!(h.app.render_job.is_none());
        for f in 1..=3 {
            assert!(dir.join(format!("out.{f:04}.png")).exists(), "frame {f}");
        }
        assert!(h.app.status.as_ref().is_some_and(|s| s.text == "Rendered 3 files" && !s.error));
        // Rendering is not an edit: no undo step beyond the three that built the script.
        assert_eq!(h.app.session.history().len(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_render_can_be_cancelled() {
        let (mut h, dir) = writer("cancel", 2000);
        h.app.invoke("render.execute");
        h.frames(2);
        assert!(h.app.automation.find("render.progress").is_some(), "progress shows");
        h.click("render.cancel", 1, Modifiers::NONE).unwrap();
        h.settle(Duration::from_secs(20));
        assert!(h.app.render_job.is_none());
        let status = h.app.status.as_ref().map(|s| s.text.clone()).unwrap_or_default();
        assert!(status.starts_with("Render cancelled after"), "{status}");
        let written = std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0);
        assert!(written < 2000, "stopped early ({written} files)");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn render_errors_are_shown_not_fatal() {
        let mut h = Headless::new(Session::new(), Vec2::new(1024.0, 700.0), 1.0);
        h.frames(2);
        h.app.session.execute("node.create", &json!({ "class": "Read", "knobs": { "file": "/nonexistent/plate.####.exr" } })).unwrap();
        h.app.viewed = h.app.session.script.id_of("Read1").ok();
        h.settle(Duration::from_secs(10));
        let err = h.app.viewer_error().expect("an error is shown");
        assert!(err.contains("Read1"), "{err}");
    }
}

mod properties {
    use super::*;
    use egui::{PointerButton, Vec2};
    use nukecraft_graph::KnobValue;
    use serde_json::json;

    fn knob(h: &Headless, node: &str, knob: &str) -> Option<KnobValue> {
        h.app.session.script.by_name(node).and_then(|n| n.knobs.get(knob)).cloned()
    }

    #[test]
    fn dragging_a_number_is_one_undo_step() {
        let mut h = demo();
        h.click("graph.node.Blur1", 2, Modifiers::NONE).unwrap();
        h.frames(2);
        let field = h.app.automation.center("props.Blur1.size").expect("size field");
        h.drag_at(field, field + Vec2::new(100.0, 0.0), PointerButton::Primary, Modifiers::NONE);
        let v = knob(&h, "Blur1", "size").expect("size set");
        let first = v.components().unwrap()[0].eval(nukecraft_time::FrameTime::ZERO);
        assert!(first > 1.8, "dragged up from 1.5, got {first}");
        assert_eq!(h.app.session.history(), ["Set size"], "one step for the whole drag");
        h.key(egui::Key::Z, Modifiers::COMMAND);
        assert_eq!(knob(&h, "Blur1", "size"), Some(KnobValue::number(1.5)));
    }

    #[test]
    fn key_button_sets_and_deletes_keys() {
        let mut h = demo();
        h.click("graph.node.Grade1", 2, Modifiers::NONE).unwrap();
        h.app.session.execute("time.goto", &json!({ "frame": 5 })).unwrap();
        h.settle(Duration::from_secs(10));
        h.click("props.Grade1.gamma.key", 1, Modifiers::NONE).unwrap();
        assert!(knob(&h, "Grade1", "gamma").is_some_and(|v| v.is_animated()), "keyed");
        h.click("props.Grade1.gamma.key", 1, Modifiers::NONE).unwrap();
        assert!(!knob(&h, "Grade1", "gamma").is_some_and(|v| v.is_animated()), "the only key deleted");
    }

    #[test]
    fn rename_and_disable() {
        let mut h = demo();
        h.click("graph.node.Blur1", 2, Modifiers::NONE).unwrap();
        h.click("props.Blur1.name", 1, Modifiers::NONE).unwrap();
        h.key(egui::Key::A, Modifiers::COMMAND);
        h.text("Soften");
        h.key(egui::Key::Enter, Modifiers::NONE);
        h.settle(Duration::from_secs(5));
        assert!(h.app.session.script.by_name("Soften").is_some(), "renamed");
        h.click("props.Soften.disable", 1, Modifiers::NONE).unwrap();
        assert!(h.app.session.script.by_name("Soften").unwrap().disabled);
        // A name that's taken is refused and the field goes back.
        h.click("props.Soften.name", 1, Modifiers::NONE).unwrap();
        h.key(egui::Key::A, Modifiers::COMMAND);
        h.text("Grade1");
        h.key(egui::Key::Enter, Modifiers::NONE);
        h.settle(Duration::from_secs(5));
        assert!(h.app.session.script.by_name("Soften").is_some());
    }

    #[test]
    fn text_knobs_commit_on_enter() {
        let mut h = demo();
        h.click("graph.node.Write1", 2, Modifiers::NONE).unwrap();
        h.click("props.Write1.file", 1, Modifiers::NONE).unwrap();
        h.key(egui::Key::A, Modifiers::COMMAND);
        h.text("out/comp.####.exr");
        // Nothing is committed while typing.
        assert_eq!(knob(&h, "Write1", "file"), Some(KnobValue::text("nukecraft-demo.####.png")));
        assert!(h.ctx().text_edit_focused(), "the field has focus while typing");
        h.key(egui::Key::Enter, Modifiers::NONE);
        h.settle(Duration::from_secs(5));
        assert_eq!(knob(&h, "Write1", "file"), Some(KnobValue::text("out/comp.####.exr")));
    }
}

mod control {
    use super::*;
    use serde_json::{Value, json};
    use std::io::{BufRead, BufReader, Write};

    #[test]
    fn requests_drive_the_app() {
        let mut h = demo();
        let ctx = h.ctx().clone();
        let port = h.app.start_control(&ctx, 0).unwrap();
        let client = std::thread::spawn(move || {
            let stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut out = stream;
            let mut ask = |req: Value| -> Value {
                writeln!(out, "{req}").unwrap();
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                serde_json::from_str(&line).unwrap()
            };
            let a = ask(
                json!({ "id": 1, "method": "engine.execute", "params": { "command": "node.create", "params": { "class": "Blur", "inputs": ["Grade1"] } } }),
            );
            let b = ask(json!({ "id": 2, "method": "app.view", "params": { "node": "Blur2" } }));
            let c = ask(json!({ "id": 3, "method": "ui.click", "params": { "id": "graph.node.Merge1" } }));
            let d = ask(json!({ "id": 4, "method": "nope" }));
            let e = ask(json!({ "id": 5, "method": "app.state" }));
            (a, b, c, d, e)
        });
        // The UI thread answers between frames.
        let start = std::time::Instant::now();
        while !client.is_finished() && start.elapsed() < Duration::from_secs(30) {
            h.frame();
            std::thread::sleep(Duration::from_millis(2));
        }
        let (a, b, c, d, e) = client.join().unwrap();
        assert_eq!(a["id"], 1);
        assert_eq!(a["result"]["name"], "Blur2");
        assert_eq!(b["result"], json!({}));
        assert_eq!(c["result"], json!({}));
        assert!(d["error"].as_str().unwrap().contains("unknown method"));
        assert_eq!(e["result"]["viewed"], "Blur2");
        h.frames(8);
        assert_eq!(h.app.selected_names(), ["Merge1"], "the injected click selected Merge1");
    }
}
