//! The kind of a place ([schema](https://github.com/teispace/teigeodata/blob/main/docs/05-data/schema.md#kind)),
//! as the one-byte code the PLACES section stores. Code 0 is never used, so a zeroed column is
//! caught; codes above the last are refused, not guessed.

/// What a place is. The finer `class` (city, village, peak) is a separate dictionary code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum PlaceKind {
    /// A country (admin level 0).
    Country = 1,
    /// An administrative unit, with its admin level.
    Admin = 2,
    /// A populated place: city, town, village, hamlet.
    Settlement = 3,
    /// A named part of a settlement or an unpopulated locality (a tole, a neighbourhood).
    Locality = 4,
    /// A natural feature: peak, pass, lake, river.
    Natural = 5,
    /// A postal area.
    PostalArea = 6,
    /// A timezone area.
    TimezoneArea = 7,
}

impl PlaceKind {
    /// Every kind, in code order.
    pub const ALL: [Self; 7] = [
        Self::Country,
        Self::Admin,
        Self::Settlement,
        Self::Locality,
        Self::Natural,
        Self::PostalArea,
        Self::TimezoneArea,
    ];

    /// The stored code.
    #[must_use]
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// The kind of a stored code, or `None` for 0 and unknown codes.
    #[must_use]
    pub const fn from_code(code: u32) -> Option<Self> {
        Some(match code {
            1 => Self::Country,
            2 => Self::Admin,
            3 => Self::Settlement,
            4 => Self::Locality,
            5 => Self::Natural,
            6 => Self::PostalArea,
            7 => Self::TimezoneArea,
            _ => return None,
        })
    }

    /// The schema's name: `country`, `admin`, `settlement`, …
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Country => "country",
            Self::Admin => "admin",
            Self::Settlement => "settlement",
            Self::Locality => "locality",
            Self::Natural => "natural",
            Self::PostalArea => "postal_area",
            Self::TimezoneArea => "timezone_area",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PlaceKind;

    #[test]
    fn codes_round_trip_and_match_the_schema() {
        let schema = include_str!("../../../docs/05-data/schema.md");
        let row = schema
            .lines()
            .find(|l| l.starts_with("| `kind` |"))
            .unwrap_or_default();
        for (i, k) in PlaceKind::ALL.iter().enumerate() {
            assert_eq!(usize::from(k.code()), i + 1);
            assert_eq!(PlaceKind::from_code(u32::from(k.code())), Some(*k));
            assert!(
                row.contains(&std::format!("`{}`", k.name())),
                "{}",
                k.name()
            );
        }
        assert_eq!(PlaceKind::from_code(0), None);
        assert_eq!(PlaceKind::from_code(8), None);
    }
}
