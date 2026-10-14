//! Exact frame time for NukeCraft.
//!
//! A compositing script lives in **frames**, as Nuke's does: keys sit on frame numbers, the root
//! holds a frame range, and changing the frame rate never moves a key. Motion blur and retiming
//! need fractions of a frame, so a time is an integer number of subframes, [`SUBFRAMES_PER_FRAME`]
//! = 720 720 per frame (the least common multiple of 1‥16): halves, thirds, … sixteenths of a frame
//! and every shutter offset built from them are exact (`plan/adr/0001-frame-time.md`).
//!
//! [`Tick`] (254 016 000 000 per second, the time base FilmCraft and EffectCraft share) converts
//! whole frames exactly at every broadcast rate, for exchanging media with the sibling apps.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::fmt;
use std::ops::{Add, Sub};

use serde::{Deserialize, Serialize};

/// Subframes per frame: lcm(1, 2, …, 16).
pub const SUBFRAMES_PER_FRAME: i64 = 720_720;

/// The largest frame number a time may hold (keeps every product below `i64::MAX`).
pub const MAX_FRAME: i64 = 1_000_000_000;

/// The most frames one frame-range expression may expand to.
pub const MAX_RANGE_FRAMES: usize = 1_000_000;

/// Ticks per second, shared with FilmCraft and EffectCraft.
pub const TICKS_PER_SECOND: i64 = 254_016_000_000;

/// A point in script time, in subframes. Files hold it as a frame number (`12`, `12.5`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FrameTime(pub i64);

/// A media time in ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Tick(pub i64);

/// A frame rate as a ratio of positive integers (24000/1001 for 23.976).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Fps {
    pub num: u32,
    pub den: u32,
}

/// An inclusive range of whole frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FrameRange {
    pub first: i64,
    pub last: i64,
}

/// Why a frame-range expression could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RangeError(pub String);

impl FrameTime {
    pub const ZERO: FrameTime = FrameTime(0);

    /// A whole frame, clamped to ±[`MAX_FRAME`].
    pub fn from_frame(frame: i64) -> FrameTime {
        FrameTime(frame.clamp(-MAX_FRAME, MAX_FRAME) * SUBFRAMES_PER_FRAME)
    }

    /// A fractional frame, rounded to the nearest subframe and clamped (NaN is frame 0), so
    /// values from files, users and agents can't overflow later arithmetic.
    pub fn from_f64(frame: f64) -> FrameTime {
        if !frame.is_finite() {
            return if frame.is_nan() { FrameTime::ZERO } else { FrameTime::from_frame(if frame > 0.0 { MAX_FRAME } else { -MAX_FRAME }) };
        }
        let f = frame.clamp(-(MAX_FRAME as f64), MAX_FRAME as f64);
        FrameTime((f * SUBFRAMES_PER_FRAME as f64).round() as i64)
    }

    /// The time as a fractional frame number.
    pub fn as_f64(self) -> f64 {
        self.0 as f64 / SUBFRAMES_PER_FRAME as f64
    }

    /// The whole frame at or before this time.
    pub fn floor_frame(self) -> i64 {
        self.0.div_euclid(SUBFRAMES_PER_FRAME)
    }

    /// Whether the time sits exactly on a frame.
    pub fn is_whole(self) -> bool {
        self.0.rem_euclid(SUBFRAMES_PER_FRAME) == 0
    }

    /// The media time of this script time at `fps`. Whole frames are exact at every rate whose
    /// frame duration divides [`TICKS_PER_SECOND`]; subframes round to the nearest tick.
    pub fn to_tick(self, fps: Fps) -> Tick {
        let fps = fps.sanitized();
        let num = self.0 as i128 * TICKS_PER_SECOND as i128 * fps.den as i128;
        let den = SUBFRAMES_PER_FRAME as i128 * fps.num as i128;
        let q = num.div_euclid(den);
        let r = num.rem_euclid(den);
        let q = if 2 * r >= den { q + 1 } else { q };
        Tick(q.clamp(i64::MIN as i128 / 4, i64::MAX as i128 / 4) as i64)
    }
}

impl Serialize for FrameTime {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if self.is_whole() { s.serialize_i64(self.floor_frame()) } else { s.serialize_f64(self.as_f64()) }
    }
}

impl<'de> Deserialize<'de> for FrameTime {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<FrameTime, D::Error> {
        // Frame numbers round-trip exactly through f64: |frame| ≤ MAX_FRAME keeps
        // subframe counts below 2^53.
        Ok(FrameTime::from_f64(f64::deserialize(d)?))
    }
}

impl Add for FrameTime {
    type Output = FrameTime;
    fn add(self, rhs: FrameTime) -> FrameTime {
        FrameTime(self.0.saturating_add(rhs.0))
    }
}

impl Sub for FrameTime {
    type Output = FrameTime;
    fn sub(self, rhs: FrameTime) -> FrameTime {
        FrameTime(self.0.saturating_sub(rhs.0))
    }
}

impl fmt::Display for FrameTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_whole() { write!(f, "{}", self.floor_frame()) } else { write!(f, "{}", self.as_f64()) }
    }
}

impl Fps {
    pub const FILM: Fps = Fps { num: 24, den: 1 };
    pub const FILM_NTSC: Fps = Fps { num: 24000, den: 1001 };
    pub const PAL: Fps = Fps { num: 25, den: 1 };
    pub const NTSC: Fps = Fps { num: 30000, den: 1001 };

    /// A rate from a decimal number, recognising the NTSC rates (23.976, 29.97, 47.952, 59.94,
    /// 119.88). Returns `None` for non-finite, non-positive or absurd (> 10 000) rates.
    pub fn from_f64(fps: f64) -> Option<Fps> {
        if !fps.is_finite() || fps <= 0.0 || fps > 10_000.0 {
            return None;
        }
        for base in [24u32, 30, 48, 60, 120] {
            let ntsc = base as f64 * 1000.0 / 1001.0;
            if (fps - ntsc).abs() < 0.005 {
                return Some(Fps { num: base * 1000, den: 1001 });
            }
        }
        if (fps - fps.round()).abs() < 1e-9 {
            return Some(Fps { num: fps.round() as u32, den: 1 });
        }
        Some(Fps { num: (fps * 1000.0).round() as u32, den: 1000 })
    }

    pub fn as_f64(self) -> f64 {
        let s = self.sanitized();
        s.num as f64 / s.den as f64
    }

    /// The rate with a zero numerator or denominator replaced by 24 fps.
    pub fn sanitized(self) -> Fps {
        if self.num == 0 || self.den == 0 { Fps::FILM } else { self }
    }
}

impl Default for Fps {
    fn default() -> Fps {
        Fps::FILM
    }
}

impl FrameRange {
    /// A range with `first <= last` (swapped if needed) and both ends clamped to ±[`MAX_FRAME`].
    pub fn new(first: i64, last: i64) -> FrameRange {
        let (a, b) = (first.clamp(-MAX_FRAME, MAX_FRAME), last.clamp(-MAX_FRAME, MAX_FRAME));
        FrameRange { first: a.min(b), last: a.max(b) }
    }

    pub fn len(self) -> u64 {
        (self.last - self.first).unsigned_abs() + 1
    }

    pub fn is_empty(self) -> bool {
        false
    }

    pub fn contains(self, frame: i64) -> bool {
        (self.first..=self.last).contains(&frame)
    }

    pub fn clamp(self, frame: i64) -> i64 {
        frame.clamp(self.first, self.last)
    }
}

impl Default for FrameRange {
    fn default() -> FrameRange {
        FrameRange { first: 1, last: 100 }
    }
}

impl fmt::Display for RangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RangeError {}

/// Expands a frame-range expression in Nuke's render-dialog syntax: comma- or space-separated
/// items, each `N`, `A-B` or `A-BxS` (every S-th frame), with negative frames allowed
/// (`-5--1`). Frames keep the order written; at most [`MAX_RANGE_FRAMES`] frames.
pub fn parse_frames(expr: &str) -> Result<Vec<i64>, RangeError> {
    let mut out = Vec::new();
    for item in expr.split([',', ' ']).map(str::trim).filter(|s| !s.is_empty()) {
        let (span, step) = match item.split_once(['x', 'X']) {
            Some((span, step)) => (span, step.trim().parse::<i64>().map_err(|_| RangeError(format!("bad step in `{item}`")))?),
            None => (item, 1),
        };
        if step <= 0 {
            return Err(RangeError(format!("step must be positive in `{item}`")));
        }
        let (a, b) = split_span(span).ok_or_else(|| RangeError(format!("bad frame range `{item}`")))?;
        let (a, b) = (a.clamp(-MAX_FRAME, MAX_FRAME), b.clamp(-MAX_FRAME, MAX_FRAME));
        let dir = if b >= a { 1 } else { -1 };
        let mut f = a;
        loop {
            if out.len() >= MAX_RANGE_FRAMES {
                return Err(RangeError(format!("more than {MAX_RANGE_FRAMES} frames")));
            }
            out.push(f);
            let next = f.saturating_add(dir * step);
            if (dir > 0 && next > b) || (dir < 0 && next < b) || next == f {
                break;
            }
            f = next;
        }
    }
    if out.is_empty() {
        return Err(RangeError("no frames".into()));
    }
    Ok(out)
}

/// `N` or `A-B`, where either end may be negative.
fn split_span(span: &str) -> Option<(i64, i64)> {
    let span = span.trim();
    if let Ok(n) = span.parse::<i64>() {
        return Some((n, n));
    }
    // The separator is the first '-' that follows a digit.
    let bytes = span.as_bytes();
    let pos = (1..bytes.len()).find(|&i| bytes.get(i) == Some(&b'-') && bytes.get(i - 1).is_some_and(u8::is_ascii_digit))?;
    let a = span.get(..pos)?.trim().parse().ok()?;
    let b = span.get(pos + 1..)?.trim().parse().ok()?;
    Some((a, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subframes_are_exact() {
        let third = FrameTime(SUBFRAMES_PER_FRAME / 3);
        assert_eq!(third.0 * 3, SUBFRAMES_PER_FRAME);
        for n in 1..=16 {
            assert_eq!(SUBFRAMES_PER_FRAME % n, 0, "1/{n} frame is exact");
        }
        assert_eq!(FrameTime::from_f64(1.5).to_string(), "1.5");
        assert_eq!(FrameTime::from_frame(-3).floor_frame(), -3);
        assert_eq!(FrameTime::from_f64(-2.5).floor_frame(), -3);
    }

    #[test]
    fn hostile_numbers_are_clamped() {
        assert_eq!(FrameTime::from_f64(f64::NAN), FrameTime::ZERO);
        assert_eq!(FrameTime::from_f64(f64::INFINITY), FrameTime::from_frame(MAX_FRAME));
        assert_eq!(FrameTime::from_frame(i64::MIN), FrameTime::from_frame(-MAX_FRAME));
        assert_eq!(FrameTime(i64::MAX) + FrameTime(1), FrameTime(i64::MAX));
        assert_eq!(Fps { num: 0, den: 0 }.as_f64(), 24.0);
        assert!(Fps::from_f64(f64::NAN).is_none());
    }

    #[test]
    fn whole_frames_convert_to_exact_ticks() {
        assert_eq!(FrameTime::from_frame(24).to_tick(Fps::FILM), Tick(TICKS_PER_SECOND));
        assert_eq!(FrameTime::from_frame(24000).to_tick(Fps::FILM_NTSC), Tick(1001 * TICKS_PER_SECOND));
        assert_eq!(FrameTime::from_frame(25).to_tick(Fps::PAL), Tick(TICKS_PER_SECOND));
    }

    #[test]
    fn ntsc_rates_are_recognised() {
        assert_eq!(Fps::from_f64(23.976), Some(Fps::FILM_NTSC));
        assert_eq!(Fps::from_f64(29.97), Some(Fps::NTSC));
        assert_eq!(Fps::from_f64(25.0), Some(Fps::PAL));
    }

    #[test]
    fn serde_uses_frame_numbers() {
        assert_eq!(serde_json::to_string(&FrameTime::from_frame(12)).unwrap(), "12");
        assert_eq!(serde_json::to_string(&FrameTime::from_f64(12.5)).unwrap(), "12.5");
        let third = FrameTime(SUBFRAMES_PER_FRAME * 7 + SUBFRAMES_PER_FRAME / 3);
        let back: FrameTime = serde_json::from_str(&serde_json::to_string(&third).unwrap()).unwrap();
        assert_eq!(back, third, "subframes round-trip exactly");
        let big = FrameTime(MAX_FRAME * SUBFRAMES_PER_FRAME - 1);
        let back: FrameTime = serde_json::from_str(&serde_json::to_string(&big).unwrap()).unwrap();
        assert_eq!(back, big);
    }

    #[test]
    fn frame_range_expressions() {
        assert_eq!(parse_frames("1-5").unwrap(), vec![1, 2, 3, 4, 5]);
        assert_eq!(parse_frames("1-10x3").unwrap(), vec![1, 4, 7, 10]);
        assert_eq!(parse_frames("7, 3 10-11").unwrap(), vec![7, 3, 10, 11]);
        assert_eq!(parse_frames("-2--4").unwrap(), vec![-2, -3, -4]);
        assert_eq!(parse_frames("5-3").unwrap(), vec![5, 4, 3]);
        assert!(parse_frames("").is_err());
        assert!(parse_frames("1-10x0").is_err());
        assert!(parse_frames("a-b").is_err());
        assert!(parse_frames("1-999999999").is_err(), "too many frames is an error, not an allocation");
    }
}
