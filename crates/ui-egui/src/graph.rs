//! The Node Graph panel: nodes and pipes, drawn and edited the way Nuke's DAG works.
//!
//! - Pipes run from a node's output (its bottom edge) to an input of another node; input 0 enters
//!   from the top, input 1 from the left, mask inputs from the right (Merge: B, A, mask).
//! - Drag a node to move it (with the rest of the selection); drag from a node's bottom edge to
//!   another node to connect; drag an input's arrow to reconnect it, or into empty space to
//!   disconnect; drag on empty space to box-select; middle-drag or Alt-drag pans; scroll zooms.
//! - With the pointer over the graph: Tab searches node classes, the node keys (B, G, M, T, ., R,
//!   W) create nodes after the selection (Shift: in a new branch), 1 views the selected node, F
//!   frames the selection (or everything), Backspace deletes.
//!
//! Positions are graph units (the script's `x`, `y`, y down, a node's top-left corner).

use egui::{Align2, Color32, FontId, Key, Modifiers, PointerButton, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2};
use nukecraft_graph::{Node, NodeId};
use serde_json::json;

use crate::{NukecraftApp, theme};

/// Node size in graph units.
const NODE_W: f32 = 90.0;
const NODE_H: f32 = 26.0;
/// Length of an unconnected input's stub, and where an input's grab handle sits, in graph units.
const STUB: f32 = 26.0;
/// Pick radius for handles, in points.
const PICK: f32 = 9.0;
const MIN_ZOOM: f32 = 0.15;
const MAX_ZOOM: f32 = 3.0;

#[derive(Default)]
pub struct GraphState {
    pan: Vec2,
    zoom: f32,
    framed: bool,
    frame_request: bool,
    drag: Option<Drag>,
    search: Option<Search>,
}

enum Drag {
    Move { origin: Pos2, start: Vec<(NodeId, f32, f32)>, delta: Vec2 },
    Pipe { from: PipeEnd, to: Pos2 },
    Box { start: Pos2, end: Pos2 },
    Pan,
}

#[derive(Clone, Copy, PartialEq)]
enum PipeEnd {
    /// A new pipe out of this node's output.
    Output(NodeId),
    /// An input being (re)connected.
    Input(NodeId, usize),
}

#[derive(Clone, Copy, PartialEq)]
enum Hit {
    Node(NodeId),
    Output(NodeId),
    Input(NodeId, usize),
    None,
}

struct Search {
    text: String,
    /// Screen position of the popup.
    at: Pos2,
    /// Graph position for a node created with nothing selected.
    graph_at: Pos2,
    index: usize,
    focus: bool,
}

/// Maps between graph units and screen points.
#[derive(Clone, Copy)]
struct View {
    origin: Pos2,
    pan: Vec2,
    zoom: f32,
}

impl View {
    fn to_screen(self, p: Pos2) -> Pos2 {
        self.origin + self.pan + p.to_vec2() * self.zoom
    }

    fn to_graph(self, p: Pos2) -> Pos2 {
        ((p - self.origin - self.pan) / self.zoom).to_pos2()
    }

    fn node_rect(self, n: &Node) -> Rect {
        Rect::from_min_size(self.to_screen(Pos2::new(n.x as f32, n.y as f32)), Vec2::new(NODE_W, NODE_H) * self.zoom)
    }
}

/// Whether input `i` of a node is a mask input.
fn is_mask(app: &NukecraftApp, n: &Node, i: usize) -> bool {
    app.session.registry.get(&n.class).and_then(|s| s.inputs.get(i)).is_some_and(|s| s.mask)
}

/// Where input `i` meets the node, and the direction a pipe leaves it in (towards its source).
/// Mask inputs come in from the right, as in Nuke.
fn input_anchor(rect: Rect, i: usize, mask: bool) -> (Pos2, Vec2) {
    if mask {
        return (rect.right_center(), Vec2::new(1.0, 0.0));
    }
    match i {
        0 => (rect.center_top(), Vec2::new(0.0, -1.0)),
        1 => (rect.left_center(), Vec2::new(-1.0, 0.0)),
        2 => (rect.right_center(), Vec2::new(1.0, 0.0)),
        k => {
            let x = rect.left() + rect.width() * (0.15 + 0.12 * (k - 3) as f32).min(0.95);
            (Pos2::new(x, rect.top()), Vec2::new(0.0, -1.0))
        }
    }
}

/// How many input slots a node shows: its class's inputs, plus connected extras.
fn shown_inputs(app: &NukecraftApp, n: &Node) -> usize {
    let spec = app.session.registry.get(&n.class);
    let declared = spec.map_or(0, |s| s.inputs.len());
    let extra = spec.is_some_and(|s| s.extra_inputs);
    if extra { declared.max(n.inputs.len() + 1) } else { declared.max(n.inputs.len()) }
}

fn input_label(app: &NukecraftApp, n: &Node, i: usize) -> String {
    match app.session.registry.get(&n.class).and_then(|s| s.inputs.get(i)) {
        Some(spec) => spec.name.to_string(),
        None => i.to_string(),
    }
}

/// The grab handle of input `i`: on the pipe (or stub), `STUB` units out from the node.
fn input_handle(view: View, app: &NukecraftApp, n: &Node, i: usize) -> Pos2 {
    let rect = view.node_rect(n);
    let (anchor, dir) = input_anchor(rect, i, is_mask(app, n, i));
    let dir = match n.input(i).and_then(|u| app.session.script.node(u)) {
        Some(up) => {
            let from = view.node_rect(up).center_bottom();
            let d = from - anchor;
            if d.length() > 1.0 { d.normalized() } else { dir }
        }
        None => dir,
    };
    anchor + dir * STUB * view.zoom
}

pub fn show(app: &mut NukecraftApp, ui: &mut egui::Ui) {
    let rect = ui.max_rect();
    let resp = ui.allocate_rect(rect, Sense::click_and_drag());
    app.automation.add("graph.canvas", rect, "Node Graph");
    let st = &mut app.graph;
    if st.zoom <= 0.0 {
        st.zoom = 1.0;
    }
    if !st.framed || st.frame_request {
        let only_selected = st.frame_request && !app.selection.is_empty();
        frame_nodes(app, rect, only_selected);
    }
    let view = View { origin: rect.min, pan: app.graph.pan, zoom: app.graph.zoom };
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme::GRAPH_BG);
    draw_grid(&painter, rect, view);

    let hover = resp.hover_pos();
    handle_zoom(app, ui, &resp, rect);
    handle_pointer(app, ui, &resp, view);
    if resp.hovered() || app.graph.search.is_some() {
        handle_keys(app, ui, view, hover);
    }

    // Draw with this frame's (possibly changed) state.
    let view = View { origin: rect.min, pan: app.graph.pan, zoom: app.graph.zoom };
    draw_pipes(app, &painter, view);
    draw_nodes(app, &painter, view);
    draw_drag(app, &painter, view);
    register_elements(app, view);
    resp.context_menu(|ui| context_menu(app, ui, hover.map(|h| view.to_graph(h))));
    search_popup(app, ui);
}

fn draw_grid(painter: &egui::Painter, rect: Rect, view: View) {
    let step = 100.0 * view.zoom;
    if step < 12.0 {
        return;
    }
    let start = view.to_graph(rect.min);
    let mut x = (start.x / 100.0).floor() * 100.0;
    while view.to_screen(Pos2::new(x, 0.0)).x < rect.right() {
        let sx = view.to_screen(Pos2::new(x, 0.0)).x;
        painter.line_segment([Pos2::new(sx, rect.top()), Pos2::new(sx, rect.bottom())], Stroke::new(1.0, theme::GRAPH_GRID));
        x += 100.0;
    }
    let mut y = (start.y / 100.0).floor() * 100.0;
    while view.to_screen(Pos2::new(0.0, y)).y < rect.bottom() {
        let sy = view.to_screen(Pos2::new(0.0, y)).y;
        painter.line_segment([Pos2::new(rect.left(), sy), Pos2::new(rect.right(), sy)], Stroke::new(1.0, theme::GRAPH_GRID));
        y += 100.0;
    }
}

/// Fits the nodes (all, or the selection) into the panel.
fn frame_nodes(app: &mut NukecraftApp, rect: Rect, only_selected: bool) {
    let st = &mut app.graph;
    st.framed = true;
    st.frame_request = false;
    let nodes = app.session.script.nodes().iter().filter(|n| !only_selected || app.selection.contains(&n.id));
    let mut bounds = Rect::NOTHING;
    for n in nodes {
        bounds = bounds.union(Rect::from_min_size(Pos2::new(n.x as f32, n.y as f32), Vec2::new(NODE_W, NODE_H)));
    }
    if !bounds.is_positive() || !bounds.is_finite() {
        st.zoom = 1.0;
        st.pan = Vec2::new(40.0, 40.0);
        return;
    }
    let bounds = bounds.expand(60.0);
    let zoom = (rect.width() / bounds.width()).min(rect.height() / bounds.height()).clamp(MIN_ZOOM, 1.2);
    st.zoom = zoom;
    st.pan = rect.size() / 2.0 - bounds.center().to_vec2() * zoom;
}

fn handle_zoom(app: &mut NukecraftApp, ui: &egui::Ui, resp: &egui::Response, rect: Rect) {
    let Some(p) = resp.hover_pos() else { return };
    let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
    let factor = (scroll * 0.0015).exp() * pinch;
    if (factor - 1.0).abs() < 1e-4 {
        return;
    }
    let st = &mut app.graph;
    let new_zoom = (st.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
    // Keep the point under the pointer fixed.
    let local = p - rect.min;
    st.pan = local - (local - st.pan) * (new_zoom / st.zoom);
    st.zoom = new_zoom;
}

fn hit_test(app: &NukecraftApp, view: View, p: Pos2) -> Hit {
    let nodes = app.session.script.nodes();
    // Node bodies first (the topmost, last drawn, wins); the bottom strip is the output.
    for n in nodes.iter().rev() {
        let r = view.node_rect(n);
        if r.contains(p) {
            return if p.y > r.bottom() - 5.0 * view.zoom.max(0.6) { Hit::Output(n.id) } else { Hit::Node(n.id) };
        }
    }
    // Then input handles (they sit outside the nodes), then the output anchors just below them.
    for n in nodes.iter().rev() {
        for i in 0..shown_inputs(app, n) {
            if input_handle(view, app, n, i).distance(p) <= PICK {
                return Hit::Input(n.id, i);
            }
        }
    }
    for n in nodes.iter().rev() {
        if view.node_rect(n).center_bottom().distance(p) <= PICK {
            return Hit::Output(n.id);
        }
    }
    Hit::None
}

fn handle_pointer(app: &mut NukecraftApp, ui: &egui::Ui, resp: &egui::Response, view: View) {
    let (modifiers, press_origin, latest) = ui.input(|i| (i.modifiers, i.pointer.press_origin(), i.pointer.latest_pos()));
    if resp.drag_started_by(PointerButton::Middle) || (resp.drag_started_by(PointerButton::Primary) && modifiers.alt) {
        app.graph.drag = Some(Drag::Pan);
    } else if resp.drag_started_by(PointerButton::Primary)
        && let Some(origin) = press_origin
    {
        app.graph.drag = Some(match hit_test(app, view, origin) {
            Hit::Output(id) => Drag::Pipe { from: PipeEnd::Output(id), to: origin },
            Hit::Input(id, i) => Drag::Pipe { from: PipeEnd::Input(id, i), to: origin },
            Hit::Node(id) => {
                if !app.selection.contains(&id) {
                    if !modifiers.shift {
                        app.selection.clear();
                    }
                    app.selection.push(id);
                }
                let start = app.selection.iter().filter_map(|s| app.session.script.node(*s)).map(|n| (n.id, n.x as f32, n.y as f32)).collect();
                Drag::Move { origin: view.to_graph(origin), start, delta: Vec2::ZERO }
            }
            Hit::None => Drag::Box { start: origin, end: origin },
        });
    }

    if resp.dragged() {
        let delta = resp.drag_delta();
        let pointer = latest.unwrap_or_default();
        match &mut app.graph.drag {
            Some(Drag::Pan) => app.graph.pan += delta,
            Some(Drag::Move { origin, delta: d, .. }) => *d = view.to_graph(pointer) - *origin,
            Some(Drag::Pipe { to, .. }) => *to = pointer,
            Some(Drag::Box { end, .. }) => *end = pointer,
            None => {}
        }
    }

    if resp.drag_stopped()
        && let Some(drag) = app.graph.drag.take()
    {
        finish_drag(app, view, drag, latest, modifiers);
    }

    if resp.clicked() {
        match latest.map_or(Hit::None, |p| hit_test(app, view, p)) {
            Hit::Node(id) | Hit::Output(id) => {
                if modifiers.shift {
                    if let Some(pos) = app.selection.iter().position(|s| *s == id) {
                        app.selection.remove(pos);
                    } else {
                        app.selection.push(id);
                    }
                } else {
                    app.selection = vec![id];
                }
            }
            Hit::Input(..) => {}
            Hit::None => app.selection.clear(),
        }
    }
    if resp.double_clicked()
        && let Some(Hit::Node(id) | Hit::Output(id)) = latest.map(|p| hit_test(app, view, p))
    {
        app.open_properties(id);
    }
}

fn finish_drag(app: &mut NukecraftApp, view: View, drag: Drag, at: Option<Pos2>, modifiers: Modifiers) {
    match drag {
        Drag::Move { start, delta, .. } => {
            if delta.length() < 0.5 {
                return;
            }
            let moves: Vec<_> = start
                .iter()
                .filter_map(|(id, x, y)| {
                    let name = app.session.script.name_of(*id).ok()?;
                    Some(json!({ "node": name, "x": (x + delta.x).round(), "y": (y + delta.y).round() }))
                })
                .collect();
            app.run("node.move", json!({ "moves": moves }));
        }
        Drag::Pipe { from, .. } => {
            let target = match at.map(|p| hit_test(app, view, p)) {
                Some(Hit::Node(id) | Hit::Output(id)) => Some(id),
                _ => None,
            };
            let name = |id: NodeId| app.session.script.name_of(id).map(str::to_string).unwrap_or_default();
            match (from, target) {
                (PipeEnd::Output(src), Some(dst)) if src != dst => {
                    let input = free_input(app, dst);
                    app.run("node.connect", json!({ "node": name(dst), "input": input, "from": name(src) }));
                }
                (PipeEnd::Input(dst, i), Some(src)) if src != dst => {
                    app.run("node.connect", json!({ "node": name(dst), "input": i, "from": name(src) }));
                }
                (PipeEnd::Input(dst, i), None) if app.session.script.node(dst).and_then(|n| n.input(i)).is_some() => {
                    app.run("node.disconnect", json!({ "node": name(dst), "input": i }));
                }
                _ => {}
            }
        }
        Drag::Box { start, end } => {
            let r = Rect::from_two_pos(start, end);
            let inside: Vec<NodeId> = app.session.script.nodes().iter().filter(|n| r.intersects(view.node_rect(n))).map(|n| n.id).collect();
            if !modifiers.shift {
                app.selection.clear();
            }
            for id in inside {
                if !app.selection.contains(&id) {
                    app.selection.push(id);
                }
            }
        }
        Drag::Pan => {}
    }
}

/// The input a pipe dropped on a node connects to: the first empty non-mask input, else the
/// first empty input, else input 0.
fn free_input(app: &NukecraftApp, id: NodeId) -> usize {
    let Some(n) = app.session.script.node(id) else { return 0 };
    let Some(spec) = app.session.registry.get(&n.class) else { return 0 };
    let slots = shown_inputs(app, n);
    let empty = |i: &usize| n.input(*i).is_none();
    (0..slots).filter(empty).find(|i| !spec.inputs.get(*i).is_some_and(|s| s.mask)).or_else(|| (0..slots).find(empty)).unwrap_or(0)
}

fn handle_keys(app: &mut NukecraftApp, ui: &egui::Ui, view: View, hover: Option<Pos2>) {
    if ui.ctx().text_edit_focused() || app.graph.search.is_some() {
        return;
    }
    let pointer = hover.unwrap_or_else(|| view.origin + Vec2::new(80.0, 80.0));
    if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab)) {
        app.graph.search = Some(Search { text: String::new(), at: pointer, graph_at: view.to_graph(pointer), index: 0, focus: true });
        return;
    }
    if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Backspace)) && !app.selection.is_empty() {
        app.invoke("node.delete");
        return;
    }
    if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Num1)) {
        if let Some(sel) = app.selection.last() {
            app.viewed = Some(*sel);
        }
        return;
    }
    if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F)) {
        app.graph.frame_request = true;
        return;
    }
    // Node keys: plain inserts after the selection, Shift branches.
    let keyed: Vec<(&'static str, Key)> = app.session.registry.iter().filter_map(|s| Some((s.class, crate::keys::parse(s.shortcut?)?.logical_key))).collect();
    for (class, key) in keyed {
        // Shift first: egui matches keys "logically", so a plain pattern also accepts Shift.
        for (mods, branch) in [(Modifiers::SHIFT, true), (Modifiers::NONE, false)] {
            if ui.input_mut(|i| i.consume_key(mods, key)) {
                let at = view.to_graph(pointer);
                app.create_node_at(class, branch, Some((at.x as f64, at.y as f64)));
                return;
            }
        }
    }
}

fn search_popup(app: &mut NukecraftApp, ui: &egui::Ui) {
    let Some(search) = &mut app.graph.search else { return };
    let query = search.text.to_lowercase();
    let mut matches: Vec<(&'static str, &'static str, &'static str)> = app
        .session
        .registry
        .iter()
        .filter(|s| query.is_empty() || s.label.to_lowercase().contains(&query) || s.class.to_lowercase().contains(&query))
        .map(|s| (s.class, s.label, s.category))
        .collect();
    // Prefix matches first, then alphabetical.
    matches.sort_by_key(|(_, label, _)| (!label.to_lowercase().starts_with(&query), label.to_string()));
    search.index = search.index.min(matches.len().saturating_sub(1));
    let (up, down, enter, escape) = ui.input_mut(|i| {
        (
            i.consume_key(Modifiers::NONE, Key::ArrowUp),
            i.consume_key(Modifiers::NONE, Key::ArrowDown),
            i.consume_key(Modifiers::NONE, Key::Enter),
            i.consume_key(Modifiers::NONE, Key::Escape),
        )
    });
    if up {
        search.index = search.index.saturating_sub(1);
    }
    if down && search.index + 1 < matches.len() {
        search.index += 1;
    }
    let mut chosen = None;
    let mut close = escape;
    let mut elements = Vec::new();
    egui::Area::new(egui::Id::new("graph_search")).fixed_pos(search.at).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_width(220.0);
            let edit = ui.add(egui::TextEdit::singleline(&mut search.text).hint_text("node name…").desired_width(f32::INFINITY));
            elements.push(("graph.search".to_string(), edit.rect, "search".to_string()));
            if search.focus {
                edit.request_focus();
                search.focus = false;
            }
            for (k, (class, label, category)) in matches.iter().take(12).enumerate() {
                let r = ui.selectable_label(k == search.index, format!("{label}   ·  {category}"));
                elements.push((format!("graph.search.item.{class}"), r.rect, label.to_string()));
                if r.clicked() {
                    chosen = Some(*class);
                }
            }
            if matches.is_empty() {
                ui.colored_label(theme::TEXT_DIM, "no match");
            }
        });
    });
    if enter {
        chosen = matches.get(search.index).map(|m| m.0);
        close = true;
    }
    let graph_at = search.graph_at;
    for (id, rect, label) in elements {
        app.automation.add(id, rect, label);
    }
    if let Some(class) = chosen {
        app.graph.search = None;
        app.create_node_at(class, false, Some((graph_at.x as f64, graph_at.y as f64)));
    } else if close {
        app.graph.search = None;
    }
}

fn context_menu(app: &mut NukecraftApp, ui: &mut egui::Ui, at: Option<Pos2>) {
    let mut create = None;
    ui.menu_button("Create", |ui| {
        let mut cats: Vec<&'static str> = app.session.registry.iter().map(|s| s.category).collect();
        cats.sort_by_key(|c| theme::CATEGORY_ORDER.iter().position(|o| o == c).unwrap_or(usize::MAX));
        cats.dedup();
        for cat in cats {
            ui.menu_button(cat, |ui| {
                for s in app.session.registry.iter().filter(|s| s.category == cat) {
                    if ui.button(s.label).clicked() {
                        create = Some(s.class);
                        ui.close();
                    }
                }
            });
        }
    });
    let has_sel = !app.selection.is_empty();
    if ui.add_enabled(has_sel, egui::Button::new("View").shortcut_text("1")).clicked() {
        app.viewed = app.selection.last().copied();
        ui.close();
    }
    if ui.add_enabled(has_sel, egui::Button::new("Disable/Enable").shortcut_text("D")).clicked() {
        app.invoke("node.disable");
        ui.close();
    }
    if ui.add_enabled(has_sel, egui::Button::new("Delete").shortcut_text("Delete")).clicked() {
        app.invoke("node.delete");
        ui.close();
    }
    if ui.button("Frame All").clicked() {
        app.selection.clear();
        app.graph.frame_request = true;
        ui.close();
    }
    if let Some(class) = create {
        app.create_node_at(class, false, at.map(|p| (p.x as f64, p.y as f64)));
    }
}

fn pipe(painter: &egui::Painter, from: Pos2, to: Pos2, color: Color32, width: f32, label: Option<&str>, zoom: f32) {
    painter.line_segment([from, to], Stroke::new(width, color));
    let d = to - from;
    if d.length() < 1.0 {
        return;
    }
    let dir = d.normalized();
    let size = 7.0 * zoom.clamp(0.6, 1.5);
    let side = Vec2::new(-dir.y, dir.x) * size * 0.5;
    painter.add(Shape::convex_polygon(vec![to, to - dir * size + side, to - dir * size - side], color, Stroke::NONE));
    if let Some(label) = label
        && zoom > 0.45
    {
        let at = to - dir * (STUB * zoom + 2.0);
        painter.text(at + Vec2::new(6.0, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(10.0 * zoom.clamp(0.8, 1.3)), theme::TEXT_DIM);
    }
}

fn draw_pipes(app: &NukecraftApp, painter: &egui::Painter, view: View) {
    let script = &app.session.script;
    for n in script.nodes() {
        let rect = view.node_rect(n);
        let slots = shown_inputs(app, n);
        for i in 0..slots {
            let (anchor, _) = input_anchor(rect, i, is_mask(app, n, i));
            let label = if slots > 1 { Some(input_label(app, n, i)) } else { None };
            let color = if is_mask(app, n, i) { theme::PIPE_MASK } else { theme::PIPE };
            match n.input(i).and_then(|u| script.node(u)) {
                Some(up) => pipe(painter, view.node_rect(up).center_bottom(), anchor, color, 1.6, label.as_deref(), view.zoom),
                None => {
                    let tip = input_handle(view, app, n, i);
                    pipe(painter, tip, anchor, theme::faded(color, 0.55), 1.2, label.as_deref(), view.zoom);
                }
            }
        }
    }
}

fn subtitle(app: &NukecraftApp, n: &Node) -> Option<String> {
    if !n.label.is_empty() {
        return Some(n.label.clone());
    }
    let text = |k: &str| match n.knobs.get(k) {
        Some(nukecraft_graph::KnobValue::Text(t)) => Some(t.clone()),
        _ => None,
    };
    match n.class.as_str() {
        "Merge2" => Some(format!("({})", text("operation").unwrap_or_else(|| "over".into()))),
        "Read" | "Write" => text("file").map(|f| f.rsplit(['/', '\\']).next().unwrap_or(&f).to_string()),
        _ => {
            let _ = app;
            None
        }
    }
}

fn draw_nodes(app: &NukecraftApp, painter: &egui::Painter, view: View) {
    let dragging = match &app.graph.drag {
        Some(Drag::Move { start, delta, .. }) => Some((start, *delta)),
        _ => None,
    };
    for n in app.session.script.nodes() {
        let mut rect = view.node_rect(n);
        if let Some((start, delta)) = dragging
            && start.iter().any(|(id, ..)| *id == n.id)
        {
            rect = rect.translate(delta * view.zoom);
        }
        let spec = app.session.registry.get(&n.class);
        let mut fill = spec.map_or(theme::ERROR, |s| theme::category_color(s.category));
        if n.disabled {
            fill = theme::faded(fill, 0.45);
        }
        let selected = app.selection.contains(&n.id);
        let radius = 4.0 * view.zoom;
        painter.rect_filled(rect, radius, fill);
        let stroke = if selected { Stroke::new(2.0, theme::SELECTED) } else { Stroke::new(1.0, Color32::from_black_alpha(160)) };
        painter.rect_stroke(rect, radius, stroke, StrokeKind::Outside);
        if n.disabled {
            painter.line_segment([rect.left_top(), rect.right_bottom()], Stroke::new(2.0, theme::ERROR));
        }
        if view.zoom > 0.3 {
            painter.text(rect.center(), Align2::CENTER_CENTER, &n.name, FontId::proportional((12.0 * view.zoom).clamp(7.0, 18.0)), theme::text_on(fill));
            if let Some(sub) = subtitle(app, n) {
                painter.text(
                    rect.center_bottom() + Vec2::new(0.0, 3.0),
                    Align2::CENTER_TOP,
                    sub,
                    FontId::proportional((10.0 * view.zoom).clamp(7.0, 14.0)),
                    theme::TEXT_DIM,
                );
            }
        }
        if app.viewed == Some(n.id) {
            let tag = Rect::from_min_size(rect.right_top() + Vec2::new(4.0, 0.0), Vec2::new(14.0, 14.0) * view.zoom.clamp(0.7, 1.4));
            painter.rect_filled(tag, 3.0, theme::ACCENT);
            painter.text(tag.center(), Align2::CENTER_CENTER, "1", FontId::proportional(tag.height() * 0.75), Color32::BLACK);
        }
    }
}

fn draw_drag(app: &NukecraftApp, painter: &egui::Painter, view: View) {
    match &app.graph.drag {
        Some(Drag::Box { start, end }) => {
            let r = Rect::from_two_pos(*start, *end);
            painter.rect_filled(r, 0.0, theme::faded(theme::SELECTED, 0.08));
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, theme::faded(theme::SELECTED, 0.7)), StrokeKind::Inside);
        }
        Some(Drag::Pipe { from, to }) => {
            let start = match from {
                PipeEnd::Output(id) => app.session.script.node(*id).map(|n| view.node_rect(n).center_bottom()),
                PipeEnd::Input(id, i) => app.session.script.node(*id).map(|n| input_anchor(view.node_rect(n), *i, is_mask(app, n, *i)).0),
            };
            if let Some(s) = start {
                let (a, b) = match from {
                    PipeEnd::Output(_) => (s, *to),
                    PipeEnd::Input(..) => (*to, s),
                };
                pipe(painter, a, b, theme::ACCENT, 2.0, None, view.zoom);
            }
        }
        _ => {}
    }
}

fn register_elements(app: &mut NukecraftApp, view: View) {
    let mut elements = Vec::new();
    for n in app.session.script.nodes() {
        let rect = view.node_rect(n);
        elements.push((format!("graph.node.{}", n.name), rect, n.name.clone()));
        elements.push((
            format!("graph.node.{}.output", n.name),
            Rect::from_center_size(rect.center_bottom() - Vec2::new(0.0, 2.0), Vec2::splat(6.0)),
            "output".into(),
        ));
        for i in 0..shown_inputs(app, n) {
            let h = input_handle(view, app, n, i);
            elements.push((format!("graph.node.{}.input.{i}", n.name), Rect::from_center_size(h, Vec2::splat(PICK)), input_label(app, n, i)));
        }
    }
    for (id, rect, label) in elements {
        app.automation.add(id, rect, label);
    }
}
