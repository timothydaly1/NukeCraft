//! Headless UI snapshot: runs the real NukeCraft UI without a window or GPU and writes PNGs, so
//! agents can look at the interface at any time.
//!
//! ```text
//! cargo run -p nukecraft-ui-egui --example snapshot -- [--out ui.png] [--size 1600x1000]
//!     [--scale 1] [--empty | --open script.ncs] [--script steps.jsonl | --step '<json>']...
//! ```
//!
//! Steps run in order, the app settling in between:
//!
//! - `{"method":"engine.execute","params":{"command":"node.create","params":{…}}}`
//! - `{"method":"ui.click","params":{"id":"graph.node.Blur1","clicks":1,"modifiers":["shift"]}}`
//! - `{"method":"ui.drag","params":{"from":"graph.node.Grade1.output","to":"graph.node.Blur1"}}`
//! - `{"method":"ui.key","params":{"key":"B","modifiers":["shift"],"hover":"graph.canvas"}}`
//! - `{"method":"ui.text","params":{"text":"blur"}}`
//! - `{"method":"ui.hover","params":{"id":"viewer.image"}}` (or `"pos":[x, y]`)
//! - `{"method":"ui.inspect","params":{"prefix":"graph."}}` prints matching element ids and rects
//! - `{"method":"snap","params":{"path":"step.png"}}` writes an intermediate snapshot

use std::time::Duration;

use egui::{Modifiers, Pos2, Vec2};
use nukecraft_engine::Session;
use nukecraft_engine::demo::demo_session;
use nukecraft_ui_egui::snapshot::Headless;
use serde_json::{Value, json};

struct Args {
    out: String,
    size: Vec2,
    scale: f32,
    session: Option<String>,
    empty: bool,
    steps: Vec<Value>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args { out: "ui.png".into(), size: Vec2::new(1600.0, 1000.0), scale: 1.0, session: None, empty: false, steps: Vec::new() };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = || it.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--out" => a.out = val()?,
            "--size" => {
                let v = val()?;
                let (w, h) = v.split_once('x').ok_or("--size WxH")?;
                a.size = Vec2::new(w.parse().map_err(|_| "bad width")?, h.parse().map_err(|_| "bad height")?);
            }
            "--scale" => a.scale = val()?.parse().map_err(|_| "bad --scale")?,
            "--empty" => a.empty = true,
            "--open" => a.session = Some(val()?),
            "--step" => a.steps.push(serde_json::from_str(&val()?).map_err(|e| format!("--step: {e}"))?),
            "--script" => {
                let path = val()?;
                let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
                for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("//")) {
                    a.steps.push(serde_json::from_str(line).map_err(|e| format!("{path}: {e}: {line}"))?);
                }
            }
            "-h" | "--help" => {
                return Err("usage: snapshot [--out ui.png] [--size WxH] [--scale 1] [--empty | --open f.ncs] [--script f.jsonl | --step json]...".into());
            }
            _ => return Err(format!("unknown argument {arg}")),
        }
    }
    Ok(a)
}

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

fn write_png(h: &Headless, path: &str) -> Result<(), String> {
    let png = h.snapshot().png()?;
    std::fs::write(path, png).map_err(|e| format!("{path}: {e}"))
}

fn step(h: &mut Headless, s: &Value) -> Result<Value, String> {
    let p = &s["params"];
    let id = |key: &str| p[key].as_str().ok_or_else(|| format!("`{key}` is required"));
    match s["method"].as_str().unwrap_or("") {
        "engine.execute" => {
            let command = id("command")?;
            let params = p.get("params").cloned().unwrap_or_else(|| json!({}));
            h.app.session.execute(command, &params).map_err(|e| e.to_string())
        }
        "ui.click" => h.click(id("id")?, p["clicks"].as_u64().unwrap_or(1) as usize, modifiers(p)).map(|_| json!({})),
        "ui.drag" => h.drag(id("from")?, id("to")?).map(|_| json!({})),
        "ui.hover" => {
            let pos = match p["pos"].as_array() {
                Some(xy) => Pos2::new(xy.first().and_then(Value::as_f64).unwrap_or(0.0) as f32, xy.get(1).and_then(Value::as_f64).unwrap_or(0.0) as f32),
                None => h.app.automation.center(id("id")?).ok_or("no such element")?,
            };
            h.hover(pos);
            Ok(json!({}))
        }
        "ui.key" => {
            if let Some(target) = p["hover"].as_str() {
                let pos = h.app.automation.center(target).ok_or_else(|| format!("no UI element `{target}`"))?;
                h.hover(pos);
            }
            let key = egui::Key::from_name(id("key")?).ok_or("unknown key")?;
            h.key(key, modifiers(p));
            Ok(json!({}))
        }
        "ui.text" => {
            h.text(id("text")?);
            Ok(json!({}))
        }
        "ui.inspect" => {
            let prefix = p["prefix"].as_str().unwrap_or("");
            let list: Vec<Value> = h
                .app
                .automation
                .query(prefix)
                .iter()
                .map(|e| json!({ "id": e.id, "label": e.label, "rect": [e.rect.min.x, e.rect.min.y, e.rect.width(), e.rect.height()] }))
                .collect();
            Ok(json!(list))
        }
        "snap" => write_png(h, id("path")?).map(|_| json!({})),
        other => Err(format!("unknown method `{other}`")),
    }
}

fn main() -> std::process::ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::from(2);
        }
    };
    let session = if args.empty {
        Ok(Session::new())
    } else if let Some(path) = &args.session {
        let mut s = Session::new();
        s.execute("file.open", &json!({ "path": path })).map(|_| s)
    } else {
        demo_session()
    };
    let session = match session {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let mut h = Headless::new(session, args.size, args.scale);
    h.settle(Duration::from_secs(30));
    for s in &args.steps {
        let reply = step(&mut h, s);
        h.settle(Duration::from_secs(30));
        match reply {
            Ok(v) => println!("{}", json!({ "ok": v })),
            Err(e) => println!("{}", json!({ "error": e })),
        }
    }
    match write_png(&h, &args.out) {
        Ok(()) => {
            println!("{}", json!({ "wrote": args.out }));
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}
