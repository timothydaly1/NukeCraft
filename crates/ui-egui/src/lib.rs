//! The NukeCraft desktop interface, in egui.
//!
//! [`NukecraftApp`] owns a [`Session`] and draws the whole window in [`NukecraftApp::ui`]: the
//! menu bar, the Viewer, the Node Graph, the Properties panel and the status bar. Every edit goes
//! through [`Session::execute`] by command id — the UI never changes the script directly — so it
//! is undoable, validated and the same thing an agent would do.
//!
//! The same `ui` runs inside eframe (`apps/nukecraft`) and headless ([`snapshot::Headless`]),
//! where a software rasteriser turns each frame into pixels for agents and tests.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod automation;
mod dialogs;
mod graph;
pub mod keys;
mod menus;
pub mod snapshot;
pub mod theme;
mod viewer;
mod worker;

use egui::KeyboardShortcut;
use nukecraft_engine::{Session, command_specs};
use nukecraft_graph::NodeId;
use serde_json::{Value, json};

/// Vertical spacing of a chain of nodes in the Node Graph, in graph units.
const NODE_ROW: f64 = 60.0;

/// Seconds between autosaves of a script with unsaved changes.
const AUTOSAVE_SECONDS: f64 = 120.0;

/// Seconds a status message stays up.
const STATUS_SECONDS: f64 = 8.0;

/// The application.
pub struct NukecraftApp {
    pub session: Session,
    /// Widget ids and rects of the last frame, for agents and the snapshot tool.
    pub automation: automation::Registry,
    pub(crate) selection: Vec<NodeId>,
    pub(crate) viewed: Option<NodeId>,
    /// Nodes whose properties are open, most recent first.
    pub(crate) open_props: Vec<NodeId>,
    pub(crate) graph: graph::GraphState,
    pub(crate) viewer: viewer::ViewerState,
    pub(crate) dialogs: dialogs::Dialogs,
    status: Option<Status>,
    shortcuts: Vec<(KeyboardShortcut, &'static str)>,
    title: String,
    allow_close: bool,
    last_autosave: f64,
    now: f64,
}

struct Status {
    text: String,
    error: bool,
    since: f64,
}

/// Every node downstream of `id` (not `id` itself), each once.
fn downstream(script: &nukecraft_graph::Script, id: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack = vec![id];
    while let Some(n) = stack.pop() {
        for (c, _) in script.consumers(n) {
            if c != id && !out.contains(&c) {
                out.push(c);
                stack.push(c);
            }
        }
    }
    out
}

/// The first position at or to the right of `(x, y)` where a node doesn't overlap another.
fn free_spot(script: &nukecraft_graph::Script, mut x: f64, y: f64) -> (f64, f64) {
    for _ in 0..100 {
        let taken = script.nodes().iter().any(|n| (n.x - x).abs() < 100.0 && (n.y - y).abs() < 30.0);
        if !taken {
            break;
        }
        x += 110.0;
    }
    (x, y)
}

impl NukecraftApp {
    pub fn new(ctx: &egui::Context, session: Session) -> NukecraftApp {
        theme::apply(ctx);
        let mut shortcuts: Vec<(KeyboardShortcut, &'static str)> =
            command_specs().iter().filter(|c| !c.menu.is_empty()).filter_map(|c| Some((keys::parse(c.shortcut?)?, c.id))).collect();
        shortcuts.sort_by_key(|(s, _)| std::cmp::Reverse(keys::modifier_count(s)));
        let mut app = NukecraftApp {
            session,
            automation: automation::Registry::default(),
            selection: Vec::new(),
            viewed: None,
            open_props: Vec::new(),
            graph: graph::GraphState::default(),
            viewer: viewer::ViewerState::new(ctx),
            dialogs: dialogs::Dialogs::default(),
            status: None,
            shortcuts,
            title: String::new(),
            allow_close: false,
            last_autosave: 0.0,
            now: 0.0,
        };
        app.view_default();
        app
    }

    /// The Viewer's error for what it is showing, if rendering it failed.
    pub fn viewer_error(&self) -> Option<String> {
        self.viewer.current_error(self)
    }

    /// Whether work is still in flight (renders), so a headless run should keep going.
    pub fn busy(&self) -> bool {
        self.viewer.busy()
    }

    /// Draws one frame.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.now = ctx.input(|i| i.time);
        self.automation.begin_frame();
        self.prune();
        self.handle_close(&ctx);
        self.handle_shortcuts(&ctx);

        egui::Panel::top("menu").show(ui, |ui| menus::menu_bar(self, ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::Panel::right("properties").resizable(true).default_size(380.0).min_size(240.0).show(ui, |ui| {
            ui.heading("Properties");
            ui.allocate_space(ui.available_size());
        });
        egui::Panel::bottom("node_graph").resizable(true).default_size(380.0).min_size(120.0).frame(egui::Frame::NONE).show(ui, |ui| graph::show(self, ui));
        egui::CentralPanel::no_frame().show(ui, |ui| viewer::show(self, ui));

        dialogs::show(self, &ctx);
        self.autosave();
        self.update_title(&ctx);
    }

    /// Runs an engine command; failures go to the status bar. Returns the result on success.
    pub fn run(&mut self, id: &str, params: Value) -> Option<Value> {
        match self.session.execute(id, &params) {
            Ok(v) => Some(v),
            Err(e) => {
                log::warn!("{id}: {e}");
                self.set_status(e.to_string(), true);
                None
            }
        }
    }

    pub(crate) fn set_status(&mut self, text: impl Into<String>, error: bool) {
        self.status = Some(Status { text: text.into(), error, since: self.now });
    }

    /// Names of the selected nodes, as command parameters expect them.
    pub(crate) fn selected_names(&self) -> Vec<String> {
        self.selection.iter().filter_map(|id| self.session.script.name_of(*id).ok().map(str::to_string)).collect()
    }

    /// Runs a menu or shortcut command, supplying what it needs from the UI (the selection, a file
    /// dialog, a confirmation).
    pub fn invoke(&mut self, id: &str) {
        match id {
            "file.new" | "file.open" if self.session.dirty => self.dialogs.confirm = Some(dialogs::Pending::from_id(id)),
            "file.new" => {
                self.run("file.new", json!({}));
                self.after_open();
            }
            "file.open" => {
                if let Some(path) = dialogs::pick_open() {
                    self.open(&path);
                }
            }
            "file.save" if self.session.path.is_some() => {
                if self.run("file.save", json!({})).is_some() {
                    self.set_status("Saved", false);
                }
            }
            "file.save" | "file.saveAs" => {
                if let Some(path) = dialogs::pick_save("script.ncs", &["ncs"]) {
                    self.run("file.save", json!({ "path": path }));
                }
            }
            "node.delete" | "node.disable" => {
                let nodes = self.selected_names();
                if !nodes.is_empty() {
                    self.run(id, json!({ "nodes": nodes }));
                }
            }
            "root.settings" => self.dialogs.open_settings(&self.session.script.root),
            "render.execute" => {
                if let Some(r) = self.run("render.execute", json!({})) {
                    let n = r["written"].as_array().map_or(0, Vec::len);
                    self.set_status(format!("Rendered {n} files"), false);
                }
            }
            "render.frame" => {
                let Some(node) = self.viewed.and_then(|v| self.session.script.name_of(v).ok()).map(str::to_string) else {
                    self.set_status("View a node first (select it and press 1)", true);
                    return;
                };
                if let Some(path) = dialogs::pick_save("frame.png", &["png", "exr"]) {
                    let frame = self.session.time.as_f64();
                    if self.run("render.frame", json!({ "node": node, "path": path, "frame": frame })).is_some() {
                        self.set_status(format!("Wrote {path}"), false);
                    }
                }
            }
            other => {
                self.run(other, json!({}));
            }
        }
    }

    /// Creates a node of `class` the way Nuke does: below the last selected node and fed by it,
    /// taking over that node's downstream pipes unless `branch`. One undo step. Selects the new
    /// node and returns it.
    pub fn create_node(&mut self, class: &str, branch: bool) -> Option<NodeId> {
        self.create_node_at(class, branch, None)
    }

    /// [`NukecraftApp::create_node`], placed at graph position `at` (its centre) when nothing
    /// is selected.
    pub fn create_node_at(&mut self, class: &str, branch: bool, at: Option<(f64, f64)>) -> Option<NodeId> {
        let spec = self.session.registry.get(class)?;
        let (label, takes_input) = (spec.label, !spec.inputs.is_empty() || spec.extra_inputs);
        let upstream = self.selection.last().copied().filter(|_| takes_input);
        let script = &self.session.script;
        let mut params = json!({ "class": class });
        if let Some(up) = upstream.and_then(|u| script.node(u)) {
            params["inputs"] = json!([up.name]);
            params["x"] = json!(up.x);
            params["y"] = json!(up.y + NODE_ROW);
        }
        if upstream.is_none()
            && let Some((x, y)) = at
        {
            params["x"] = json!((x - 45.0).round());
            params["y"] = json!((y - 13.0).round());
        }
        let consumers = upstream.map(|u| script.consumers(u)).unwrap_or_default();
        // Make room: an insert pushes everything downstream of the upstream node down a row; a
        // branch moves right until the spot is free.
        let mut shift = Vec::new();
        if let Some(up) = upstream {
            if !branch && !consumers.is_empty() {
                for id in downstream(script, up) {
                    if let Some(n) = script.node(id) {
                        shift.push(json!({ "node": n.name, "x": n.x, "y": n.y + NODE_ROW }));
                    }
                }
            } else if let (Some(x), Some(y)) = (params["x"].as_f64(), params["y"].as_f64()) {
                let (x, _) = free_spot(script, x, y);
                params["x"] = json!(x);
            }
        }
        self.session.begin_group(&format!("Create {label}"));
        if !shift.is_empty() {
            self.run("node.move", json!({ "moves": shift }));
        }
        let created = self.run("node.create", params).and_then(|r| r["name"].as_str().and_then(|n| self.session.script.id_of(n).ok()));
        if let (Some(new), false) = (created, branch) {
            let new_name = self.session.script.name_of(new).map(str::to_string).unwrap_or_default();
            for (c, i) in consumers {
                let Ok(c_name) = self.session.script.name_of(c).map(str::to_string) else { continue };
                self.run("node.connect", json!({ "node": c_name, "input": i, "from": new_name }));
            }
        }
        self.session.end_group();
        if let Some(new) = created {
            self.selection = vec![new];
            self.open_properties(new);
        }
        created
    }

    /// Opens a node's properties at the top of the Properties panel.
    pub fn open_properties(&mut self, id: NodeId) {
        self.open_props.retain(|o| *o != id);
        self.open_props.insert(0, id);
        self.open_props.truncate(8);
    }

    /// Opens a script from a path.
    pub fn open(&mut self, path: &str) {
        if let Some(r) = self.run("file.open", json!({ "path": path })) {
            let n = r["warnings"].as_array().map_or(0, Vec::len);
            if n > 0 {
                self.set_status(format!("Opened with {n} repairs (see the log)"), true);
            }
            self.after_open();
        }
    }

    fn after_open(&mut self) {
        self.selection.clear();
        self.open_props.clear();
        self.graph = graph::GraphState::default();
        self.view_default();
    }

    /// Views the last Write node, or the last node, as a starting point.
    fn view_default(&mut self) {
        let nodes = self.session.script.nodes();
        self.viewed = nodes.iter().rev().find(|n| n.class == "Write").or(nodes.last()).map(|n| n.id);
    }

    /// Drops selection and view references to nodes that no longer exist (after undo, delete…).
    fn prune(&mut self) {
        let script = &self.session.script;
        self.selection.retain(|id| script.node(*id).is_some());
        self.open_props.retain(|id| script.node(*id).is_some());
        if self.viewed.is_some_and(|v| script.node(v).is_none()) {
            self.viewed = None;
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let typing = ctx.text_edit_focused();
        let mut fired = None;
        ctx.input_mut(|i| {
            for (sc, id) in &self.shortcuts {
                if typing && keys::is_bare(sc) {
                    continue;
                }
                if i.consume_shortcut(sc) {
                    fired = Some(*id);
                    break;
                }
            }
        });
        if let Some(id) = fired {
            self.invoke(id);
        }
    }

    fn handle_close(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && self.session.dirty && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.dialogs.confirm = Some(dialogs::Pending::Quit);
        }
    }

    /// Closes the window without asking again (after the user chose to discard or saved).
    pub(crate) fn quit(&mut self, ctx: &egui::Context) {
        self.allow_close = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(s) = &self.status
                && self.now - s.since < STATUS_SECONDS
            {
                let color = if s.error { theme::ERROR } else { theme::TEXT };
                ui.colored_label(color, &s.text);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let root = &self.session.script.root;
                let name = self.session.path.as_ref().and_then(|p| p.file_name()).map_or("untitled".to_string(), |n| n.to_string_lossy().to_string());
                let dirty = if self.session.dirty { "*" } else { "" };
                ui.colored_label(
                    theme::TEXT_DIM,
                    format!("{name}{dirty}  ·  frame {}  ·  {}×{}  ·  {:.3} fps", self.session.time, root.format.width, root.format.height, root.fps.as_f64()),
                );
            });
        });
    }

    fn autosave(&mut self) {
        if !self.session.dirty || self.now - self.last_autosave < AUTOSAVE_SECONDS {
            return;
        }
        self.last_autosave = self.now;
        let path = match &self.session.path {
            Some(p) => {
                let mut s = p.clone().into_os_string();
                s.push(".autosave");
                std::path::PathBuf::from(s)
            }
            None => std::env::temp_dir().join("nukecraft-untitled.autosave.ncs"),
        };
        match self.session.script.to_json() {
            Ok(text) => {
                if let Err(e) = std::fs::write(&path, text) {
                    log::warn!("autosave to {}: {e}", path.display());
                }
            }
            Err(e) => log::warn!("autosave: {e}"),
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let name = self.session.path.as_ref().and_then(|p| p.file_name()).map_or("untitled".to_string(), |n| n.to_string_lossy().to_string());
        let title = format!("NukeCraft — {name}{}", if self.session.dirty { " *" } else { "" });
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

#[cfg(test)]
mod tests;
