//! Knob declarations and the evaluated knob values a node sees while it renders.

use std::collections::BTreeMap;

use nukecraft_geom::{Format, V2};
use nukecraft_graph::{KnobValue, Value};

/// What kind of value a knob holds; decides validation, the properties panel widget and the
/// MCP schema.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KnobKind {
    /// One number.
    Float,
    /// One whole number.
    Int,
    /// A 2D position or size; one number means both.
    Xy,
    /// An RGBA colour or per-channel value; one number means all four, three mean RGB.
    Color,
    /// `x y r t`.
    Box,
    Bool,
    /// One of a fixed list of names.
    Choice(&'static [&'static str]),
    Text,
    /// A file path, with `####` / `%04d` frame-number patterns.
    File,
    /// A channel set (`rgba`, `rgb`, `alpha`, `all`, `depth.Z`…).
    Channels,
    /// A format name or `WxH`; empty means the root format.
    Format,
}

/// A knob's default value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KnobDefault {
    Number(f64),
    Numbers(&'static [f64]),
    Bool(bool),
    Text(&'static str),
    /// The centre of the root format (Transform's `center`).
    FormatCenter,
    /// The root format's box (Crop's `box`).
    FormatBox,
    /// Fractions of the root format, alternately of its width and height: `[x, y, r, t]` for a
    /// box (Rectangle's `area`), `[x, y]` for a point (CornerPin2D's corners).
    FormatFraction(&'static [f64]),
}

impl KnobDefault {
    /// Whether the default depends on the root format: such defaults are fixed when a node is
    /// made, as Nuke does.
    pub fn is_format_dependent(self) -> bool {
        matches!(self, KnobDefault::FormatCenter | KnobDefault::FormatBox | KnobDefault::FormatFraction(_))
    }
}

/// One knob of a node class.
#[derive(Clone, Copy, Debug)]
pub struct KnobSpec {
    /// The script name (`translate`), as in Nuke.
    pub name: &'static str,
    pub label: &'static str,
    pub kind: KnobKind,
    pub default: KnobDefault,
    pub help: &'static str,
}

/// A node's knobs evaluated at one time, with defaults filled in. Getters never fail: a missing
/// or ill-typed value reads as the default.
#[derive(Clone, Debug, Default)]
pub struct Knobs {
    values: BTreeMap<&'static str, Value>,
}

impl KnobKind {
    /// A short name for agents and the MCP schema.
    pub fn name(self) -> &'static str {
        match self {
            KnobKind::Float => "float",
            KnobKind::Int => "int",
            KnobKind::Xy => "xy",
            KnobKind::Color => "color",
            KnobKind::Box => "box",
            KnobKind::Bool => "bool",
            KnobKind::Choice(_) => "choice",
            KnobKind::Text => "text",
            KnobKind::File => "file",
            KnobKind::Channels => "channels",
            KnobKind::Format => "format",
        }
    }

    /// How many numeric components a value of this kind may have (empty for non-numeric kinds).
    fn arities(self) -> &'static [usize] {
        match self {
            KnobKind::Float | KnobKind::Int => &[1],
            KnobKind::Xy => &[1, 2],
            KnobKind::Color => &[1, 3, 4],
            KnobKind::Box => &[4],
            _ => &[],
        }
    }

    pub fn is_numeric(self) -> bool {
        !self.arities().is_empty()
    }
}

impl KnobSpec {
    /// The default as a stored value, resolved against `format`.
    pub fn default_value(&self, format: &Format) -> KnobValue {
        match self.default {
            KnobDefault::Number(v) => KnobValue::number(v),
            KnobDefault::Numbers(vs) => KnobValue::numbers(vs),
            KnobDefault::Bool(b) => KnobValue::Bool(b),
            KnobDefault::Text(t) => KnobValue::text(t),
            KnobDefault::FormatCenter => {
                let c = format.center();
                KnobValue::numbers(&[c.x, c.y])
            }
            KnobDefault::FormatBox => KnobValue::numbers(&[0.0, 0.0, format.width as f64, format.height as f64]),
            KnobDefault::FormatFraction(fs) => {
                let (w, h) = (format.width as f64, format.height as f64);
                let v: Vec<f64> = fs.iter().enumerate().map(|(i, f)| f * if i % 2 == 0 { w } else { h }).collect();
                KnobValue::numbers(&v)
            }
        }
    }

    /// Checks that `v` fits this knob; returns the value normalised (a 0/1 number for a bool
    /// knob becomes a bool).
    pub fn validate(&self, v: KnobValue) -> Result<KnobValue, String> {
        let name = self.name;
        match self.kind {
            KnobKind::Bool => match v {
                KnobValue::Bool(_) => Ok(v),
                KnobValue::Number(nukecraft_graph::Anim::Static(n)) if n == 0.0 || n == 1.0 => Ok(KnobValue::Bool(n == 1.0)),
                _ => Err(format!("`{name}` takes true or false")),
            },
            KnobKind::Choice(options) => match &v {
                KnobValue::Text(t) => match options.iter().find(|o| o.eq_ignore_ascii_case(t)) {
                    Some(o) => Ok(KnobValue::text(*o)),
                    None => Err(format!("`{name}` takes one of: {}", options.join(", "))),
                },
                _ => Err(format!("`{name}` takes one of: {}", options.join(", "))),
            },
            KnobKind::Text | KnobKind::File => match v {
                KnobValue::Text(_) => Ok(v),
                _ => Err(format!("`{name}` takes text")),
            },
            KnobKind::Channels => match &v {
                // Layer names are checked against the image at render time; only the syntax here.
                KnobValue::Text(t) if t.split([' ', ',']).all(|w| w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')) => Ok(v),
                _ => Err(format!("`{name}` takes a channel set such as rgba, rgb, alpha, all or depth.Z")),
            },
            KnobKind::Format => match &v {
                KnobValue::Text(t) if t.trim().is_empty() || Format::parse(t).is_some() => Ok(v),
                _ => Err(format!("`{name}` takes a format name (HD_1080) or WxH")),
            },
            kind => {
                let Some(comps) = v.components() else {
                    return Err(format!("`{name}` takes {}", describe_arity(kind)));
                };
                if !kind.arities().contains(&comps.len()) {
                    return Err(format!("`{name}` takes {}", describe_arity(kind)));
                }
                Ok(v)
            }
        }
    }
}

fn describe_arity(kind: KnobKind) -> &'static str {
    match kind {
        KnobKind::Float => "a number",
        KnobKind::Int => "a whole number",
        KnobKind::Xy => "a number or [x, y]",
        KnobKind::Color => "a number, [r, g, b] or [r, g, b, a]",
        KnobKind::Box => "[x, y, r, t]",
        _ => "a value",
    }
}

impl Knobs {
    /// Evaluates the stored knobs of a node against its class's specs at one time.
    pub fn evaluate(specs: &[KnobSpec], stored: &BTreeMap<String, KnobValue>, t: nukecraft_time::FrameTime, format: &Format) -> Knobs {
        let mut values = BTreeMap::new();
        for spec in specs {
            let v = stored.get(spec.name).and_then(|v| spec.validate(v.clone()).ok()).unwrap_or_else(|| spec.default_value(format));
            values.insert(spec.name, v.eval(t));
        }
        Knobs { values }
    }

    /// Builds knobs from explicit values (tests and tools).
    pub fn from_values(values: impl IntoIterator<Item = (&'static str, Value)>) -> Knobs {
        Knobs { values: values.into_iter().collect() }
    }

    fn numbers(&self, name: &str) -> &[f64] {
        self.values.get(name).and_then(Value::as_numbers).unwrap_or(&[])
    }

    /// The first component of a numeric knob (0 if absent).
    pub fn float(&self, name: &str) -> f64 {
        self.numbers(name).first().copied().unwrap_or(0.0)
    }

    /// An integer knob, rounded and clamped to `i32`.
    pub fn int(&self, name: &str) -> i32 {
        let v = self.float(name).round();
        v.clamp(i32::MIN as f64, i32::MAX as f64) as i32
    }

    /// An XY knob; one component means both.
    pub fn xy(&self, name: &str) -> V2 {
        match self.numbers(name) {
            [x, y, ..] => V2::new(*x, *y),
            [v] => V2::new(*v, *v),
            [] => V2::ZERO,
        }
    }

    /// A colour knob as RGBA: four components as given, three are RGB with alpha `alpha3`, one
    /// is used for all four.
    pub fn rgba(&self, name: &str, alpha3: f64) -> [f64; 4] {
        match self.numbers(name) {
            [r, g, b, a, ..] => [*r, *g, *b, *a],
            [r, g, b] => [*r, *g, *b, alpha3],
            [v] => [*v; 4],
            _ => [0.0; 4],
        }
    }

    /// A box knob `[x, y, r, t]`.
    pub fn bbox(&self, name: &str) -> [f64; 4] {
        match self.numbers(name) {
            [x, y, r, t, ..] => [*x, *y, *r, *t],
            _ => [0.0; 4],
        }
    }

    pub fn boolean(&self, name: &str) -> bool {
        match self.values.get(name) {
            Some(Value::Bool(b)) => *b,
            Some(Value::Numbers(v)) => v.first().is_some_and(|x| *x != 0.0),
            _ => false,
        }
    }

    /// A choice, text, file, channels or format knob.
    pub fn text(&self, name: &str) -> &str {
        match self.values.get(name) {
            Some(Value::Text(t)) => t,
            _ => "",
        }
    }
}
