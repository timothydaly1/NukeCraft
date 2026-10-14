//! The Properties panel: the knobs of open nodes (double-click a node, or create one).
//!
//! Every edit is an engine command (`node.setKnob`, `node.rename`…). Dragging a number is one undo
//! step however many values it passes through (an undo group). The key button beside a numeric
//! knob sets or deletes a key at the current frame; editing an animated knob keys the current
//! frame, as in Nuke. Right-click the key button for Set Key, Delete Key, No Animation and Reset.

use std::collections::HashMap;

use egui::{Color32, RichText};
use nukecraft_geom::NAMED_FORMATS;
use nukecraft_graph::{Anim, KnobValue, NodeId, Value};
use nukecraft_nodes::{KnobKind, KnobSpec};
use nukecraft_time::FrameTime;
use serde_json::{Value as Json, json};

use crate::{NukecraftApp, theme};

#[derive(Default)]
pub struct PropsState {
    /// Text being typed into text knobs and name fields, until committed.
    buffers: HashMap<String, String>,
}

/// What the panel asks for; applied after drawing.
enum Action {
    Set { node: String, knob: &'static str, value: Json },
    Begin(String),
    End,
    Rename { node: String, name: String },
    Disable { node: String, disabled: bool },
    Close(NodeId),
    Key { node: String, knob: &'static str, op: KeyOp },
    Browse { node: String, knob: &'static str, save: bool },
    Render(String),
}

#[derive(Clone, Copy, PartialEq)]
enum KeyOp {
    Toggle,
    Set,
    Delete,
    NoAnimation,
    Reset,
}

/// A knob's state at the current frame, for drawing.
struct KnobView {
    value: Value,
    animated: bool,
    key_here: bool,
}

pub fn show(app: &mut NukecraftApp, ui: &mut egui::Ui) {
    let mut actions = Vec::new();
    ui.horizontal(|ui| {
        ui.heading("Properties");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let r = ui.small_button("Close all");
            app.automation.add("props.closeAll", r.rect, "Close all");
            if r.clicked() {
                app.open_props.clear();
            }
        });
    });
    ui.separator();
    if app.open_props.is_empty() {
        ui.colored_label(theme::TEXT_DIM, "Double-click a node to edit its knobs.");
    }
    let ids = app.open_props.clone();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for id in ids {
            node_panel(app, ui, id, &mut actions);
            ui.add_space(6.0);
        }
    });
    ui.allocate_space(ui.available_size());
    apply(app, actions);
}

fn node_panel(app: &mut NukecraftApp, ui: &mut egui::Ui, id: NodeId, actions: &mut Vec<Action>) {
    let Some(node) = app.session.script.node(id).cloned() else { return };
    let Some(spec) = app.session.registry.get(&node.class) else {
        ui.colored_label(theme::ERROR, format!("{}: unknown class {}", node.name, node.class));
        return;
    };
    // Everything needed from the class is 'static: copy it out so `app` is free to change.
    let (class, label, help, knobs) = (spec.class, spec.label, spec.help, spec.knobs);
    let fill = theme::category_color(spec.category);
    let mut elements: Vec<(String, egui::Rect, String)> = Vec::new();
    egui::Frame::group(ui.style()).fill(theme::PANEL_DARK).show(ui, |ui| {
        ui.horizontal(|ui| {
            let (swatch, _) = ui.allocate_exact_size(egui::Vec2::new(10.0, 18.0), egui::Sense::hover());
            ui.painter().rect_filled(swatch, 2.0, fill);
            let key = format!("name:{}", id.0);
            let buf = app.props.buffers.entry(key.clone()).or_insert_with(|| node.name.clone());
            let r = ui.add(egui::TextEdit::singleline(buf).desired_width(130.0).font(egui::TextStyle::Monospace));
            elements.push((format!("props.{}.name", node.name), r.rect, "name".into()));
            if r.lost_focus() {
                let new = app.props.buffers.remove(&key).unwrap_or_default();
                if new != node.name && !new.is_empty() {
                    actions.push(Action::Rename { node: node.name.clone(), name: new });
                }
            } else if !r.has_focus() {
                app.props.buffers.insert(key, node.name.clone());
            }
            ui.colored_label(theme::TEXT_DIM, label);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let r = ui.small_button("×").on_hover_text("Close");
                elements.push((format!("props.{}.close", node.name), r.rect, "close".into()));
                if r.clicked() {
                    actions.push(Action::Close(id));
                }
                let mut disabled = node.disabled;
                let r = ui.checkbox(&mut disabled, "disable");
                elements.push((format!("props.{}.disable", node.name), r.rect, "disable".into()));
                if r.changed() {
                    actions.push(Action::Disable { node: node.name.clone(), disabled });
                }
            });
        });
        if !help.is_empty() {
            ui.colored_label(theme::TEXT_DIM, RichText::new(help).small());
        }
        ui.add_space(2.0);
        egui::Grid::new(("knobs", id.0)).num_columns(3).spacing([8.0, 4.0]).min_col_width(20.0).show(ui, |ui| {
            for k in knobs {
                let view = knob_view(app, &node, k);
                knob_row(app, ui, &node.name, id, k, &view, actions, &mut elements);
                ui.end_row();
            }
        });
        if class == "Write" {
            ui.add_space(4.0);
            let r = ui.button("Render").on_hover_text("Render this Write node over the frame range");
            elements.push((format!("props.{}.render", node.name), r.rect, "Render".into()));
            if r.clicked() {
                actions.push(Action::Render(node.name.clone()));
            }
        }
    });
    for (id, rect, label) in elements {
        app.automation.add(id, rect, label);
    }
}

fn knob_view(app: &NukecraftApp, node: &nukecraft_graph::Node, k: &KnobSpec) -> KnobView {
    let t = app.session.time;
    let stored = node.knobs.get(k.name).cloned().unwrap_or_else(|| k.default_value(&app.session.script.root.format));
    let stored = k.validate(stored.clone()).unwrap_or(stored);
    let key_here = stored.components().is_some_and(|cs| cs.iter().any(|a| matches!(a, Anim::Curve { curve } if curve.keys().iter().any(|key| key.time == t))));
    KnobView { value: stored.eval(t), animated: stored.is_animated(), key_here }
}

fn label_color(view: &KnobView) -> Color32 {
    if view.key_here {
        theme::KEY
    } else if view.animated {
        theme::ANIMATED
    } else {
        theme::TEXT
    }
}

#[allow(clippy::too_many_arguments)]
fn knob_row(
    app: &mut NukecraftApp,
    ui: &mut egui::Ui,
    node: &str,
    id: NodeId,
    k: &KnobSpec,
    view: &KnobView,
    actions: &mut Vec<Action>,
    elements: &mut Vec<(String, egui::Rect, String)>,
) {
    ui.label(RichText::new(k.label).color(label_color(view))).on_hover_text(k.help);
    let base = format!("props.{node}.{}", k.name);
    let set = |actions: &mut Vec<Action>, value: Json| actions.push(Action::Set { node: node.to_string(), knob: k.name, value });
    ui.horizontal(|ui| match k.kind {
        KnobKind::Bool => {
            let mut b = matches!(view.value, Value::Bool(true));
            let r = ui.checkbox(&mut b, "");
            elements.push((base.clone(), r.rect, k.label.into()));
            if r.changed() {
                set(actions, json!(b));
            }
        }
        KnobKind::Choice(options) => {
            let current = text_of(&view.value);
            let mut chosen = current.clone();
            let r = egui::ComboBox::from_id_salt((id.0, k.name)).selected_text(&current).width(140.0).show_ui(ui, |ui| {
                for o in options {
                    ui.selectable_value(&mut chosen, o.to_string(), *o);
                }
            });
            elements.push((base.clone(), r.response.rect, k.label.into()));
            if chosen != current {
                set(actions, json!(chosen));
            }
        }
        KnobKind::Channels => {
            let current = text_of(&view.value);
            let mut chosen = current.clone();
            let r = egui::ComboBox::from_id_salt((id.0, k.name)).selected_text(&current).width(140.0).show_ui(ui, |ui| {
                for o in ["rgba", "rgb", "alpha", "rgba.alpha", "all", "none"] {
                    ui.selectable_value(&mut chosen, o.to_string(), o);
                }
            });
            elements.push((base.clone(), r.response.rect, k.label.into()));
            if chosen != current {
                set(actions, json!(chosen));
            }
        }
        KnobKind::Format => {
            let current = text_of(&view.value);
            let shown = if current.is_empty() { "root format".to_string() } else { current.clone() };
            let mut chosen = current.clone();
            let r = egui::ComboBox::from_id_salt((id.0, k.name)).selected_text(shown).width(160.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut chosen, String::new(), "root format");
                for (name, w, h, _) in NAMED_FORMATS {
                    ui.selectable_value(&mut chosen, name.to_string(), format!("{name}  {w}×{h}"));
                }
            });
            elements.push((base.clone(), r.response.rect, k.label.into()));
            if chosen != current {
                set(actions, json!(chosen));
            }
        }
        KnobKind::Text | KnobKind::File => {
            let key = format!("{}:{}", id.0, k.name);
            let current = text_of(&view.value);
            let buf = app.props.buffers.entry(key.clone()).or_insert_with(|| current.clone());
            let r = ui.add(egui::TextEdit::singleline(buf).desired_width(if k.kind == KnobKind::File { 190.0 } else { 220.0 }));
            elements.push((base.clone(), r.rect, k.label.into()));
            if r.lost_focus() {
                let new = app.props.buffers.remove(&key).unwrap_or_default();
                if new != current {
                    set(actions, json!(new));
                }
            } else if !r.has_focus() {
                app.props.buffers.insert(key, current);
            }
            if k.kind == KnobKind::File {
                let r = ui.small_button("…").on_hover_text("Browse");
                elements.push((format!("{base}.browse"), r.rect, "browse".into()));
                if r.clicked() {
                    actions.push(Action::Browse { node: node.to_string(), knob: k.name, save: node_class(app, node) == "Write" });
                }
            }
        }
        KnobKind::Float | KnobKind::Int | KnobKind::Xy | KnobKind::Color | KnobKind::Box => {
            let n = match k.kind {
                KnobKind::Float | KnobKind::Int => 1,
                KnobKind::Xy => 2,
                _ => 4,
            };
            let nums = view.value.as_numbers().unwrap_or(&[]).to_vec();
            let mut values: Vec<f64> = (0..n).map(|i| nums.get(i).or(nums.last()).copied().unwrap_or(0.0)).collect();
            let names: &[&str] = match k.kind {
                KnobKind::Xy => &["x", "y"],
                KnobKind::Color => &["r", "g", "b", "a"],
                KnobKind::Box => &["x", "y", "r", "t"],
                _ => &[""],
            };
            let magnitude = values.iter().fold(0.0f64, |m, v| m.max(v.abs()));
            let speed = match k.kind {
                KnobKind::Int => 0.1,
                KnobKind::Color => 0.005,
                _ if magnitude <= 2.0 => 0.01,
                _ => 0.5,
            };
            let mut changed = false;
            let (mut started, mut stopped) = (false, false);
            for i in 0..n {
                if !names.get(i).is_some_and(|s| s.is_empty()) {
                    ui.colored_label(theme::TEXT_DIM, names.get(i).copied().unwrap_or(""));
                }
                let Some(v) = values.get_mut(i) else { continue };
                let mut dv = egui::DragValue::new(v).speed(speed).max_decimals(4);
                if k.kind == KnobKind::Int {
                    dv = dv.fixed_decimals(0);
                }
                let r = ui.add(dv);
                elements.push((if i == 0 { base.clone() } else { format!("{base}.{i}") }, r.rect, format!("{} {}", k.label, names.get(i).unwrap_or(&""))));
                changed |= r.changed();
                started |= r.drag_started();
                stopped |= r.drag_stopped();
            }
            if k.kind == KnobKind::Color {
                let to8 = |v: f64| (nukecraft_color::linear_to_srgb(v as f32).clamp(0.0, 1.0) * 255.0) as u8;
                let c = Color32::from_rgb(to8(values[0]), to8(values[1]), to8(values[2]));
                let (rect, _) = ui.allocate_exact_size(egui::Vec2::new(18.0, 18.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 2.0, c);
            }
            if started {
                actions.push(Action::Begin(format!("Set {}", k.label)));
            }
            if changed {
                let value = if n == 1 { json!(values[0]) } else { json!(values) };
                set(actions, value);
            }
            if stopped {
                actions.push(Action::End);
            }
        }
    });
    // Key button for numeric knobs: a filled diamond on a key, an outlined one when animated
    // elsewhere, a small circle when not animated (drawn, not glyphs: fonts may lack them).
    if k.kind.is_numeric() {
        let (rect, r) = ui.allocate_exact_size(egui::Vec2::new(16.0, 16.0), egui::Sense::click());
        let c = rect.center();
        let d = 5.0;
        let diamond = vec![c + egui::Vec2::new(0.0, -d), c + egui::Vec2::new(d, 0.0), c + egui::Vec2::new(0.0, d), c + egui::Vec2::new(-d, 0.0)];
        let p = ui.painter();
        if view.key_here {
            p.add(egui::Shape::convex_polygon(diamond, theme::KEY, egui::Stroke::NONE));
        } else if view.animated {
            p.add(egui::Shape::closed_line(diamond, egui::Stroke::new(1.5, theme::ANIMATED)));
        } else {
            p.circle_stroke(c, 3.5, egui::Stroke::new(1.0, if r.hovered() { theme::TEXT } else { theme::TEXT_DIM }));
        }
        let r = r.on_hover_text("Set or delete a key at this frame (right-click: more)");
        elements.push((format!("{base}.key"), r.rect, "key".into()));
        if r.clicked() {
            actions.push(Action::Key { node: node.to_string(), knob: k.name, op: KeyOp::Toggle });
        }
        r.context_menu(|ui| {
            for (label, op) in [("Set Key", KeyOp::Set), ("Delete Key", KeyOp::Delete), ("No Animation", KeyOp::NoAnimation), ("Set to Default", KeyOp::Reset)]
            {
                if ui.button(label).clicked() {
                    actions.push(Action::Key { node: node.to_string(), knob: k.name, op });
                    ui.close();
                }
            }
        });
    } else {
        ui.label("");
    }
}

fn node_class(app: &NukecraftApp, name: &str) -> String {
    app.session.script.by_name(name).map(|n| n.class.clone()).unwrap_or_default()
}

fn text_of(v: &Value) -> String {
    match v {
        Value::Text(t) => t.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Numbers(n) => n.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" "),
    }
}

/// The knob's current value as command parameters expect it.
fn current_json(app: &NukecraftApp, node: &str, knob: &str) -> Option<Json> {
    let n = app.session.script.by_name(node)?;
    let spec = app.session.registry.get(&n.class)?.knob(knob)?;
    let v: KnobValue = n.knobs.get(knob).cloned().unwrap_or_else(|| spec.default_value(&app.session.script.root.format));
    Some(match v.eval(app.session.time) {
        Value::Numbers(nums) if nums.len() == 1 => json!(nums.first()),
        Value::Numbers(nums) => json!(nums),
        Value::Bool(b) => json!(b),
        Value::Text(t) => json!(t),
    })
}

fn apply(app: &mut NukecraftApp, actions: Vec<Action>) {
    let frame = |t: FrameTime| t.as_f64();
    for a in actions {
        match a {
            Action::Set { node, knob, value } => {
                app.run("node.setKnob", json!({ "node": node, "knob": knob, "value": value }));
            }
            Action::Begin(label) => app.session.begin_group(&label),
            Action::End => app.session.end_group(),
            Action::Rename { node, name } => {
                app.run("node.rename", json!({ "node": node, "name": name }));
            }
            Action::Disable { node, disabled } => {
                app.run("node.disable", json!({ "node": node, "disabled": disabled }));
            }
            Action::Close(id) => app.open_props.retain(|o| *o != id),
            Action::Key { node, knob, op } => {
                let t = app.session.time;
                let animated = app.session.script.by_name(&node).and_then(|n| n.knobs.get(knob)).is_some_and(KnobValue::is_animated);
                let key_here = app
                    .session
                    .script
                    .by_name(&node)
                    .and_then(|n| n.knobs.get(knob))
                    .and_then(KnobValue::components)
                    .is_some_and(|cs| cs.iter().any(|a| matches!(a, Anim::Curve { curve } if curve.keys().iter().any(|k| k.time == t))));
                let op = match op {
                    KeyOp::Toggle if animated && key_here => KeyOp::Delete,
                    KeyOp::Toggle => KeyOp::Set,
                    other => other,
                };
                match op {
                    KeyOp::Set => {
                        if let Some(value) = current_json(app, &node, knob) {
                            app.run("node.setKnob", json!({ "node": node, "knob": knob, "value": value, "frame": frame(t) }));
                        }
                    }
                    KeyOp::Delete => {
                        app.run("node.removeKey", json!({ "node": node, "knob": knob, "frame": frame(t) }));
                    }
                    KeyOp::NoAnimation => {
                        app.run("node.clearAnimation", json!({ "node": node, "knob": knob }));
                    }
                    KeyOp::Reset => {
                        app.run("node.resetKnob", json!({ "node": node, "knob": knob }));
                    }
                    KeyOp::Toggle => {}
                }
            }
            Action::Browse { node, knob, save } => {
                let picked = if save { crate::dialogs::pick_save("render.####.exr", &["exr", "png"]) } else { crate::dialogs::pick_image() };
                if let Some(path) = picked {
                    app.run("node.setKnob", json!({ "node": node, "knob": knob, "value": path }));
                }
            }
            Action::Render(node) => app.start_render(json!({ "nodes": [node] })),
        }
    }
}
