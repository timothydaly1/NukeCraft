//! Windows and dialogs: unsaved-changes confirmation, Project Settings, About, keyboard
//! shortcuts, and the platform file pickers.

use egui::{Context, Id};
use nukecraft_engine::command_specs;
use nukecraft_geom::{Format, NAMED_FORMATS};
use nukecraft_graph::Root;
use serde_json::json;

use crate::NukecraftApp;

/// What to do once unsaved changes are dealt with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pending {
    New,
    Open,
    Quit,
}

impl Pending {
    pub fn from_id(id: &str) -> Pending {
        if id == "file.open" { Pending::Open } else { Pending::New }
    }
}

/// The Project Settings form, edited locally and applied as one `root.settings`.
#[derive(Clone, Debug)]
pub struct SettingsForm {
    width: u32,
    height: u32,
    pixel_aspect: f64,
    fps: f64,
    first: i64,
    last: i64,
}

#[derive(Default)]
pub struct Dialogs {
    pub confirm: Option<Pending>,
    pub about: bool,
    pub shortcuts: bool,
    pub settings: Option<SettingsForm>,
}

impl Dialogs {
    pub fn open_settings(&mut self, root: &Root) {
        self.settings = Some(SettingsForm {
            width: root.format.width,
            height: root.format.height,
            pixel_aspect: root.format.pixel_aspect,
            fps: root.fps.as_f64(),
            first: root.frames.first,
            last: root.frames.last,
        });
    }
}

pub fn show(app: &mut NukecraftApp, ctx: &Context) {
    confirm(app, ctx);
    settings(app, ctx);
    if app.dialogs.about {
        let mut open = true;
        egui::Window::new("About NukeCraft").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.heading(format!("NukeCraft {}", env!("CARGO_PKG_VERSION")));
            ui.label("A clean-room, open-source node-based compositor in pure Rust.");
            ui.label("Licensed under MIT OR Apache-2.0.");
            ui.add_space(6.0);
            ui.small("Nuke is a trademark of The Foundry Visionmongers Ltd. NukeCraft is an independent project, not affiliated with or endorsed by Foundry.");
        });
        app.dialogs.about = open;
    }
    if app.dialogs.shortcuts {
        let mut open = true;
        egui::Window::new("Keyboard Shortcuts").open(&mut open).default_width(420.0).show(ctx, |ui| {
            egui::Grid::new("shortcuts").striped(true).show(ui, |ui| {
                for c in command_specs() {
                    if let Some(sc) = c.shortcut {
                        ui.label(c.label);
                        ui.monospace(sc);
                        ui.end_row();
                    }
                }
                for s in app.session.registry.iter() {
                    if let Some(sc) = s.shortcut {
                        ui.label(format!("Create {} (Node Graph)", s.label));
                        ui.monospace(sc);
                        ui.end_row();
                    }
                }
                for (what, key) in [
                    ("Node search (Node Graph)", "Tab"),
                    ("View the selected node", "1"),
                    ("Frame all / selected (Node Graph)", "F"),
                    ("Fit image (Viewer)", "F"),
                    ("Branch instead of insert", "Shift + node key"),
                    ("Previous / next frame", "Left / Right"),
                    ("Play / stop", "L / K"),
                ] {
                    ui.label(what);
                    ui.monospace(key);
                    ui.end_row();
                }
            });
        });
        app.dialogs.shortcuts = open;
    }
}

fn confirm(app: &mut NukecraftApp, ctx: &Context) {
    let Some(pending) = app.dialogs.confirm else { return };
    let mut choice = None;
    egui::Modal::new(Id::new("unsaved")).show(ctx, |ui| {
        ui.heading("Unsaved changes");
        ui.label("Save the changes to this script first?");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            for (label, c) in [("Save", 1), ("Don't Save", 2), ("Cancel", 0)] {
                let r = ui.button(label);
                app.automation.add(format!("dialog.unsaved.{}", label.to_lowercase().replace(['\'', ' '], "")), r.rect, label);
                if r.clicked() {
                    choice = Some(c);
                }
            }
        });
    });
    let Some(choice) = choice else { return };
    app.dialogs.confirm = None;
    if choice == 0 {
        return;
    }
    if choice == 1 {
        app.invoke("file.save");
        if app.session.dirty {
            return; // Saving failed or was cancelled.
        }
    }
    match pending {
        Pending::Quit => app.quit(ctx),
        Pending::New => {
            app.session.dirty = false;
            app.invoke("file.new");
        }
        Pending::Open => {
            app.session.dirty = false;
            app.invoke("file.open");
        }
    }
}

fn settings(app: &mut NukecraftApp, ctx: &Context) {
    let Some(mut form) = app.dialogs.settings.clone() else { return };
    let mut open = true;
    let mut apply = false;
    egui::Window::new("Project Settings").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        egui::Grid::new("root_settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("format");
            let current = Format::new(form.width, form.height, form.pixel_aspect);
            let label = NAMED_FORMATS
                .iter()
                .find(|(_, w, h, pa)| *w == current.width && *h == current.height && (*pa - current.pixel_aspect).abs() < 1e-6)
                .map_or_else(|| format!("{}×{}", current.width, current.height), |(n, ..)| n.to_string());
            egui::ComboBox::from_id_salt("format").selected_text(label).show_ui(ui, |ui| {
                for (name, w, h, pa) in NAMED_FORMATS {
                    if ui.selectable_label(false, format!("{name}  {w}×{h}")).clicked() {
                        (form.width, form.height, form.pixel_aspect) = (*w, *h, *pa);
                    }
                }
            });
            ui.end_row();
            ui.label("size");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut form.width).range(1..=32768).suffix(" w"));
                ui.add(egui::DragValue::new(&mut form.height).range(1..=32768).suffix(" h"));
            });
            ui.end_row();
            ui.label("pixel aspect");
            ui.add(egui::DragValue::new(&mut form.pixel_aspect).range(0.1..=4.0).speed(0.01));
            ui.end_row();
            ui.label("fps");
            ui.add(egui::DragValue::new(&mut form.fps).range(1.0..=1000.0).speed(0.1));
            ui.end_row();
            ui.label("frame range");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut form.first));
                ui.label("to");
                ui.add(egui::DragValue::new(&mut form.last));
            });
            ui.end_row();
        });
        ui.add_space(8.0);
        let r = ui.button("Apply");
        app.automation.add("dialog.settings.apply", r.rect, "Apply");
        apply = r.clicked();
    });
    if apply {
        app.run(
            "root.settings",
            json!({
                "format": { "width": form.width, "height": form.height, "pixel_aspect": form.pixel_aspect },
                "fps": form.fps, "first": form.first, "last": form.last,
            }),
        );
    }
    app.dialogs.settings = if open && !apply { Some(form) } else { None };
}

/// A script to open, from the platform file picker.
pub fn pick_open() -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        rfd::FileDialog::new().add_filter("NukeCraft script", &["ncs"]).pick_file().map(|p| p.display().to_string())
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}

/// A path to save to, from the platform file picker.
pub fn pick_save(default_name: &str, extensions: &[&str]) -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        rfd::FileDialog::new().set_file_name(default_name).add_filter("file", extensions).save_file().map(|p| p.display().to_string())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (default_name, extensions);
        None
    }
}

/// A file to read, from the platform file picker (Read nodes).
pub fn pick_image() -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        rfd::FileDialog::new().add_filter("image", &["exr", "png"]).pick_file().map(|p| p.display().to_string())
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}
