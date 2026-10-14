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
pub mod control;
mod curves;
mod dialogs;
mod graph;
mod handles;
pub mod keys;
mod menus;
mod properties;
mod render_job;
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
    pub(crate) curves: curves::CurveState,
    /// Which panel the bottom pane shows.
    pub(crate) bottom: BottomTab,
    pub(crate) viewer: viewer::ViewerState,
    pub(crate) props: properties::PropsState,
    control: Option<control::ControlServer>,
    /// Input queued by the control channel, one batch per frame.
    injected: Vec<Vec<egui::Event>>,
    /// Modifiers held by synthetic input.
    held: control::HeldModifiers,
    /// Screenshot requests waiting for eframe: (path, request id, reply).
    screenshots: Vec<(String, Value, std::sync::mpsc::Sender<Value>)>,
    pub(crate) dialogs: dialogs::Dialogs,
    /// Render… running in the background.
    pub(crate) render_job: Option<render_job::RenderJob>,
    /// The last nodes copied or cut (also put on the system clipboard), for Edit ▸ Paste.
    clipboard: String,
    /// Wakes the window when background work finishes.
    repaint: Option<egui::Context>,
    status: Option<Status>,
    shortcuts: Vec<(KeyboardShortcut, &'static str)>,
    title: String,
    allow_close: bool,
    last_autosave: f64,
    now: f64,
}

/// The panels of the bottom pane, as Nuke's tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BottomTab {
    NodeGraph,
    CurveEditor,
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

/// The first position at or to the right of `(x, y)` where a node of `group` doesn't overlap
/// another.
fn free_spot(script: &nukecraft_graph::Script, group: Option<NodeId>, mut x: f64, y: f64) -> (f64, f64) {
    for _ in 0..100 {
        let taken = script.children(group).any(|n| (n.x - x).abs() < 100.0 && (n.y - y).abs() < 30.0);
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
            curves: curves::CurveState::default(),
            bottom: BottomTab::NodeGraph,
            viewer: viewer::ViewerState::new(ctx),
            props: properties::PropsState::default(),
            control: None,
            injected: Vec::new(),
            held: control::HeldModifiers::default(),
            screenshots: Vec::new(),
            dialogs: dialogs::Dialogs::default(),
            render_job: None,
            clipboard: String::new(),
            repaint: Some(ctx.clone()),
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

    /// The Viewer's zoom (screen points per image pixel) and pan, for tests and tools.
    pub fn viewer_zoom(&self) -> f32 {
        self.viewer.zoom()
    }

    pub fn viewer_pan(&self) -> egui::Vec2 {
        self.viewer.pan()
    }

    /// How many keys the Curve Editor has selected.
    pub fn curves_selected(&self) -> usize {
        self.curves.selected_count()
    }

    /// Whether work is still in flight (renders), so a headless run should keep going.
    pub fn busy(&self) -> bool {
        self.viewer.busy() || self.render_job.is_some()
    }

    /// Starts Render… in the background for `render.execute`'s parameters (all enabled Write
    /// nodes over the script's range by default). One job at a time.
    pub fn start_render(&mut self, params: Value) {
        if self.render_job.is_some() {
            self.set_status("A render is already running", true);
            return;
        }
        match nukecraft_engine::render_plan(&self.session, &params) {
            Ok((writes, frames)) => {
                self.render_job = Some(render_job::RenderJob::start(self.session.script.clone(), writes, frames, self.repaint.clone()));
            }
            Err(e) => self.set_status(e.to_string(), true),
        }
    }

    /// Draws one frame.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.now = ctx.input(|i| i.time);
        self.automation.begin_frame();
        self.handle_control(&ctx);
        self.prune();
        self.handle_close(&ctx);
        self.handle_clipboard(&ctx);
        self.handle_shortcuts(&ctx);

        egui::Panel::top("menu").show(ui, |ui| menus::menu_bar(self, ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::Panel::right("properties").resizable(true).default_size(400.0).min_size(260.0).show(ui, |ui| properties::show(self, ui));
        egui::Panel::bottom("node_graph").resizable(true).default_size(380.0).min_size(120.0).frame(egui::Frame::NONE).show(ui, |ui| self.bottom_ui(ui));
        egui::CentralPanel::no_frame().show(ui, |ui| viewer::show(self, ui));

        dialogs::show(self, &ctx);
        render_job::show(self, &ctx);
        self.autosave();
        self.update_title(&ctx);
    }

    /// The bottom pane: a tab strip, then the Node Graph or the Curve Editor.
    fn bottom_ui(&mut self, ui: &mut egui::Ui) {
        let full = ui.max_rect();
        let strip = egui::Rect::from_min_size(full.min, egui::vec2(full.width(), 22.0));
        ui.painter().rect_filled(strip, 0.0, theme::PANEL_DARK);
        let mut child =
            ui.new_child(egui::UiBuilder::new().max_rect(strip.shrink2(egui::vec2(6.0, 1.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
        for (tab, label, id) in [(BottomTab::NodeGraph, "Node Graph", "tab.nodeGraph"), (BottomTab::CurveEditor, "Curve Editor", "tab.curveEditor")] {
            let r = child.selectable_label(self.bottom == tab, label);
            if r.clicked() {
                self.bottom = tab;
            }
            self.automation.add(id, r.rect, label);
        }
        // Inside a group: where the Node Graph is, each step clickable.
        if self.bottom == BottomTab::NodeGraph && self.graph.group.is_some() {
            child.separator();
            let mut chain = Vec::new();
            let mut cur = self.graph.group;
            while let Some(g) = cur {
                if chain.len() > nukecraft_graph::MAX_NESTING {
                    break;
                }
                chain.push(g);
                cur = self.session.script.node(g).and_then(|n| n.parent);
            }
            chain.reverse();
            let mut go = None;
            let r = child.link("Root");
            self.automation.add("graph.crumb.root", r.rect, "Root");
            if r.clicked() {
                go = Some(None);
            }
            for g in chain {
                child.label("/");
                let name = self.session.script.name_of(g).unwrap_or("?").to_string();
                let r = child.link(&name);
                self.automation.add(format!("graph.crumb.{}", self.path(g)), r.rect, name);
                if r.clicked() {
                    go = Some(Some(g));
                }
            }
            if let Some(g) = go {
                self.enter_group(g);
            }
        }
        let body = egui::Rect::from_min_max(egui::pos2(full.left(), strip.bottom()), full.max);
        let mut body_ui = ui.new_child(egui::UiBuilder::new().max_rect(body));
        match self.bottom {
            BottomTab::NodeGraph => {
                self.curves.set_rect(None);
                graph::show(self, &mut body_ui);
            }
            BottomTab::CurveEditor => {
                // The Node Graph isn't under the pointer while hidden.
                self.graph.rect = None;
                curves::show(self, &mut body_ui);
            }
        }
        // The panel keeps the size it was given (it is sized by what it holds).
        ui.expand_to_include_rect(full);
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

    /// A node's path (`Group1.Blur1`), as commands name nodes.
    pub(crate) fn path(&self, id: NodeId) -> String {
        self.session.script.path_of(id).unwrap_or_default()
    }

    /// Shows the nodes of `group` (`None`: the top level) in the Node Graph, framed.
    pub(crate) fn enter_group(&mut self, group: Option<NodeId>) {
        let was = self.graph.group;
        self.graph.group = group;
        self.selection = was.filter(|w| self.session.script.node(*w).is_some_and(|n| n.parent == group)).into_iter().collect();
        self.graph.reframe();
    }

    /// The path of the group the Node Graph shows (`None`: the top level).
    pub(crate) fn group_path(&self) -> Option<String> {
        self.graph.group.and_then(|g| self.session.script.path_of(g).ok())
    }

    /// Paths of the selected nodes, as command parameters expect them.
    pub(crate) fn selected_names(&self) -> Vec<String> {
        self.selection.iter().filter_map(|id| self.session.script.path_of(*id).ok()).collect()
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
            "node.group" => {
                let nodes = self.selected_names();
                if nodes.is_empty() {
                    self.set_status("Select nodes to group", true);
                    return;
                }
                if let Some(g) =
                    self.run("node.group", json!({ "nodes": nodes })).and_then(|r| r["group"].as_str().and_then(|g| self.session.script.id_of(g).ok()))
                {
                    self.selection = vec![g];
                }
            }
            "node.ungroup" => {
                let groups: Vec<String> =
                    self.selection.iter().filter(|id| self.session.script.node(**id).is_some_and(|n| n.class == "Group")).map(|id| self.path(*id)).collect();
                let Some(g) = groups.first() else {
                    self.set_status("Select a Group to expand", true);
                    return;
                };
                if let Some(r) = self.run("node.ungroup", json!({ "node": g })) {
                    let names: Vec<String> =
                        r["nodes"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
                    self.selection = names.iter().filter_map(|n| self.session.script.id_of(n).ok()).collect();
                }
            }
            "node.delete" | "node.disable" => {
                let nodes = self.selected_names();
                if !nodes.is_empty() {
                    self.run(id, json!({ "nodes": nodes }));
                }
            }
            "root.settings" => self.dialogs.open_settings(&self.session.script.root),
            "render.execute" => self.start_render(json!({})),
            "edit.copy" | "edit.cut" => {
                let nodes = self.selected_names();
                if nodes.is_empty() {
                    self.set_status("Select nodes first", true);
                    return;
                }
                if let Some(text) = self.run(id, json!({ "nodes": nodes })).and_then(|r| r["text"].as_str().map(str::to_string)) {
                    if let Some(ctx) = &self.repaint {
                        ctx.copy_text(text.clone());
                    }
                    self.clipboard = text;
                    if id == "edit.cut" {
                        self.selection.clear();
                    }
                }
            }
            "edit.paste" => self.paste(None),
            "render.frame" => {
                let Some(node) = self.viewed.and_then(|v| self.session.script.path_of(v).ok()) else {
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
        if spec.class == "BackdropNode" && !self.selection.is_empty() {
            return self.backdrop_around_selection();
        }
        let (label, takes_input) = (spec.label, !spec.inputs.is_empty() || spec.extra_inputs);
        let upstream = self.selection.last().copied().filter(|_| takes_input);
        let script = &self.session.script;
        let mut params = json!({ "class": class, "group": self.group_path() });
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
                        shift.push(json!({ "node": self.path(n.id), "x": n.x, "y": n.y + NODE_ROW }));
                    }
                }
            } else if let (Some(x), Some(y)) = (params["x"].as_f64(), params["y"].as_f64()) {
                let (x, _) = free_spot(script, self.graph.group, x, y);
                params["x"] = json!(x);
            }
        }
        self.session.begin_group(&format!("Create {label}"));
        if !shift.is_empty() {
            self.run("node.move", json!({ "moves": shift }));
        }
        let created = self.run("node.create", params).and_then(|r| r["name"].as_str().and_then(|n| self.session.script.id_of(n).ok()));
        if let (Some(new), false) = (created, branch) {
            let new_name = self.path(new);
            for (c, i) in consumers {
                let Ok(c_name) = self.session.script.path_of(c) else { continue };
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

    /// A Backdrop behind the selected nodes, with room for its title (Nuke's way).
    fn backdrop_around_selection(&mut self) -> Option<NodeId> {
        let nodes: Vec<&nukecraft_graph::Node> = self.selection.iter().filter_map(|id| self.session.script.node(*id)).collect();
        let x0 = nodes.iter().map(|n| n.x).fold(f64::INFINITY, f64::min);
        let y0 = nodes.iter().map(|n| n.y).fold(f64::INFINITY, f64::min);
        let x1 = nodes.iter().map(|n| n.x + 90.0).fold(f64::NEG_INFINITY, f64::max);
        let y1 = nodes.iter().map(|n| n.y + 26.0).fold(f64::NEG_INFINITY, f64::max);
        if !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()) {
            return None;
        }
        let (pad, title) = (20.0, 30.0);
        let knobs = json!({ "bdwidth": (x1 - x0 + 2.0 * pad).round(), "bdheight": (y1 - y0 + 2.0 * pad + title).round() });
        let params = json!({ "class": "BackdropNode", "group": self.group_path(), "x": (x0 - pad).round(), "y": (y0 - pad - title).round(), "knobs": knobs });
        let created = self.run("node.create", params).and_then(|r| r["name"].as_str().and_then(|n| self.session.script.id_of(n).ok()));
        if let Some(id) = created {
            self.selection = vec![id];
            self.open_properties(id);
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
        if self.graph.group.is_some_and(|g| script.node(g).is_none()) {
            self.graph.group = None;
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let typing = ctx.text_edit_focused();
        // Bare keys go to the panel under the pointer: over the Curve Editor, Delete deletes keys.
        let over_curves = ctx.input(|i| i.pointer.hover_pos()).zip(self.curves.rect()).is_some_and(|(p, r)| r.contains(p));
        let mut fired = None;
        ctx.input_mut(|i| {
            for (sc, id) in &self.shortcuts {
                if (typing || over_curves) && keys::is_bare(sc) {
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

    /// The system's copy, cut and paste (they arrive as events, not keys), outside text fields.
    fn handle_clipboard(&mut self, ctx: &egui::Context) {
        if ctx.text_edit_focused() {
            return;
        }
        let events: Vec<egui::Event> =
            ctx.input(|i| i.events.iter().filter(|e| matches!(e, egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_))).cloned().collect());
        for e in events {
            match e {
                egui::Event::Copy => self.invoke("edit.copy"),
                egui::Event::Cut => self.invoke("edit.cut"),
                egui::Event::Paste(text) => self.paste(Some(text)),
                _ => {}
            }
        }
    }

    /// Pastes nodes from `text` (the system clipboard) or the last copy, and selects them.
    pub(crate) fn paste(&mut self, text: Option<String>) {
        let text = text.filter(|t| t.contains("\"nukecraft\"")).unwrap_or_else(|| self.clipboard.clone());
        if text.is_empty() {
            self.set_status("Nothing to paste", true);
            return;
        }
        if let Some(r) = self.run("edit.paste", json!({ "text": text, "group": self.group_path() })) {
            let names: Vec<&str> = r["names"].as_array().map(|a| a.iter().filter_map(serde_json::Value::as_str).collect()).unwrap_or_default();
            self.selection = names.iter().filter_map(|n| self.session.script.id_of(n).ok()).collect();
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
