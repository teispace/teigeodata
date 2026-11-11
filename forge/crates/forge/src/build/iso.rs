//! ISO 3166 codes of a country's row and units (D-197, [ISO 3166
//! codes](../../../../../docs/02-research/notes/iso-3166-codes.md)): `data/countries/<cc>/iso3166.yaml`
//! read, and gate G-iso on the rows of a pack, whose codes the pack stage writes to `ISO_CODES`.
//!
//! G-iso, on every pack the stage writes (a split part included):
//! 1. every code is a valid ISO 3166 code (the file is refused otherwise);
//! 2. every current (not former) level-1 unit has a code or a `no_code` reason;
//! 3. codes are unique among the pack's rows, and every subdivision code carries the country's
//!    alpha-2 prefix;
//! 4. on the country pack, every key of `units`, `no_code` and `former` is a place of the build.
//!
//! The differential against CLDR's subdivision list (the note's §5.3 item 3) waits for CLDR to
//! be pinned in `data/sources.lock`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use teigeodata_core::{IsoCode, PlaceKind};
use teigeodata_format::places::FLAG_FORMER;

use super::Place;

const SCHEMA: &str = "teigeodata/iso3166/v1";

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    schema: String,
    country: String,
    source: String,
    units: BTreeMap<String, String>,
    #[serde(default)]
    #[allow(
        dead_code,
        reason = "world sources' stale codes: read by the world part later"
    )]
    renamed: Vec<serde::de::IgnoredAny>,
    #[serde(default)]
    no_code: BTreeMap<String, String>,
    #[serde(default)]
    former: BTreeMap<String, Former>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Former {
    code: String,
    #[allow(
        dead_code,
        reason = "checked by check-data; the pack stores the code only"
    )]
    until: String,
}

/// A country's codes.
#[derive(Debug)]
pub(crate) struct Iso {
    /// The country row's code.
    pub(crate) country: IsoCode,
    /// Where the codes come from, with the ISO change checked.
    pub(crate) source: String,
    /// Each unit's code, by place key.
    pub(crate) units: BTreeMap<String, IsoCode>,
    /// Current units with no code, and why.
    pub(crate) no_code: BTreeMap<String, String>,
    /// Former units' deleted codes, by place key.
    pub(crate) former: BTreeMap<String, IsoCode>,
}

impl Iso {
    /// The code of `p`: the country's for the country row, its unit code, or a former unit's
    /// deleted code.
    fn code_of(&self, p: &Place) -> Option<IsoCode> {
        if p.kind == PlaceKind::Country {
            return Some(self.country);
        }
        if p.flags & FLAG_FORMER != 0 {
            self.former.get(&p.key).copied()
        } else {
            self.units.get(&p.key).copied()
        }
    }

    /// Each unit's pack address part: its code lowercased, the country prefix dropped
    /// (`NP-P3` → `p3`), by place key (split builds, D-191).
    pub(crate) fn addresses(&self) -> BTreeMap<String, String> {
        self.units
            .iter()
            .filter(|(_, c)| !c.is_country())
            .map(|(k, c)| {
                let text = c.as_str().get(3..).unwrap_or_default();
                (k.clone(), text.to_ascii_lowercase())
            })
            .collect()
    }
}

fn code(path: &Path, text: &str) -> Result<IsoCode, String> {
    text.parse()
        .map_err(|e| format!("{}: {text:?}: {e}", path.display()))
}

/// Reads the country's codes; `None` when it has no `iso3166.yaml`.
pub(crate) fn read(root: &Path, cc: &str) -> Result<Option<Iso>, String> {
    let path = root.join("data/countries").join(cc).join("iso3166.yaml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let f: File = serde_saphyr::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if f.schema != SCHEMA {
        return Err(format!("{}: schema must be {SCHEMA}", path.display()));
    }
    let country = code(&path, &f.country)?;
    if !country.is_country() || !country.as_str().eq_ignore_ascii_case(cc) {
        return Err(format!(
            "{}: country {country} is not the alpha-2 code {cc}",
            path.display()
        ));
    }
    if f.source.trim().is_empty() {
        return Err(format!("{}: no source", path.display()));
    }
    let units = f
        .units
        .iter()
        .map(|(k, v)| Ok((k.clone(), code(&path, v)?)))
        .collect::<Result<_, String>>()?;
    let former = f
        .former
        .iter()
        .map(|(k, v)| Ok((k.clone(), code(&path, &v.code)?)))
        .collect::<Result<_, String>>()?;
    Ok(Some(Iso {
        country,
        source: f.source,
        units,
        no_code: f.no_code,
        former,
    }))
}

/// Gate G-iso (see the [module](self)) on `rows`: the coded rows and their codes, in row order.
pub(crate) fn gate(iso: &Iso, rows: &[&Place], whole: bool) -> Result<Vec<(u32, IsoCode)>, String> {
    let mut codes = Vec::new();
    let mut seen: BTreeMap<IsoCode, &str> = BTreeMap::new();
    let mut missing = Vec::new();
    for (r, p) in rows.iter().enumerate() {
        let row = u32::try_from(r).map_err(|_| "too many rows")?;
        let Some(c) = iso.code_of(p) else {
            let current = p.flags & FLAG_FORMER == 0;
            if current
                && p.kind == PlaceKind::Admin
                && p.admin_level == 1
                && !iso.no_code.contains_key(&p.key)
            {
                missing.push(p.key.as_str());
            }
            continue;
        };
        if c.country() != iso.country.country() {
            return Err(format!(
                "G-iso: {} has code {c}, not prefixed by the country {}",
                p.key, iso.country
            ));
        }
        if p.kind != PlaceKind::Country && c.is_country() {
            return Err(format!("G-iso: {} has the country's code {c}", p.key));
        }
        if let Some(other) = seen.insert(c, &p.key) {
            return Err(format!("G-iso: {c} is the code of {other} and {}", p.key));
        }
        codes.push((row, c));
    }
    if !missing.is_empty() {
        return Err(format!(
            "G-iso: {} current level-1 unit(s) with no ISO code and no no_code reason: {}",
            missing.len(),
            missing.join(", ")
        ));
    }
    if whole {
        let keys: BTreeSet<&str> = rows.iter().map(|p| p.key.as_str()).collect();
        let stale: Vec<&str> = iso
            .units
            .keys()
            .chain(iso.no_code.keys())
            .chain(iso.former.keys())
            .map(String::as_str)
            .filter(|k| !keys.contains(k))
            .collect();
        if !stale.is_empty() {
            return Err(format!(
                "G-iso: iso3166.yaml names {}, which is not a place of this build",
                stale.join(", ")
            ));
        }
    }
    Ok(codes)
}

#[cfg(test)]
mod tests {
    use super::{Iso, gate, read};
    use crate::build::{Place, PlaceName};
    use std::collections::BTreeMap;
    use teigeodata_core::{IsoCode, PlaceKind};
    use teigeodata_format::places::FLAG_FORMER;

    fn place(key: &str, kind: PlaceKind, level: u8, flags: u32) -> Place {
        Place {
            key: key.into(),
            kind,
            admin_level: level,
            flags,
            point: (0, 0),
            names: vec![PlaceName {
                text: key.into(),
                lang: None,
                kind: None,
                origin: crate::build::Origin::Unknown,
            }],
            population: None,
            rank: 0,
            parent: None,
            sources: vec![key.into()],
        }
    }

    fn c(s: &str) -> IsoCode {
        s.parse().unwrap()
    }

    fn iso() -> Iso {
        Iso {
            country: c("KQ"),
            source: "test".into(),
            units: BTreeMap::from([("admin:1".into(), c("KQ-A")), ("admin:2".into(), c("KQ-B"))]),
            no_code: BTreeMap::new(),
            former: BTreeMap::from([("geonames:9".into(), c("KQ-Z"))]),
        }
    }

    fn rows() -> Vec<Place> {
        vec![
            place("admin:2", PlaceKind::Admin, 1, 0),
            place("wikidata:Q1", PlaceKind::Country, 0, 0),
            place("admin:11", PlaceKind::Admin, 2, 0),
            place("geonames:9", PlaceKind::Admin, 1, FLAG_FORMER),
            place("admin:1", PlaceKind::Admin, 1, 0),
            place("geonames:5", PlaceKind::Settlement, 0, 0),
        ]
    }

    #[test]
    fn the_country_units_and_former_units_get_their_codes() {
        let all = rows();
        let refs: Vec<&Place> = all.iter().collect();
        let got = gate(&iso(), &refs, true).unwrap();
        assert_eq!(
            got,
            [(0, c("KQ-B")), (1, c("KQ")), (3, c("KQ-Z")), (4, c("KQ-A"))]
        );
        // A part with only some of the units passes (no stale-key check there).
        let part: Vec<&Place> = all.iter().take(2).collect();
        assert_eq!(gate(&iso(), &part, false).unwrap().len(), 2);
    }

    /// Planted faults: each must fail the gate.
    #[test]
    fn g_iso_fails_on_each_planted_fault() {
        let all = rows();
        let refs: Vec<&Place> = all.iter().collect();
        let fails = |i: Iso, want: &str| {
            let e = gate(&i, &refs, true).unwrap_err();
            assert!(e.contains(want), "{want}: {e}");
        };
        // A dropped code.
        let mut i = iso();
        i.units.remove("admin:1");
        fails(i, "no ISO code");
        // The same unit with a no_code reason passes.
        let mut i = iso();
        i.units.remove("admin:1");
        i.no_code.insert("admin:1".into(), "disputed".into());
        assert!(gate(&i, &refs, true).is_ok());
        // A duplicate code.
        let mut i = iso();
        i.units.insert("admin:1".into(), c("KQ-B"));
        fails(i, "is the code of");
        // A wrong prefix.
        let mut i = iso();
        i.units.insert("admin:1".into(), c("IN-TN"));
        fails(i, "not prefixed");
        // A unit with the country's code.
        let mut i = iso();
        i.units.insert("admin:1".into(), c("KQ"));
        fails(i, "the country's code");
        // A stale key.
        let mut i = iso();
        i.units.insert("admin:404".into(), c("KQ-C"));
        fails(i, "not a place of this build");
    }

    #[test]
    fn files_are_read_and_checked() {
        let dir = std::env::temp_dir().join(format!("forge-iso-{}", std::process::id()));
        let cc = dir.join("data/countries/kq");
        std::fs::create_dir_all(&cc).unwrap();
        assert!(read(&dir, "kq").unwrap().is_none(), "no file, no codes");
        let write = |text: &str| std::fs::write(cc.join("iso3166.yaml"), text).unwrap();
        let good = "schema: teigeodata/iso3166/v1\ncountry: KQ\nsource: test\nunits:\n  \"admin:1\": KQ-A\n  \"admin:2\": KQ-B\n";
        write(good);
        let i = read(&dir, "kq").unwrap().unwrap();
        assert_eq!(i.country, c("KQ"));
        assert_eq!(
            i.addresses(),
            BTreeMap::from([
                ("admin:1".into(), "a".into()),
                ("admin:2".into(), "b".into())
            ])
        );
        for bad in [
            good.replace("iso3166/v1", "iso3166/v2"),
            good.replace("country: KQ", "country: KR"),
            good.replace(": KQ-B", ": KQ-B1234"),
            good.replace("source: test", "source: \" \""),
            format!("{good}extra: 1\n"),
        ] {
            write(&bad);
            assert!(read(&dir, "kq").is_err(), "{bad}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
