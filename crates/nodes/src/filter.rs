//! Resampling filters for Transform and other warping nodes.
//!
//! Filter names follow Nuke's filter menu. The cubic family is the Mitchell–Netravali
//! two-parameter cubic (B, C); the (B, C) chosen for each name are NukeCraft's reading of Nuke's
//! descriptions ("smooth, no sharpening" … "most sharpening") and are listed for verification in
//! `docs/fidelity/README.md`. Notch, Lanczos and Sinc are not implemented yet.

/// A resampling filter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Filter {
    /// Nearest pixel, no filtering.
    Impulse,
    /// A Mitchell–Netravali cubic with parameters `(B, C)`.
    Cubic { b: f64, c: f64 },
}

/// The filter menu, in Nuke's order, with the names implemented so far.
pub const FILTER_NAMES: &[&str] = &["Impulse", "Cubic", "Keys", "Simon", "Rifman", "Mitchell", "Parzen"];

impl Filter {
    pub fn from_name(name: &str) -> Option<Filter> {
        Some(match name.to_ascii_lowercase().as_str() {
            "impulse" => Filter::Impulse,
            "cubic" => Filter::Cubic { b: 0.0, c: 0.0 },
            "keys" => Filter::Cubic { b: 0.0, c: 0.5 },
            "simon" => Filter::Cubic { b: 0.0, c: 0.75 },
            "rifman" => Filter::Cubic { b: 0.0, c: 1.0 },
            "mitchell" => Filter::Cubic { b: 1.0 / 3.0, c: 1.0 / 3.0 },
            "parzen" => Filter::Cubic { b: 1.0, c: 0.0 },
            _ => return None,
        })
    }

    /// The kernel's half-width in source pixels at unit scale.
    pub fn support(self) -> f64 {
        match self {
            Filter::Impulse => 0.5,
            Filter::Cubic { .. } => 2.0,
        }
    }

    /// The kernel at distance `x` (in units of the kernel scale).
    pub fn weight(self, x: f64) -> f64 {
        match self {
            Filter::Impulse => {
                if (-0.5..0.5).contains(&x) {
                    1.0
                } else {
                    0.0
                }
            }
            Filter::Cubic { b, c } => mitchell(x.abs(), b, c),
        }
    }
}

/// The Mitchell–Netravali cubic (SIGGRAPH 1988) at `x ≥ 0`.
fn mitchell(x: f64, b: f64, c: f64) -> f64 {
    let (x2, x3) = (x * x, x * x * x);
    let v = if x < 1.0 {
        (12.0 - 9.0 * b - 6.0 * c) * x3 + (-18.0 + 12.0 * b + 6.0 * c) * x2 + (6.0 - 2.0 * b)
    } else if x < 2.0 {
        (-b - 6.0 * c) * x3 + (6.0 * b + 30.0 * c) * x2 + (-12.0 * b - 48.0 * c) * x + (8.0 * b + 24.0 * c)
    } else {
        0.0
    };
    v / 6.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cubics_interpolate_or_approximate_as_expected() {
        for name in FILTER_NAMES {
            let f = Filter::from_name(name).unwrap();
            // Partition of unity at any offset.
            for off in [0.0, 0.25, 0.5] {
                let sum: f64 = (-3..=3).map(|i| f.weight(i as f64 + off)).sum();
                assert!((sum - 1.0).abs() < 1e-9, "{name} at {off}: {sum}");
            }
        }
        // B = 0 cubics interpolate: weight 1 at 0, 0 at integers.
        let keys = Filter::from_name("keys").unwrap();
        assert!((keys.weight(0.0) - 1.0).abs() < 1e-12);
        assert!(keys.weight(1.0).abs() < 1e-12);
        assert!(Filter::from_name("lanczos4").is_none());
    }
}
