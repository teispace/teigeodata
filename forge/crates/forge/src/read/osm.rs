//! `read/osm`: places, natural point features (summits, hills, volcanoes, passes: D-201) and
//! admin boundaries from an OSM PBF extract, with `osmpbf`'s parallel
//! decoder. Place nodes keep their exact 1e-7° position (OSM's own grid). Place areas (ways)
//! and boundary relations are read without geometry here; area assembly is its own stage
//! (forge note §3, benchmark B2).

use std::path::Path;

use osmpbf::{Element, ElementReader};

use crate::record::{Link, Name, Output, Record};

/// Name tags and the kind each maps to. A `:<lang>` suffix carries the language.
const NAME_KEYS: [(&str, &str); 9] = [
    ("name", "name"),
    ("official_name", "official"),
    ("alt_name", "alternate"),
    ("old_name", "old"),
    ("short_name", "short"),
    ("loc_name", "colloquial"),
    ("int_name", "alternate"),
    ("nat_name", "official"),
    ("reg_name", "alternate"),
];

/// Reads an extract.
pub(crate) fn read(path: &Path) -> Result<Output, String> {
    let reader = ElementReader::from_path(path).map_err(|e| format!("{}: {e}", path.display()))?;
    reader
        .par_map_reduce(
            // Almost every element is untagged or irrelevant: test the tags lazily and allocate
            // only for places and boundaries (the planet has billions of nodes).
            |el| match el {
                Element::Node(n) if n.tags().any(is_point_feature) => node(
                    n.id(),
                    n.decimicro_lat(),
                    n.decimicro_lon(),
                    &n.tags().collect::<Vec<_>>(),
                ),
                Element::DenseNode(n) if n.tags().any(is_point_feature) => node(
                    n.id(),
                    n.decimicro_lat(),
                    n.decimicro_lon(),
                    &n.tags().collect::<Vec<_>>(),
                ),
                Element::Way(w) if w.tags().any(is_place) => {
                    area("way", w.id(), &w.tags().collect::<Vec<_>>())
                }
                Element::Relation(r) if r.tags().any(|t| is_place(t) || is_boundary(t)) => {
                    area("relation", r.id(), &r.tags().collect::<Vec<_>>())
                }
                _ => Output::default(),
            },
            Output::default,
            |mut a, mut b| {
                a.records.append(&mut b.records);
                a.names.append(&mut b.names);
                a.links.append(&mut b.links);
                a
            },
        )
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn is_place((k, _): (&str, &str)) -> bool {
    k == "place"
}

/// A place, or a natural point feature (D-201): which nodes are read.
fn is_point_feature(t: (&str, &str)) -> bool {
    is_place(t)
        || matches!(
            t,
            ("natural", "peak" | "volcano" | "hill" | "saddle") | ("mountain_pass", "yes")
        )
}

fn is_boundary(t: (&str, &str)) -> bool {
    t == ("boundary", "administrative")
}

fn place_kind(place: &str) -> &'static str {
    match place {
        "city" | "town" | "village" | "hamlet" | "isolated_dwelling" | "suburb" | "quarter"
        | "neighbourhood" | "borough" | "farm" => "populated_place",
        "locality" => "locality",
        _ => "other",
    }
}

fn node(id: i64, lat: i32, lon: i32, tags: &[(&str, &str)]) -> Output {
    area_or_point(&format!("osm:node/{id}"), Some((lat, lon)), tags)
}

fn area(kind: &str, id: i64, tags: &[(&str, &str)]) -> Output {
    area_or_point(&format!("osm:{kind}/{id}"), None, tags)
}

fn area_or_point(rid: &str, point: Option<(i32, i32)>, tags: &[(&str, &str)]) -> Output {
    let mut out = Output::default();
    if let Some(place) = tag(tags, "place") {
        push(
            &mut out,
            rid,
            &format!("place={place}"),
            place_kind(place),
            point,
            tags,
        );
    } else if tag(tags, "boundary") == Some("administrative") {
        let level = tag(tags, "admin_level").unwrap_or("");
        push(
            &mut out,
            rid,
            &format!("admin_level={level}"),
            "admin",
            point,
            tags,
        );
    } else if point.is_some()
        && let Some(class) = natural_class(tags)
    {
        push(&mut out, rid, &class, "natural", point, tags);
    }
    out
}

/// The class of a named point feature with a height of its own (D-201): a summit, a hill, a
/// volcano or a pass. Ranges, ridges and valleys are areas or lines, not places.
fn natural_class(tags: &[(&str, &str)]) -> Option<String> {
    match tag(tags, "natural") {
        Some(v @ ("peak" | "volcano" | "hill" | "saddle")) => Some(format!("natural={v}")),
        _ if tag(tags, "mountain_pass") == Some("yes") => Some("mountain_pass=yes".into()),
        _ => None,
    }
}

fn tag<'a>(tags: &[(&str, &'a str)], k: &str) -> Option<&'a str> {
    tags.iter().find(|(key, _)| *key == k).map(|(_, v)| *v)
}

fn push(
    out: &mut Output,
    rid: &str,
    class: &str,
    kind: &str,
    point: Option<(i32, i32)>,
    tags: &[(&str, &str)],
) {
    out.records.push(Record {
        id: rid.to_string(),
        class: class.to_string(),
        kind: kind.to_string(),
        name: tag(tags, "name").unwrap_or("").to_string(),
        lat_e7: point.map(|p| p.0),
        lon_e7: point.map(|p| p.1),
        elevation_m: tag(tags, "ele").and_then(parse_metres),
        dem_m: None,
        population: tag(tags, "population").and_then(|p| p.replace([',', ' '], "").parse().ok()),
        dissolved: None,
        timezone: None,
    });
    for (k, v) in tags {
        let (base, lang) = k.split_once(':').unwrap_or((k, ""));
        let Some((_, name_kind)) = NAME_KEYS.iter().find(|(key, _)| *key == base) else {
            continue;
        };
        // name:<lang> only; skip qualifiers such as name:etymology or name:prefix.
        if !lang.is_empty() && !is_lang(lang) {
            continue;
        }
        // Several values are separated by ';' in alternate-name tags.
        for text in v.split(';').map(str::trim).filter(|t| !t.is_empty()) {
            out.names.push(Name {
                id: rid.to_string(),
                text: text.to_string(),
                lang: lang.to_string(),
                kind: (*name_kind).to_string(),
            });
        }
    }
    if let Some(q) = tag(tags, "wikidata") {
        for q in q.split(';').map(str::trim).filter(|q| q.starts_with('Q')) {
            out.links.push(Link {
                id: rid.to_string(),
                rel: "same_as".into(),
                target: format!("wikidata:{q}"),
            });
        }
    }
}

/// A BCP 47-like language subtag sequence: `ne`, `new`, `zh-Hans`, `sr-Latn`.
fn is_lang(s: &str) -> bool {
    let mut parts = s.split('-');
    let first = parts.next().unwrap_or("");
    (2..=3).contains(&first.len())
        && first.bytes().all(|b| b.is_ascii_lowercase())
        && parts.all(|p| (2..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// `1400`, `1400 m`, `1,400`; feet are converted.
fn parse_metres(v: &str) -> Option<f32> {
    let t = v.trim().replace(',', "");
    if let Some(ft) = t.strip_suffix("ft").or_else(|| t.strip_suffix('\'')) {
        return ft.trim().parse::<f32>().ok().map(|f| f * 0.3048);
    }
    t.trim_end_matches('m').trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::{area, is_lang, node, parse_metres};

    #[test]
    fn summits_and_passes_are_natural_points() {
        let peak = node(
            1,
            279_881_000,
            869_250_000,
            &[
                ("natural", "peak"),
                ("name", "Sagarmatha"),
                ("ele", "8848.86"),
            ],
        );
        let r = &peak.records[0];
        assert_eq!(
            (r.kind.as_str(), r.class.as_str()),
            ("natural", "natural=peak")
        );
        assert_eq!(r.elevation_m, Some(8848.86));
        let pass = node(2, 1, 1, &[("mountain_pass", "yes"), ("name", "Thorong La")]);
        assert_eq!(pass.records[0].class, "mountain_pass=yes");
        // A ridge is a line, a range an area: neither is a place; a peak area is not read.
        assert!(
            node(3, 1, 1, &[("natural", "ridge"), ("name", "x")])
                .records
                .is_empty()
        );
        assert!(
            area("way", 4, &[("natural", "peak"), ("name", "x")])
                .records
                .is_empty()
        );
        // A place keeps its own kind whatever else it is tagged.
        let both = node(5, 1, 1, &[("place", "village"), ("natural", "hill")]);
        assert_eq!(both.records[0].kind, "populated_place");
    }

    #[test]
    fn languages_and_metres() {
        assert!(is_lang("ne") && is_lang("new") && is_lang("zh-Hans"));
        assert!(!is_lang("etymology") && !is_lang("prefix") && !is_lang("NE"));
        assert_eq!(parse_metres("1400"), Some(1400.0));
        assert_eq!(parse_metres("1,400 m"), Some(1400.0));
        assert_eq!(parse_metres("1000 ft").map(f32::round), Some(305.0));
        assert_eq!(parse_metres("high"), None);
    }
}
