//! Colour for NukeCraft: the transfer functions Read and Write convert through, Rec. 709
//! luminance, and the Merge node's operations.
//!
//! NukeCraft works the way Nuke does: pixels are 32-bit float, **linear light**, with
//! **premultiplied** alpha. Read nodes convert file values into linear; Write nodes convert back.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use serde::{Deserialize, Serialize};

/// Rec. 709 luma weights, applied to linear values (Nuke's Saturation and luminance math).
pub const REC709_LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// The colour spaces of Nuke's default (non-OCIO) colour management that NukeCraft implements.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Colorspace {
    #[serde(rename = "linear")]
    Linear,
    #[serde(rename = "sRGB")]
    Srgb,
    #[serde(rename = "rec709")]
    Rec709,
    #[serde(rename = "Gamma1.8")]
    Gamma18,
    #[serde(rename = "Gamma2.2")]
    Gamma22,
    #[serde(rename = "Cineon")]
    Cineon,
}

/// A Merge node operation. Names and formulas follow Nuke's published Merge operation table,
/// where `A`/`B` are the A and B inputs' values of a channel and `a`/`b` their alphas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MergeOp {
    Atop,
    Average,
    ColorBurn,
    ColorDodge,
    ConjointOver,
    Copy,
    Difference,
    DisjointOver,
    Divide,
    Exclusion,
    From,
    Geometric,
    HardLight,
    Hypot,
    In,
    Mask,
    Matte,
    Max,
    Min,
    Minus,
    Multiply,
    Out,
    Over,
    Overlay,
    Plus,
    Screen,
    Stencil,
    Under,
    Xor,
}

impl Colorspace {
    pub const ALL: [Colorspace; 6] = [Colorspace::Linear, Colorspace::Srgb, Colorspace::Rec709, Colorspace::Gamma18, Colorspace::Gamma22, Colorspace::Cineon];

    /// The name a script or a Read/Write `colorspace` knob uses.
    pub fn name(self) -> &'static str {
        match self {
            Colorspace::Linear => "linear",
            Colorspace::Srgb => "sRGB",
            Colorspace::Rec709 => "rec709",
            Colorspace::Gamma18 => "Gamma1.8",
            Colorspace::Gamma22 => "Gamma2.2",
            Colorspace::Cineon => "Cineon",
        }
    }

    pub fn from_name(name: &str) -> Option<Colorspace> {
        Colorspace::ALL.into_iter().find(|c| c.name().eq_ignore_ascii_case(name))
    }

    /// File value → linear light.
    pub fn to_linear(self, v: f32) -> f32 {
        match self {
            Colorspace::Linear => v,
            Colorspace::Srgb => srgb_to_linear(v),
            Colorspace::Rec709 => rec709_to_linear(v),
            Colorspace::Gamma18 => signed_pow(v, 1.8),
            Colorspace::Gamma22 => signed_pow(v, 2.2),
            Colorspace::Cineon => cineon_to_linear(v),
        }
    }

    /// Linear light → file value.
    pub fn from_linear(self, v: f32) -> f32 {
        match self {
            Colorspace::Linear => v,
            Colorspace::Srgb => linear_to_srgb(v),
            Colorspace::Rec709 => linear_to_rec709(v),
            Colorspace::Gamma18 => signed_pow(v, 1.0 / 1.8),
            Colorspace::Gamma22 => signed_pow(v, 1.0 / 2.2),
            Colorspace::Cineon => linear_to_cineon(v),
        }
    }
}

/// The IEC 61966-2-1 sRGB decoding function (mirrored for negative values).
pub fn srgb_to_linear(v: f32) -> f32 {
    let a = v.abs();
    let l = if a <= 0.04045 { a / 12.92 } else { ((a + 0.055) / 1.055).powf(2.4) };
    l.copysign(v)
}

/// The IEC 61966-2-1 sRGB encoding function (mirrored for negative values).
pub fn linear_to_srgb(v: f32) -> f32 {
    let a = v.abs();
    let e = if a <= 0.003_130_8 { a * 12.92 } else { 1.055 * a.powf(1.0 / 2.4) - 0.055 };
    e.copysign(v)
}

/// The ITU-R BT.709 OETF inverse.
pub fn rec709_to_linear(v: f32) -> f32 {
    let a = v.abs();
    let l = if a < 0.081 { a / 4.5 } else { ((a + 0.099) / 1.099).powf(1.0 / 0.45) };
    l.copysign(v)
}

/// The ITU-R BT.709 OETF.
pub fn linear_to_rec709(v: f32) -> f32 {
    let a = v.abs();
    let e = if a < 0.018 { a * 4.5 } else { 1.099 * a.powf(0.45) - 0.099 };
    e.copysign(v)
}

// Kodak Cineon printing-density log encoding, with the conventional 95 black / 685 white code
// values and a 0.6 negative gamma (10-bit code values, normalised to 0‥1 in the image).
const CINEON_BLACK: f32 = 95.0;
const CINEON_WHITE: f32 = 685.0;
const CINEON_GAMMA: f32 = 0.6;

fn cineon_offset() -> f32 {
    10f32.powf((CINEON_BLACK - CINEON_WHITE) * 0.002 / CINEON_GAMMA)
}

/// Cineon log (0‥1) → linear.
pub fn cineon_to_linear(v: f32) -> f32 {
    let code = v * 1023.0;
    let off = cineon_offset();
    (10f32.powf((code - CINEON_WHITE) * 0.002 / CINEON_GAMMA) - off) / (1.0 - off)
}

/// Linear → Cineon log (0‥1). Values at or below the log of zero clamp to code 0.
pub fn linear_to_cineon(v: f32) -> f32 {
    let off = cineon_offset();
    let x = v * (1.0 - off) + off;
    if x <= 0.0 {
        return 0.0;
    }
    (CINEON_WHITE + x.log10() * CINEON_GAMMA / 0.002) / 1023.0
}

fn signed_pow(v: f32, e: f32) -> f32 {
    v.abs().powf(e).copysign(v)
}

/// Rec. 709 luminance of linear RGB.
pub fn luminance(r: f32, g: f32, b: f32) -> f32 {
    REC709_LUMA[0] * r + REC709_LUMA[1] * g + REC709_LUMA[2] * b
}

impl MergeOp {
    pub const ALL: [MergeOp; 29] = [
        MergeOp::Atop,
        MergeOp::Average,
        MergeOp::ColorBurn,
        MergeOp::ColorDodge,
        MergeOp::ConjointOver,
        MergeOp::Copy,
        MergeOp::Difference,
        MergeOp::DisjointOver,
        MergeOp::Divide,
        MergeOp::Exclusion,
        MergeOp::From,
        MergeOp::Geometric,
        MergeOp::HardLight,
        MergeOp::Hypot,
        MergeOp::In,
        MergeOp::Mask,
        MergeOp::Matte,
        MergeOp::Max,
        MergeOp::Min,
        MergeOp::Minus,
        MergeOp::Multiply,
        MergeOp::Out,
        MergeOp::Over,
        MergeOp::Overlay,
        MergeOp::Plus,
        MergeOp::Screen,
        MergeOp::Stencil,
        MergeOp::Under,
        MergeOp::Xor,
    ];

    /// The name the Merge node's `operation` knob uses.
    pub fn name(self) -> &'static str {
        match self {
            MergeOp::Atop => "atop",
            MergeOp::Average => "average",
            MergeOp::ColorBurn => "color-burn",
            MergeOp::ColorDodge => "color-dodge",
            MergeOp::ConjointOver => "conjoint-over",
            MergeOp::Copy => "copy",
            MergeOp::Difference => "difference",
            MergeOp::DisjointOver => "disjoint-over",
            MergeOp::Divide => "divide",
            MergeOp::Exclusion => "exclusion",
            MergeOp::From => "from",
            MergeOp::Geometric => "geometric",
            MergeOp::HardLight => "hard-light",
            MergeOp::Hypot => "hypot",
            MergeOp::In => "in",
            MergeOp::Mask => "mask",
            MergeOp::Matte => "matte",
            MergeOp::Max => "max",
            MergeOp::Min => "min",
            MergeOp::Minus => "minus",
            MergeOp::Multiply => "multiply",
            MergeOp::Out => "out",
            MergeOp::Over => "over",
            MergeOp::Overlay => "overlay",
            MergeOp::Plus => "plus",
            MergeOp::Screen => "screen",
            MergeOp::Stencil => "stencil",
            MergeOp::Under => "under",
            MergeOp::Xor => "xor",
        }
    }

    pub fn from_name(name: &str) -> Option<MergeOp> {
        MergeOp::ALL.into_iter().find(|m| m.name().eq_ignore_ascii_case(name))
    }

    /// One channel of the operation: `ca`/`cb` are the A and B values of the channel, `a`/`b`
    /// the A and B alphas. The alpha channel itself goes through the same formula with
    /// `ca = a`, `cb = b`. Never returns NaN for finite inputs (divisions by zero give 0 or the
    /// value Nuke's table states).
    pub fn apply(self, ca: f32, cb: f32, a: f32, b: f32) -> f32 {
        let r = match self {
            MergeOp::Atop => ca * b + cb * (1.0 - a),
            MergeOp::Average => (ca + cb) / 2.0,
            MergeOp::ColorBurn => {
                if ca <= 0.0 {
                    0.0
                } else {
                    1.0 - (1.0 - cb) / ca
                }
            }
            MergeOp::ColorDodge => {
                if ca >= 1.0 {
                    if cb > 0.0 { 1.0 } else { 0.0 }
                } else {
                    cb / (1.0 - ca)
                }
            }
            MergeOp::ConjointOver => {
                if a > b {
                    ca
                } else if b > 0.0 {
                    ca + cb * (1.0 - a) / b
                } else {
                    ca + cb
                }
            }
            MergeOp::Copy => ca,
            MergeOp::Difference => (ca - cb).abs(),
            MergeOp::DisjointOver => {
                if a + b < 1.0 {
                    ca + cb
                } else if b > 0.0 {
                    ca + cb * (1.0 - a) / b
                } else {
                    ca
                }
            }
            MergeOp::Divide => {
                if (ca < 0.0 && cb < 0.0) || cb == 0.0 {
                    0.0
                } else {
                    ca / cb
                }
            }
            MergeOp::Exclusion => ca + cb - 2.0 * ca * cb,
            MergeOp::From => cb - ca,
            MergeOp::Geometric => {
                let s = ca + cb;
                if s == 0.0 { 0.0 } else { 2.0 * ca * cb / s }
            }
            MergeOp::HardLight => {
                if ca < 0.5 {
                    2.0 * ca * cb
                } else {
                    let (x, y) = (2.0 * ca - 1.0, cb);
                    screen(x, y)
                }
            }
            MergeOp::Hypot => ca.hypot(cb),
            MergeOp::In => ca * b,
            MergeOp::Mask => cb * a,
            MergeOp::Matte => ca * a + cb * (1.0 - a),
            MergeOp::Max => ca.max(cb),
            MergeOp::Min => ca.min(cb),
            MergeOp::Minus => ca - cb,
            MergeOp::Multiply => {
                if ca < 0.0 && cb < 0.0 {
                    ca
                } else {
                    ca * cb
                }
            }
            MergeOp::Out => ca * (1.0 - b),
            MergeOp::Over => ca + cb * (1.0 - a),
            MergeOp::Overlay => {
                if cb < 0.5 {
                    2.0 * ca * cb
                } else {
                    screen(ca, 2.0 * cb - 1.0)
                }
            }
            MergeOp::Plus => ca + cb,
            MergeOp::Screen => screen(ca, cb),
            MergeOp::Stencil => cb * (1.0 - a),
            MergeOp::Under => ca * (1.0 - b) + cb,
            MergeOp::Xor => ca * (1.0 - b) + cb * (1.0 - a),
        };
        if r.is_nan() { 0.0 } else { r }
    }
}

/// `A + B − AB` when both are within 0‥1, else `max(A, B)` (Nuke's screen).
fn screen(a: f32, b: f32) -> f32 {
    if (0.0..=1.0).contains(&a) && (0.0..=1.0).contains(&b) { a + b - a * b } else { a.max(b) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_functions_round_trip() {
        for cs in Colorspace::ALL {
            for v in [0.0f32, 0.001, 0.018, 0.18, 0.5, 1.0, 2.0] {
                let back = cs.to_linear(cs.from_linear(v));
                assert!((back - v).abs() < 1e-4 * v.max(1.0), "{} {v} → {back}", cs.name());
            }
        }
        assert!((srgb_to_linear(0.5) - 0.214_041).abs() < 1e-5);
        assert_eq!(srgb_to_linear(-0.5), -srgb_to_linear(0.5));
    }

    #[test]
    fn cineon_reference_points() {
        // Code 685 is linear 1, code 95 is linear 0.
        assert!((cineon_to_linear(685.0 / 1023.0) - 1.0).abs() < 1e-5);
        assert!(cineon_to_linear(95.0 / 1023.0).abs() < 1e-5);
        assert!(linear_to_cineon(-10.0).is_finite());
    }

    #[test]
    fn merge_formulas() {
        let (ca, cb, a, b) = (0.6, 0.4, 0.5, 1.0);
        assert!((MergeOp::Over.apply(ca, cb, a, b) - 0.8).abs() < 1e-6);
        assert!((MergeOp::Under.apply(ca, cb, a, b) - 0.4).abs() < 1e-6);
        assert!((MergeOp::Plus.apply(ca, cb, a, b) - 1.0).abs() < 1e-6);
        assert!((MergeOp::Screen.apply(ca, cb, a, b) - 0.76).abs() < 1e-6);
        assert!((MergeOp::Screen.apply(2.0, 0.5, a, b) - 2.0).abs() < 1e-6, "screen is max outside 0‥1");
        assert!((MergeOp::In.apply(ca, cb, a, b) - 0.6).abs() < 1e-6);
        assert!((MergeOp::Stencil.apply(ca, cb, a, b) - 0.2).abs() < 1e-6);
        assert_eq!(MergeOp::Multiply.apply(-1.0, -2.0, a, b), -1.0);
        assert_eq!(MergeOp::Divide.apply(1.0, 0.0, a, b), 0.0);
    }

    #[test]
    fn merge_never_returns_nan() {
        let vals = [0.0f32, 1.0, -1.0, 0.5, f32::MAX];
        for op in MergeOp::ALL {
            for &ca in &vals {
                for &cb in &vals {
                    for &a in &vals {
                        for &b in &vals {
                            assert!(!op.apply(ca, cb, a, b).is_nan(), "{} {ca} {cb} {a} {b}", op.name());
                        }
                    }
                }
            }
            assert_eq!(MergeOp::from_name(op.name()), Some(op));
        }
    }
}
