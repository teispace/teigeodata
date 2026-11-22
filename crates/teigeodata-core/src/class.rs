//! The class of a place ([schema](https://github.com/teispace/teigeodata/blob/main/docs/05-data/schema.md#kind)):
//! a finer code than its kind, stored in the PLACES `CLASS` column (0 = none). World-general:
//! one list for every country (P16). The list only grows: a code keeps its meaning, a reader
//! that does not know a code reads no class, and a pack's class never changes how it is read,
//! so a 1.x reader of an older list reads a newer pack (D-381).

/// Every class name, by code: code `n` is `NAMES[n - 1]`.
const NAMES: [&str; 14] = [
    // Landmarks (TG-32): a built or sacred place people name and visit.
    "palace",
    "temple",
    "monastery",
    "shrine",
    "religious_site",
    "monument",
    "museum",
    "historic_site",
    "castle",
    "ruin",
    "pagoda",
    "mosque",
    "church",
    "airport",
];

/// The class of `code`, or `None` for 0 and a code this list does not have.
#[must_use]
pub fn class_name(code: u32) -> Option<&'static str> {
    let i = usize::try_from(code).ok()?.checked_sub(1)?;
    NAMES.get(i).copied()
}

/// The code of the class `name`, or `None` for a name not in the list.
#[must_use]
pub fn class_code(name: &str) -> Option<u32> {
    let i = NAMES.iter().position(|n| *n == name)?;
    u32::try_from(i + 1).ok()
}

/// Whether the class is a landmark (TG-32): every class of the list so far.
#[must_use]
pub fn is_landmark(code: u32) -> bool {
    class_name(code).is_some()
}

/// Every class, by code from 1.
pub fn classes() -> impl Iterator<Item = (u32, &'static str)> {
    NAMES.iter().zip(1_u32..).map(|(n, c)| (c, *n))
}

#[cfg(test)]
mod tests {
    use super::{class_code, class_name, classes};

    #[test]
    fn codes_and_names_round_trip_and_the_schema_lists_them() {
        let schema = include_str!("../../../docs/05-data/schema.md");
        let row = schema
            .lines()
            .find(|l| l.starts_with("| `class` |"))
            .unwrap_or_default();
        for (code, name) in classes() {
            assert_eq!(class_name(code), Some(name));
            assert_eq!(class_code(name), Some(code));
            assert!(row.contains(&std::format!("`{name}`")), "{name}");
        }
        assert_eq!(class_name(0), None);
        assert_eq!(class_name(15), None);
        assert_eq!(class_name(u32::MAX), None);
        assert_eq!(class_code("village"), None);
    }
}
