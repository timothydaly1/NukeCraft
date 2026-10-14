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
