//! `build.yaml` (schema `teigeodata/build/v1`): how `forge make <cc>` builds a country's packs
//! (D-095). Checked by `check-data`.

use std::collections::BTreeSet;

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
    #[serde(default)]
    timezones: Option<String>,
    points: Vec<Points>,
    #[serde(default)]
    extent: Option<String>,
    #[serde(default)]
    buildings: Vec<Buildings>,
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
    out
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
        ] {
            assert!(!check_text(text, &pinned).is_empty(), "{why}");
        }
    }
}
