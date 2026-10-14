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
