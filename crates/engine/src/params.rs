//! Reading command parameters. Every value from a caller is untrusted: wrong types and missing
//! keys are errors that name the parameter, never panics.

use nukecraft_graph::{KnobValue, NodeId, Script};
use nukecraft_time::FrameTime;
use serde_json::Value;

use crate::{EngineError, Result};

pub fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

pub fn str<'a>(p: &'a Value, key: &str) -> Result<&'a str> {
    p.get(key).and_then(Value::as_str).ok_or_else(|| bad(format!("`{key}` (text) is required")))
}

pub fn opt_str<'a>(p: &'a Value, key: &str) -> Result<Option<&'a str>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(bad(format!("`{key}` must be text"))),
    }
}

pub fn opt_f64(p: &Value, key: &str) -> Result<Option<f64>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().filter(|f| f.is_finite()).map(Some).ok_or_else(|| bad(format!("`{key}` must be a finite number"))),
    }
}

pub fn f64(p: &Value, key: &str) -> Result<f64> {
    opt_f64(p, key)?.ok_or_else(|| bad(format!("`{key}` (number) is required")))
}

pub fn opt_i64(p: &Value, key: &str) -> Result<Option<i64>> {
    match opt_f64(p, key)? {
        None => Ok(None),
        Some(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => Ok(Some(f as i64)),
        Some(_) => Err(bad(format!("`{key}` must be a whole number"))),
    }
}

pub fn usize(p: &Value, key: &str) -> Result<usize> {
    match opt_i64(p, key)? {
        Some(i) if i >= 0 => Ok(i as usize),
        Some(_) => Err(bad(format!("`{key}` must not be negative"))),
        None => Err(bad(format!("`{key}` (whole number) is required"))),
    }
}

pub fn opt_bool(p: &Value, key: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(format!("`{key}` must be true or false"))),
    }
}

/// A frame number parameter as a time.
pub fn opt_frame(p: &Value, key: &str) -> Result<Option<FrameTime>> {
    Ok(opt_f64(p, key)?.map(FrameTime::from_f64))
}

/// The node a name parameter refers to.
pub fn node(script: &Script, p: &Value, key: &str) -> Result<NodeId> {
    Ok(script.id_of(str(p, key)?)?)
}

/// A list of node names from `nodes` (array) or `node` (one name).
pub fn nodes(script: &Script, p: &Value) -> Result<Vec<NodeId>> {
    if let Some(list) = p.get("nodes") {
        let arr = list.as_array().ok_or_else(|| bad("`nodes` must be a list of node names"))?;
        return arr.iter().map(|v| v.as_str().ok_or_else(|| bad("`nodes` must be a list of node names")).and_then(|n| Ok(script.id_of(n)?))).collect();
    }
    Ok(vec![node(script, p, "node")?])
}

/// A knob value from JSON: a number, a list of numbers, true/false, or text.
pub fn knob_value(v: &Value) -> Result<KnobValue> {
    match v {
        Value::Bool(b) => Ok(KnobValue::Bool(*b)),
        Value::Number(n) => n.as_f64().filter(|f| f.is_finite()).map(KnobValue::number).ok_or_else(|| bad("knob numbers must be finite")),
        Value::String(s) => {
            if s.len() > nukecraft_graph::MAX_TEXT {
                return Err(bad("knob text is too long"));
            }
            Ok(KnobValue::text(s.clone()))
        }
        Value::Array(items) => {
            if items.is_empty() || items.len() > nukecraft_graph::MAX_COMPONENTS {
                return Err(bad(format!("a knob takes 1 to {} numbers", nukecraft_graph::MAX_COMPONENTS)));
            }
            let nums: Option<Vec<f64>> = items.iter().map(|i| i.as_f64().filter(|f| f.is_finite())).collect();
            nums.map(|n| KnobValue::numbers(&n)).ok_or_else(|| bad("a knob list must hold only finite numbers"))
        }
        Value::Null | Value::Object(_) => Err(bad("a knob value is a number, a list of numbers, true/false or text")),
    }
}
