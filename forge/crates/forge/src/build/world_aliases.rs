//! `forge world --aliases <source>` (D-268): a world row's other names in one language from a
//! Wikidata snapshot, so a search by a former or everyday name finds the place (Poona, Simla,
//! Tanjore, Calcutta, Saigon, Ceylon).
//!
//! The snapshot (`forge/queries/wikidata-aliases.rq`, its language in `query_params.LANG`) is a
//! `QLever` TSV of `?q ?gn ?text`: every alias of every item with a `GeoNames` ID and a
//! population, the item as a bare `Q…` string. The query drops what is no name: under 4
//! characters, starting in lower case (codes such as `th`, `lk`), or inverted with a comma
//! (`Lanka, Sri`). The forge then keeps an alias only when it adds a search key: not folded
//! equal to one of the row's names, and not the name of another place of the row's country
//! (a state's name filed under its capital). World-general (P16): no country or language named.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use teigeodata_names::fold;

use super::world_labels::unquote;
use teigeodata_format::names::{NameType, Origin};

use super::{Place, PlaceKind, PlaceName};

/// A snapshot's aliases: each item's, and the item of each source key (`wikidata:Q…`,
/// `geonames:<id>`).
#[derive(Debug, Default)]
pub(super) struct Aliases {
    items: Vec<Vec<String>>,
    by_key: HashMap<String, usize>,
}

/// Reads a snapshot (sorted by item, as the query writes it).
pub(super) fn read(path: &Path) -> Result<Aliases, String> {
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut lines = BufReader::with_capacity(1 << 20, file).lines();
    let header = lines
        .next()
        .ok_or("empty snapshot")?
        .map_err(|e| e.to_string())?;
    if header.split('\t').collect::<Vec<_>>() != ["?q", "?gn", "?text"] {
        return Err(format!("unexpected header `{header}`"));
    }
    let mut out = Aliases::default();
    for line in lines {
        let line = line.map_err(|e| e.to_string())?;
        let mut cols = line.split('\t');
        let (Some(q), Some(gn), Some(text), None) =
            (cols.next(), cols.next(), cols.next(), cols.next())
        else {
            return Err(format!("a row without three columns: `{line}`"));
        };
        let q = unquote(q)
            .filter(|q| {
                q.strip_prefix('Q')
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            })
            .ok_or_else(|| format!("not an item: `{q}`"))?;
        let text = unquote(text).ok_or_else(|| format!("not a literal: `{text}`"))?;
        let next = out.items.len();
        let item = *out.by_key.entry(format!("wikidata:{q}")).or_insert(next);
        if item == next {
            out.items.push(Vec::new());
        }
        if let Some(g) = unquote(gn).filter(|g| !g.is_empty()) {
            out.by_key.entry(format!("geonames:{g}")).or_insert(item);
        }
        if let Some(v) = out.items.get_mut(item)
            && !v.contains(&text)
        {
            v.push(text);
        }
    }
    Ok(out)
}

/// Which row takes an item's aliases when several rows carry it: a city before the unit of
/// its name and the unit before its country (D-158), then the first row.
fn order(kind: PlaceKind) -> Option<u8> {
    match kind {
        PlaceKind::Settlement => Some(0),
        PlaceKind::Admin => Some(1),
        PlaceKind::Country => Some(2),
        _ => None,
    }
}

/// Gives each country, unit and settlement its aliases in `lang` by its source keys, keeping
/// only those that add a search key. Returns the report's line.
pub(super) fn apply(
    source: &str,
    lang: &str,
    aliases: &Aliases,
    places: &mut [(&mut Place, Option<[u8; 2]>)],
) -> String {
    // Each country's place names, folded: an alias naming another place there is that place's.
    let mut named: HashMap<(Option<[u8; 2]>, String), usize> = HashMap::new();
    // Each item's row: one row takes an item's aliases, so a city and its unit don't both.
    let mut owner: HashMap<usize, (u8, usize)> = HashMap::new();
    for (i, (p, cc)) in places.iter().enumerate() {
        if let Some(n) = p.names.first() {
            named.entry((*cc, fold(&n.text))).or_insert(i);
        }
        let Some(rank) = order(p.kind) else { continue };
        for &item in p.sources.iter().filter_map(|k| aliases.by_key.get(k)) {
            let e = owner.entry(item).or_insert((rank, i));
            if rank < e.0 {
                *e = (rank, i);
            }
        }
    }
    let (mut added, mut rows, mut taken, mut around) = (0usize, 0usize, 0usize, 0usize);
    let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, (p, cc)) in places.iter_mut().enumerate() {
        let kind = match p.kind {
            PlaceKind::Country => "countries",
            PlaceKind::Admin => "units",
            PlaceKind::Settlement => "cities",
            _ => continue,
        };
        let own: Vec<String> = p
            .names
            .iter()
            .map(|n| fold(&n.text))
            .filter(|k| k.chars().count() >= MIN_CHARS)
            .collect();
        let mut seen: BTreeSet<String> = p.names.iter().map(|n| fold(&n.text)).collect();
        let mut new = Vec::new();
        let items: BTreeSet<usize> = p
            .sources
            .iter()
            .filter_map(|k| aliases.by_key.get(k).copied())
            .filter(|item| owner.get(item).is_some_and(|&(_, o)| o == i))
            .collect();
        for text in items
            .iter()
            .filter_map(|&item| aliases.items.get(item))
            .flatten()
        {
            let key = fold(text);
            if key.chars().count() < MIN_CHARS
                || text.contains('(')
                || !super::pack::fits(text)
                || !seen.insert(key.clone())
            {
                continue;
            }
            // Words around one of its names ("State of Japan", "Tokyo Prefecture"): the name
            // finds it already, by prefix or a word inside.
            if own.iter().any(|o| key.contains(o.as_str())) {
                around += 1;
                continue;
            }
            if named.get(&(*cc, key)).is_some_and(|&o| o != i) {
                taken += 1;
                continue;
            }
            new.push(PlaceName {
                text: text.clone(),
                lang: Some(lang.to_string()),
                kind: Some(NameType::Alt),
                origin: Origin::Wikidata,
            });
        }
        if !new.is_empty() {
            added += new.len();
            rows += 1;
            *by_kind.entry(kind).or_default() += 1;
            p.names.extend(new);
        }
    }
    let kinds: Vec<String> = by_kind.iter().map(|(k, n)| format!("{n} {k}")).collect();
    format!(
        "- {added} aliases ({lang}) on {rows} places ({}) from {source}; left out: {around} \
         holding one of the place's names, {taken} naming another place of its country (D-268).\n",
        kinds.join(", ")
    )
}

/// The fewest code points, folded, an alias must have to be worth its bytes, and a name must
/// have to make an alias holding it redundant.
const MIN_CHARS: usize = 4;

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests")]
mod tests {
    use super::{apply, read};
    use crate::build::{Place, PlaceKind, PlaceName};
    use std::fs;
    use teigeodata_format::names::Origin;

    fn place(kind: PlaceKind, name: &str, keys: &[&str]) -> Place {
        Place {
            key: keys[0].into(),
            kind,
            admin_level: 0,
            flags: 0,
            point: (0, 0),
            names: vec![PlaceName {
                text: name.into(),
                lang: None,
                kind: None,
                origin: Origin::GeoNames,
            }],
            population: None,
            rank: 0,
            parent: None,
            sources: keys.iter().map(|k| (*k).to_string()).collect(),
        }
    }

    #[test]
    fn aliases_add_only_new_search_keys() {
        let dir = std::env::temp_dir().join(format!("forge-alias-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.tsv");
        fs::write(
            &path,
            "?q\t?gn\t?text\n\
             \"Q1\"\t\"11\"\t\"Poona\"\n\
             \"Q1\"\t\"11\"\t\"PUNE\"\n\
             \"Q1\"\t\"11\"\t\"Deccan\"\n\
             \"Q1\"\t\"11\"\t\"Pune City\"\n\
             \"Q1\"\t\"11\"\t\"Pune (India)\"\n\
             \"Q2\"\t\"\"\t\"Ceylon\"\n\
             \"Q3\"\t\"\"\t\"Bee\"\n",
        )
        .unwrap();
        let aliases = read(&path).unwrap();
        let mut city = place(PlaceKind::Settlement, "Pune", &["geonames:11"]);
        let mut unit = place(PlaceKind::Admin, "Deccan", &["wikidata:Q9"]);
        let mut country = place(PlaceKind::Country, "Sri Lanka", &["wikidata:Q2"]);
        let mut tiny = place(PlaceKind::Settlement, "B", &["wikidata:Q3"]);
        // The unit of Pune carries the same item: the city takes its aliases.
        let mut district = place(PlaceKind::Admin, "Pune District", &["wikidata:Q1"]);
        let report = apply(
            "s",
            "en",
            &aliases,
            &mut [
                (&mut district, Some(*b"in")),
                (&mut city, Some(*b"in")),
                (&mut unit, Some(*b"in")),
                (&mut country, Some(*b"lk")),
                (&mut tiny, Some(*b"xx")),
            ],
        );
        let names = |p: &Place| p.names.iter().map(|n| n.text.clone()).collect::<Vec<_>>();
        // `PUNE` folds to the name, `Pune City` holds it, `Pune (India)` is qualified,
        // `Deccan` is another place of the country.
        assert_eq!(names(&city), ["Pune", "Poona"]);
        assert_eq!(names(&district), ["Pune District"]);
        assert_eq!(names(&country), ["Sri Lanka", "Ceylon"]);
        // Under four characters folded: no search key worth its bytes.
        assert_eq!(names(&tiny), ["B"]);
        assert!(report.contains("2 aliases (en) on 2 places"), "{report}");
        assert!(report.contains("1 holding one"), "{report}");
        assert!(report.contains("1 naming another place"), "{report}");
        fs::write(&path, "?item\t?text\n").unwrap();
        assert!(read(&path).is_err());
        fs::write(&path, "?q\t?gn\t?text\n\"X1\"\t\"\"\t\"A\"\n").unwrap();
        assert!(read(&path).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
