//! The demo script (`--demo`): a small composite built entirely through commands, so it doubles
//! as a smoke test of the command layer. All of its content is procedural.
//!
//! ```text
//!   CheckerBoard1     Ramp1      Constant1
//!        │              └──A──┐      │ B
//!   Transform1               Merge1 (plus)
//!   (rotates)                    │ B
//!        └──────────A──────── Merge2 (over)
//!                                │
//!                       Grade1 → Blur1 → Write1
//! ```

use serde_json::json;

use crate::{Result, Session};

/// A session holding the demo script, with no undo history.
pub fn demo_session() -> Result<Session> {
    let mut s = Session::new();
    build(&mut s)?;
    s.clear_history();
    s.dirty = false;
    Ok(s)
}

fn build(s: &mut Session) -> Result<()> {
    s.execute("root.settings", &json!({ "format": { "width": 640, "height": 360 }, "fps": 24, "first": 1, "last": 24 }))?;
    s.execute("node.create", &json!({ "class": "Constant", "x": 300, "y": 0, "knobs": { "color": [0.02, 0.025, 0.05, 1] } }))?;
    s.execute(
        "node.create",
        &json!({ "class": "Ramp", "x": 170, "y": 0, "knobs": { "p0": [0, 0], "p1": [0, 360], "color": [0.08, 0.16, 0.45, 0], "type": "smooth" } }),
    )?;
    s.execute("node.create", &json!({ "class": "Merge", "inputs": ["Constant1", "Ramp1"], "knobs": { "operation": "plus" }, "x": 300, "y": 80 }))?;
    s.execute(
        "node.create",
        &json!({ "class": "CheckerBoard", "x": 40, "y": 0, "knobs": { "format": "square_256", "boxsize": 32, "color0": [0.9, 0.35, 0.05, 1], "color1": [0.95, 0.85, 0.6, 1] } }),
    )?;
    s.execute(
        "node.create",
        &json!({ "class": "Transform", "inputs": ["CheckerBoard1"], "knobs": { "center": [128, 128], "translate": [192, 52], "scale": 0.8 } }),
    )?;
    s.execute("node.setKnob", &json!({ "node": "Transform1", "knob": "rotate", "value": 0, "frame": 1, "interp": "linear" }))?;
    s.execute("node.setKnob", &json!({ "node": "Transform1", "knob": "rotate", "value": 90, "frame": 24, "interp": "linear" }))?;
    s.execute("node.create", &json!({ "class": "Merge", "inputs": ["Merge1", "Transform1"], "x": 300, "y": 170 }))?;
    s.execute("node.create", &json!({ "class": "Grade", "inputs": ["Merge2"], "knobs": { "white": [1.1, 1.05, 1.0, 1.0], "gamma": 0.95 } }))?;
    s.execute("node.create", &json!({ "class": "Blur", "inputs": ["Grade1"], "knobs": { "size": 1.5 } }))?;
    s.execute("node.create", &json!({ "class": "Write", "inputs": ["Blur1"], "knobs": { "file": "nukecraft-demo.####.png" } }))?;
    s.execute("time.goto", &json!({ "frame": 1 }))?;
    Ok(())
}
