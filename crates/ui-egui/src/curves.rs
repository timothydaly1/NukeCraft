//! The Curve Editor: the animation curves of the nodes whose properties are open, one per
//! animated knob component, with their keys.
//!
//! - Click a key to select it (Shift adds or removes), drag on empty space to box-select, drag
//!   selected keys to move them (frames snap to whole frames); Delete removes the selected keys.
//! - The toolbar sets the selected keys' interpolation (Constant, Linear, Smooth, Flat) and fits
//!   the view (F); scroll zooms, middle- or Alt-drag pans.
//!
//! Every edit is an engine command (`node.editKeys`, `node.setInterpolation`); a drag is one undo
//! step.

use egui::{Align2, Color32, FontId, Modifiers, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2};
use nukecraft_curve::{Interp, Key};
use nukecraft_graph::{Anim, NodeId};
use nukecraft_time::FrameTime;
use serde_json::json;

use crate::{NukecraftApp, theme};

const LIST_W: f32 = 180.0;
const TOOLBAR_H: f32 = 24.0;
const PICK: f32 = 7.0;

/// One curve: a component of a node's knob.
#[derive(Clone, Debug, PartialEq)]
pub struct CurveRef {
    node: NodeId,
    knob: &'static str,
    comp: usize,
}

/// A key on a curve.
#[derive(Clone, Debug, PartialEq)]
struct KeyRef {
    curve: CurveRef,
    time: FrameTime,
}

enum Drag {
    /// Moving the selected keys: each with its original value and interpolation; `applied` is
    /// where they are now (a whole-frame and value offset); `others` are the curves' unselected
    /// keys (restored if a moved key passed over them).
    Keys {
        start: Pos2,
        keys: Vec<(KeyRef, f64, Interp)>,
        others: Vec<(CurveRef, Key)>,
        applied: (i64, f64),
    },
    Box {
        start: Pos2,
        end: Pos2,
    },
    Pan,
}

#[derive(Default)]
pub struct CurveState {
    /// Visible frames and values: `[f0, f1, v0, v1]`.
    view: Option<[f64; 4]>,
    selected: Vec<KeyRef>,
    drag: Option<Drag>,
    fit_request: bool,
    /// Where the editor was last drawn (keys go to the panel under the pointer).
    rect: Option<Rect>,
}

impl CurveState {
    pub fn selected_count(&self) -> usize {
        self.selected.len()
    }

    pub fn rect(&self) -> Option<Rect> {
        self.rect
    }

    pub fn set_rect(&mut self, rect: Option<Rect>) {
        self.rect = rect;
    }
}

/// A shown curve: what it is, its label, colour and keys.
struct Shown {
    curve: CurveRef,
    label: String,
    color: Color32,
    keys: Vec<Key>,
    anim: Anim,
}

/// Component names for multi-number knobs.
fn component_name(kind: nukecraft_nodes::KnobKind, comp: usize) -> &'static str {
    use nukecraft_nodes::KnobKind;
    let names: &[&str] = match kind {
        KnobKind::Xy => &["x", "y"],
        KnobKind::Color => &["r", "g", "b", "a"],
        KnobKind::Box => &["x", "y", "r", "t"],
        _ => &[],
    };
    names.get(comp).copied().unwrap_or("")
}

fn component_color(kind: nukecraft_nodes::KnobKind, comp: usize) -> Color32 {
    match (kind, comp) {
        (nukecraft_nodes::KnobKind::Color, 0) | (nukecraft_nodes::KnobKind::Xy, 0) => Color32::from_rgb(230, 100, 90),
        (nukecraft_nodes::KnobKind::Color, 1) | (nukecraft_nodes::KnobKind::Xy, 1) => Color32::from_rgb(110, 200, 110),
        (nukecraft_nodes::KnobKind::Color, 2) => Color32::from_rgb(100, 150, 240),
        _ => theme::ACCENT,
    }
}

/// The animated components of the nodes whose properties are open.
fn shown_curves(app: &NukecraftApp) -> Vec<Shown> {
    let mut out = Vec::new();
    for &id in &app.open_props {
        let Some(node) = app.session.script.node(id) else { continue };
        let Some(spec) = app.session.registry.get(&node.class) else { continue };
        for ks in spec.knobs {
            let Some(comps) = node.knobs.get(ks.name).and_then(|v| v.components()) else { continue };
            let single = comps.len() == 1;
            for (comp, anim) in comps.iter().enumerate() {
                let Anim::Curve { curve } = anim else { continue };
                let suffix = if single { String::new() } else { format!(".{}", component_name(ks.kind, comp)) };
                out.push(Shown {
                    curve: CurveRef { node: id, knob: ks.name, comp },
                    label: format!("{}.{}{suffix}", app.path(id), ks.name),
                    color: component_color(ks.kind, comp),
                    keys: curve.keys().to_vec(),
                    anim: (*anim).clone(),
                });
            }
        }
    }
    out
}

/// Maps frames and values to the canvas.
#[derive(Clone, Copy)]
struct Map {
    rect: Rect,
    view: [f64; 4],
}

impl Map {
    fn to_screen(self, f: f64, v: f64) -> Pos2 {
        let [f0, f1, v0, v1] = self.view;
        let x = self.rect.left() + ((f - f0) / (f1 - f0).max(1e-9)) as f32 * self.rect.width();
        let y = self.rect.bottom() - ((v - v0) / (v1 - v0).max(1e-12)) as f32 * self.rect.height();
        Pos2::new(x, y)
    }

    fn to_curve(self, p: Pos2) -> (f64, f64) {
        let [f0, f1, v0, v1] = self.view;
        let u = ((p.x - self.rect.left()) / self.rect.width().max(1.0)) as f64;
        let w = ((self.rect.bottom() - p.y) / self.rect.height().max(1.0)) as f64;
        (f0 + u * (f1 - f0), v0 + w * (v1 - v0))
    }

    /// Frames and values per point.
    fn scale(self) -> (f64, f64) {
        let [f0, f1, v0, v1] = self.view;
        ((f1 - f0) / self.rect.width().max(1.0) as f64, (v1 - v0) / self.rect.height().max(1.0) as f64)
    }
}

/// A view framing every key (and the script's frame range when there are none).
fn fit_view(app: &NukecraftApp, curves: &[Shown]) -> [f64; 4] {
    let range = app.session.script.root.frames;
    let keys = curves.iter().flat_map(|c| c.keys.iter());
    let (mut f0, mut f1, mut v0, mut v1) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for k in keys {
        f0 = f0.min(k.time.as_f64());
        f1 = f1.max(k.time.as_f64());
        v0 = v0.min(k.value);
        v1 = v1.max(k.value);
    }
    if !f0.is_finite() {
        (f0, f1, v0, v1) = (range.first as f64, range.last as f64, 0.0, 1.0);
    }
    if f1 - f0 < 1.0 {
        (f0, f1) = (f0 - 5.0, f1 + 5.0);
    }
    if v1 - v0 < 1e-6 {
        (v0, v1) = (v0 - 1.0, v1 + 1.0);
    }
    let (pf, pv) = ((f1 - f0) * 0.08, (v1 - v0) * 0.12);
    [f0 - pf, f1 + pf, v0 - pv, v1 + pv]
}

/// Builds the curve label's automation id part: `<node>.<knob>.<comp>`.
fn curve_id(app: &NukecraftApp, c: &CurveRef) -> String {
    format!("{}.{}.{}", app.path(c.node), c.knob, c.comp)
}

pub fn show(app: &mut NukecraftApp, ui: &mut egui::Ui) {
    let full = ui.max_rect();
    app.curves.rect = Some(full);
    ui.painter().rect_filled(full, 0.0, theme::GRAPH_BG);
    let curves = shown_curves(app);
    // Forget selections whose keys are gone.
    app.curves.selected.retain(|k| curves.iter().any(|c| c.curve == k.curve && c.keys.iter().any(|x| x.time == k.time)));
    let list = Rect::from_min_max(full.min, Pos2::new(full.left() + LIST_W, full.bottom()));
    let toolbar = Rect::from_min_max(Pos2::new(list.right(), full.top()), Pos2::new(full.right(), full.top() + TOOLBAR_H));
    let canvas = Rect::from_min_max(Pos2::new(list.right(), toolbar.bottom()), full.max);
    list_ui(app, ui, list, &curves);
    toolbar_ui(app, ui, toolbar);
    if app.curves.view.is_none() || app.curves.fit_request {
        app.curves.view = Some(fit_view(app, &curves));
        app.curves.fit_request = false;
    }
    let Some(view) = app.curves.view else { return };
    canvas_ui(app, ui, Map { rect: canvas, view }, &curves);
}

fn list_ui(app: &mut NukecraftApp, ui: &mut egui::Ui, rect: Rect, curves: &[Shown]) {
    ui.painter().rect_filled(rect, 0.0, theme::PANEL_DARK);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(6.0)).layout(egui::Layout::top_down(egui::Align::Min)));
    if curves.is_empty() {
        child.colored_label(theme::TEXT_DIM, "Open a node's properties and animate a knob to see its curves.");
        return;
    }
    let mut select_all = None;
    for c in curves {
        let selected = app.curves.selected.iter().any(|k| k.curve == c.curve);
        let r = child.selectable_label(selected, egui::RichText::new(&c.label).color(c.color));
        app.automation.add(format!("curves.curve.{}", curve_id(app, &c.curve)), r.rect, c.label.clone());
        if r.clicked() {
            select_all = Some(c);
        }
    }
    if let Some(c) = select_all {
        // Clicking a curve's name selects all its keys.
        app.curves.selected = c.keys.iter().map(|k| KeyRef { curve: c.curve.clone(), time: k.time }).collect();
    }
}

fn toolbar_ui(app: &mut NukecraftApp, ui: &mut egui::Ui, rect: Rect) {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(6.0, 2.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let ui = &mut child;
    let mut elements = Vec::new();
    let r = ui.button("Fit").on_hover_text("Frame every key (F)");
    if r.clicked() {
        app.curves.fit_request = true;
    }
    elements.push(("curves.fit".to_string(), r.rect, "Fit"));
    ui.separator();
    let has_sel = !app.curves.selected.is_empty();
    for (label, interp, name) in [
        ("Constant", Interp::Constant, "constant"),
        ("Linear", Interp::Linear, "linear"),
        ("Smooth", Interp::CatmullRom, "catmull-rom"),
        ("Flat", Interp::Horizontal, "horizontal"),
    ] {
        let r = ui.add_enabled(has_sel, egui::Button::new(label));
        if r.clicked() {
            set_interpolation(app, interp, name);
        }
        elements.push((format!("curves.interp.{name}"), r.rect, label));
    }
    ui.separator();
    let r = ui.add_enabled(has_sel, egui::Button::new("Delete"));
    if r.clicked() {
        delete_selected(app);
    }
    elements.push(("curves.delete".to_string(), r.rect, "Delete"));
    if let [one] = &app.curves.selected[..] {
        let v = app
            .session
            .script
            .node(one.curve.node)
            .and_then(|n| n.knobs.get(one.curve.knob))
            .and_then(|v| v.components())
            .and_then(|c| c.get(one.curve.comp).map(|a| a.eval(one.time)));
        if let Some(v) = v {
            ui.colored_label(theme::TEXT_DIM, format!("frame {}   value {v:.4}", one.time));
        }
    } else if has_sel {
        ui.colored_label(theme::TEXT_DIM, format!("{} keys", app.curves.selected.len()));
    }
    for (id, rect, label) in elements {
        app.automation.add(id, rect, label);
    }
}

/// The selected keys grouped by curve, with their frames.
fn by_curve(selected: &[KeyRef]) -> Vec<(CurveRef, Vec<FrameTime>)> {
    let mut out: Vec<(CurveRef, Vec<FrameTime>)> = Vec::new();
    for k in selected {
        match out.iter_mut().find(|(c, _)| *c == k.curve) {
            Some((_, frames)) => frames.push(k.time),
            None => out.push((k.curve.clone(), vec![k.time])),
        }
    }
    out
}

fn set_interpolation(app: &mut NukecraftApp, _interp: Interp, name: &str) {
    let groups = by_curve(&app.curves.selected);
    app.session.begin_group("Set Interpolation");
    for (c, frames) in groups {
        let Ok(node) = app.session.script.path_of(c.node) else { continue };
        let frames: Vec<f64> = frames.iter().map(|t| t.as_f64()).collect();
        app.run("node.setInterpolation", json!({ "node": node, "knob": c.knob, "component": c.comp, "frames": frames, "interp": name }));
    }
    app.session.end_group();
}

fn delete_selected(app: &mut NukecraftApp) {
    let groups = by_curve(&app.curves.selected);
    app.session.begin_group("Delete Keys");
    for (c, frames) in groups {
        let Ok(node) = app.session.script.path_of(c.node) else { continue };
        let frames: Vec<f64> = frames.iter().map(|t| t.as_f64()).collect();
        app.run("node.editKeys", json!({ "node": node, "knob": c.knob, "component": c.comp, "remove": frames }));
    }
    app.session.end_group();
    app.curves.selected.clear();
}

/// The key under `p`, if any.
fn key_at(curves: &[Shown], map: Map, p: Pos2) -> Option<KeyRef> {
    let mut best: Option<(f32, KeyRef)> = None;
    for c in curves {
        for k in &c.keys {
            let d = map.to_screen(k.time.as_f64(), k.value).distance(p);
            if d <= PICK && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                best = Some((d, KeyRef { curve: c.curve.clone(), time: k.time }));
            }
        }
    }
    best.map(|(_, k)| k)
}

fn canvas_ui(app: &mut NukecraftApp, ui: &mut egui::Ui, map: Map, curves: &[Shown]) {
    let resp = ui.interact(map.rect, egui::Id::new("curves_canvas"), Sense::click_and_drag());
    app.automation.add("curves.canvas", map.rect, "Curve Editor");
    let painter = ui.painter_at(map.rect);
    let modifiers = ui.input(|i| i.modifiers);

    // Zoom about the pointer; F fits.
    if let Some(p) = resp.hover_pos() {
        let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        let factor = ((-scroll * 0.002).exp() / pinch) as f64;
        if (factor - 1.0).abs() > 1e-4 {
            let (f, v) = map.to_curve(p);
            let [f0, f1, v0, v1] = map.view;
            app.curves.view = Some([f + (f0 - f) * factor, f + (f1 - f) * factor, v + (v0 - v) * factor, v + (v1 - v) * factor]);
        }
        if !ui.ctx().text_edit_focused() {
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, egui::Key::F)) {
                app.curves.fit_request = true;
            }
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, egui::Key::Delete) || i.consume_key(Modifiers::NONE, egui::Key::Backspace)) {
                delete_selected(app);
            }
        }
    }

    // Press: on a key (select it, start moving), elsewhere a box, or a pan.
    if resp.drag_started()
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
    {
        let middle = ui.input(|i| i.pointer.middle_down());
        app.curves.drag = if middle || modifiers.alt {
            Some(Drag::Pan)
        } else if let Some(k) = key_at(curves, map, origin) {
            if !app.curves.selected.contains(&k) {
                if !modifiers.shift {
                    app.curves.selected.clear();
                }
                app.curves.selected.push(k);
            }
            let keys = selected_keys(app, curves);
            let others = curves
                .iter()
                .filter(|c| keys.iter().any(|(k, ..)| k.curve == c.curve))
                .flat_map(|c| {
                    c.keys.iter().filter(|k| !app.curves.selected.iter().any(|s| s.curve == c.curve && s.time == k.time)).map(|k| (c.curve.clone(), *k))
                })
                .collect();
            app.session.begin_group("Move Keys");
            Some(Drag::Keys { start: origin, keys, others, applied: (0, 0.0) })
        } else {
            Some(Drag::Box { start: origin, end: origin })
        };
    }
    if resp.dragged()
        && let Some(p) = resp.interact_pointer_pos()
    {
        match &mut app.curves.drag {
            Some(Drag::Pan) => {
                let d = resp.drag_delta();
                let (sf, sv) = map.scale();
                let [f0, f1, v0, v1] = map.view;
                let (df, dv) = (-(d.x as f64) * sf, d.y as f64 * sv);
                app.curves.view = Some([f0 + df, f1 + df, v0 + dv, v1 + dv]);
            }
            Some(Drag::Box { end, .. }) => *end = p,
            Some(Drag::Keys { start, .. }) => {
                let (sf, sv) = map.scale();
                let d = p - *start;
                let offset = ((d.x as f64 * sf).round() as i64, -(d.y as f64) * sv);
                move_keys(app, offset);
            }
            None => {}
        }
    }
    if resp.drag_stopped() || (app.curves.drag.is_some() && !ui.input(|i| i.pointer.any_down())) {
        match app.curves.drag.take() {
            Some(Drag::Box { start, end }) => {
                let r = Rect::from_two_pos(start, end);
                if !modifiers.shift {
                    app.curves.selected.clear();
                }
                for c in curves {
                    for k in &c.keys {
                        let kr = KeyRef { curve: c.curve.clone(), time: k.time };
                        if r.contains(map.to_screen(k.time.as_f64(), k.value)) && !app.curves.selected.contains(&kr) {
                            app.curves.selected.push(kr);
                        }
                    }
                }
            }
            Some(Drag::Keys { .. }) => app.session.end_group(),
            _ => {}
        }
    }
    if resp.clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        match key_at(curves, map, p) {
            Some(k) if modifiers.shift => {
                if let Some(i) = app.curves.selected.iter().position(|s| *s == k) {
                    app.curves.selected.remove(i);
                } else {
                    app.curves.selected.push(k);
                }
            }
            Some(k) => app.curves.selected = vec![k],
            None if !modifiers.shift => app.curves.selected.clear(),
            None => {}
        }
    }

    draw(app, &painter, map, curves);
}

/// The selected keys with their values and interpolations.
fn selected_keys(app: &NukecraftApp, curves: &[Shown]) -> Vec<(KeyRef, f64, Interp)> {
    app.curves
        .selected
        .iter()
        .filter_map(|s| {
            let c = curves.iter().find(|c| c.curve == s.curve)?;
            let k = c.keys.iter().find(|k| k.time == s.time)?;
            Some((s.clone(), k.value, k.interp))
        })
        .collect()
}

/// Moves the dragged keys to their original places plus `offset` (whole frames, value).
fn move_keys(app: &mut NukecraftApp, offset: (i64, f64)) {
    let Some(Drag::Keys { keys, others, applied, .. }) = &app.curves.drag else { return };
    if *applied == offset {
        return;
    }
    let (keys, others, prev) = (keys.clone(), others.clone(), *applied);
    let shift = |t: FrameTime, df: i64| t + FrameTime::from_frame(df);
    let mut curves: Vec<CurveRef> = Vec::new();
    for (k, ..) in &keys {
        if !curves.contains(&k.curve) {
            curves.push(k.curve.clone());
        }
    }
    for c in &curves {
        let Ok(node) = app.session.script.path_of(c.node) else { continue };
        let mine: Vec<&(KeyRef, f64, Interp)> = keys.iter().filter(|(k, ..)| k.curve == *c).collect();
        let remove: Vec<f64> = mine.iter().map(|(k, ..)| shift(k.time, prev.0).as_f64()).collect();
        let new_times: Vec<FrameTime> = mine.iter().map(|(k, ..)| shift(k.time, offset.0)).collect();
        let mut set: Vec<serde_json::Value> =
            mine.iter().map(|(k, v, i)| json!({ "frame": shift(k.time, offset.0).as_f64(), "value": v + offset.1, "interp": interp_name(*i) })).collect();
        // Unselected keys a moved key covered before (and no longer does) come back.
        let old_times: Vec<FrameTime> = mine.iter().map(|(k, ..)| shift(k.time, prev.0)).collect();
        for (oc, k) in others.iter().filter(|(oc, _)| oc == c) {
            let _ = oc;
            if old_times.contains(&k.time) && !new_times.contains(&k.time) {
                set.push(json!({ "frame": k.time.as_f64(), "value": k.value, "interp": interp_name(k.interp) }));
            }
        }
        app.run("node.editKeys", json!({ "node": node, "knob": c.knob, "component": c.comp, "remove": remove, "keys": set }));
    }
    // The selection follows the keys.
    for s in &mut app.curves.selected {
        if keys.iter().any(|(k, ..)| k.curve == s.curve && shift(k.time, prev.0) == s.time) {
            s.time = shift(s.time, offset.0 - prev.0);
        }
    }
    if let Some(Drag::Keys { applied, .. }) = &mut app.curves.drag {
        *applied = offset;
    }
}

fn interp_name(i: Interp) -> &'static str {
    match i {
        Interp::Constant => "constant",
        Interp::Linear => "linear",
        Interp::Horizontal => "horizontal",
        Interp::CatmullRom => "catmull-rom",
    }
}

/// Grid steps of 1, 2 or 5 times a power of ten, about `target` units apart.
fn nice_step(target: f64) -> f64 {
    if !target.is_finite() || target <= 0.0 {
        return 1.0;
    }
    let p = 10f64.powf(target.log10().floor());
    [1.0, 2.0, 5.0, 10.0].into_iter().map(|m| m * p).find(|s| *s >= target).unwrap_or(10.0 * p)
}

fn draw(app: &mut NukecraftApp, painter: &egui::Painter, map: Map, curves: &[Shown]) {
    let r = map.rect;
    let [f0, f1, v0, v1] = map.view;
    let (sf, sv) = map.scale();
    // Grid and labels.
    let fstep = nice_step(sf * 70.0).max(1.0);
    let mut f = (f0 / fstep).ceil() * fstep;
    while f <= f1 && f.is_finite() {
        let x = map.to_screen(f, v0).x;
        painter.line_segment([Pos2::new(x, r.top()), Pos2::new(x, r.bottom())], Stroke::new(1.0, theme::GRAPH_GRID));
        painter.text(Pos2::new(x + 2.0, r.bottom() - 2.0), Align2::LEFT_BOTTOM, format!("{}", f + 0.0), FontId::proportional(10.0), theme::TEXT_DIM);
        f += fstep;
    }
    let vstep = nice_step(sv * 40.0);
    let mut v = (v0 / vstep).ceil() * vstep;
    while v <= v1 && v.is_finite() {
        let y = map.to_screen(f0, v).y;
        painter.line_segment([Pos2::new(r.left(), y), Pos2::new(r.right(), y)], Stroke::new(1.0, theme::GRAPH_GRID));
        painter.text(Pos2::new(r.left() + 2.0, y - 1.0), Align2::LEFT_BOTTOM, format!("{:.3}", v + 0.0), FontId::proportional(10.0), theme::TEXT_DIM);
        v += vstep;
    }
    // The current frame.
    let x = map.to_screen(app.session.time.as_f64(), v0).x;
    painter.line_segment([Pos2::new(x, r.top()), Pos2::new(x, r.bottom())], Stroke::new(1.0, theme::KEY));
    // Curves, sampled every two points.
    for c in curves {
        let active = app.curves.selected.iter().any(|k| k.curve == c.curve);
        let color = if active || app.curves.selected.is_empty() { c.color } else { theme::faded(c.color, 0.5) };
        let pts: Vec<Pos2> = (0..=(r.width() / 2.0) as usize)
            .map(|i| {
                let px = r.left() + i as f32 * 2.0;
                let (fr, _) = map.to_curve(Pos2::new(px, r.top()));
                map.to_screen(fr, c.anim.eval(FrameTime::from_f64(fr)))
            })
            .collect();
        painter.add(Shape::line(pts, Stroke::new(1.5, color)));
        for k in &c.keys {
            let p = map.to_screen(k.time.as_f64(), k.value);
            let kr = KeyRef { curve: c.curve.clone(), time: k.time };
            let selected = app.curves.selected.contains(&kr);
            let fill = if selected { theme::SELECTED } else { color };
            painter.rect(Rect::from_center_size(p, Vec2::splat(7.0)), 1.0, fill, Stroke::new(1.0, Color32::BLACK), StrokeKind::Middle);
            app.automation.add(format!("curves.key.{}.{}", curve_id(app, &c.curve), k.time), Rect::from_center_size(p, Vec2::splat(9.0)), c.label.clone());
        }
    }
    if let Some(Drag::Box { start, end }) = &app.curves.drag {
        let b = Rect::from_two_pos(*start, *end);
        painter.rect(b, 0.0, theme::faded(theme::SELECTED, 0.1), Stroke::new(1.0, theme::SELECTED), StrokeKind::Middle);
    }
}
