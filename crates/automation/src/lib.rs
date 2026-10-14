//! The NukeCraft MCP server: the Model Context Protocol over stdio (JSON-RPC 2.0, one message
//! per line), so agents can build, inspect and render scripts.
//!
//! Tools:
//!
//! - `execute` runs any engine command by id (`node.create`, `node.setKnob`, `render.execute`,
//!   `file.save`…) with JSON parameters: the same registry the UI and the CLI use;
//! - `list_commands`, `node_classes` and `get_script` describe what exists;
//! - `render_image` returns a node's output as a PNG the agent can look at.
//!
//! See `docs/agents.md`.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::io::{BufRead, Write};

use base64::Engine as _;
use nukecraft_color::Colorspace;
use nukecraft_engine::Session;
use nukecraft_render::{Renderer, to_rgba8, with_render_stack};
use nukecraft_time::FrameTime;
use serde_json::{Value, json};

/// The protocol revisions this server speaks, newest last.
const PROTOCOL_VERSIONS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18"];

/// The longest request line accepted, in bytes.
const MAX_LINE: usize = 64 * 1024 * 1024;

/// Default longest side of images returned by `render_image`.
const DEFAULT_MAX_SIZE: u32 = 768;

const INSTRUCTIONS: &str = "NukeCraft is a node-based compositor in the spirit of Nuke. A script is a graph of nodes \
(Read, Constant, Grade, Merge, Transform, Blur, Write…) with knobs. Use `execute` with command ids from `list_commands` \
(node.create, node.connect, node.setKnob, render.execute, file.save…); `node_classes` lists every node's knobs. \
Merge's input 0 is B (background) and input 1 is A (foreground). Coordinates are pixels with y up from the bottom-left. \
Use `render_image` to look at any node's output.";

/// An MCP server over one session.
pub struct Server {
    pub session: Session,
}

impl Server {
    pub fn new(session: Session) -> Server {
        Server { session }
    }

    /// Serves requests from `input` until it closes, writing responses to `output`.
    pub fn serve(&mut self, mut input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
        loop {
            let Some(line) = read_line(&mut input)? else { return Ok(()) };
            let response = match line {
                Line::TooLong => Some(error_response(Value::Null, -32700, &format!("request longer than {MAX_LINE} bytes"))),
                Line::Text(text) if text.trim().is_empty() => None,
                Line::Text(text) => match serde_json::from_str::<Value>(&text) {
                    Ok(msg) => self.handle(&msg),
                    Err(e) => Some(error_response(Value::Null, -32700, &format!("parse error: {e}"))),
                },
            };
            if let Some(r) = response {
                writeln!(output, "{r}")?;
                output.flush()?;
            }
        }
    }

    /// Handles one JSON-RPC message; `None` for notifications.
    pub fn handle(&mut self, msg: &Value) -> Option<Value> {
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let id = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        // Notifications (no id) get no response.
        let id = id?;
        let result = match method {
            "initialize" => Ok(self.initialize(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => self.call_tool(&params),
            "" => Err((-32600, "missing method".to_string())),
            other => Err((-32601, format!("method not found: {other}"))),
        };
        Some(match result {
            Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err((code, message)) => error_response(id, code, &message),
        })
    }

    fn initialize(&self, params: &Value) -> Value {
        let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
        let version = PROTOCOL_VERSIONS.iter().find(|v| **v == asked).or(PROTOCOL_VERSIONS.last()).copied().unwrap_or("2025-06-18");
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "nukecraft", "version": env!("CARGO_PKG_VERSION") },
            "instructions": INSTRUCTIONS,
        })
    }

    fn call_tool(&mut self, params: &Value) -> Result<Value, (i64, String)> {
        let name = params.get("name").and_then(Value::as_str).ok_or((-32602, "tools/call needs `name`".to_string()))?;
        let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
        let outcome = match name {
            "execute" => {
                let Some(command) = args.get("command").and_then(Value::as_str) else {
                    return Ok(tool_error("`command` (a command id such as node.create) is required"));
                };
                let p = args.get("params").cloned().unwrap_or_else(|| json!({}));
                self.session.execute(command, &p).map_err(|e| e.to_string())
            }
            "list_commands" => self.session.execute("query.commands", &json!({})).map_err(|e| e.to_string()),
            "node_classes" => self.session.execute("query.nodeClasses", &json!({})).map_err(|e| e.to_string()),
            "get_script" => self.session.execute("query.script", &json!({})).map_err(|e| e.to_string()),
            "render_image" => return Ok(self.render_image(&args)),
            other => return Err((-32602, format!("unknown tool `{other}`"))),
        };
        Ok(match outcome {
            Ok(v) => json!({ "content": [{ "type": "text", "text": v.to_string() }], "isError": false }),
            Err(e) => tool_error(&e),
        })
    }

    fn render_image(&mut self, args: &Value) -> Value {
        let Some(name) = args.get("node").and_then(Value::as_str) else { return tool_error("`node` is required") };
        let id = match self.session.script.id_of(name) {
            Ok(id) => id,
            Err(e) => return tool_error(&e.to_string()),
        };
        let t = args.get("frame").and_then(Value::as_f64).map_or(self.session.time, FrameTime::from_f64);
        let max = args.get("max_size").and_then(Value::as_u64).map_or(DEFAULT_MAX_SIZE, |m| m.clamp(16, 4096) as u32);
        let (script, registry) = (&self.session.script, &self.session.registry);
        let rendered = match with_render_stack(|| Renderer::new(script, registry).render_format(id, t)) {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => return tool_error(&e.to_string()),
            Err(e) => return tool_error(&e),
        };
        let (info, img) = rendered;
        let (w, h) = (info.format.width, info.format.height);
        let rgba = to_rgba8(&img, &info.format, Colorspace::Srgb);
        let (sw, sh, small) = downsample(&rgba, w, h, max);
        let png = match nukecraft_io::encode_png_rgba8(sw, sh, &small) {
            Ok(p) => p,
            Err(e) => return tool_error(&e.to_string()),
        };
        let caption = format!("{name} at frame {t}: format {w}×{h}, bbox {:?}, channels {}, shown at {sw}×{sh}", info.bbox, info.channels.to_knob_string());
        json!({
            "content": [
                { "type": "image", "data": base64::engine::general_purpose::STANDARD.encode(png), "mimeType": "image/png" },
                { "type": "text", "text": caption },
            ],
            "isError": false,
        })
    }
}

fn tool_error(message: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": message }], "isError": true })
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tools() -> Value {
    json!([
        {
            "name": "execute",
            "description": "Run a NukeCraft command by id with JSON parameters (see list_commands). Edits are undoable with edit.undo.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Command id, e.g. node.create, node.setKnob, node.connect, render.execute, file.save" },
                    "params": { "type": "object", "description": "The command's parameters" }
                },
                "required": ["command"]
            }
        },
        { "name": "list_commands", "description": "Every command with its parameters, menu and shortcut.", "inputSchema": { "type": "object", "properties": {} } },
        { "name": "node_classes", "description": "Every node class with its inputs and knobs (names, kinds, defaults, choices).", "inputSchema": { "type": "object", "properties": {} } },
        { "name": "get_script", "description": "The open script as JSON (.ncs).", "inputSchema": { "type": "object", "properties": {} } },
        {
            "name": "render_image",
            "description": "Render a node's output over its format and return it as a PNG (sRGB, downscaled to max_size).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "node": { "type": "string" },
                    "frame": { "type": "number" },
                    "max_size": { "type": "integer", "description": "Longest side in pixels (default 768)" }
                },
                "required": ["node"]
            }
        }
    ])
}

/// Box-filters RGBA8 so the longest side is at most `max`.
fn downsample(rgba: &[u8], w: u32, h: u32, max: u32) -> (u32, u32, Vec<u8>) {
    let longest = w.max(h);
    if longest <= max || w == 0 || h == 0 {
        return (w, h, rgba.to_vec());
    }
    let f = longest.div_ceil(max).max(1);
    let (sw, sh) = (w.div_ceil(f), h.div_ceil(f));
    let mut out = vec![0u8; sw as usize * sh as usize * 4];
    for sy in 0..sh {
        for sx in 0..sw {
            let mut acc = [0u32; 4];
            let mut n = 0u32;
            for y in sy * f..((sy + 1) * f).min(h) {
                for x in sx * f..((sx + 1) * f).min(w) {
                    let i = (y as usize * w as usize + x as usize) * 4;
                    for (c, a) in acc.iter_mut().enumerate() {
                        *a += rgba.get(i + c).copied().unwrap_or(0) as u32;
                    }
                    n += 1;
                }
            }
            let o = (sy as usize * sw as usize + sx as usize) * 4;
            for (c, a) in acc.iter().enumerate() {
                if let Some(px) = out.get_mut(o + c) {
                    *px = (a / n.max(1)) as u8;
                }
            }
        }
    }
    (sw, sh, out)
}

enum Line {
    Text(String),
    TooLong,
}

/// One line, at most [`MAX_LINE`] bytes (the rest of a longer line is skipped); `None` at EOF.
fn read_line(input: &mut impl BufRead) -> std::io::Result<Option<Line>> {
    let mut buf = Vec::new();
    let mut too_long = false;
    loop {
        let chunk = input.fill_buf()?;
        if chunk.is_empty() {
            return Ok(if buf.is_empty() && !too_long { None } else { Some(finish(buf, too_long)) });
        }
        let (part, consumed, newline) = match chunk.iter().position(|b| *b == b'\n') {
            Some(i) => (chunk.get(..i).unwrap_or(&[]), i + 1, true),
            None => (chunk, chunk.len(), false),
        };
        if !too_long {
            if buf.len() + part.len() > MAX_LINE {
                too_long = true;
                buf.clear();
            } else {
                buf.extend_from_slice(part);
            }
        }
        input.consume(consumed);
        if newline {
            return Ok(Some(finish(buf, too_long)));
        }
    }
}

fn finish(buf: Vec<u8>, too_long: bool) -> Line {
    if too_long { Line::TooLong } else { Line::Text(String::from_utf8_lossy(&buf).into_owned()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(server: &mut Server, id: i64, method: &str, params: Value) -> Value {
        server.handle(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).unwrap()
    }

    #[test]
    fn handshake_and_tools() {
        let mut s = Server::new(Session::new());
        let init = call(&mut s, 1, "initialize", json!({ "protocolVersion": "2025-03-26", "capabilities": {} }));
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert!(s.handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none());
        let tools = call(&mut s, 2, "tools/list", json!({}));
        assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 5);
        assert_eq!(call(&mut s, 3, "nope", json!({}))["error"]["code"], -32601);
    }

    #[test]
    fn execute_and_render() {
        let mut s = Server::new(Session::new());
        let r = call(
            &mut s,
            1,
            "tools/call",
            json!({ "name": "execute", "arguments": { "command": "node.create", "params": { "class": "Constant", "knobs": { "color": [1, 0, 0, 1] } } } }),
        );
        assert_eq!(r["result"]["isError"], false, "{r}");
        let r = call(&mut s, 2, "tools/call", json!({ "name": "render_image", "arguments": { "node": "Constant1", "max_size": 64 } }));
        assert_eq!(r["result"]["content"][0]["type"], "image");
        let data = r["result"]["content"][0]["data"].as_str().unwrap();
        let png = base64::engine::general_purpose::STANDARD.decode(data).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        // Errors are tool results the agent can read, not protocol failures.
        let r = call(&mut s, 3, "tools/call", json!({ "name": "execute", "arguments": { "command": "node.create", "params": { "class": "Nope" } } }));
        assert_eq!(r["result"]["isError"], true);
        let r = call(&mut s, 4, "tools/call", json!({ "name": "render_image", "arguments": { "node": "Missing" } }));
        assert_eq!(r["result"]["isError"], true);
    }

    #[test]
    fn serve_reads_lines_and_survives_garbage() {
        let input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\nnot json\n\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}";
        let mut out = Vec::new();
        Server::new(Session::new()).serve(&input[..], &mut out).unwrap();
        let lines: Vec<Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["result"], json!({}));
        assert_eq!(lines[1]["error"]["code"], -32700);
        assert!(lines[2]["result"]["tools"].is_array());
    }

    #[test]
    fn downsample_bounds_the_size() {
        let px = vec![255u8; 100 * 50 * 4];
        let (w, h, out) = downsample(&px, 100, 50, 30);
        assert!(w <= 30 && h <= 30);
        assert_eq!(out.len(), (w * h * 4) as usize);
        assert!(out.iter().all(|v| *v == 255));
    }
}
