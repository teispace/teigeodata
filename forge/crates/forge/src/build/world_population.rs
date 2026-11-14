//! `forge world --populations <source>` (Q-026, D-266): each unit's population from a
//! Wikidata snapshot, so a unit's importance weighs its people as a city's does.
//!
//! The snapshot (`forge/queries/wikidata-unit-population.rq`) is a `QLever` TSV of
//! `?item ?gn ?pop ?time`: every best-rank population statement of every first- and
//! second-level unit, with its date when it has one. Per item the forge takes the latest dated
//! value, else the largest undated one. A row takes the value of the first of its source keys
//! (`wikidata:Q…`, `geonames:<id>`) that has one, and only when it has no population of its own:
//! a source's own count is never replaced. World-general (P16): no country or level is named.

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use super::world_labels::{ENTITY, unquote};
use super::{Place, PlaceKind};

/// An item's best value so far, `(date, population)`, and its `GeoNames` IDs.
type Item = ((Option<String>, i64), Vec<String>);

/// Each key's population: `wikidata:Q…` and, when the item has one, `geonames:<id>`.
pub(super) fn read(path: &Path) -> Result<HashMap<String, i64>, String> {
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut lines = BufReader::with_capacity(1 << 20, file).lines();
    let header = lines
        .next()
        .ok_or("empty snapshot")?
        .map_err(|e| e.to_string())?;
    if header.split('\t').collect::<Vec<_>>() != ["?item", "?gn", "?pop", "?time"] {
        return Err(format!("unexpected header `{header}`"));
    }
    // Per item: the best (date, population) so far and its GeoNames IDs. A dated value beats
    // an undated one (`Some` > `None`), a later date an earlier one, then the larger count.
    let mut items: HashMap<String, Item> = HashMap::new();
    for line in lines {
        let line = line.map_err(|e| e.to_string())?;
        let mut cols = line.split('\t');
        let (Some(item), Some(gn), Some(pop), Some(time), None) = (
            cols.next(),
            cols.next(),
            cols.next(),
            cols.next(),
            cols.next(),
        ) else {
            return Err(format!("a row without four columns: `{line}`"));
        };
        let q = item
            .strip_prefix(ENTITY)
            .and_then(|r| r.strip_suffix('>'))
            .ok_or_else(|| format!("not an entity: `{item}`"))?;
        let Some(n) = count(pop) else {
            continue;
        };
        let value = ((!time.is_empty()).then(|| time.to_string()), n);
        let entry = items
            .entry(q.to_string())
            .or_insert_with(|| (value.clone(), Vec::new()));
        if value > entry.0 {
            entry.0 = value;
        }
        if let Some(g) = unquote(gn).filter(|g| !g.is_empty())
            && !entry.1.contains(&g)
        {
            entry.1.push(g);
        }
    }
    let mut out = HashMap::with_capacity(items.len() * 2);
    for (q, ((_, n), gns)) in items {
        out.insert(format!("wikidata:{q}"), n);
        for g in gns {
            // A GeoNames ID on two items keeps the larger count, so the order of the file
            // does not decide it.
            let e = out.entry(format!("geonames:{g}")).or_insert(n);
            *e = (*e).max(n);
        }
    }
    Ok(out)
}

/// A population literal (`1119307.0`, `"1119307"^^<…#decimal>` or `1119307`) as a positive
/// count; `None` for anything else.
fn count(v: &str) -> Option<i64> {
    let v = v
        .strip_prefix('"')
        .map_or(v, |r| r.split('"').next().unwrap_or(r));
    let n: f64 = v.parse().ok()?;
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a finite count far below i64::MAX, checked"
    )]
    (n.is_finite() && (1.0..1e11).contains(&n)).then(|| n.round() as i64)
}

/// Gives each unit without a population the snapshot's, by its first source key that has one.
/// Returns the report's line.
pub(super) fn apply<'p>(
    source: &str,
    by_key: &HashMap<String, i64>,
    places: impl Iterator<Item = &'p mut Place>,
) -> String {
    // By level: (units, with their own population, given one).
    let mut by_level: BTreeMap<u8, (usize, usize, usize)> = BTreeMap::new();
    for p in places.filter(|p| p.kind == PlaceKind::Admin) {
        let c = by_level.entry(p.admin_level).or_default();
        c.0 += 1;
        if p.population.is_some() {
            c.1 += 1;
        } else if let Some(n) = p.sources.iter().find_map(|k| by_key.get(k)) {
            p.population = Some(*n);
            c.2 += 1;
        }
    }
    let levels: Vec<String> = by_level
        .iter()
        .map(|(l, (n, own, given))| format!("level {l}: {given} given, {own} their own, of {n}"))
        .collect();
    format!(
        "- Unit populations from {source} (D-266): {}.\n",
        levels.join("; ")
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests")]
mod tests {
    use super::{count, read};
    use std::fs;

    #[test]
    fn counts_parse_and_refuse() {
        assert_eq!(count("1119307.0"), Some(1_119_307));
        assert_eq!(
            count("\"42\"^^<http://www.w3.org/2001/XMLSchema#decimal>"),
            Some(42)
        );
        assert_eq!(count("0"), None);
        assert_eq!(count("-5"), None);
        assert_eq!(count("NaN"), None);
        assert_eq!(count("x"), None);
    }

    #[test]
    fn the_latest_dated_value_wins_then_the_largest() {
        let dir = std::env::temp_dir().join(format!("forge-pop-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pop.tsv");
        let e = "<http://www.wikidata.org/entity/";
        fs::write(
            &path,
            format!(
                "?item\t?gn\t?pop\t?time\n\
                 {e}Q1>\t\"11\"\t500.0\t2011-01-01T00:00:00Z\n\
                 {e}Q1>\t\"11\"\t400.0\t2021-01-01T00:00:00Z\n\
                 {e}Q1>\t\"12\"\t900.0\t\n\
                 {e}Q2>\t\t70.0\t\n\
                 {e}Q2>\t\t80.0\t\n\
                 {e}Q3>\t\"11\"\t9000.0\t\n"
            ),
        )
        .unwrap();
        let m = read(&path).unwrap();
        // The 2021 census beats the 2011 one and the undated value.
        assert_eq!(m.get("wikidata:Q1"), Some(&400));
        assert_eq!(m.get("geonames:12"), Some(&400));
        // Undated only: the largest.
        assert_eq!(m.get("wikidata:Q2"), Some(&80));
        // A GeoNames ID on two items keeps the larger count.
        assert_eq!(m.get("geonames:11"), Some(&9000));
        fs::write(&path, "?item\t?text\n").unwrap();
        assert!(read(&path).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
