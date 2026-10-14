//! The Viewer: shows the viewed node over its format, rendered on a background thread.
//!
//! - A cache of rendered frames is keyed by (script revision, node, time); any edit bumps the
//!   revision, so cached frames are never stale. While a new frame renders, the last one stays up.
//! - Display: channels (RGB, R, G, B, A, luminance), exposure in stops and gamma, then the sRGB
//!   viewer transform. Outside the bbox is black. The format is outlined; a bbox that differs
//!   from it is drawn dashed.
//! - Scroll zooms about the pointer, drag (any button) pans, F fits; R/G/B/A/Y toggle channels.
//! - Timeline: frame slider over the root range, step and play (L plays, K stops, Left/Right
//!   step).

use std::collections::HashMap;
use std::sync::Arc;

use egui::{Align2, Color32, ColorImage, FontId, Key, Modifiers, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, TextureHandle, Vec2};
use nukecraft_color::{linear_to_srgb, luminance};
use nukecraft_geom::IRect;
use nukecraft_raster::{Channel, Image};
use serde_json::json;

use crate::worker::{Frame, Key as FrameKey, Worker};
use crate::{NukecraftApp, theme};

/// Memory the frame cache may hold, in bytes.
const CACHE_BYTES: usize = 768 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ChannelMode {
    #[default]
    Rgb,
    Red,
    Green,
    Blue,
    Alpha,
    Luminance,
}

impl ChannelMode {
    const ALL: [ChannelMode; 6] = [ChannelMode::Rgb, ChannelMode::Red, ChannelMode::Green, ChannelMode::Blue, ChannelMode::Alpha, ChannelMode::Luminance];

    fn label(self) -> &'static str {
        match self {
            ChannelMode::Rgb => "RGB",
            ChannelMode::Red => "R",
            ChannelMode::Green => "G",
            ChannelMode::Blue => "B",
            ChannelMode::Alpha => "A",
            ChannelMode::Luminance => "Y",
        }
    }
}

/// Display settings that change the texture but not the render.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Display {
    channel: ChannelMode,
    exposure: f32,
    gamma: f32,
}

pub struct ViewerState {
    worker: Option<Worker>,
    cache: HashMap<FrameKey, Arc<Frame>>,
    order: Vec<FrameKey>,
    cache_revision: u64,
    requested: Option<FrameKey>,
    shown: Option<(FrameKey, Arc<Frame>)>,
    error: Option<(FrameKey, String)>,
    texture: Option<TextureHandle>,
    texture_key: Option<(FrameKey, Display)>,
    pub channel: ChannelMode,
    pub exposure: f32,
    pub gamma: f32,
    zoom: f32,
    pan: Vec2,
    fit_request: bool,
    fitted_format: Option<(u32, u32)>,
    pub playing: bool,
    last_step: f64,
    readout: Option<String>,
    /// An on-viewer handle being dragged.
    pub(crate) handle_drag: Option<crate::handles::Drag>,
}

impl ViewerState {
    pub fn new(ctx: &egui::Context) -> ViewerState {
        ViewerState {
            worker: Worker::spawn(ctx.clone()),
            cache: HashMap::new(),
            order: Vec::new(),
            cache_revision: 0,
            requested: None,
            shown: None,
            error: None,
            texture: None,
            texture_key: None,
            channel: ChannelMode::Rgb,
            exposure: 0.0,
            gamma: 1.0,
            zoom: 1.0,
            pan: Vec2::ZERO,
            fit_request: true,
            fitted_format: None,
            playing: false,
            last_step: 0.0,
            readout: None,
            handle_drag: None,
        }
    }

    /// The error for the frame the Viewer should be showing now.
    pub fn current_error(&self, app: &NukecraftApp) -> Option<String> {
        let node = app.viewed?;
        let key = FrameKey { revision: app.session.revision, node, time: app.session.time };
        self.error.as_ref().filter(|(k, _)| *k == key).map(|(_, e)| e.clone())
    }

    /// A render is in flight.
    pub fn busy(&self) -> bool {
        self.worker.as_ref().is_some_and(Worker::busy)
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    pub fn pan(&self) -> Vec2 {
        self.pan
    }

    fn insert(&mut self, key: FrameKey, frame: Arc<Frame>) {
        self.cache.insert(key, frame);
        self.order.retain(|k| *k != key);
        self.order.push(key);
        let bytes = |f: &Frame| f.image.bbox().area() as usize * f.image.channels().len() * 4;
        let mut total: usize = self.cache.values().map(|f| bytes(f)).sum();
        while total > CACHE_BYTES && self.order.len() > 1 {
            let old = self.order.remove(0);
            if let Some(f) = self.cache.remove(&old) {
                total = total.saturating_sub(bytes(&f));
            }
        }
    }
}

/// Requests, receives and caches the viewed frame. Runs every frame, before drawing.
fn update(app: &mut NukecraftApp) {
    let revision = app.session.revision;
    let v = &mut app.viewer;
    if v.cache_revision != revision {
        v.cache.clear();
        v.order.clear();
        v.cache_revision = revision;
    }
    if let Some(w) = &mut v.worker {
        for done in w.poll() {
            match done.result {
                Ok(frame) if done.key.revision == revision => v.insert(done.key, frame),
                Ok(_) => {}
                Err(e) => v.error = Some((done.key, e)),
            }
        }
    }
    let Some(node) = app.viewed else {
        v.shown = None;
        return;
    };
    let key = FrameKey { revision, node, time: app.session.time };
    if let Some(frame) = v.cache.get(&key) {
        v.shown = Some((key, frame.clone()));
        if v.error.as_ref().is_some_and(|(k, _)| *k == key) {
            v.error = None;
        }
        return;
    }
    if v.error.as_ref().is_some_and(|(k, _)| *k == key) {
        return;
    }
    if v.requested != Some(key) {
        v.requested = Some(key);
        match &mut v.worker {
            Some(w) => w.request(key, app.session.script.clone()),
            None => match crate::worker::render(&app.session.script, &app.session.registry, key) {
                Ok(frame) => {
                    v.insert(key, frame.clone());
                    v.shown = Some((key, frame));
                }
                Err(e) => v.error = Some((key, e)),
            },
        }
    }
}

pub fn show(app: &mut NukecraftApp, ui: &mut egui::Ui) {
    update(app);
    let full = ui.max_rect();
    ui.painter().rect_filled(full, 0.0, theme::VIEWER_BG);
    let toolbar = Rect::from_min_size(full.min, Vec2::new(full.width(), 28.0));
    let timeline = Rect::from_min_max(Pos2::new(full.left(), full.bottom() - 30.0), full.max);
    let info = Rect::from_min_max(Pos2::new(full.left(), timeline.top() - 20.0), Pos2::new(full.right(), timeline.top()));
    let area = Rect::from_min_max(Pos2::new(full.left(), toolbar.bottom()), Pos2::new(full.right(), info.top()));

    toolbar_ui(app, ui, toolbar);
    image_ui(app, ui, area);
    info_ui(app, ui, info);
    timeline_ui(app, ui, timeline);
    playback(app, ui);
}

fn toolbar_ui(app: &mut NukecraftApp, ui: &mut egui::Ui, rect: Rect) {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(6.0, 2.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let ui = &mut child;
    let v = &mut app.viewer;
    let mut elements = Vec::new();
    let r = egui::ComboBox::from_id_salt("viewer_channels").width(60.0).selected_text(v.channel.label()).show_ui(ui, |ui| {
        for m in ChannelMode::ALL {
            ui.selectable_value(&mut v.channel, m, m.label());
        }
    });
    elements.push(("viewer.channel", r.response.rect, "channels"));
    ui.label("E");
    let r = ui.add(egui::DragValue::new(&mut v.exposure).speed(0.05).range(-20.0..=20.0).suffix(" stops")).on_hover_text("Exposure (f-stops)");
    elements.push(("viewer.exposure", r.rect, "exposure"));
    ui.label("γ");
    let r = ui.add(egui::DragValue::new(&mut v.gamma).speed(0.01).range(0.05..=8.0)).on_hover_text("Viewer gamma");
    elements.push(("viewer.gamma", r.rect, "gamma"));
    let r = ui.button("Fit").on_hover_text("Fit the format in the Viewer (F)");
    if r.clicked() {
        v.fit_request = true;
    }
    elements.push(("viewer.fit", r.rect, "Fit"));
    let r = ui.button("1:1");
    if r.clicked() {
        v.zoom = 1.0;
        v.pan = Vec2::ZERO;
    }
    elements.push(("viewer.oneToOne", r.rect, "1:1"));
    ui.colored_label(theme::TEXT_DIM, format!("{:.0}%", v.zoom * 100.0));
    let viewed = app.viewed.and_then(|id| app.session.script.path_of(id).ok()).unwrap_or_else(|| "—".to_string());
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if let Some((_, f)) = &app.viewer.shown {
            let b = f.info.bbox;
            ui.colored_label(theme::TEXT_DIM, format!("{}×{}   bbox {} {} {} {}", f.info.format.width, f.info.format.height, b.x0, b.y0, b.x1, b.y1));
        }
        ui.label(format!("viewing {viewed}"));
    });
    for (id, rect, label) in elements {
        app.automation.add(id, rect, label);
    }
}

/// Where the format sits on screen.
fn image_rect(app: &NukecraftApp, area: Rect, format: (u32, u32, f64)) -> Rect {
    let v = &app.viewer;
    let size = Vec2::new(format.0 as f32 * format.2 as f32, format.1 as f32) * v.zoom;
    Rect::from_center_size(area.center() + v.pan, size)
}

fn fit(app: &mut NukecraftApp, area: Rect, format: (u32, u32, f64)) {
    let w = format.0 as f32 * format.2 as f32;
    let h = format.1 as f32;
    let v = &mut app.viewer;
    if w > 0.0 && h > 0.0 {
        v.zoom = ((area.width() - 24.0) / w).min((area.height() - 24.0) / h).clamp(0.01, 64.0);
    }
    v.pan = Vec2::ZERO;
    v.fit_request = false;
}

fn image_ui(app: &mut NukecraftApp, ui: &mut egui::Ui, area: Rect) {
    let resp = ui.interact(area, egui::Id::new("viewer_image"), Sense::click_and_drag());
    app.automation.add("viewer.image", area, "Viewer");
    let painter = ui.painter_at(area);
    let format = app.viewer.shown.as_ref().map(|(_, f)| (f.info.format.width, f.info.format.height, f.info.format.pixel_aspect));
    if let Some(fmt) = format
        && (app.viewer.fit_request || app.viewer.fitted_format != Some((fmt.0, fmt.1)))
    {
        fit(app, area, fmt);
        app.viewer.fitted_format = Some((fmt.0, fmt.1));
    }

    // Handles take the pointer first; elsewhere a drag pans.
    let mapping = format.map(|fmt| crate::handles::Mapping { rect: image_rect(app, area, fmt), size: (fmt.0 as f32, fmt.1 as f32) });
    let on_handle = mapping.is_some_and(|m| crate::handles::interact(app, ui, &resp, m));

    // Navigation.
    if let Some(p) = resp.hover_pos() {
        let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        let factor = (scroll * 0.002).exp() * pinch;
        if (factor - 1.0).abs() > 1e-4 {
            let v = &mut app.viewer;
            let new_zoom = (v.zoom * factor).clamp(0.01, 64.0);
            let local = p - area.center();
            v.pan = local - (local - v.pan) * (new_zoom / v.zoom);
            v.zoom = new_zoom;
        }
    }
    if resp.dragged() && !on_handle {
        app.viewer.pan += resp.drag_delta();
    }
    if resp.hovered() && !ui.ctx().text_edit_focused() {
        viewer_keys(app, ui);
    }

    let Some((key, frame)) = app.viewer.shown.clone() else {
        let msg = if app.viewed.is_some() { "rendering…" } else { "Select a node and press 1 to view it" };
        painter.text(area.center(), Align2::CENTER_CENTER, msg, FontId::proportional(14.0), theme::TEXT_DIM);
        error_overlay(app, &painter, area);
        return;
    };
    let display = Display { channel: app.viewer.channel, exposure: app.viewer.exposure, gamma: app.viewer.gamma };
    if app.viewer.texture_key != Some((key, display)) {
        let image = display_image(&frame, display);
        let options = egui::TextureOptions { magnification: egui::TextureFilter::Nearest, minification: egui::TextureFilter::Linear, ..Default::default() };
        match &mut app.viewer.texture {
            Some(t) => t.set(image, options),
            None => app.viewer.texture = Some(ui.ctx().load_texture("viewer", image, options)),
        }
        app.viewer.texture_key = Some((key, display));
    }
    let fmt = (frame.info.format.width, frame.info.format.height, frame.info.format.pixel_aspect);
    let img_rect = image_rect(app, area, fmt);
    if let Some(t) = &app.viewer.texture {
        painter.image(t.id(), img_rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }
    painter.rect_stroke(img_rect, 0.0, Stroke::new(1.0, theme::FORMAT_LINE), StrokeKind::Outside);
    painter.text(
        img_rect.right_top() + Vec2::new(0.0, -3.0),
        Align2::RIGHT_BOTTOM,
        format!("{}×{}", fmt.0, fmt.1),
        FontId::proportional(11.0),
        theme::TEXT_DIM,
    );
    let b = frame.info.bbox;
    if b != frame.info.format.rect() && !b.is_empty() {
        let to_screen = |x: i32, y: i32| {
            Pos2::new(
                img_rect.left() + x as f32 * img_rect.width() / fmt.0.max(1) as f32,
                img_rect.bottom() - y as f32 * img_rect.height() / fmt.1.max(1) as f32,
            )
        };
        let r = Rect::from_two_pos(to_screen(b.x0, b.y0), to_screen(b.x1, b.y1));
        let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
        painter.extend(Shape::dashed_line(&pts, Stroke::new(1.0, theme::BBOX_LINE), 6.0, 4.0));
    }

    let map = crate::handles::Mapping { rect: img_rect, size: (fmt.0 as f32, fmt.1 as f32) };
    crate::handles::draw(app, &painter, map, resp.hover_pos());

    // Pixel readout under the pointer.
    app.viewer.readout = resp.hover_pos().map(|p| {
        let x = ((p.x - img_rect.left()) / img_rect.width() * fmt.0 as f32).floor() as i32;
        let y = ((img_rect.bottom() - p.y) / img_rect.height() * fmt.1 as f32).floor() as i32;
        let values: Vec<String> =
            frame.image.channels().iter().enumerate().map(|(i, c)| format!("{} {:.4}", short_name(c), frame.image.get_or_zero_at(i, x, y))).collect();
        format!("x {x}  y {y}    {}", values.join("  "))
    });
    error_overlay(app, &painter, area);
}

fn error_overlay(app: &NukecraftApp, painter: &egui::Painter, area: Rect) {
    let current = app.viewed.map(|node| FrameKey { revision: app.session.revision, node, time: app.session.time });
    if let Some((k, e)) = &app.viewer.error
        && Some(*k) == current
    {
        painter.text(area.center_top() + Vec2::new(0.0, 24.0), Align2::CENTER_TOP, e, FontId::proportional(13.0), theme::ERROR);
    }
}

fn short_name(c: &Channel) -> &str {
    match c.name() {
        "rgba.red" => "r",
        "rgba.green" => "g",
        "rgba.blue" => "b",
        "rgba.alpha" => "a",
        other => other,
    }
}

fn viewer_keys(app: &mut NukecraftApp, ui: &egui::Ui) {
    let v = &mut app.viewer;
    let pressed = |key: Key| ui.input_mut(|i| i.consume_key(Modifiers::NONE, key));
    if pressed(Key::F) {
        v.fit_request = true;
    }
    for (key, mode) in
        [(Key::R, ChannelMode::Red), (Key::G, ChannelMode::Green), (Key::B, ChannelMode::Blue), (Key::A, ChannelMode::Alpha), (Key::Y, ChannelMode::Luminance)]
    {
        if pressed(key) {
            v.channel = if v.channel == mode { ChannelMode::Rgb } else { mode };
        }
    }
}

/// The frame as display pixels: channel selection, exposure, gamma, sRGB; rows top-down.
fn display_image(frame: &Frame, d: Display) -> ColorImage {
    let (w, h) = (frame.info.format.width as usize, frame.info.format.height as usize);
    let img: &Image = &frame.image;
    let idx = |c: Channel| img.channels().index_of(&c);
    let (r, g, b, a) = (idx(Channel::red()), idx(Channel::green()), idx(Channel::blue()), idx(Channel::alpha()));
    let gain = 2f32.powf(d.exposure);
    let inv_gamma = if d.gamma > 0.0 { 1.0 / d.gamma } else { 1.0 };
    let tone = |v: f32| {
        let v = v * gain;
        let v = if (inv_gamma - 1.0).abs() > 1e-6 && v > 0.0 { v.powf(inv_gamma) } else { v };
        let e = linear_to_srgb(v);
        if e.is_nan() { 0 } else { (e.clamp(0.0, 1.0) * 255.0 + 0.5) as u8 }
    };
    let bbox: IRect = img.bbox();
    let mut px = vec![Color32::BLACK; w.saturating_mul(h)];
    for row in 0..h {
        let y = (h - 1 - row) as i32;
        if y < bbox.y0 || y >= bbox.y1 {
            continue;
        }
        for x in bbox.x0.max(0)..bbox.x1.min(w as i32) {
            let get = |i: Option<usize>| i.map_or(0.0, |i| img.get_or_zero_at(i, x, y));
            let (cr, cg, cb) = (get(r), get(g), get(b));
            let rgb = match d.channel {
                ChannelMode::Rgb => [tone(cr), tone(cg), tone(cb)],
                ChannelMode::Red => [tone(cr); 3],
                ChannelMode::Green => [tone(cg); 3],
                ChannelMode::Blue => [tone(cb); 3],
                ChannelMode::Alpha => [tone(get(a)); 3],
                ChannelMode::Luminance => [tone(luminance(cr, cg, cb)); 3],
            };
            if let Some(p) = px.get_mut(row * w + x as usize) {
                *p = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
            }
        }
    }
    ColorImage::new([w, h], px)
}

fn info_ui(app: &mut NukecraftApp, ui: &mut egui::Ui, rect: Rect) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme::PANEL_DARK);
    if let Some(text) = &app.viewer.readout {
        painter.text(rect.left_center() + Vec2::new(8.0, 0.0), Align2::LEFT_CENTER, text, FontId::monospace(11.0), theme::TEXT);
    }
    let status =
        if app.viewer.busy() { "rendering…".to_string() } else { app.viewer.shown.as_ref().map(|(_, f)| format!("{:.0} ms", f.millis)).unwrap_or_default() };
    painter.text(rect.right_center() - Vec2::new(8.0, 0.0), Align2::RIGHT_CENTER, status, FontId::proportional(11.0), theme::TEXT_DIM);
}

fn timeline_ui(app: &mut NukecraftApp, ui: &mut egui::Ui, rect: Rect) {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(6.0, 3.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let ui = &mut child;
    let range = app.session.script.root.frames;
    let mut frame = app.session.time.floor_frame();
    let mut elements = Vec::new();
    let mut goto = None;
    let r = ui.button("⏮").on_hover_text("First frame");
    elements.push(("viewer.first", r.rect, "first"));
    if r.clicked() {
        goto = Some(range.first);
    }
    let r = ui.button("◀").on_hover_text("Previous frame (Left)");
    elements.push(("viewer.prev", r.rect, "previous"));
    if r.clicked() {
        goto = Some(frame - 1);
    }
    let r = ui.button(if app.viewer.playing { "⏸" } else { "▶" }).on_hover_text("Play / stop (L / K)");
    elements.push(("viewer.play", r.rect, "play"));
    if r.clicked() {
        app.viewer.playing = !app.viewer.playing;
    }
    let r = ui.button("▶|").on_hover_text("Next frame (Right)");
    elements.push(("viewer.next", r.rect, "next"));
    if r.clicked() {
        goto = Some(frame + 1);
    }
    let r = ui.button("⏭").on_hover_text("Last frame");
    elements.push(("viewer.last", r.rect, "last"));
    if r.clicked() {
        goto = Some(range.last);
    }
    let r = ui.add(egui::DragValue::new(&mut frame).speed(0.2));
    elements.push(("viewer.frame", r.rect, "frame"));
    if r.changed() {
        goto = Some(frame);
    }
    ui.spacing_mut().slider_width = (ui.available_width() - 70.0).max(60.0);
    let mut slider_frame = frame.clamp(range.first, range.last);
    let r = ui.add(egui::Slider::new(&mut slider_frame, range.first..=range.last).show_value(false));
    elements.push(("viewer.slider", r.rect, "timeline"));
    if r.changed() {
        goto = Some(slider_frame);
    }
    ui.colored_label(theme::TEXT_DIM, format!("{}–{}", range.first, range.last));
    for (id, rect, label) in elements {
        app.automation.add(id, rect, label);
    }
    if let Some(f) = goto {
        app.run("time.goto", json!({ "frame": f }));
    }
}

/// Advances the frame while playing, at the script's frame rate or as fast as frames render.
fn playback(app: &mut NukecraftApp, ui: &egui::Ui) {
    let typing = ui.ctx().text_edit_focused();
    // Keys go to the panel under the pointer: over the Node Graph, K is Copy, not stop.
    let over_graph = ui.input(|i| i.pointer.hover_pos()).zip(app.graph.rect).is_some_and(|(p, r)| r.contains(p));
    let range = app.session.script.root.frames;
    let frame = app.session.time.floor_frame();
    if !typing && !over_graph {
        let (l, k, left, right) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::L),
                i.consume_key(Modifiers::NONE, Key::K),
                i.consume_key(Modifiers::NONE, Key::ArrowLeft),
                i.consume_key(Modifiers::NONE, Key::ArrowRight),
            )
        });
        if l {
            app.viewer.playing = true;
        }
        if k {
            app.viewer.playing = false;
        }
        if left {
            app.run("time.goto", json!({ "frame": frame - 1 }));
        }
        if right {
            app.run("time.goto", json!({ "frame": frame + 1 }));
        }
    }
    if !app.viewer.playing {
        return;
    }
    let now = ui.input(|i| i.time);
    let period = 1.0 / app.session.script.root.fps.as_f64().max(1.0);
    let current_ready = app.viewer.shown.as_ref().is_some_and(|(k, _)| k.time == app.session.time);
    if current_ready && now - app.viewer.last_step >= period {
        app.viewer.last_step = now;
        let next = if frame >= range.last || frame < range.first { range.first } else { frame + 1 };
        app.run("time.goto", json!({ "frame": next }));
    }
    ui.ctx().request_repaint();
}
