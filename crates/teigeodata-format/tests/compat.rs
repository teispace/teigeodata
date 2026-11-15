//! Packs written before the freeze stay readable (D-336). `tests/data/format-0/` holds two
//! packs as format 0.1 wrote them (the binding fixtures of 2026-11-15, zone rules and one DEM
//! tile), never rewritten: a reader of format 1 opens them, verifies every section's hash and
//! runs every section's full check. Every data release before 1.0 is format 0.
#![allow(clippy::panic, reason = "tests: a panic reports a failure")]

use std::path::PathBuf;

use teigeodata_format::{FORMAT_MAJOR, Pack, READER_MAJOR, check};

#[test]
fn a_format_0_pack_still_opens_and_checks() {
    assert_eq!((FORMAT_MAJOR, READER_MAJOR), (1, 1));
    for name in ["tz.tgd", "dem.tgd"] {
        let at = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/format-0")
            .join(name);
        let bytes = std::fs::read(&at).unwrap_or_else(|e| panic!("{}: {e}", at.display()));
        let p = Pack::open_verified(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(p.format_version(), (0, 1), "{name}");
        assert_eq!(p.min_reader_major(), 0, "{name}");
        let mut checked = 0;
        check::sections(&p, |kind, r| match r {
            Ok(true) => checked += 1,
            other => panic!("{name}: {kind:?} {other:?}"),
        });
        assert!(checked > 0, "{name}: no section checked");
    }
}
