//! The fidelity harness: does NukeCraft compute what Nuke computes?
//!
//! A **case** is a tiny synthetic graph (Nuke's class and knob names, a 64×64 format) and what to
//! measure on it: pixel values at chosen points and frames, and knob values over time. The same
//! case runs two ways:
//!
//! - in NukeCraft ([`run_case`]), through the engine's commands;
//! - in Nuke, through a Python script this crate generates ([`export_python`]) and a person runs
//!   in Nuke's Script Editor. It is written for Nuke Non-commercial's limits: tiny graphs, nodes kept
//!   by reference and never looked up (its Python API retrieves only 10 nodes per script), no
//!   command-line rendering. It writes one JSON file, the **oracle**.
//!
//! [`compare`] lines the two up value by value. Cases whose feature NukeCraft doesn't have yet
//! (`nukecraft: false`) are measured in Nuke anyway, so the numbers are ready when the feature is
//! built. Only numbers from these synthetic graphs are kept; nothing from Nuke itself is committed.
//! See `docs/fidelity/README.md`.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod cases;

use std::collections::BTreeMap;

use nukecraft_engine::Session;
use nukecraft_geom::{Format, IRect};
use nukecraft_raster::Channel;
use nukecraft_render::{Renderer, with_render_stack};
use nukecraft_time::FrameTime;
use serde_json::{Map, Value, json};

pub use cases::cases;

/// The oracle file format this version writes and reads.
pub const GENERATOR: &str = "nukecraft-fidelity/1";

/// The Nuke-side script, with `__CASES__` and `__DEFAULTS__` to fill.
const ORACLE_PY: &str = include_str!("../python/oracle.py");

/// One synthetic test.
#[derive(Clone, Debug)]
pub struct Case {
    /// Stable id: `area.subject.variant` (`transform.filter.cubic`).
    pub id: String,
    /// The `docs/fidelity/README.md` entry it settles (`F1`…).
    pub area: &'static str,
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub nodes: Vec<NodeDef>,
    pub samples: Vec<Sample>,
    pub knob_samples: Vec<KnobSample>,
    /// NukeCraft implements what this case measures (false: Nuke-only for now).
    pub nukecraft: bool,
    /// Largest absolute difference that still matches.
    pub tolerance: f64,
}

/// A node of a case, in creation order (inputs name earlier nodes).
#[derive(Clone, Debug, Default)]
pub struct NodeDef {
    pub name: String,
    pub class: String,
    pub knobs: Map<String, Value>,
    /// Knobs set only in Nuke (ones NukeCraft doesn't have, set to neutral values).
    pub nuke_knobs: Map<String, Value>,
    pub inputs: Vec<Option<String>>,
    /// Animation keys, set with each application's default interpolation.
    pub keys: Vec<(String, f64, Value)>,
}

/// Pixel values to measure.
#[derive(Clone, Debug)]
pub struct Sample {
    pub node: String,
    pub frame: f64,
    pub points: Vec<(i32, i32)>,
    pub channels: Vec<String>,
}

/// Knob values to measure over time.
#[derive(Clone, Debug)]
pub struct KnobSample {
    pub node: String,
    pub knob: String,
    pub components: usize,
    pub frames: Vec<f64>,
}

/// What one side measured for a case: `key → value`, or why it couldn't.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Measured {
    pub values: BTreeMap<String, f64>,
    pub error: Option<String>,
}

/// How one case compares.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Match {
        max_error: f64,
    },
    Differs {
        max_error: f64,
        worst: String,
        ours: f64,
        nuke: f64,
        mismatched: usize,
    },
    /// The oracle has no numbers for this case (not run in Nuke yet).
    NoOracle,
    NukeError(String),
    OursError(String),
    /// NukeCraft doesn't implement this yet; the oracle numbers are kept for when it does.
    NotImplemented,
}

#[derive(Clone, Debug)]
pub struct CaseReport {
    pub id: String,
    pub area: &'static str,
    pub title: String,
    pub values: usize,
    pub outcome: Outcome,
}

/// A knob whose default differs between Nuke and NukeCraft.
#[derive(Clone, Debug, PartialEq)]
pub struct DefaultDiff {
    pub class: String,
    pub knob: String,
    pub ours: Value,
    pub nuke: Value,
}

#[derive(Clone, Debug)]
pub struct Report {
    pub nuke: String,
    pub cases: Vec<CaseReport>,
    pub defaults: Vec<DefaultDiff>,
    /// Knobs NukeCraft has that the Nuke class lacks (a naming mismatch to fix).
    pub missing_knobs: Vec<(String, String)>,
}

impl NodeDef {
    pub fn new(name: &str, class: &str) -> NodeDef {
        NodeDef { name: name.into(), class: class.into(), ..NodeDef::default() }
    }

    pub fn knob(mut self, k: &str, v: Value) -> NodeDef {
        self.knobs.insert(k.into(), v);
        self
    }

    pub fn nuke_knob(mut self, k: &str, v: Value) -> NodeDef {
        self.nuke_knobs.insert(k.into(), v);
        self
    }

    pub fn input(mut self, i: usize, from: &str) -> NodeDef {
        if self.inputs.len() <= i {
            self.inputs.resize(i + 1, None);
        }
        if let Some(slot) = self.inputs.get_mut(i) {
            *slot = Some(from.into());
        }
        self
    }

    pub fn key(mut self, knob: &str, frame: f64, v: Value) -> NodeDef {
        self.keys.push((knob.into(), frame, v));
        self
    }
}

/// A frame number as it appears in value keys (`1`, `1.5`), the same on both sides.
pub fn frame_label(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 { format!("{}", f as i64) } else { format!("{f}") }
}

fn pixel_key(node: &str, frame: f64, x: i32, y: i32, channel: &str) -> String {
    format!("px:{node}:{}:{x}:{y}:{channel}", frame_label(frame))
}

fn knob_key(node: &str, knob: &str, frame: f64, i: usize) -> String {
    format!("knob:{node}:{knob}:{}:{i}", frame_label(frame))
}

impl Case {
    /// The case as the Nuke script reads it (labels precomputed so both sides key alike).
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "width": self.width,
            "height": self.height,
            "nodes": self.nodes.iter().map(|n| json!({
                "name": n.name, "class": n.class, "knobs": n.knobs, "nuke_knobs": n.nuke_knobs, "inputs": n.inputs,
                "keys": n.keys.iter().map(|(k, f, v)| json!({ "knob": k, "frame": f, "value": v })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "samples": self.samples.iter().map(|s| json!({
                "node": s.node, "frame": s.frame, "label": frame_label(s.frame),
                "points": s.points.iter().map(|(x, y)| json!([x, y])).collect::<Vec<_>>(), "channels": s.channels,
            })).collect::<Vec<_>>(),
            "knob_samples": self.knob_samples.iter().map(|k| json!({
                "node": k.node, "knob": k.knob, "components": k.components,
                "frames": k.frames, "labels": k.frames.iter().map(|f| frame_label(*f)).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })
    }
}

/// The Python script that measures every case in Nuke and writes the oracle.
pub fn export_python(cases: &[Case]) -> Result<String, String> {
    let list: Vec<Value> = cases.iter().map(Case::to_json).collect();
    let cases_json = serde_json::to_string(&list).map_err(|e| e.to_string())?;
    let defaults_json = serde_json::to_string(&default_knob_names()).map_err(|e| e.to_string())?;
    // The JSON goes inside r'''…''': it must not contain three quotes in a row.
    if cases_json.contains("'''") || defaults_json.contains("'''") {
        return Err("case data contains ''' and can't be embedded".into());
    }
    Ok(ORACLE_PY.replace("__CASES__", &cases_json).replace("__DEFAULTS__", &defaults_json))
}

/// For each built-in class, its knob names (the Nuke script records Nuke's defaults for them).
fn default_knob_names() -> BTreeMap<String, Vec<String>> {
    nukecraft_nodes::Registry::builtin().iter().map(|s| (s.class.to_string(), s.knobs.iter().map(|k| k.name.to_string()).collect())).collect()
}

/// Measures a case in NukeCraft.
pub fn run_case(case: &Case) -> Measured {
    match build(case).and_then(|s| measure(case, &s)) {
        Ok(values) => Measured { values, error: None },
        Err(e) => Measured { values: BTreeMap::new(), error: Some(e) },
    }
}

fn build(case: &Case) -> Result<Session, String> {
    let mut s = Session::new();
    let run = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).map(|_| ()).map_err(|e| format!("{id}: {e}"));
    run(&mut s, "root.settings", json!({ "format": { "width": case.width, "height": case.height }, "first": 1, "last": 100 }))?;
    for n in &case.nodes {
        run(&mut s, "node.create", json!({ "class": n.class, "name": n.name, "knobs": n.knobs, "inputs": n.inputs }))?;
        for (knob, frame, value) in &n.keys {
            run(&mut s, "node.setKnob", json!({ "node": n.name, "knob": knob, "value": value, "frame": frame }))?;
        }
    }
    Ok(s)
}

fn measure(case: &Case, s: &Session) -> Result<BTreeMap<String, f64>, String> {
    let mut values = BTreeMap::new();
    for sample in &case.samples {
        let id = s.script.id_of(&sample.node).map_err(|e| e.to_string())?;
        let (x0, y0, x1, y1) =
            sample.points.iter().fold((i32::MAX, i32::MAX, i32::MIN, i32::MIN), |(a, b, c, d), (x, y)| (a.min(*x), b.min(*y), c.max(*x), d.max(*y)));
        let region = IRect::new(x0, y0, x1.saturating_add(1), y1.saturating_add(1));
        let t = FrameTime::from_f64(sample.frame);
        let (script, registry) = (&s.script, &s.registry);
        let img = with_render_stack(|| Renderer::new(script, registry).render(id, t, region))?.map_err(|e| e.to_string())?;
        for &(x, y) in &sample.points {
            for ch in &sample.channels {
                let c = Channel::parse(ch).map_err(|e| e.to_string())?;
                // Nuke's rule: outside the bbox the edge repeats (Image::get does that).
                values.insert(pixel_key(&sample.node, sample.frame, x, y, ch), img.get(&c, x, y) as f64);
            }
        }
    }
    let format = Format::new(case.width, case.height, 1.0);
    for ks in &case.knob_samples {
        let node = s.script.by_name(&ks.node).ok_or_else(|| format!("no node {}", ks.node))?;
        let spec = s.registry.get(&node.class).and_then(|c| c.knob(&ks.knob)).ok_or_else(|| format!("{} has no knob {}", node.class, ks.knob))?;
        let v = node.knobs.get(&ks.knob).cloned().unwrap_or_else(|| spec.default_value(&format));
        for &f in &ks.frames {
            let nums = v.eval(FrameTime::from_f64(f)).as_numbers().map(<[f64]>::to_vec).unwrap_or_default();
            for i in 0..ks.components {
                let n = nums.get(i).or(nums.last()).copied().unwrap_or(0.0);
                values.insert(knob_key(&ks.node, &ks.knob, f, i), n);
            }
        }
    }
    Ok(values)
}

/// Every case, measured in NukeCraft, as the oracle file's `cases` object (for inspection).
pub fn run_all(cases: &[Case]) -> Value {
    let mut out = Map::new();
    for c in cases.iter().filter(|c| c.nukecraft) {
        let m = run_case(c);
        out.insert(c.id.clone(), json!({ "values": m.values, "error": m.error }));
    }
    json!({ "generator": GENERATOR, "nukecraft": env!("CARGO_PKG_VERSION"), "cases": out })
}

/// Reads one side's measurements for a case out of an oracle document.
fn oracle_case(oracle: &Value, id: &str) -> Option<Measured> {
    let c = oracle.get("cases")?.get(id)?;
    let values =
        c.get("values").and_then(Value::as_object).map(|m| m.iter().filter_map(|(k, v)| v.as_f64().map(|f| (k.clone(), f))).collect()).unwrap_or_default();
    let error = c.get("error").and_then(Value::as_str).map(str::to_string);
    Some(Measured { values, error })
}

/// Compares NukeCraft with an oracle document.
pub fn compare(cases: &[Case], oracle: &Value) -> Report {
    let nuke = oracle.get("nuke").and_then(|n| n.get("version")).and_then(Value::as_str).unwrap_or("unknown").to_string();
    let mut reports = Vec::new();
    for case in cases {
        let theirs = oracle_case(oracle, &case.id);
        let (outcome, values) = match theirs {
            None => (Outcome::NoOracle, 0),
            Some(Measured { error: Some(e), .. }) => (Outcome::NukeError(first_line(&e)), 0),
            Some(_) if !case.nukecraft => (Outcome::NotImplemented, 0),
            Some(t) => {
                let ours = run_case(case);
                match ours.error {
                    Some(e) => (Outcome::OursError(e), 0),
                    None => (diff(&ours.values, &t.values, case.tolerance), t.values.len()),
                }
            }
        };
        reports.push(CaseReport { id: case.id.clone(), area: case.area, title: case.title.clone(), values, outcome });
    }
    let (defaults, missing_knobs) = compare_defaults(oracle);
    Report { nuke, cases: reports, defaults, missing_knobs }
}

fn first_line(s: &str) -> String {
    s.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or(s).trim().to_string()
}

fn diff(ours: &BTreeMap<String, f64>, nuke: &BTreeMap<String, f64>, tolerance: f64) -> Outcome {
    let mut max_error = 0.0f64;
    let mut worst: Option<(String, f64, f64)> = None;
    let mut mismatched = 0;
    for (k, theirs) in nuke {
        let mine = ours.get(k).copied().unwrap_or(f64::NAN);
        let err = (mine - theirs).abs();
        let err = if err.is_nan() { f64::INFINITY } else { err };
        if err > tolerance {
            mismatched += 1;
        }
        if worst.is_none() || err > max_error {
            max_error = err;
            worst = Some((k.clone(), mine, *theirs));
        }
    }
    match worst {
        Some((worst, ours, nuke)) if mismatched > 0 => Outcome::Differs { max_error, worst, ours, nuke, mismatched },
        _ => Outcome::Match { max_error },
    }
}

/// Knob defaults: NukeCraft's (at the cases' 64×64 root format) against those Nuke recorded.
fn compare_defaults(oracle: &Value) -> (Vec<DefaultDiff>, Vec<(String, String)>) {
    let Some(defaults) = oracle.get("defaults").and_then(Value::as_object) else { return (Vec::new(), Vec::new()) };
    let format = Format::new(64, 64, 1.0);
    let mut diffs = Vec::new();
    let mut missing = Vec::new();
    for spec in nukecraft_nodes::Registry::builtin().iter() {
        let Some(theirs) = defaults.get(spec.class).and_then(Value::as_object) else { continue };
        for k in spec.knobs {
            let Some(nuke) = theirs.get(k.name) else {
                if !theirs.contains_key("error") {
                    missing.push((spec.class.to_string(), k.name.to_string()));
                }
                continue;
            };
            let ours = match k.default_value(&format).eval(FrameTime::ZERO) {
                nukecraft_graph::Value::Bool(b) => json!(b),
                nukecraft_graph::Value::Numbers(n) if n.len() == 1 => json!(n.first()),
                nukecraft_graph::Value::Numbers(n) => json!(n),
                nukecraft_graph::Value::Text(t) => json!(t),
            };
            if !same_default(&ours, nuke) {
                diffs.push(DefaultDiff { class: spec.class.into(), knob: k.name.into(), ours, nuke: nuke.clone() });
            }
        }
    }
    (diffs, missing)
}

/// Defaults match: equal text or bool (bool against 0/1), or equal numbers where one number
/// stands for all components.
fn same_default(ours: &Value, nuke: &Value) -> bool {
    let nums = |v: &Value| -> Option<Vec<f64>> {
        match v {
            Value::Number(n) => n.as_f64().map(|f| vec![f]),
            Value::Bool(b) => Some(vec![if *b { 1.0 } else { 0.0 }]),
            Value::Array(a) => a.iter().map(Value::as_f64).collect(),
            _ => None,
        }
    };
    match (nums(ours), nums(nuke)) {
        (Some(a), Some(b)) => {
            let n = a.len().max(b.len());
            (0..n).all(|i| {
                let x = a.get(i).or(a.last()).copied().unwrap_or(f64::NAN);
                let y = b.get(i).or(b.last()).copied().unwrap_or(f64::NAN);
                (x - y).abs() < 1e-6
            })
        }
        _ => ours.as_str().map(str::trim) == nuke.as_str().map(str::trim),
    }
}

impl Report {
    pub fn matched(&self) -> Vec<&str> {
        self.cases.iter().filter(|c| matches!(c.outcome, Outcome::Match { .. })).map(|c| c.id.as_str()).collect()
    }

    /// The report as Markdown (for `docs/fidelity/report.md`).
    pub fn markdown(&self) -> String {
        let count = |f: fn(&Outcome) -> bool| self.cases.iter().filter(|c| f(&c.outcome)).count();
        let mut s = format!(
            "# Fidelity report\n\nNuke: {} · cases: {} · match: {} · differ: {} · NukeCraft lacks the feature: {} · no oracle: {} · errors: {}\n\n",
            self.nuke,
            self.cases.len(),
            count(|o| matches!(o, Outcome::Match { .. })),
            count(|o| matches!(o, Outcome::Differs { .. })),
            count(|o| matches!(o, Outcome::NotImplemented)),
            count(|o| matches!(o, Outcome::NoOracle)),
            count(|o| matches!(o, Outcome::NukeError(_) | Outcome::OursError(_))),
        );
        s.push_str("| Case | Area | Values | Result |\n|---|---|---:|---|\n");
        for c in &self.cases {
            let result = match &c.outcome {
                Outcome::Match { max_error } => format!("match (max error {max_error:.2e})"),
                Outcome::Differs { max_error, worst, ours, nuke, mismatched } => {
                    format!("**differs** at {mismatched} values; worst `{worst}`: NukeCraft {ours:.6}, Nuke {nuke:.6} (error {max_error:.3e})")
                }
                Outcome::NoOracle => "no oracle numbers".into(),
                Outcome::NukeError(e) => format!("Nuke error: {e}"),
                Outcome::OursError(e) => format!("NukeCraft error: {e}"),
                Outcome::NotImplemented => "not in NukeCraft yet (numbers kept)".into(),
            };
            s.push_str(&format!("| `{}` | {} | {} | {} |\n", c.id, c.area, c.values, result));
        }
        if !self.defaults.is_empty() {
            s.push_str("\n## Knob defaults that differ\n\n| Class | Knob | NukeCraft | Nuke |\n|---|---|---|---|\n");
            for d in &self.defaults {
                s.push_str(&format!("| {} | `{}` | `{}` | `{}` |\n", d.class, d.knob, d.ours, d.nuke));
            }
        }
        if !self.missing_knobs.is_empty() {
            s.push_str("\n## NukeCraft knobs Nuke's class doesn't have (check the names)\n\n");
            for (c, k) in &self.missing_knobs {
                s.push_str(&format!("- {c}: `{k}`\n"));
            }
        }
        s
    }
}

#[cfg(test)]
mod tests;
