//! `read/wikidata`: a snapshot of `forge/queries/wikidata-places.rq` (`QLever` TSV). One row per
//! item; multi-valued fields are joined with `|`; names are `text@lang`.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::record::{Link, Name, Output, Record, e7};

const ENTITY: &str = "http://www.wikidata.org/entity/";

/// Reads a snapshot. The columns are found by header name, so their order may change.
pub(crate) fn read(path: &Path) -> Result<Output, String> {
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut lines = BufReader::with_capacity(1 << 20, file).lines();
    let first = lines
        .next()
        .ok_or("empty snapshot")?
        .map_err(|e| e.to_string())?;
    let header: Vec<&str> = first.split('\t').collect();
    let col = |n: &str| {
        header
            .iter()
            .position(|h| *h == format!("?{n}"))
            .ok_or_else(|| format!("snapshot has no column ?{n}"))
    };
    let c = Cols {
        item: col("item")?,
        coords: col("coords")?,
        classes: col("classes")?,
        roots: col("roots")?,
        parents: col("parents")?,
        geonames: col("geonames_ids")?,
        osm: col("osm_relations")?,
        population: col("populations")?,
        elevation: col("elevations")?,
        dissolved: col("dissolved_dates")?,
        labels: col("labels")?,
        aliases: col("aliases")?,
        nicknames: col("nicknames")?,
        short: col("short_names")?,
    };
    let mut out = Output::default();
    for line in lines {
        let line = line.map_err(|e| e.to_string())?;
        row(&line, &c, &mut out);
    }
    Ok(out)
}

/// One snapshot row into records, names and links.
fn row(line: &str, c: &Cols, out: &mut Output) {
    let f: Vec<String> = line.split('\t').map(unquote).collect();
    let get = |i: usize| f.get(i).map_or("", String::as_str);
    let Some(q) = get(c.item).strip_prefix(ENTITY) else {
        return;
    };
    let id = format!("wikidata:{q}");
    // The first coordinate; Wikidata may state several (sorted, so this is stable).
    let (lat, lon) = values(get(c.coords))
        .into_iter()
        .find_map(point)
        .map_or((None, None), |(lat, lon)| (e7(lat), e7(lon)));
    let classes: Vec<String> = values(get(c.classes))
        .into_iter()
        .filter_map(|v| v.strip_prefix(ENTITY).map(str::to_string))
        .collect();
    let labels = names(get(c.labels));
    // The display name: the English label, else any label.
    let name = labels
        .iter()
        .find(|(_, l)| l == "en")
        .or_else(|| labels.first())
        .map(|(t, _)| t.clone())
        .unwrap_or_default();
    out.records.push(Record {
        id: id.clone(),
        class: classes.join("|"),
        kind: kind(get(c.roots)).into(),
        name,
        lat_e7: lat,
        lon_e7: lon,
        elevation_m: values(get(c.elevation))
            .into_iter()
            .find_map(|v| v.parse().ok()),
        dem_m: None,
        population: values(get(c.population))
            .into_iter()
            .filter_map(|v| v.parse::<f64>().ok())
            .map(|p| {
                #[allow(clippy::cast_possible_truncation, reason = "populations fit i64")]
                let p = p.round() as i64;
                p
            })
            .max(),
        dissolved: values(get(c.dissolved))
            .into_iter()
            .next()
            .map(str::to_string),
        timezone: None,
    });
    for (col, kind) in [
        (c.labels, "label"),
        (c.aliases, "alias"),
        (c.nicknames, "colloquial"),
        (c.short, "short"),
    ] {
        for (text, lang) in names(get(col)) {
            out.names.push(Name {
                id: id.clone(),
                text,
                lang,
                kind: kind.into(),
            });
        }
    }
    for p in values(get(c.parents)) {
        if let Some(pq) = p.strip_prefix(ENTITY) {
            out.links.push(Link {
                id: id.clone(),
                rel: "parent".into(),
                target: format!("wikidata:{pq}"),
            });
        }
    }
    for g in values(get(c.geonames)) {
        out.links.push(Link {
            id: id.clone(),
            rel: "same_as".into(),
            target: format!("geonames:{g}"),
        });
    }
    for r in values(get(c.osm)) {
        out.links.push(Link {
            id: id.clone(),
            rel: "same_as".into(),
            target: format!("osm:relation/{r}"),
        });
    }
}

struct Cols {
    item: usize,
    coords: usize,
    classes: usize,
    roots: usize,
    parents: usize,
    geonames: usize,
    osm: usize,
    population: usize,
    elevation: usize,
    dissolved: usize,
    labels: usize,
    aliases: usize,
    nicknames: usize,
    short: usize,
}

/// A TSV cell: `"literal"` (with `\"`, `\\`, `\t`, `\n` escapes), `<iri>` or empty.
pub(crate) fn unquote(cell: &str) -> String {
    if let Some(inner) = cell.strip_prefix('<').and_then(|c| c.strip_suffix('>')) {
        return inner.to_string();
    }
    let Some(inner) = cell.strip_prefix('"') else {
        return cell.to_string();
    };
    // A typed literal ends `"^^<type>`; a plain one ends `"`.
    let inner = inner.rsplit_once('"').map_or(inner, |(v, _)| v);
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// The common kind from the root classes an item reaches (the query's `?roots`), in order
/// of precedence: a city that is also an admin unit is a populated place, as in `GeoNames`.
fn kind(roots: &str) -> &'static str {
    const KINDS: [(&str, &str); 2] = [("Q486972", "populated_place"), ("Q56061", "admin")];
    let roots = values(roots);
    KINDS
        .iter()
        .find(|(q, _)| roots.iter().any(|r| r.strip_prefix(ENTITY) == Some(*q)))
        .map_or("other", |(_, k)| k)
}

fn values(cell: &str) -> Vec<&str> {
    cell.split('|').filter(|v| !v.is_empty()).collect()
}

/// `text@lang` pairs; the language is after the last `@`.
fn names(cell: &str) -> Vec<(String, String)> {
    values(cell)
        .into_iter()
        .filter_map(|v| v.rsplit_once('@'))
        .filter(|(t, _)| !t.trim().is_empty())
        .map(|(t, l)| (t.to_string(), l.to_string()))
        .collect()
}

/// `POINT(lon lat)` → (lat, lon). Coordinates on other globes carry a prefix IRI and are
/// skipped.
fn point(v: &str) -> Option<(f64, f64)> {
    let inner = v.strip_prefix("POINT(")?.strip_suffix(')')?;
    let (lon, lat) = inner.split_once(' ')?;
    Some((lat.parse().ok()?, lon.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::{ENTITY, kind, names, point, unquote};

    #[test]
    fn parses_cells() {
        let both = format!("{ENTITY}Q56061|{ENTITY}Q486972");
        assert_eq!(kind(&both), "populated_place");
        assert_eq!(kind(&format!("{ENTITY}Q56061")), "admin");
        assert_eq!(kind(""), "other");
        assert_eq!(unquote("<http://x/Q1>"), "http://x/Q1");
        assert_eq!(unquote("\"a\\\"b\""), "a\"b");
        assert_eq!(
            unquote("\"12\"^^<http://www.w3.org/2001/XMLSchema#int>"),
            "12"
        );
        assert_eq!(point("POINT(85.3 27.7)"), Some((27.7, 85.3)));
        assert_eq!(
            point("<http://www.wikidata.org/entity/Q405> POINT(1 2)"),
            None
        );
        assert_eq!(
            names("Yen@en|काठमाडौं@ne|x@y@z"),
            vec![
                ("Yen".to_string(), "en".to_string()),
                ("काठमाडौं".to_string(), "ne".to_string()),
                ("x@y".to_string(), "z".to_string())
            ]
        );
    }
}
