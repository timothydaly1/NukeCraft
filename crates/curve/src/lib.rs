//! Animation curves for NukeCraft knobs.
//!
//! A [`Curve`] is a list of keys sorted by time. Each key's interpolation decides the shape of
//! the segment that *leaves* it. Before the first key and after the last the curve holds the end
//! values (constant extrapolation, Nuke's default).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use nukecraft_time::FrameTime;
use serde::{Deserialize, Serialize};

/// The most keys one curve may hold.
pub const MAX_KEYS: usize = 100_000;

/// How a segment is shaped between a key and the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Interp {
    /// Hold the key's value until the next key.
    Constant,
    /// A straight line to the next key.
    Linear,
    /// A cubic with flat (zero-slope) tangents at both keys.
    Horizontal,
    /// A cubic whose tangent at each key is the Catmull-Rom slope through its neighbours.
    #[default]
    CatmullRom,
}

/// One key: a time, a value and the interpolation of the segment after it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Key {
    #[serde(rename = "t")]
    pub time: FrameTime,
    #[serde(rename = "v")]
    pub value: f64,
    #[serde(default, rename = "i", skip_serializing_if = "is_default_interp")]
    pub interp: Interp,
}

fn is_default_interp(i: &Interp) -> bool {
    *i == Interp::default()
}

/// An animation curve.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Curve {
    keys: Vec<Key>,
}

impl Curve {
    pub fn new() -> Curve {
        Curve::default()
    }

    /// A curve from keys in any order: non-finite values are dropped, keys at the same time
    /// keep the last one, and at most [`MAX_KEYS`] keys are kept. Returns the curve and how many
    /// keys were dropped.
    pub fn from_keys(keys: impl IntoIterator<Item = Key>) -> (Curve, usize) {
        let mut c = Curve::new();
        let mut dropped = 0;
        for k in keys {
            if c.set_key(k).is_err() {
                dropped += 1;
            }
        }
        (c, dropped)
    }

    pub fn keys(&self) -> &[Key] {
        &self.keys
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Adds a key, replacing any key at the same time.
    pub fn set_key(&mut self, key: Key) -> Result<(), String> {
        if !key.value.is_finite() {
            return Err(format!("key value {} is not finite", key.value));
        }
        match self.keys.binary_search_by(|k| k.time.cmp(&key.time)) {
            Ok(i) => {
                if let Some(k) = self.keys.get_mut(i) {
                    *k = key;
                }
            }
            Err(i) => {
                if self.keys.len() >= MAX_KEYS {
                    return Err(format!("a curve holds at most {MAX_KEYS} keys"));
                }
                self.keys.insert(i, key);
            }
        }
        Ok(())
    }

    /// Removes the key at `time`; returns whether there was one.
    pub fn remove_key(&mut self, time: FrameTime) -> bool {
        match self.keys.binary_search_by(|k| k.time.cmp(&time)) {
            Ok(i) => {
                self.keys.remove(i);
                true
            }
            Err(_) => false,
        }
    }

    /// The value at `t` (0 for a curve without keys).
    pub fn eval(&self, t: FrameTime) -> f64 {
        let (Some(first), Some(last)) = (self.keys.first(), self.keys.last()) else {
            return 0.0;
        };
        if t <= first.time {
            return first.value;
        }
        if t >= last.time {
            return last.value;
        }
        // The segment [i, i+1] holding t.
        let i = match self.keys.binary_search_by(|k| k.time.cmp(&t)) {
            Ok(i) => return self.keys.get(i).map_or(0.0, |k| k.value),
            Err(i) => i.saturating_sub(1),
        };
        let (Some(k0), Some(k1)) = (self.keys.get(i), self.keys.get(i + 1)) else {
            return last.value;
        };
        let span = (k1.time.0 - k0.time.0) as f64;
        if span <= 0.0 {
            return k1.value;
        }
        let u = (t.0 - k0.time.0) as f64 / span;
        match k0.interp {
            Interp::Constant => k0.value,
            Interp::Linear => k0.value + (k1.value - k0.value) * u,
            Interp::Horizontal => hermite(k0.value, 0.0, k1.value, 0.0, u),
            Interp::CatmullRom => {
                // Slopes in value per segment-length, from the neighbouring keys.
                let m0 = self.slope_at(i) * span;
                let m1 = self.slope_at(i + 1) * span;
                hermite(k0.value, m0, k1.value, m1, u)
            }
        }
    }

    /// Catmull-Rom slope (value per subframe) at key `i`; one-sided at the ends.
    fn slope_at(&self, i: usize) -> f64 {
        let prev = i.checked_sub(1).and_then(|p| self.keys.get(p));
        let next = self.keys.get(i + 1);
        let here = self.keys.get(i);
        let (a, b) = match (prev, here, next) {
            (Some(p), _, Some(n)) => (p, n),
            (None, Some(h), Some(n)) => (h, n),
            (Some(p), Some(h), None) => (p, h),
            _ => return 0.0,
        };
        let dt = (b.time.0 - a.time.0) as f64;
        if dt <= 0.0 { 0.0 } else { (b.value - a.value) / dt }
    }
}

/// Cubic Hermite on `u ∈ [0, 1]` with end values `p0`, `p1` and end tangents `m0`, `m1`.
fn hermite(p0: f64, m0: f64, p1: f64, m1: f64, u: f64) -> f64 {
    let (u2, u3) = (u * u, u * u * u);
    (2.0 * u3 - 3.0 * u2 + 1.0) * p0 + (u3 - 2.0 * u2 + u) * m0 + (-2.0 * u3 + 3.0 * u2) * p1 + (u3 - u2) * m1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(f: i64, v: f64, interp: Interp) -> Key {
        Key { time: FrameTime::from_frame(f), value: v, interp }
    }

    #[test]
    fn extrapolation_holds_the_ends() {
        let (c, _) = Curve::from_keys([key(10, 1.0, Interp::Linear), key(20, 3.0, Interp::Linear)]);
        assert_eq!(c.eval(FrameTime::from_frame(0)), 1.0);
        assert_eq!(c.eval(FrameTime::from_frame(30)), 3.0);
        assert_eq!(c.eval(FrameTime::from_frame(15)), 2.0);
        assert_eq!(Curve::new().eval(FrameTime::ZERO), 0.0);
    }

    #[test]
    fn interpolations() {
        let at = |i: Interp, f: f64| {
            let (c, _) = Curve::from_keys([key(0, 0.0, i), key(10, 10.0, i)]);
            c.eval(FrameTime::from_f64(f))
        };
        assert_eq!(at(Interp::Constant, 9.9), 0.0);
        assert!((at(Interp::Linear, 2.5) - 2.5).abs() < 1e-9);
        assert!((at(Interp::Horizontal, 5.0) - 5.0).abs() < 1e-9);
        assert!(at(Interp::Horizontal, 1.0) < 1.0, "flat tangents ease in");
        // Two keys: Catmull-Rom slopes are the chord, so the curve is the straight line.
        assert!((at(Interp::CatmullRom, 2.5) - 2.5).abs() < 1e-9);
    }

    #[test]
    fn keys_replace_and_reject_nan() {
        let mut c = Curve::new();
        c.set_key(key(1, 1.0, Interp::Linear)).unwrap();
        c.set_key(key(1, 2.0, Interp::Linear)).unwrap();
        assert_eq!(c.keys().len(), 1);
        assert!(c.set_key(key(2, f64::NAN, Interp::Linear)).is_err());
        assert!(c.remove_key(FrameTime::from_frame(1)));
        assert!(!c.remove_key(FrameTime::from_frame(1)));
    }

    #[test]
    fn serde_is_compact() {
        let (c, _) = Curve::from_keys([key(1, 0.5, Interp::CatmullRom), key(2, 1.0, Interp::Constant)]);
        let s = serde_json::to_string(&c).unwrap();
        assert_eq!(s, r#"[{"t":1,"v":0.5},{"t":2,"v":1.0,"i":"constant"}]"#);
        let back: Curve = serde_json::from_str(&s).unwrap();
        assert_eq!(back, c);
    }
}
