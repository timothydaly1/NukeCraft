//! The menu bar, built from the command registry (each command's `menu` path) and the node
//! registry (one Nodes submenu per category).

use nukecraft_engine::command_specs;

use crate::{NukecraftApp, theme};

/// Top-level menus, in order. Commands name their menu; these are the ones that exist.
const MENUS: &[&str] = &["File", "Edit", "Render"];

pub fn menu_bar(app: &mut NukecraftApp, ui: &mut egui::Ui) {
    egui::MenuBar::new().ui(ui, |ui| {
        for &menu in MENUS {
            let r = ui.menu_button(menu, |ui| command_menu(app, ui, menu));
            app.automation.add(format!("menu.{menu}"), r.response.rect, menu);
        }
        let r = ui.menu_button("Nodes", |ui| nodes_menu(app, ui));
        app.automation.add("menu.Nodes", r.response.rect, "Nodes");
        let r = ui.menu_button("Help", |ui| {
            if ui.button("About NukeCraft").clicked() {
                app.dialogs.about = true;
                ui.close();
            }
            if ui.button("Keyboard Shortcuts").clicked() {
                app.dialogs.shortcuts = true;
                ui.close();
            }
        });
        app.automation.add("menu.Help", r.response.rect, "Help");
    });
}

fn command_menu(app: &mut NukecraftApp, ui: &mut egui::Ui, menu: &str) {
    for spec in command_specs().iter().filter(|c| c.menu.first() == Some(&menu)) {
        let enabled = (spec.enabled)(&app.session).is_ok() && needs_met(app, spec.id);
        let mut button = egui::Button::new(spec.label);
        if let Some(sc) = spec.shortcut {
            button = button.shortcut_text(sc);
        }
        let r = ui.add_enabled(enabled, button);
        app.automation.add(format!("menu.{menu}.{}", spec.id), r.rect, spec.label);
        if r.clicked() {
            ui.close();
            app.invoke(spec.id);
        }
    }
    if menu == "File" {
        ui.separator();
        if ui.button("Quit").clicked() {
            ui.close();
            if app.session.dirty {
                app.dialogs.confirm = Some(crate::dialogs::Pending::Quit);
            } else {
                let ctx = ui.ctx().clone();
                app.quit(&ctx);
            }
        }
    }
}

/// Whether the UI context a command needs is there (a selection to delete…).
fn needs_met(app: &NukecraftApp, id: &str) -> bool {
    match id {
        "node.delete" | "node.disable" => !app.selection.is_empty(),
        "render.frame" => app.viewed.is_some(),
        _ => true,
    }
}

fn nodes_menu(app: &mut NukecraftApp, ui: &mut egui::Ui) {
    let mut categories: Vec<&'static str> = app.session.registry.iter().map(|s| s.category).collect();
    // Nuke's toolbar order; any other category after them, by name (so duplicates are adjacent).
    categories.sort_by_key(|c| (theme::CATEGORY_ORDER.iter().position(|o| o == c).unwrap_or(usize::MAX), *c));
    categories.dedup();
    let mut create = None;
    for cat in categories {
        ui.menu_button(cat, |ui| {
            for spec in app.session.registry.iter().filter(|s| s.category == cat) {
                let mut b = egui::Button::new(spec.label);
                if let Some(sc) = spec.shortcut {
                    b = b.shortcut_text(sc);
                }
                let r = ui.add(b);
                app.automation.add(format!("menu.Nodes.{}", spec.class), r.rect, spec.label);
                if r.clicked() {
                    create = Some(spec.class);
                    ui.close();
                }
            }
        });
    }
    if let Some(class) = create {
        app.create_node(class, false);
    }
}
