//! On-viewer handles: the position knobs of nodes whose properties are open, drawn over the
//! Viewer and dragged there, as in Nuke. Transform (translate and a rotation arm), CornerPin2D
//! (the four `to` corners), Rectangle, Radial and Crop (the box's corners and centre) and Ramp
//! (`p0`, `p1`).
//!
//! A drag is one undo step (an undo group around the `node.setKnob` commands it sends), and an
//! animated knob is keyed at the current frame, as when it is edited in the Properties panel.
//! Each handle registers an automation id: `viewer.handle.<node>.<knob>` (box corners add
//! `.0`..`.3` from the bottom-left, counter-clockwise; the centre adds `.move`).

use egui::{Color32, Pos2, Rect, Shape, Stroke, Vec2};
use nukecraft_geom::V2;
use nukecraft_graph::NodeId;
use nukecraft_nodes::Knobs;
use serde_json::json;

use crate::{NukecraftApp, theme};

/// Pick radius, in points.
const PICK: f32 = 9.0;
/// Length of Transform's rotation arm, in points.
const ARM: f32 = 70.0;

/// Maps between image pixels (y up) and the screen.
#[derive(Clone, Copy)]
pub struct Mapping {
    pub rect: Rect,
    /// Format width and height in pixels.
    pub size: (f32, f32),
}

impl Mapping {
    pub fn to_screen(self, p: V2) -> Pos2 {
        Pos2::new(
            self.rect.left() + p.x as f32 * self.rect.width() / self.size.0.max(1.0),
            self.rect.bottom() - p.y as f32 * self.rect.height() / self.size.1.max(1.0),
        )
    }

    pub fn to_image(self, p: Pos2) -> V2 {
        let w = self.rect.width().max(1e-6);
        let h = self.rect.height().max(1e-6);
        V2::new(((p.x - self.rect.left()) * self.size.0 / w) as f64, ((self.rect.bottom() - p.y) * self.size.1 / h) as f64)
    }
}

/// What dragging a handle changes.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    /// An XY knob, moved with the pointer.
    Point,
    /// Transform's translate, drawn at `center + translate`.
    Translate,
    /// Transform's rotation, by the arm's angle about `center + translate`.
    Rotate,
    /// A box knob's corner `0..4` (bottom-left, bottom-right, top-right, top-left).
    Corner(usize),
    /// A box knob as a whole.
    MoveBox,
}

#[derive(Clone, Debug)]
pub struct Handle {
    node: NodeId,
    node_name: String,
    knob: &'static str,
    kind: Kind,
    /// Where it is drawn, on screen.
    at: Pos2,
}

impl Handle {
    fn automation_id(&self) -> String {
        match self.kind {
            Kind::Corner(i) => format!("viewer.handle.{}.{}.{i}", self.node_name, self.knob),
            Kind::MoveBox => format!("viewer.handle.{}.{}.move", self.node_name, self.knob),
            _ => format!("viewer.handle.{}.{}", self.node_name, self.knob),
        }
    }
}

/// A handle being dragged: where it started, in image pixels, and the knob values then.
pub struct Drag {
    handle: Handle,
    start: V2,
    values: Vec<f64>,
    /// Transform's pivot (`center + translate`) when the drag started.
    pivot: V2,
}

/// Overlay lines and handles of one node.
struct Overlay {
    lines: Vec<Shape>,
    handles: Vec<Handle>,
}

/// The overlays of the nodes whose properties are open (the newest first).
fn overlays(app: &NukecraftApp, map: Mapping) -> Vec<Overlay> {
    let mut out = Vec::new();
    for &id in app.open_props.iter().take(8) {
        let Some(node) = app.session.script.node(id) else { continue };
        let Some(spec) = app.session.registry.get(&node.class) else { continue };
        let k = Knobs::evaluate(spec.knobs, &node.knobs, app.session.time, &app.session.script.root.format);
        let h = |knob: &'static str, kind: Kind, p: V2| Handle { node: id, node_name: node.name.clone(), knob, kind, at: map.to_screen(p) };
        let stroke = Stroke::new(1.5, theme::ACCENT);
        let mut o = Overlay { lines: Vec::new(), handles: Vec::new() };
        match spec.class {
            "Transform" => {
                let pivot = k.xy("center") + k.xy("translate");
                let at = map.to_screen(pivot);
                let (s, c) = (k.float("rotate") as f32).to_radians().sin_cos();
                let arm = at + Vec2::new(c, -s) * ARM;
                o.lines.push(Shape::line_segment([at, arm], stroke));
                o.lines.push(Shape::circle_stroke(at, 14.0, stroke));
                o.handles.push(h("translate", Kind::Translate, pivot));
                o.handles.push(Handle { at: arm, ..h("rotate", Kind::Rotate, pivot) });
            }
            "CornerPin2D" => {
                let pts: Vec<V2> = ["to1", "to2", "to3", "to4"].iter().map(|n| k.xy(n)).collect();
                let mut screen: Vec<Pos2> = pts.iter().map(|p| map.to_screen(*p)).collect();
                if let Some(first) = screen.first().copied() {
                    screen.push(first);
                }
                o.lines.push(Shape::line(screen, stroke));
                for (name, p) in ["to1", "to2", "to3", "to4"].iter().zip(pts) {
                    o.handles.push(h(name, Kind::Point, p));
                }
            }
            "Rectangle" | "Radial" | "Crop" => {
                let knob = if spec.class == "Crop" { "box" } else { "area" };
                let [x, y, r, t] = k.bbox(knob);
                let corners = [V2::new(x, y), V2::new(r, y), V2::new(r, t), V2::new(x, t)];
                let screen = corners.map(|p| map.to_screen(p));
                o.lines.push(Shape::closed_line(screen.to_vec(), Stroke::new(1.0, theme::ACCENT)));
                if spec.class == "Radial" {
                    let c = map.to_screen(V2::new((x + r) / 2.0, (y + t) / 2.0));
                    let (rx, ry) = ((screen[1].x - screen[0].x).abs() / 2.0, (screen[0].y - screen[3].y).abs() / 2.0);
                    let ellipse: Vec<Pos2> =
                        (0..=64).map(|i| i as f32 / 64.0 * std::f32::consts::TAU).map(|a| c + Vec2::new(a.cos() * rx, a.sin() * ry)).collect();
                    o.lines.push(Shape::line(ellipse, stroke));
                }
                for (i, p) in corners.into_iter().enumerate() {
                    o.handles.push(h(knob, Kind::Corner(i), p));
                }
                o.handles.push(h(knob, Kind::MoveBox, V2::new((x + r) / 2.0, (y + t) / 2.0)));
            }
            "Ramp" => {
                let (p0, p1) = (k.xy("p0"), k.xy("p1"));
                o.lines.push(Shape::line_segment([map.to_screen(p0), map.to_screen(p1)], stroke));
                o.handles.push(h("p0", Kind::Point, p0));
                o.handles.push(h("p1", Kind::Point, p1));
            }
            _ => continue,
        }
        out.push(o);
    }
    out
}

/// The handle under `p`, nearest first, if any is within reach.
fn hit(overlays: &[Overlay], p: Pos2) -> Option<Handle> {
    overlays.iter().flat_map(|o| o.handles.iter()).filter(|h| h.at.distance(p) <= PICK).min_by(|a, b| a.at.distance(p).total_cmp(&b.at.distance(p))).cloned()
}

/// Starts, continues or ends a handle drag. Returns whether the pointer belongs to a handle (so
/// the Viewer must not pan).
pub fn interact(app: &mut NukecraftApp, ui: &egui::Ui, resp: &egui::Response, map: Mapping) -> bool {
    if resp.drag_started_by(egui::PointerButton::Primary)
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        && let Some(handle) = hit(&overlays(app, map), origin)
    {
        let values = knob_numbers(app, handle.node, handle.knob);
        let pivot = transform_pivot(app, handle.node);
        app.session.begin_group(&format!("Move {}", handle.knob));
        app.viewer.handle_drag = Some(Drag { handle, start: map.to_image(origin), values, pivot });
    }
    let Some(drag) = &app.viewer.handle_drag else { return false };
    if resp.dragged()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let value = dragged_value(drag, map.to_image(p));
        let (node, knob) = (drag.handle.node_name.clone(), drag.handle.knob);
        app.run("node.setKnob", json!({ "node": node, "knob": knob, "value": value }));
    }
    if resp.drag_stopped() || !ui.input(|i| i.pointer.any_down()) {
        app.viewer.handle_drag = None;
        app.session.end_group();
    }
    true
}

/// The knob's numbers at the current time (defaults filled in).
fn knob_numbers(app: &NukecraftApp, node: NodeId, knob: &str) -> Vec<f64> {
    let Some(n) = app.session.script.node(node) else { return Vec::new() };
    let Some(spec) = app.session.registry.get(&n.class) else { return Vec::new() };
    let k = Knobs::evaluate(spec.knobs, &n.knobs, app.session.time, &app.session.script.root.format);
    let s = spec.knob(knob).map(|s| s.kind);
    match s {
        Some(nukecraft_nodes::KnobKind::Box) => k.bbox(knob).to_vec(),
        Some(nukecraft_nodes::KnobKind::Xy) => {
            let v = k.xy(knob);
            vec![v.x, v.y]
        }
        _ => vec![k.float(knob)],
    }
}

/// Transform's `center + translate` (where its handles are drawn), else the origin.
fn transform_pivot(app: &NukecraftApp, node: NodeId) -> V2 {
    let Some(n) = app.session.script.node(node) else { return V2::ZERO };
    let Some(spec) = app.session.registry.get(&n.class) else { return V2::ZERO };
    let k = Knobs::evaluate(spec.knobs, &n.knobs, app.session.time, &app.session.script.root.format);
    k.xy("center") + k.xy("translate")
}

/// The knob value for the pointer at `p` (image pixels), rounded to a thousandth of a pixel.
fn dragged_value(drag: &Drag, p: V2) -> serde_json::Value {
    let d = p - drag.start;
    let v = &drag.values;
    let at = |i: usize| v.get(i).copied().unwrap_or(0.0);
    let round = |x: f64| (x * 1000.0).round() / 1000.0;
    let out: Vec<f64> = match drag.handle.kind {
        Kind::Point | Kind::Translate => vec![at(0) + d.x, at(1) + d.y],
        Kind::Rotate => {
            let angle = |q: V2| (q.y - drag.pivot.y).atan2(q.x - drag.pivot.x).to_degrees();
            let mut delta = angle(p) - angle(drag.start);
            if delta > 180.0 {
                delta -= 360.0;
            } else if delta < -180.0 {
                delta += 360.0;
            }
            vec![at(0) + delta]
        }
        Kind::Corner(i) => {
            let (mx, my) = match i {
                0 => (0, 1),
                1 => (2, 1),
                2 => (2, 3),
                _ => (0, 3),
            };
            let mut b = [at(0), at(1), at(2), at(3)];
            if let Some(x) = b.get_mut(mx) {
                *x += d.x;
            }
            if let Some(y) = b.get_mut(my) {
                *y += d.y;
            }
            b.to_vec()
        }
        Kind::MoveBox => vec![at(0) + d.x, at(1) + d.y, at(2) + d.x, at(3) + d.y],
    };
    let out: Vec<f64> = out.into_iter().map(round).collect();
    if out.len() == 1 { json!(out.first()) } else { json!(out) }
}

/// Draws the overlays and registers the handles' automation ids.
pub fn draw(app: &mut NukecraftApp, painter: &egui::Painter, map: Mapping, hover: Option<Pos2>) {
    let overlays = overlays(app, map);
    let hovered = hover.and_then(|p| hit(&overlays, p));
    let active = app.viewer.handle_drag.as_ref().map(|d| d.handle.automation_id());
    for o in &overlays {
        for line in &o.lines {
            painter.add(line.clone());
        }
        for h in &o.handles {
            let id = h.automation_id();
            let lit = hovered.as_ref().is_some_and(|x| x.automation_id() == id) || active.as_deref() == Some(id.as_str());
            let fill = if lit { theme::SELECTED } else { theme::ACCENT };
            match h.kind {
                Kind::MoveBox | Kind::Translate => {
                    painter.circle(h.at, 4.5, fill, Stroke::new(1.0, Color32::BLACK));
                }
                _ => {
                    painter.rect(Rect::from_center_size(h.at, Vec2::splat(8.0)), 1.0, fill, Stroke::new(1.0, Color32::BLACK), egui::StrokeKind::Middle);
                }
            }
            app.automation.add(id, Rect::from_center_size(h.at, Vec2::splat(10.0)), h.knob);
        }
    }
}
