//! Headless runs of the real UI, and a software rasteriser for what it draws.
//!
//! [`Headless`] drives [`NukecraftApp`] frame by frame without a window or a GPU: it feeds
//! synthetic input (clicks, drags, keys, by screen position or automation id), keeps egui's
//! textures, and [`Headless::snapshot`] rasterises the last frame's triangles on the CPU into
//! RGBA pixels. Agents use it (through `examples/snapshot.rs`) to look at the UI, and tests use
//! it to check what the UI shows.
//!
//! The rasteriser follows egui's rendering contract: vertex colours and textures are
//! premultiplied sRGB, blended in gamma space with `src + dst·(1 − src.a)`.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use egui::epaint::{ClippedPrimitive, ColorImage, ImageData, Primitive, TextureId, Vertex};
use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};

use crate::NukecraftApp;

/// The longest side a snapshot may have, in pixels.
const MAX_SIDE: usize = 8192;

/// An RGBA8 image (rows top-down, straight alpha) produced by [`Headless::snapshot`].
pub struct Snapshot {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

/// The app running without a window.
pub struct Headless {
    pub app: NukecraftApp,
    ctx: egui::Context,
    size: Vec2,
    pixels_per_point: f32,
    time: f64,
    pending: Vec<Event>,
    pointer: Pos2,
    textures: HashMap<TextureId, ColorImage>,
    primitives: Vec<ClippedPrimitive>,
}

impl Headless {
    /// A headless app of `size` points at `pixels_per_point`.
    pub fn new(session: nukecraft_engine::Session, size: Vec2, pixels_per_point: f32) -> Headless {
        let ctx = egui::Context::default();
        let app = NukecraftApp::new(&ctx, session);
        Headless {
            app,
            ctx,
            size,
            pixels_per_point: pixels_per_point.clamp(0.5, 4.0),
            time: 0.0,
            pending: Vec::new(),
            pointer: Pos2::ZERO,
            textures: HashMap::new(),
            primitives: Vec::new(),
        }
    }

    pub fn ctx(&self) -> &egui::Context {
        &self.ctx
    }

    /// Runs one frame with the queued input.
    pub fn frame(&mut self) {
        let mut input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
            time: Some(self.time),
            predicted_dt: 1.0 / 60.0,
            focused: true,
            ..Default::default()
        };
        if let Some(vp) = input.viewports.get_mut(&input.viewport_id) {
            vp.native_pixels_per_point = Some(self.pixels_per_point);
        }
        input.events = std::mem::take(&mut self.pending);
        input.events.extend(self.app.take_injected());
        let app = &mut self.app;
        let mut output = self.ctx.run_ui(input, |ui| app.ui(ui));
        self.time += 1.0 / 60.0;
        let set = std::mem::take(&mut output.textures_delta.set);
        let free = std::mem::take(&mut output.textures_delta.free);
        for (id, deltas) in set {
            for delta in deltas {
                let ImageData::Color(image) = delta.image;
                match delta.pos {
                    None => {
                        self.textures.insert(id, (*image).clone());
                    }
                    Some([x, y]) => {
                        if let Some(tex) = self.textures.get_mut(&id) {
                            patch(tex, &image, x, y);
                        }
                    }
                }
            }
        }
        let shapes = std::mem::take(&mut output.shapes);
        self.primitives = self.ctx.tessellate(shapes, output.pixels_per_point);
        for id in free {
            self.textures.remove(&id);
        }
    }

    /// Runs `n` frames.
    pub fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame();
        }
    }

    /// Runs frames until the app has nothing in flight (renders done) or `timeout` passes.
    pub fn settle(&mut self, timeout: Duration) {
        let start = Instant::now();
        self.frames(3);
        while self.app.busy() && start.elapsed() < timeout {
            std::thread::sleep(Duration::from_millis(5));
            self.frame();
        }
        self.frames(2);
        // Separate user actions are not one gesture: let egui's multi-click window lapse.
        self.time += 0.5;
    }

    /// Moves the pointer.
    pub fn hover(&mut self, pos: Pos2) {
        self.pointer = pos;
        self.pending.push(Event::PointerMoved(pos));
        self.frame();
    }

    /// A primary click at `pos` (`clicks` = 2 for a double click).
    pub fn click_at(&mut self, pos: Pos2, clicks: usize, modifiers: Modifiers) {
        // A new gesture: earlier clicks must not merge into a double or triple click.
        self.time += 0.5;
        self.hover(pos);
        for _ in 0..clicks.max(1) {
            self.pending.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers });
            self.frame();
            self.pending.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers });
            self.frame();
        }
        // Panels drawn before the one that handled the click show its effect a frame later.
        self.frame();
    }

    /// Clicks the centre of an automation element.
    pub fn click(&mut self, id: &str, clicks: usize, modifiers: Modifiers) -> Result<(), String> {
        // Layouts (grids, panels that just opened) settle over a frame or two.
        self.frames(2);
        let pos = self.app.automation.center(id).ok_or_else(|| format!("no UI element `{id}`"))?;
        self.click_at(pos, clicks, modifiers);
        Ok(())
    }

    /// A primary-button drag from `from` to `to` in `steps` moves.
    pub fn drag_at(&mut self, from: Pos2, to: Pos2, button: PointerButton, modifiers: Modifiers) {
        self.time += 0.5;
        self.hover(from);
        self.pending.push(Event::PointerButton { pos: from, button, pressed: true, modifiers });
        self.frame();
        let steps = 8;
        for i in 1..=steps {
            let p = from + (to - from) * (i as f32 / steps as f32);
            self.pending.push(Event::PointerMoved(p));
            self.frame();
        }
        self.pending.push(Event::PointerButton { pos: to, button, pressed: false, modifiers });
        self.frame();
        self.frame();
        self.pointer = to;
    }

    /// Drags from one automation element's centre to another's.
    pub fn drag(&mut self, from: &str, to: &str) -> Result<(), String> {
        self.frames(2);
        let a = self.app.automation.center(from).ok_or_else(|| format!("no UI element `{from}`"))?;
        let b = self.app.automation.center(to).ok_or_else(|| format!("no UI element `{to}`"))?;
        self.drag_at(a, b, PointerButton::Primary, Modifiers::NONE);
        Ok(())
    }

    /// Presses and releases a key at the current pointer position.
    pub fn key(&mut self, key: Key, modifiers: Modifiers) {
        self.pending.push(Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers });
        self.frame();
        self.pending.push(Event::Key { key, physical_key: Some(key), pressed: false, repeat: false, modifiers });
        self.frame();
        self.frame();
    }

    /// Types text into the focused widget.
    pub fn text(&mut self, text: &str) {
        self.pending.push(Event::Text(text.to_string()));
        self.frame();
    }

    /// Scrolls (mouse wheel, in points) at the current pointer position.
    pub fn scroll(&mut self, delta: Vec2) {
        self.pending.push(Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta, phase: egui::TouchPhase::Move, modifiers: Modifiers::NONE });
        self.frame();
    }

    /// The last frame, rasterised.
    pub fn snapshot(&self) -> Snapshot {
        rasterize(&self.primitives, &self.textures, self.size, self.pixels_per_point)
    }
}

/// Copies `src` into `dst` at `(x, y)`, clipped.
fn patch(dst: &mut ColorImage, src: &ColorImage, x: usize, y: usize) {
    let [dw, dh] = dst.size;
    let [sw, sh] = src.size;
    for row in 0..sh {
        let ty = y + row;
        if ty >= dh {
            break;
        }
        for col in 0..sw {
            let tx = x + col;
            if tx >= dw {
                break;
            }
            if let (Some(d), Some(s)) = (dst.pixels.get_mut(ty * dw + tx), src.pixels.get(row * sw + col)) {
                *d = *s;
            }
        }
    }
}

/// Rasterises tessellated egui output into an opaque RGBA8 image.
pub fn rasterize(primitives: &[ClippedPrimitive], textures: &HashMap<TextureId, ColorImage>, size: Vec2, ppp: f32) -> Snapshot {
    let w = ((size.x * ppp).round().max(1.0) as usize).min(MAX_SIDE);
    let h = ((size.y * ppp).round().max(1.0) as usize).min(MAX_SIDE);
    // Premultiplied RGBA in 0‥1, gamma space.
    let mut canvas = vec![[0.0f32, 0.0, 0.0, 1.0]; w * h];
    for prim in primitives {
        let Primitive::Mesh(mesh) = &prim.primitive else { continue };
        let tex = textures.get(&mesh.texture_id);
        let clip = Rect::from_min_max(prim.clip_rect.min * ppp, prim.clip_rect.max * ppp);
        let cx0 = clip.min.x.floor().max(0.0) as usize;
        let cy0 = clip.min.y.floor().max(0.0) as usize;
        let cx1 = (clip.max.x.ceil().max(0.0) as usize).min(w);
        let cy1 = (clip.max.y.ceil().max(0.0) as usize).min(h);
        if cx0 >= cx1 || cy0 >= cy1 {
            continue;
        }
        for tri in mesh.indices.chunks_exact(3) {
            let get = |i: u32| mesh.vertices.get(i as usize);
            let (Some(a), Some(b), Some(c)) = (get(tri[0]), get(tri[1]), get(tri[2])) else { continue };
            draw_triangle(&mut canvas, w, [cx0, cy0, cx1, cy1], [a, b, c], ppp, tex);
        }
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    for px in canvas {
        for c in &px[..3] {
            rgba.push((c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
        }
        rgba.push(255);
    }
    Snapshot { width: w, height: h, rgba }
}

fn edge(a: Pos2, b: Pos2, p: Pos2) -> f32 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}

fn draw_triangle(canvas: &mut [[f32; 4]], w: usize, clip: [usize; 4], v: [&Vertex; 3], ppp: f32, tex: Option<&ColorImage>) {
    let p = [v[0].pos * ppp, v[1].pos * ppp, v[2].pos * ppp];
    if p.iter().any(|q| !q.x.is_finite() || !q.y.is_finite()) {
        return;
    }
    let area = edge(p[0], p[1], p[2]);
    if area.abs() < 1e-6 {
        return;
    }
    let min_x = p.iter().map(|q| q.x).fold(f32::INFINITY, f32::min).floor().max(clip[0] as f32) as usize;
    let max_x = (p.iter().map(|q| q.x).fold(f32::NEG_INFINITY, f32::max).ceil().max(0.0) as usize).min(clip[2]);
    let min_y = p.iter().map(|q| q.y).fold(f32::INFINITY, f32::min).floor().max(clip[1] as f32) as usize;
    let max_y = (p.iter().map(|q| q.y).fold(f32::NEG_INFINITY, f32::max).ceil().max(0.0) as usize).min(clip[3]);
    let col = |c: egui::Color32| [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, c.a() as f32 / 255.0];
    let cols = [col(v[0].color), col(v[1].color), col(v[2].color)];
    for y in min_y..max_y {
        for x in min_x..max_x {
            let q = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
            let w0 = edge(p[1], p[2], q) / area;
            let w1 = edge(p[2], p[0], q) / area;
            let w2 = 1.0 - w0 - w1;
            // Half-open coverage: pixel centres exactly on a shared edge go to one triangle.
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 || (w0 == 0.0 && w1 == 0.0) {
                continue;
            }
            let mut src = [0.0f32; 4];
            for (k, s) in src.iter_mut().enumerate() {
                *s = cols[0][k] * w0 + cols[1][k] * w1 + cols[2][k] * w2;
            }
            if let Some(t) = tex {
                let u = v[0].uv.x * w0 + v[1].uv.x * w1 + v[2].uv.x * w2;
                let vv = v[0].uv.y * w0 + v[1].uv.y * w1 + v[2].uv.y * w2;
                let texel = sample(t, u, vv);
                for k in 0..4 {
                    src[k] *= texel[k];
                }
            }
            if let Some(dst) = canvas.get_mut(y * w + x) {
                let inv = 1.0 - src[3];
                for k in 0..4 {
                    dst[k] = src[k] + dst[k] * inv;
                }
            }
        }
    }
}

/// Bilinear texture sample (premultiplied, 0‥1), clamped at the edges.
fn sample(t: &ColorImage, u: f32, v: f32) -> [f32; 4] {
    let [tw, th] = t.size;
    if tw == 0 || th == 0 || !u.is_finite() || !v.is_finite() {
        return [1.0; 4];
    }
    let fx = (u * tw as f32 - 0.5).clamp(0.0, (tw - 1) as f32);
    let fy = (v * th as f32 - 0.5).clamp(0.0, (th - 1) as f32);
    let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(tw - 1), (y0 + 1).min(th - 1));
    let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
    let px = |x: usize, y: usize| {
        t.pixels.get(y * tw + x).map_or([0.0; 4], |c| [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, c.a() as f32 / 255.0])
    };
    let (a, b, c, d) = (px(x0, y0), px(x1, y0), px(x0, y1), px(x1, y1));
    let mut out = [0.0; 4];
    for k in 0..4 {
        let top = a[k] * (1.0 - tx) + b[k] * tx;
        let bottom = c[k] * (1.0 - tx) + d[k] * tx;
        out[k] = top * (1.0 - ty) + bottom * ty;
    }
    out
}

impl Snapshot {
    /// Encodes the snapshot as PNG.
    pub fn png(&self) -> Result<Vec<u8>, String> {
        nukecraft_io::encode_png_rgba8(self.width as u32, self.height as u32, &self.rgba).map_err(|e| e.to_string())
    }

    /// The colour at a point (in pixels).
    pub fn pixel(&self, x: usize, y: usize) -> Option<[u8; 4]> {
        let i = (y.checked_mul(self.width)?.checked_add(x)?).checked_mul(4)?;
        let px = self.rgba.get(i..i + 4)?;
        Some([px[0], px[1], px[2], px[3]])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::epaint::{Mesh, TextureId};
    use egui::{Color32, Rect};

    #[test]
    fn rasterizes_a_quad_with_blending() {
        let mut mesh = Mesh::with_texture(TextureId::default());
        mesh.add_colored_rect(Rect::from_min_max(Pos2::new(2.0, 2.0), Pos2::new(6.0, 6.0)), Color32::from_rgb(255, 0, 0));
        let mut half = Mesh::with_texture(TextureId::default());
        half.add_colored_rect(Rect::from_min_max(Pos2::new(4.0, 4.0), Pos2::new(8.0, 8.0)), Color32::from_rgba_premultiplied(0, 0, 128, 128));
        let full = Rect::from_min_max(Pos2::ZERO, Pos2::new(10.0, 10.0));
        let prims = vec![
            ClippedPrimitive { clip_rect: full, primitive: Primitive::Mesh(mesh) },
            ClippedPrimitive { clip_rect: full, primitive: Primitive::Mesh(half) },
        ];
        let white = ColorImage::new([1, 1], vec![Color32::WHITE]);
        let textures = HashMap::from([(TextureId::default(), white)]);
        let snap = rasterize(&prims, &textures, Vec2::new(10.0, 10.0), 1.0);
        assert_eq!(snap.pixel(3, 3), Some([255, 0, 0, 255]));
        assert_eq!(snap.pixel(0, 0), Some([0, 0, 0, 255]));
        // Red under half-transparent blue: 255·(1−½) red, ½ blue.
        let p = snap.pixel(5, 5).unwrap();
        assert!((126..=129).contains(&p[0]) && (126..=129).contains(&p[2]), "{p:?}");
        // No double blending on the diagonal shared by the quad's two triangles.
        assert_eq!(snap.pixel(7, 7).map(|p| p[2]), snap.pixel(6, 4).map(|p| p[2]));
    }
}
