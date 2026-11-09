//! `build.yaml` (schema `teigeodata/build/v1`): how `forge make <cc>` builds a country's packs
//! (D-095). Checked by `check-data`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;

const SCHEMA: &str = "teigeodata/build/v1";
/// The forge's points readers (`forge read <reader>`).
const READERS: [&str; 3] = ["geonames", "osm", "wikidata"];
/// The forge's building readers (`forge buildings <reader>`).
const BUILDING_READERS: [&str; 3] = ["google", "msbuildings", "osm"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    schema: String,
    admin: String,
    /// The timezone sources, in priority order (D-138).
    #[serde(default)]
    timezones: Vec<String>,
    /// The zones that win an overlap (D-084), as tzdb IDs.
    #[serde(default)]
    timezone_first: Vec<String>,
    points: Vec<Points>,
    #[serde(default)]
    extent: Option<String>,
    #[serde(default)]
    buildings: Vec<Buildings>,
    /// The DEM tilesets of place elevations (D-111), in priority order (D-132).
    #[serde(default)]
    dem: Vec<String>,
    /// The population grid of admin units' modelled population (D-123).
    #[serde(default)]
    population: Option<String>,
    /// The resources one build may use (G-budget, enforced by `forge make`).
    #[serde(default)]
    budget: Option<Budget>,
    /// Wire budgets (G-wire, enforced by `check-pack`): component → the most kB (1,000 B) a
    /// client downloads for it with what it needs.
    #[serde(default)]
    wire_kb: BTreeMap<String, u64>,
    /// The country's slice of the embedded baseline (D-145).
    #[serde(default)]
    baseline: Option<Baseline>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Baseline {
    /// The city list: every listed city of the country is in the slice (G-base).
    cities: String,
    /// The outline tolerance in metres: the slice takes the pack's finest level within it.
    tolerance_m: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Budget {
    rss_mb: u64,
    disk_mb: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Buildings {
    reader: String,
    source: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Points {
    reader: String,
    source: String,
    #[serde(default)]
    extra: Vec<String>,
}

/// `west,south,east,north` in degrees, west < east, south < north, on the globe.
fn extent_ok(text: &str) -> bool {
    let v: Vec<f64> = text
        .split(',')
        .filter_map(|x| x.trim().parse().ok())
        .collect();
    matches!(v.as_slice(), &[west, south, east, north]
        if west < east && south < north && west >= -180.0 && east <= 180.0
            && south >= -90.0 && north <= 90.0)
}

/// Problems in one recipe; `pinned` are the source IDs of `data/sources.lock`.
pub(crate) fn check_text(text: &str, pinned: &BTreeSet<String>) -> Vec<String> {
    let r: Recipe = match serde_saphyr::from_str(text) {
        Ok(r) => r,
        Err(e) => return vec![format!("not a valid recipe: {e}")],
    };
    let mut out = Vec::new();
    if r.schema != SCHEMA {
        out.push(format!("schema is `{}`, expected `{SCHEMA}`", r.schema));
    }
    let mut named: Vec<(&str, &str)> = vec![("admin source", r.admin.as_str())];
    named.extend(r.timezones.iter().map(|t| ("timezone source", t.as_str())));
    for p in &r.points {
        named.push(("points source", p.source.as_str()));
        named.extend(p.extra.iter().map(|e| ("extra source", e.as_str())));
    }
    named.extend(
        r.buildings
            .iter()
            .map(|b| ("buildings source", b.source.as_str())),
    );
    named.extend(r.dem.iter().map(|d| ("DEM tileset", d.as_str())));
    named.extend(r.population.iter().map(|p| ("population grid", p.as_str())));
    named.extend(
        r.baseline
            .iter()
            .map(|b| ("baseline city list", b.cities.as_str())),
    );
    for (what, id) in named {
        if !pinned.contains(id) {
            out.push(format!("{what} `{id}` is not pinned in data/sources.lock"));
        }
    }
    let mut seen = BTreeSet::new();
    for p in &r.points {
        if !READERS.contains(&p.reader.as_str()) {
            out.push(format!(
                "`{}`: unknown reader `{}` (one of {})",
                p.source,
                p.reader,
                READERS.join(", ")
            ));
        }
        if !seen.insert(p.source.as_str()) {
            out.push(format!("`{}` is listed twice", p.source));
        }
        if !p.extra.is_empty() && p.reader != "geonames" {
            out.push(format!(
                "`{}`: only the geonames reader takes extra sources",
                p.source
            ));
        }
    }
    for b in &r.buildings {
        if !BUILDING_READERS.contains(&b.reader.as_str()) {
            out.push(format!(
                "`{}`: unknown buildings reader `{}`",
                b.source, b.reader
            ));
        }
    }
    for z in &r.timezone_first {
        if !z.contains('/') || z.contains(char::is_whitespace) {
            out.push(format!("timezone_first: `{z}` is not a tzdb zone ID"));
        }
    }
    if r.extent.is_none() && !r.dem.is_empty() {
        out.push("a DEM needs an extent".into());
    }
    match (&r.extent, r.buildings.is_empty()) {
        (None, false) => out.push("buildings need an extent".into()),
        (Some(e), _) if !extent_ok(e) => {
            out.push(format!(
                "extent `{e}`: expected west,south,east,north in degrees"
            ));
        }
        _ => {}
    }
    if r.points.len() < 2 {
        out.push("conflation needs at least two points sources".into());
    }
    if let Some(b) = &r.budget
        && (b.rss_mb == 0 || b.disk_mb == 0)
    {
        out.push("a budget states rss_mb and disk_mb above zero".into());
    }
    if let Some(b) = &r.baseline
        && !(50..=5_000).contains(&b.tolerance_m)
    {
        out.push("baseline: tolerance_m is 50 to 5,000 metres".into());
    }
    for (name, &kb) in &r.wire_kb {
        if teigeodata_format::component::Component::from_name(name).is_none() || kb == 0 {
            out.push(format!(
                "wire_kb: `{name}` is not a component, or its budget is 0"
            ));
        }
    }
    out
}

/// The wire budgets of the country at `address` (`data/countries/<address>/build.yaml`); none
/// for an address without a recipe.
pub(crate) fn wire_kb(root: &Path, address: &str) -> Result<BTreeMap<String, u64>, String> {
    let path = root.join("data/countries").join(address).join("build.yaml");
    if !path.is_file() {
        return Ok(BTreeMap::new());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let r: Recipe =
        serde_saphyr::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(r.wire_kb)
}

#[cfg(test)]
mod tests {
    use super::check_text;
    use std::collections::BTreeSet;

    #[test]
    fn recipes_are_checked_against_the_lock() {
        let pinned: BTreeSet<String> = ["a", "b", "c", "x"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        let good = "schema: teigeodata/build/v1\nadmin: a\npoints:\n  - { reader: osm, source: b }\n  - { reader: geonames, source: c, extra: [x] }\n";
        assert_eq!(check_text(good, &pinned), Vec::<String>::new());
        let with_dem = format!("{good}extent: \"80,26,88,30\"\ndem: [x]\n");
        assert_eq!(check_text(&with_dem, &pinned), Vec::<String>::new());
        let with_budget = format!("{good}budget: {{ rss_mb: 4000, disk_mb: 2000 }}\n");
        assert_eq!(check_text(&with_budget, &pinned), Vec::<String>::new());
        let wire = format!("{good}wire_kb: {{ point: 550, timezone: 600 }}\n");
        assert_eq!(check_text(&wire, &pinned), Vec::<String>::new());
        for bad in ["points: 550", "point: 0"] {
            let text = format!("{good}wire_kb: {{ {bad} }}\n");
            assert!(!check_text(&text, &pinned).is_empty(), "{bad}");
        }
        let zero = format!("{good}budget: {{ rss_mb: 0, disk_mb: 2000 }}\n");
        assert!(!check_text(&zero, &pinned).is_empty(), "a zero budget");
        let partial = format!("{good}budget: {{ rss_mb: 4000 }}\n");
        assert!(
            !check_text(&partial, &pinned).is_empty(),
            "a partial budget"
        );
        for (text, why) in [
            (
                "schema: teigeodata/build/v2\nadmin: a\npoints:\n  - { reader: osm, source: b }\n  - { reader: osm, source: c }\n",
                "schema",
            ),
            (
                "schema: teigeodata/build/v1\nadmin: z\npoints:\n  - { reader: osm, source: b }\n  - { reader: osm, source: c }\n",
                "unpinned admin",
            ),
            (
                "schema: teigeodata/build/v1\nadmin: a\npoints:\n  - { reader: gtfs, source: b }\n  - { reader: osm, source: c }\n",
                "reader",
            ),
            (
                "schema: teigeodata/build/v1\nadmin: a\npoints:\n  - { reader: osm, source: b }\n  - { reader: osm, source: b }\n",
                "twice",
            ),
            (
                "schema: teigeodata/build/v1\nadmin: a\npoints:\n  - { reader: osm, source: b, extra: [x] }\n  - { reader: osm, source: c }\n",
                "extra",
            ),
            (
                "schema: teigeodata/build/v1\nadmin: a\npoints:\n  - { reader: osm, source: b }\n",
                "one source",
            ),
            (
                "schema: teigeodata/build/v1\nadmin: a\nzones: x\npoints: []\n",
                "unknown field",
            ),
            (
                "schema: teigeodata/build/v1\nadmin: a\npoints:\n  - { reader: osm, source: b }\n  - { reader: osm, source: c }\nbuildings:\n  - { reader: osm, source: b }\n",
                "buildings without an extent",
            ),
            (
                "schema: teigeodata/build/v1\nadmin: a\nextent: \"88,26,80,30\"\npoints:\n  - { reader: osm, source: b }\n  - { reader: osm, source: c }\n",
                "west after east",
            ),
            (
                "schema: teigeodata/build/v1\nadmin: a\nextent: \"80,26,88,30\"\npoints:\n  - { reader: osm, source: b }\n  - { reader: osm, source: c }\nbuildings:\n  - { reader: lidar, source: b }\n",
                "buildings reader",
            ),
            (
                "schema: teigeodata/build/v1\nadmin: a\ndem: [a]\npoints:\n  - { reader: osm, source: b }\n  - { reader: osm, source: c }\n",
                "a DEM without an extent",
            ),
            (
                "schema: teigeodata/build/v1\nadmin: a\ndem: [z]\nextent: \"80,26,88,30\"\npoints:\n  - { reader: osm, source: b }\n  - { reader: osm, source: c }\n",
                "an unpinned DEM",
            ),
        ] {
            assert!(!check_text(text, &pinned).is_empty(), "{why}");
        }
    }
}
