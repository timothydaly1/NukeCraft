//! The control channel: JSON lines over TCP on `127.0.0.1`, so agents and scripts can drive the
//! running app (`nukecraft --control PORT`). See `docs/control-protocol.md`.
//!
//! Each request is one line, `{"id": …, "method": "…", "params": {…}}`; each reply is one line,
//! `{"id": …, "result": …}` or `{"id": …, "error": "…"}`. Connections are served on their own
//! threads; requests are handled on the UI thread at the start of the next frame, so they see and
//! change exactly what the user sees.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use egui::{Event, Key, Modifiers, PointerButton};
use serde_json::{Value, json};

use crate::NukecraftApp;

/// The longest request line, in bytes.
const MAX_LINE: u64 = 16 * 1024 * 1024;

/// How long a connection waits for the UI to answer.
const REPLY_TIMEOUT: Duration = Duration::from_secs(120);

pub struct Request {
    pub body: Value,
    pub reply: Sender<Value>,
}

pub struct ControlServer {
    rx: Receiver<Request>,
    pub port: u16,
}

impl ControlServer {
    /// Listens on `127.0.0.1:port` (0 picks a free port).
    pub fn start(port: u16, ctx: egui::Context) -> std::io::Result<ControlServer> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let port = listener.local_addr()?.port();
        let (tx, rx) = channel::<Request>();
        std::thread::Builder::new().name("control".into()).spawn(move || {
            for stream in listener.incoming().flatten() {
                let tx = tx.clone();
                let ctx = ctx.clone();
                let _ = std::thread::Builder::new().name("control-conn".into()).spawn(move || serve(stream, tx, ctx));
            }
        })?;
        log::info!("control channel on 127.0.0.1:{port}");
        Ok(ControlServer { rx, port })
    }

    pub fn drain(&self) -> Vec<Request> {
        self.rx.try_iter().collect()
    }
}

fn serve(stream: TcpStream, tx: Sender<Request>, ctx: egui::Context) {
    let Ok(write_half) = stream.try_clone() else { return };
    let mut out = write_half;
    let mut reader = BufReader::new(stream);
    loop {
        let mut line = String::new();
        match Read::by_ref(&mut reader).take(MAX_LINE).read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Err(e) => json!({ "id": Value::Null, "error": format!("parse error: {e}") }),
            Ok(body) => {
                let id = body.get("id").cloned().unwrap_or(Value::Null);
                let (rtx, rrx) = channel();
                if tx.send(Request { body, reply: rtx }).is_err() {
                    return;
                }
                ctx.request_repaint();
                match rrx.recv_timeout(REPLY_TIMEOUT) {
                    Ok(v) => v,
                    Err(_) => json!({ "id": id, "error": "the app did not answer in time" }),
                }
            }
        };
        if writeln!(out, "{reply}").and_then(|_| out.flush()).is_err() {
            return;
        }
    }
}

/// Modifiers named in a request (`["shift", "cmd"]`).
fn modifiers(p: &Value) -> Modifiers {
    let mut m = Modifiers::NONE;
    for name in p["modifiers"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        match name {
            "shift" => m |= Modifiers::SHIFT,
            "alt" => m |= Modifiers::ALT,
            "cmd" | "command" | "ctrl" => m |= Modifiers::COMMAND,
            _ => {}
        }
    }
    m
}

impl NukecraftApp {
    /// Starts the control channel; returns the port it listens on.
    pub fn start_control(&mut self, ctx: &egui::Context, port: u16) -> Result<u16, String> {
        let server = ControlServer::start(port, ctx.clone()).map_err(|e| format!("control channel on port {port}: {e}"))?;
        let p = server.port;
        self.control = Some(server);
        Ok(p)
    }

    /// Input queued by the control channel, one frame's worth (for eframe's raw input hook).
    pub fn take_injected(&mut self) -> Vec<Event> {
        if self.injected.is_empty() { Vec::new() } else { self.injected.remove(0) }
    }

    /// Handles control requests that arrived since the last frame.
    pub(crate) fn handle_control(&mut self, ctx: &egui::Context) {
        let Some(server) = &self.control else { return };
        for req in server.drain() {
            let id = req.body.get("id").cloned().unwrap_or(Value::Null);
            let method = req.body.get("method").and_then(Value::as_str).unwrap_or("").to_string();
            let params = req.body.get("params").cloned().unwrap_or_else(|| json!({}));
            if method == "ui.screenshot" {
                match params["path"].as_str() {
                    Some(path) => {
                        self.screenshots.push((path.to_string(), id, req.reply));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                    }
                    None => {
                        let _ = req.reply.send(json!({ "id": id, "error": "`path` is required" }));
                    }
                }
                continue;
            }
            let reply = match self.control_request(&method, &params) {
                Ok(v) => json!({ "id": id, "result": v }),
                Err(e) => json!({ "id": id, "error": e }),
            };
            let _ = req.reply.send(reply);
        }
        if !self.injected.is_empty() {
            ctx.request_repaint();
        }
        self.finish_screenshots(ctx);
    }

    fn control_request(&mut self, method: &str, p: &Value) -> Result<Value, String> {
        match method {
            "engine.execute" => {
                let command = p["command"].as_str().ok_or("`command` is required")?;
                let params = p.get("params").cloned().unwrap_or_else(|| json!({}));
                self.session.execute(command, &params).map_err(|e| e.to_string())
            }
            "app.state" => Ok(json!({
                "viewed": self.viewed.and_then(|v| self.session.script.name_of(v).ok()),
                "selection": self.selected_names(),
                "frame": self.session.time.as_f64(),
                "revision": self.session.revision,
                "dirty": self.session.dirty,
                "path": self.session.path.as_ref().map(|p| p.display().to_string()),
                "busy": self.busy(),
                "viewerError": self.viewer_error(),
            })),
            "app.view" => {
                let node = p["node"].as_str().ok_or("`node` is required")?;
                self.viewed = Some(self.session.script.id_of(node).map_err(|e| e.to_string())?);
                Ok(json!({}))
            }
            "ui.inspect" => {
                let prefix = p["prefix"].as_str().unwrap_or("");
                Ok(json!(
                    self.automation
                        .query(prefix)
                        .iter()
                        .map(|e| json!({ "id": e.id, "label": e.label, "rect": [e.rect.min.x, e.rect.min.y, e.rect.width(), e.rect.height()] }))
                        .collect::<Vec<_>>()
                ))
            }
            "ui.click" => {
                let target = p["id"].as_str().ok_or("`id` is required")?;
                let pos = self.automation.center(target).ok_or_else(|| format!("no UI element `{target}` (ui.inspect lists them)"))?;
                let m = modifiers(p);
                self.injected.push(vec![Event::PointerMoved(pos)]);
                for _ in 0..p["clicks"].as_u64().unwrap_or(1).clamp(1, 3) {
                    self.injected.push(vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: m }]);
                    self.injected.push(vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: m }]);
                }
                Ok(json!({}))
            }
            "ui.key" => {
                let key = p["key"].as_str().and_then(Key::from_name).ok_or("`key` is missing or unknown")?;
                let m = modifiers(p);
                self.injected.push(vec![Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: m }]);
                self.injected.push(vec![Event::Key { key, physical_key: Some(key), pressed: false, repeat: false, modifiers: m }]);
                Ok(json!({}))
            }
            "ui.text" => {
                let text = p["text"].as_str().ok_or("`text` is required")?;
                self.injected.push(vec![Event::Text(text.to_string())]);
                Ok(json!({}))
            }
            other => Err(format!("unknown method `{other}` (engine.execute, app.state, app.view, ui.inspect, ui.click, ui.key, ui.text, ui.screenshot)")),
        }
    }

    /// Saves screenshots that eframe delivered this frame and answers their requests.
    fn finish_screenshots(&mut self, ctx: &egui::Context) {
        if self.screenshots.is_empty() {
            return;
        }
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        let Some(image) = image else { return };
        let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
        let png = nukecraft_io::encode_png_rgba8(image.size[0] as u32, image.size[1] as u32, &rgba);
        for (path, id, reply) in self.screenshots.drain(..) {
            let result = match &png {
                Ok(bytes) => {
                    std::fs::write(&path, bytes).map(|_| json!({ "id": id, "result": { "path": path, "width": image.size[0], "height": image.size[1] } }))
                }
                Err(e) => Ok(json!({ "id": id, "error": e.to_string() })),
            };
            let _ = reply.send(result.unwrap_or_else(|e| json!({ "id": Value::Null, "error": e.to_string() })));
        }
    }
}
