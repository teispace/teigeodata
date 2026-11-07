//! Positions on a fixed integer grid (ADR-0029).
//!
//! Every coordinate is quantised once, at ingest, and every later step works on the integers.
//! [`E7`] (1e-7°, about 1.1 cm) is the grid of geometry; [`E6`] (1e-6°, about 11 cm) is the grid
//! of place points (S1). Mixing grids is a type error; [`Point::to_e7`] converts exactly and
//! [`Point::to_e6`] rounds.

use core::fmt;
use core::marker::PhantomData;

mod sealed {
    pub trait Sealed {}
}

/// A coordinate grid: the number of units per degree.
pub trait Grid: sealed::Sealed + Copy + fmt::Debug + Eq + Ord + core::hash::Hash {
    /// Units per degree.
    const PER_DEGREE: i32;
    /// The number of decimal digits after the degree, for messages.
    const DIGITS: u8;
}

/// The 1e-6° grid of place points.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum E6 {}
/// The 1e-7° grid of geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum E7 {}

impl sealed::Sealed for E6 {}
impl sealed::Sealed for E7 {}
impl Grid for E6 {
    const PER_DEGREE: i32 = 1_000_000;
    const DIGITS: u8 = 6;
}
impl Grid for E7 {
    const PER_DEGREE: i32 = 10_000_000;
    const DIGITS: u8 = 7;
}

/// A latitude and longitude on grid `G`, always in range: latitude −90° to 90°, longitude
/// −180° to 180°.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Point<G: Grid> {
    lat: i32,
    lon: i32,
    grid: PhantomData<G>,
}

/// Why degrees or grid units are not a point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum GridError {
    /// A coordinate is NaN or infinite.
    NotFinite,
    /// The latitude is outside −90° to 90°.
    Latitude,
    /// The longitude is outside −180° to 180°.
    Longitude,
}

impl fmt::Display for GridError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotFinite => "a coordinate is not a finite number",
            Self::Latitude => "the latitude is outside -90 to 90 degrees",
            Self::Longitude => "the longitude is outside -180 to 180 degrees",
        })
    }
}

impl core::error::Error for GridError {}

/// Rounds `deg × per_degree` to the nearest integer, halves away from zero, exactly.
///
/// `x + 0.5` then truncation is wrong for values such as 0.499999999999999944 (the sum rounds up
/// to 1.0). Splitting into the truncated integer and its exact fraction is not: for
/// `|v| < 2^31` both parts are exact in `f64`.
fn round_units(deg: f64, per_degree: i32, limit: i32) -> Option<i32> {
    let v = deg * f64::from(per_degree);
    let max = f64::from(limit) * f64::from(per_degree);
    // The tolerance of half a unit lets 90.00000004 round to 90 at 1e-7 and stay in range.
    if v.abs() > max + 0.5 {
        return None;
    }
    // |v| ≤ 1.8e9 + 0.5 here, so the cast is exact truncation toward zero.
    #[allow(clippy::cast_possible_truncation, reason = "range checked above")]
    let t = v as i64;
    #[allow(clippy::cast_precision_loss, reason = "t is below 2^31, exact in f64")]
    let frac = v - t as f64;
    let r = if frac >= 0.5 {
        t + 1
    } else if frac <= -0.5 {
        t - 1
    } else {
        t
    };
    let bound = i64::from(limit) * i64::from(per_degree);
    if r.abs() <= bound {
        i32::try_from(r).ok()
    } else {
        None
    }
}

impl<G: Grid> Point<G> {
    const LAT_MAX: i32 = 90 * G::PER_DEGREE;
    const LON_MAX: i32 = 180 * G::PER_DEGREE;

    /// The point at these grid units; `None` outside the valid range.
    #[must_use]
    pub const fn from_units(lat: i32, lon: i32) -> Option<Self> {
        if lat.unsigned_abs() <= Self::LAT_MAX.unsigned_abs()
            && lon.unsigned_abs() <= Self::LON_MAX.unsigned_abs()
        {
            Some(Self {
                lat,
                lon,
                grid: PhantomData,
            })
        } else {
            None
        }
    }

    /// The nearest grid point to these degrees (halves round away from zero).
    ///
    /// # Errors
    /// [`GridError`] if a value is not finite or is out of range after rounding.
    pub fn from_degrees(lat: f64, lon: f64) -> Result<Self, GridError> {
        if !lat.is_finite() || !lon.is_finite() {
            return Err(GridError::NotFinite);
        }
        let lat = round_units(lat, G::PER_DEGREE, 90).ok_or(GridError::Latitude)?;
        let lon = round_units(lon, G::PER_DEGREE, 180).ok_or(GridError::Longitude)?;
        Self::from_units(lat, lon).ok_or(GridError::Latitude)
    }

    /// Latitude in grid units.
    #[must_use]
    pub const fn lat(self) -> i32 {
        self.lat
    }

    /// Longitude in grid units.
    #[must_use]
    pub const fn lon(self) -> i32 {
        self.lon
    }

    /// Latitude in degrees: the nearest `f64` to the grid value.
    #[must_use]
    pub fn lat_degrees(self) -> f64 {
        f64::from(self.lat) / f64::from(G::PER_DEGREE)
    }

    /// Longitude in degrees: the nearest `f64` to the grid value.
    #[must_use]
    pub fn lon_degrees(self) -> f64 {
        f64::from(self.lon) / f64::from(G::PER_DEGREE)
    }

    /// The same position on the 1e-7° grid. Exact from both grids.
    #[must_use]
    pub const fn to_e7(self) -> Point<E7> {
        let k = E7::PER_DEGREE / G::PER_DEGREE;
        // In range on every grid: 180 × PER_DEGREE × k = 1.8e9 < 2^31.
        Point {
            lat: self.lat * k,
            lon: self.lon * k,
            grid: PhantomData,
        }
    }
}

impl Point<E7> {
    /// The nearest point on the 1e-6° grid (halves away from zero).
    #[must_use]
    pub const fn to_e6(self) -> Point<E6> {
        const fn div10(v: i32) -> i32 {
            let q = v / 10;
            let r = v % 10;
            if r >= 5 {
                q + 1
            } else if r <= -5 {
                q - 1
            } else {
                q
            }
        }
        // Rounding never leaves the range: ±1.8e9 / 10 is exactly ±1.8e8.
        Point {
            lat: div10(self.lat),
            lon: div10(self.lon),
            grid: PhantomData,
        }
    }
}

impl<G: Grid> fmt::Display for Point<G> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let d = usize::from(G::DIGITS);
        write!(f, "{:.d$}, {:.d$}", self.lat_degrees(), self.lon_degrees())
    }
}

#[cfg(test)]
mod tests {
    use super::{E6, E7, GridError, Point, round_units};
    use std::string::ToString;

    #[test]
    fn rounding_is_exact_and_half_away_from_zero() {
        let p = |lat, lon| Point::<E6>::from_degrees(lat, lon).map(|p| (p.lat(), p.lon()));
        assert_eq!(p(27.7172, 85.324), Ok((27_717_200, 85_324_000)));
        assert_eq!(p(0.000_000_5, -0.000_000_5), Ok((1, -1)));
        assert_eq!(p(0.000_000_499_999, -0.000_000_499_999), Ok((0, 0)));
        assert_eq!(p(-0.0, -0.0), Ok((0, 0)));
        assert_eq!(p(90.0, 180.0), Ok((90_000_000, 180_000_000)));
        assert_eq!(p(-90.0, -180.0), Ok((-90_000_000, -180_000_000)));
        // Within half a unit of the edge rounds onto it; beyond does not.
        assert_eq!(p(90.000_000_4, 0.0), Ok((90_000_000, 0)));
        assert_eq!(p(90.000_000_6, 0.0), Err(GridError::Latitude));
        assert_eq!(p(0.0, -180.000_000_6), Err(GridError::Longitude));
        assert_eq!(p(f64::NAN, 0.0), Err(GridError::NotFinite));
        assert_eq!(p(0.0, f64::INFINITY), Err(GridError::NotFinite));
        assert_eq!(p(1e300, 0.0), Err(GridError::Latitude));
        let e7 = Point::<E7>::from_degrees(-90.0, 180.0).map(|p| (p.lat(), p.lon()));
        assert_eq!(e7, Ok((-900_000_000, 1_800_000_000)));
    }

    #[test]
    fn the_fraction_is_exact() {
        // The classic `x + 0.5` bug: 0.49999999999999994 + 0.5 rounds to 1.0 in f64. The rounding
        // here works on the product of degrees and units; one unit per degree tests it directly.
        let below_half = 0.5 - f64::EPSILON / 4.0;
        assert!(below_half < 0.5);
        assert_eq!(round_units(below_half, 1, 90), Some(0));
        assert_eq!(round_units(-below_half, 1, 90), Some(0));
        assert_eq!(round_units(0.5, 1, 90), Some(1));
        assert_eq!(round_units(-0.5, 1, 90), Some(-1));
        assert_eq!(round_units(2.5, 1, 90), Some(3));
        assert_eq!(round_units(89.5, 1, 90), Some(90));
        assert_eq!(round_units(90.5, 1, 90), None);
    }

    #[test]
    fn units_are_range_checked() {
        assert!(Point::<E7>::from_units(900_000_000, -1_800_000_000).is_some());
        assert!(Point::<E7>::from_units(900_000_001, 0).is_none());
        assert!(Point::<E7>::from_units(0, i32::MIN).is_none());
        assert!(Point::<E6>::from_units(90_000_001, 0).is_none());
    }

    #[test]
    fn grids_convert() {
        let a = Point::<E6>::from_units(-27_717_200, 179_999_999).unwrap_or_else(|| unreachable!());
        let b = a.to_e7();
        assert_eq!((b.lat(), b.lon()), (-277_172_000, 1_799_999_990));
        assert_eq!(b.to_e6(), a);
        let r = |lat, lon| {
            let p = Point::<E7>::from_units(lat, lon).unwrap_or_else(|| unreachable!());
            (p.to_e6().lat(), p.to_e6().lon())
        };
        assert_eq!(r(15, -15), (2, -2));
        assert_eq!(r(14, -14), (1, -1));
        assert_eq!(r(900_000_000, -1_800_000_000), (90_000_000, -180_000_000));
        assert_eq!(r(899_999_999, 1_799_999_995), (90_000_000, 180_000_000));
    }

    #[test]
    fn degrees_round_trip_through_the_grid() {
        // Every grid value comes back from its f64 degrees to the same units.
        let mut s = 1_u64;
        for _ in 0..crate::cases(500_000) {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            let lat = i32::try_from(s % 1_800_000_001).unwrap_or_default() - 900_000_000;
            let lon = i32::try_from((s >> 32) % 3_600_000_001).unwrap_or_default() - 1_800_000_000;
            let p = Point::<E7>::from_units(lat, lon).unwrap_or_else(|| unreachable!());
            let q = Point::<E7>::from_degrees(p.lat_degrees(), p.lon_degrees());
            assert_eq!(q, Ok(p));
        }
    }

    #[test]
    fn display() {
        let p = Point::<E6>::from_degrees(27.7172, -85.324).map(|p| p.to_string());
        assert_eq!(p.as_deref(), Ok("27.717200, -85.324000"));
    }
}
