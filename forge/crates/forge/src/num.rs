//! Deterministic maths from basic IEEE operations only (+, −, ×, ÷, √), so a stage gives the
//! same bits on every platform, whatever its libm (D-096).

/// cos of an angle in degrees from basic operations: reduced to [−90°, 90°], Taylor to x^20.
pub(crate) fn cos_deg(deg: f64) -> f64 {
    let x = deg.abs() % 360.0;
    let x = if x > 180.0 { 360.0 - x } else { x };
    let (x, sign) = if x > 90.0 {
        (180.0 - x, -1.0)
    } else {
        (x, 1.0)
    };
    let r = x * std::f64::consts::PI / 180.0;
    let (mut term, mut sum) = (1.0, 1.0);
    for i in 1..=10_u8 {
        let n = f64::from(2 * i);
        term *= -r * r / (n * (n - 1.0));
        sum += term;
    }
    sign * sum
}

/// sin of an angle in degrees, as [`cos_deg`] of its complement.
pub(crate) fn sin_deg(deg: f64) -> f64 {
    cos_deg(90.0 - deg)
}

#[cfg(test)]
mod tests {
    use super::{cos_deg, sin_deg};

    #[test]
    fn matches_libm_closely() {
        for d in [
            -720.5, -180.0, -91.0, -45.0, 0.0, 1e-9, 28.3, 60.0, 89.999, 90.0, 135.0, 359.0,
        ] {
            assert!((cos_deg(d) - d.to_radians().cos()).abs() < 1e-15, "cos {d}");
            assert!((sin_deg(d) - d.to_radians().sin()).abs() < 1e-15, "sin {d}");
        }
    }
}
