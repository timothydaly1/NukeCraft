//! 2D geometry for NukeCraft.
//!
//! Coordinates follow Nuke's image space: the origin is the **bottom-left** corner of the
//! format, x grows to the right, y grows **up**, and pixel `(x, y)` covers the square
//! `[x, x+1) × [y, y+1)` with its centre at `(x + 0.5, y + 0.5)`.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::ops::{Add, Mul, Neg, Sub};

use serde::{Deserialize, Serialize};

/// The largest coordinate an integer box may hold. Keeps widths, heights and their products
/// far from overflow, however hostile the input.
pub const MAX_COORD: i32 = 1 << 24;

/// A 2D point or vector.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct V2 {
    pub x: f64,
    pub y: f64,
}

/// A 2D affine transform: `x' = a·x + b·y + c`, `y' = d·x + e·y + f`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

/// A 2D projective transform (CornerPin's): `(x', y') = ((m0·x + m1·y + m2) / w, (m3·x + m4·y + m5) / w)`
/// with `w = m6·x + m7·y + m8`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Homography {
    pub m: [f64; 9],
}

/// A half-open integer pixel box `[x0, x1) × [y0, y1)`: a node's bbox (data window) or a
/// region of interest. Empty when `x1 <= x0` or `y1 <= y0`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IRect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

/// An image format (Nuke's display window): a size and a pixel aspect ratio.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Format {
    pub width: u32,
    pub height: u32,
    #[serde(default = "one")]
    pub pixel_aspect: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
}

fn one() -> f64 {
    1.0
}

/// Named formats, by the names Nuke scripts use for them (width, height, pixel aspect).
pub const NAMED_FORMATS: &[(&str, u32, u32, f64)] = &[
    ("PC_Video", 640, 480, 1.0),
    ("NTSC", 720, 486, 0.91),
    ("PAL", 720, 576, 1.09),
    ("HD_720", 1280, 720, 1.0),
    ("HD_1080", 1920, 1080, 1.0),
    ("UHD_4K", 3840, 2160, 1.0),
    ("1K_Super_35(full-ap)", 1024, 778, 1.0),
    ("2K_Super_35(full-ap)", 2048, 1556, 1.0),
    ("4K_Super_35(full-ap)", 4096, 3112, 1.0),
    ("2K_DCP", 2048, 1080, 1.0),
    ("4K_DCP", 4096, 2160, 1.0),
    ("square_256", 256, 256, 1.0),
    ("square_512", 512, 512, 1.0),
    ("square_1K", 1024, 1024, 1.0),
    ("square_2K", 2048, 2048, 1.0),
];

impl V2 {
    pub const ZERO: V2 = V2 { x: 0.0, y: 0.0 };

    pub const fn new(x: f64, y: f64) -> V2 {
        V2 { x, y }
    }

    pub fn length(self) -> f64 {
        self.x.hypot(self.y)
    }

    pub fn dot(self, o: V2) -> f64 {
        self.x * o.x + self.y * o.y
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl Add for V2 {
    type Output = V2;
    fn add(self, o: V2) -> V2 {
        V2::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for V2 {
    type Output = V2;
    fn sub(self, o: V2) -> V2 {
        V2::new(self.x - o.x, self.y - o.y)
    }
}

impl Mul<f64> for V2 {
    type Output = V2;
    fn mul(self, s: f64) -> V2 {
        V2::new(self.x * s, self.y * s)
    }
}

impl Neg for V2 {
    type Output = V2;
    fn neg(self) -> V2 {
        V2::new(-self.x, -self.y)
    }
}

impl Affine {
    pub const IDENTITY: Affine = Affine { a: 1.0, b: 0.0, c: 0.0, d: 0.0, e: 1.0, f: 0.0 };

    pub fn translate(t: V2) -> Affine {
        Affine { c: t.x, f: t.y, ..Affine::IDENTITY }
    }

    pub fn scale(s: V2) -> Affine {
        Affine { a: s.x, e: s.y, ..Affine::IDENTITY }
    }

    /// Counter-clockwise rotation (y is up) by `degrees`.
    pub fn rotate_degrees(degrees: f64) -> Affine {
        let (s, c) = degrees.to_radians().sin_cos();
        Affine { a: c, b: -s, c: 0.0, d: s, e: c, f: 0.0 }
    }

    /// Shear: `x' = x + kx·y`, `y' = y + ky·x`.
    pub fn skew(kx: f64, ky: f64) -> Affine {
        Affine { b: kx, d: ky, ..Affine::IDENTITY }
    }

    /// `self ∘ o`: apply `o` first, then `self`.
    pub fn then_after(self, o: Affine) -> Affine {
        Affine {
            a: self.a * o.a + self.b * o.d,
            b: self.a * o.b + self.b * o.e,
            c: self.a * o.c + self.b * o.f + self.c,
            d: self.d * o.a + self.e * o.d,
            e: self.d * o.b + self.e * o.e,
            f: self.d * o.c + self.e * o.f + self.f,
        }
    }

    pub fn apply(self, p: V2) -> V2 {
        V2::new(self.a * p.x + self.b * p.y + self.c, self.d * p.x + self.e * p.y + self.f)
    }

    pub fn determinant(self) -> f64 {
        self.a * self.e - self.b * self.d
    }

    /// The inverse, or `None` when the transform is singular (zero scale) or not finite.
    pub fn inverse(self) -> Option<Affine> {
        let det = self.determinant();
        if !det.is_finite() || det.abs() < 1e-12 {
            return None;
        }
        let inv = Affine {
            a: self.e / det,
            b: -self.b / det,
            c: (self.b * self.f - self.e * self.c) / det,
            d: -self.d / det,
            e: self.a / det,
            f: (self.d * self.c - self.a * self.f) / det,
        };
        inv.is_finite().then_some(inv)
    }

    pub fn is_finite(self) -> bool {
        [self.a, self.b, self.c, self.d, self.e, self.f].iter().all(|v| v.is_finite())
    }
}

impl Default for Affine {
    fn default() -> Affine {
        Affine::IDENTITY
    }
}

impl Homography {
    pub const IDENTITY: Homography = Homography { m: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0] };

    /// The transform taking each `from[i]` to `to[i]`, or `None` when the quads are degenerate
    /// (three points in a line, or not finite).
    pub fn from_quads(from: [V2; 4], to: [V2; 4]) -> Option<Homography> {
        // Eight equations in m0..m7 (m8 = 1): for each pair,
        // x' (m6 x + m7 y + 1) = m0 x + m1 y + m2 and likewise for y'.
        let mut a = [[0.0f64; 9]; 8];
        for (i, (p, q)) in from.iter().zip(to.iter()).enumerate() {
            let (x, y, u, v) = (p.x, p.y, q.x, q.y);
            let r0 = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
            let r1 = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
            *a.get_mut(2 * i)? = r0;
            *a.get_mut(2 * i + 1)? = r1;
        }
        let sol = solve8(a)?;
        let h = Homography { m: [sol[0], sol[1], sol[2], sol[3], sol[4], sol[5], sol[6], sol[7], 1.0] };
        (h.is_finite() && h.determinant().abs() > 1e-12).then_some(h)
    }

    /// Where `p` maps, or `None` for a point on or behind the horizon (`w <= 0`).
    pub fn apply(self, p: V2) -> Option<V2> {
        let m = self.m;
        let w = m[6] * p.x + m[7] * p.y + m[8];
        if w.is_nan() || w <= 1e-12 {
            return None;
        }
        let q = V2::new((m[0] * p.x + m[1] * p.y + m[2]) / w, (m[3] * p.x + m[4] * p.y + m[5]) / w);
        q.is_finite().then_some(q)
    }

    /// The derivatives of the mapped x and y at `p`: `[[dx'/dx, dx'/dy], [dy'/dx, dy'/dy]]`.
    pub fn jacobian(self, p: V2) -> Option<[[f64; 2]; 2]> {
        let m = self.m;
        let w = m[6] * p.x + m[7] * p.y + m[8];
        if w.is_nan() || w <= 1e-12 {
            return None;
        }
        let q = self.apply(p)?;
        Some([[(m[0] - q.x * m[6]) / w, (m[1] - q.x * m[7]) / w], [(m[3] - q.y * m[6]) / w, (m[4] - q.y * m[7]) / w]])
    }

    pub fn determinant(self) -> f64 {
        let m = self.m;
        m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6]) + m[2] * (m[3] * m[7] - m[4] * m[6])
    }

    /// The inverse, scaled so `m8` is 1 where possible, or `None` when singular.
    pub fn inverse(self) -> Option<Homography> {
        let m = self.m;
        let det = self.determinant();
        if !det.is_finite() || det.abs() < 1e-12 {
            return None;
        }
        let adj = [
            m[4] * m[8] - m[5] * m[7],
            m[2] * m[7] - m[1] * m[8],
            m[1] * m[5] - m[2] * m[4],
            m[5] * m[6] - m[3] * m[8],
            m[0] * m[8] - m[2] * m[6],
            m[2] * m[3] - m[0] * m[5],
            m[3] * m[7] - m[4] * m[6],
            m[1] * m[6] - m[0] * m[7],
            m[0] * m[4] - m[1] * m[3],
        ];
        // Keep w positive for points in front: scale by 1/det's sign and magnitude.
        let inv = Homography { m: adj.map(|v| v / det) };
        inv.is_finite().then_some(inv)
    }

    pub fn is_finite(self) -> bool {
        self.m.iter().all(|v| v.is_finite())
    }
}

/// Solves an 8×8 linear system given as rows `[a0..a7 | b]` by Gaussian elimination with
/// partial pivoting; `None` when it is singular.
fn solve8(mut a: [[f64; 9]; 8]) -> Option<[f64; 8]> {
    for col in 0..8 {
        let pivot = (col..8).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
        let p = a.get(pivot)?.get(col)?.abs();
        if p.is_nan() || p <= 1e-12 {
            return None;
        }
        a.swap(col, pivot);
        let prow = *a.get(col)?;
        for row in 0..8 {
            if row == col {
                continue;
            }
            let r = a.get_mut(row)?;
            let f = r[col] / prow[col];
            for k in col..9 {
                r[k] -= f * prow[k];
            }
        }
    }
    let mut x = [0.0; 8];
    for (i, xi) in x.iter_mut().enumerate() {
        let r = a.get(i)?;
        *xi = r[8] / r[i];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

impl IRect {
    pub const EMPTY: IRect = IRect { x0: 0, y0: 0, x1: 0, y1: 0 };

    /// A box with every coordinate clamped to ±[`MAX_COORD`].
    pub fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> IRect {
        let c = |v: i32| v.clamp(-MAX_COORD, MAX_COORD);
        IRect { x0: c(x0), y0: c(y0), x1: c(x1), y1: c(y1) }
    }

    /// The smallest box holding every pixel that the real-valued box touches. Non-finite
    /// bounds give an empty box.
    pub fn enclosing(x0: f64, y0: f64, x1: f64, y1: f64) -> IRect {
        if ![x0, y0, x1, y1].iter().all(|v| v.is_finite()) {
            return IRect::EMPTY;
        }
        let c = |v: f64| v.clamp(-(MAX_COORD as f64), MAX_COORD as f64) as i32;
        IRect::new(c(x0.floor()), c(y0.floor()), c(x1.ceil()), c(y1.ceil()))
    }

    pub fn width(self) -> u32 {
        (self.x1 as i64 - self.x0 as i64).max(0) as u32
    }

    pub fn height(self) -> u32 {
        (self.y1 as i64 - self.y0 as i64).max(0) as u32
    }

    pub fn area(self) -> u64 {
        self.width() as u64 * self.height() as u64
    }

    pub fn is_empty(self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    pub fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }

    pub fn intersect(self, o: IRect) -> IRect {
        let r = IRect { x0: self.x0.max(o.x0), y0: self.y0.max(o.y0), x1: self.x1.min(o.x1), y1: self.y1.min(o.y1) };
        if r.is_empty() { IRect::EMPTY } else { r }
    }

    /// The smallest box holding both (an empty box adds nothing).
    pub fn union(self, o: IRect) -> IRect {
        if self.is_empty() {
            return o;
        }
        if o.is_empty() {
            return self;
        }
        IRect { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }

    /// Grown by `dx` and `dy` on each side (shrunk when negative), saturating.
    pub fn expand(self, dx: i32, dy: i32) -> IRect {
        if self.is_empty() {
            return self;
        }
        let r = IRect::new(self.x0.saturating_sub(dx), self.y0.saturating_sub(dy), self.x1.saturating_add(dx), self.y1.saturating_add(dy));
        if r.is_empty() { IRect::EMPTY } else { r }
    }

    /// The part of `bounds` needed to answer every read inside `self` when reads outside
    /// `bounds` repeat its edge pixels (Nuke's rule outside a bbox): `self ∩ bounds` when they
    /// overlap, otherwise the strip or corner of `bounds` nearest to `self`. Empty only when
    /// `self` or `bounds` is.
    pub fn clamp_into(self, bounds: IRect) -> IRect {
        if self.is_empty() || bounds.is_empty() {
            return IRect::EMPTY;
        }
        let cx = |v: i32| v.clamp(bounds.x0, bounds.x1 - 1);
        let cy = |v: i32| v.clamp(bounds.y0, bounds.y1 - 1);
        IRect { x0: cx(self.x0), y0: cy(self.y0), x1: cx(self.x1 - 1) + 1, y1: cy(self.y1 - 1) + 1 }
    }
}

impl Format {
    /// A format of the given size, each side at least 1 and at most [`MAX_COORD`], with a
    /// finite positive pixel aspect (else 1).
    pub fn new(width: u32, height: u32, pixel_aspect: f64) -> Format {
        let side = |v: u32| v.clamp(1, MAX_COORD as u32);
        let pa = if pixel_aspect.is_finite() && pixel_aspect > 0.0 { pixel_aspect } else { 1.0 };
        Format { width: side(width), height: side(height), pixel_aspect: pa, name: String::new() }
    }

    /// One of [`NAMED_FORMATS`], by name (case-insensitive).
    pub fn named(name: &str) -> Option<Format> {
        NAMED_FORMATS.iter().find(|(n, ..)| n.eq_ignore_ascii_case(name)).map(|&(n, w, h, pa)| Format { name: n.to_string(), ..Format::new(w, h, pa) })
    }

    /// Parses a format name or `WxH` / `WxH aspect` (e.g. `1920x1080`, `720 576 1.09`).
    pub fn parse(text: &str) -> Option<Format> {
        let text = text.trim();
        if let Some(f) = Format::named(text) {
            return Some(f);
        }
        let parts: Vec<&str> = text.split(['x', 'X', ' ']).filter(|s| !s.is_empty()).collect();
        let w = parts.first()?.parse::<u32>().ok()?;
        let h = parts.get(1)?.parse::<u32>().ok()?;
        let pa = match parts.get(2) {
            Some(p) => p.parse::<f64>().ok()?,
            None => 1.0,
        };
        (w > 0 && h > 0).then(|| Format::new(w, h, pa))
    }

    /// The display window as a pixel box at the origin.
    pub fn rect(&self) -> IRect {
        IRect::new(0, 0, self.width.min(MAX_COORD as u32) as i32, self.height.min(MAX_COORD as u32) as i32)
    }

    pub fn center(&self) -> V2 {
        V2::new(self.width as f64 / 2.0, self.height as f64 / 2.0)
    }
}

impl Default for Format {
    fn default() -> Format {
        Format { name: "HD_1080".into(), ..Format::new(1920, 1080, 1.0) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: V2, b: V2) -> bool {
        (a - b).length() < 1e-9
    }

    #[test]
    fn affine_composes_and_inverts() {
        let m = Affine::translate(V2::new(10.0, 5.0)).then_after(Affine::rotate_degrees(90.0)).then_after(Affine::scale(V2::new(2.0, 2.0)));
        let p = V2::new(1.0, 0.0);
        // scale → (2,0); rotate 90° CCW → (0,2); translate → (10,7)
        assert!(close(m.apply(p), V2::new(10.0, 7.0)));
        let inv = m.inverse().unwrap();
        assert!(close(inv.apply(m.apply(p)), p));
        assert!(Affine::scale(V2::new(0.0, 1.0)).inverse().is_none());
        assert!(Affine::scale(V2::new(f64::NAN, 1.0)).inverse().is_none());
    }

    #[test]
    fn homography_maps_quads_and_inverts() {
        let from = [V2::new(0.0, 0.0), V2::new(10.0, 0.0), V2::new(10.0, 10.0), V2::new(0.0, 10.0)];
        let to = [V2::new(1.0, 2.0), V2::new(21.0, 0.0), V2::new(18.0, 15.0), V2::new(3.0, 11.0)];
        let h = Homography::from_quads(from, to).unwrap();
        for (p, q) in from.iter().zip(to) {
            assert!(close(h.apply(*p).unwrap(), q));
        }
        let inv = h.inverse().unwrap();
        let p = V2::new(4.0, 7.0);
        assert!(close(inv.apply(h.apply(p).unwrap()).unwrap(), p));
        // The Jacobian matches finite differences.
        let j = h.jacobian(p).unwrap();
        let e = 1e-6;
        let dx = (h.apply(V2::new(p.x + e, p.y)).unwrap() - h.apply(p).unwrap()) * (1.0 / e);
        assert!((j[0][0] - dx.x).abs() < 1e-4 && (j[1][0] - dx.y).abs() < 1e-4);
        // An affine quad gives an affine homography.
        let shifted = from.map(|p| p + V2::new(5.0, -3.0));
        let t = Homography::from_quads(from, shifted).unwrap();
        assert!(close(t.apply(V2::new(2.0, 2.0)).unwrap(), V2::new(7.0, -1.0)));
        // Degenerate: three points in a line, or not finite.
        let line = [V2::new(0.0, 0.0), V2::new(1.0, 0.0), V2::new(2.0, 0.0), V2::new(3.0, 0.0)];
        assert!(Homography::from_quads(from, line).is_none());
        assert!(Homography::from_quads(from, [V2::new(f64::NAN, 0.0), to[1], to[2], to[3]]).is_none());
    }

    #[test]
    fn rect_ops() {
        let a = IRect::new(0, 0, 10, 10);
        let b = IRect::new(5, 5, 20, 20);
        assert_eq!(a.intersect(b), IRect::new(5, 5, 10, 10));
        assert_eq!(a.union(b), IRect::new(0, 0, 20, 20));
        assert_eq!(a.union(IRect::EMPTY), a);
        assert!(a.intersect(IRect::new(50, 50, 60, 60)).is_empty());
        assert_eq!(a.expand(2, 3), IRect::new(-2, -3, 12, 13));
        assert!(a.expand(-6, 0).is_empty());
        assert_eq!(IRect::enclosing(0.5, -0.5, 9.1, 3.0), IRect::new(0, -1, 10, 3));
        assert!(IRect::enclosing(f64::NAN, 0.0, 1.0, 1.0).is_empty());
    }

    #[test]
    fn clamp_into_covers_edge_repeats() {
        let bounds = IRect::new(0, 0, 10, 10);
        assert_eq!(IRect::new(5, 5, 20, 20).clamp_into(bounds), IRect::new(5, 5, 10, 10));
        // Entirely to the right: the rightmost column, rows clamped.
        assert_eq!(IRect::new(30, 2, 40, 4).clamp_into(bounds), IRect::new(9, 2, 10, 4));
        // Diagonal corner.
        assert_eq!(IRect::new(-30, -30, -20, -20).clamp_into(bounds), IRect::new(0, 0, 1, 1));
        assert!(IRect::new(0, 0, 5, 5).clamp_into(IRect::EMPTY).is_empty());
    }

    #[test]
    fn hostile_coordinates_saturate() {
        let r = IRect::new(i32::MIN, i32::MIN, i32::MAX, i32::MAX);
        assert_eq!(r.width(), 2 * MAX_COORD as u32);
        assert_eq!(r.expand(i32::MAX, i32::MAX).x0, -MAX_COORD);
        assert_eq!(Format::new(0, u32::MAX, f64::NAN), Format::new(1, MAX_COORD as u32, 1.0));
    }

    #[test]
    fn formats_parse() {
        assert_eq!(Format::parse("HD_1080").unwrap().width, 1920);
        assert_eq!(Format::parse("hd_720").unwrap().height, 720);
        let f = Format::parse("720x576 1.09").unwrap();
        assert_eq!((f.width, f.height, f.pixel_aspect), (720, 576, 1.09));
        assert!(Format::parse("0x10").is_none());
        assert!(Format::parse("wide").is_none());
    }
}
