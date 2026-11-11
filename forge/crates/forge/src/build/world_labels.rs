//! `forge world --labels <source>…` (TG-14, D-221): names in a language for the world part's
//! rows from Wikidata labels (`forge/queries/wikidata-labels.rq`, CC0), joined on a row's
//! `wikidata:` or `geonames:` source key. The sources are a chain: a row takes a name from the
//! first source in the order given that labels it, so `ne` then `hi` gives a Nepali name where
//! Wikidata has one and a Hindi one (truly tagged `hi`) only where it has none. World-general
//! (P16): the language is each source's `query_params.LANG`, never named in code.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use super::{Place, PlaceName};
use crate::lock::Lock;

const ENTITY: &str = "<http://www.wikidata.org/entity/";

/// One label source: its language and each source key's label.
pub(super) struct Labels {
    id: String,
    lang: String,
    by_key: HashMap<String, String>,
}

/// Reads each source, in the chain's order.
pub(super) fn read(root: &Path, lock: &Lock, ids: &[String]) -> Result<Vec<Labels>, String> {
    ids.iter()
        .map(|id| {
            let s = lock.get(id)?;
            let lang = s
                .query_params
                .get("LANG")
                .cloned()
                .ok_or_else(|| format!("{id}: a label source names its LANG"))?;
            let path = lock.path(root, id)?;
            let by_key = read_tsv(&path).map_err(|e| format!("{id}: {e}"))?;
            Ok(Labels {
                id: id.clone(),
                lang,
                by_key,
            })
        })
        .collect()
}

/// A `QLever` TSV of `?item ?gn ?text`: each label under `wikidata:Q…` and, when the item has
/// one, `geonames:<id>`. An item with two labels in one language cannot occur (Wikidata has one
/// label per language); a `GeoNames` ID on two items keeps the first item's label.
fn read_tsv(path: &Path) -> Result<HashMap<String, String>, String> {
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut lines = BufReader::with_capacity(1 << 20, file).lines();
    let header = lines
        .next()
        .ok_or("empty snapshot")?
        .map_err(|e| e.to_string())?;
    if header.split('\t').collect::<Vec<_>>() != ["?item", "?gn", "?text"] {
        return Err(format!("unexpected header `{header}`"));
    }
    let mut out = HashMap::new();
    for line in lines {
        let line = line.map_err(|e| e.to_string())?;
        let mut cols = line.split('\t');
        let (Some(item), Some(gn), Some(text), None) =
            (cols.next(), cols.next(), cols.next(), cols.next())
        else {
            return Err(format!("a row without three columns: `{line}`"));
        };
        let q = item
            .strip_prefix(ENTITY)
            .and_then(|r| r.strip_suffix('>'))
            .ok_or_else(|| format!("not an entity: `{item}`"))?;
        let text = unquote(text).ok_or_else(|| format!("not a literal: `{text}`"))?;
        if text.is_empty() {
            continue;
        }
        out.entry(format!("wikidata:{q}"))
            .or_insert_with(|| text.clone());
        if let Some(g) = unquote(gn).filter(|g| !g.is_empty()) {
            out.entry(format!("geonames:{g}")).or_insert(text);
        }
    }
    Ok(out)
}

/// A TSV literal's text: `"…"` with `\"`, `\\`, `\t` and `\n` escapes, NFC as Wikidata stores it.
fn unquote(v: &str) -> Option<String> {
    let inner = v.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next()? {
                't' => out.push('\t'),
                'n' => out.push('\n'),
                e => out.push(e),
            }
        } else {
            out.push(c);
        }
    }
    Some(out)
}

/// Adds the chain's names to `places` (after each one's existing names) and returns the
/// summary's lines: rows named by each source.
pub(super) fn apply<'p>(chain: &[Labels], places: impl Iterator<Item = &'p mut Place>) -> String {
    let mut named: BTreeMap<usize, usize> = BTreeMap::new();
    for p in places {
        let hit = chain.iter().enumerate().find_map(|(i, l)| {
            p.sources
                .iter()
                .find_map(|k| l.by_key.get(k))
                .map(|t| (i, l, t))
        });
        let Some((i, l, text)) = hit else { continue };
        if !super::pack::fits(text) || p.names.iter().any(|n| n.text == *text) {
            continue;
        }
        p.names.push(PlaceName {
            text: text.clone(),
            lang: Some(l.lang.clone()),
            kind: None,
        });
        *named.entry(i).or_default() += 1;
    }
    let mut out = String::new();
    for (i, l) in chain.iter().enumerate() {
        let _ = writeln!(
            out,
            "- {} places named in `{}` from {} ({} labels).",
            named.get(&i).copied().unwrap_or(0),
            l.lang,
            l.id,
            l.by_key.len()
        );
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests")]
mod tests {
    use super::{Labels, apply, unquote};
    use crate::build::{Place, PlaceName};
    use std::collections::HashMap;

    fn place(sources: &[&str]) -> Place {
        Place {
            key: sources[0].into(),
            kind: teigeodata_core::PlaceKind::Settlement,
            admin_level: 0,
            flags: 0,
            point: (0, 0),
            names: vec![PlaceName {
                text: "Dubai".into(),
                lang: None,
                kind: None,
            }],
            population: None,
            rank: 0,
            parent: None,
            sources: sources.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    fn labels(lang: &str, pairs: &[(&str, &str)]) -> Labels {
        Labels {
            id: format!("wikidata-labels-{lang}"),
            lang: lang.into(),
            by_key: pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect::<HashMap<_, _>>(),
        }
    }

    #[test]
    fn the_first_source_that_labels_a_row_names_it() {
        let ne = labels("ne", &[("geonames:1", "दुबई")]);
        let hi = labels("hi", &[("geonames:1", "दुबई2"), ("wikidata:Q2", "बोस्टन")]);
        let mut ps = [
            place(&["geonames:1"]),
            place(&["ne:X", "wikidata:Q2"]),
            place(&["geonames:9"]),
        ];
        let s = apply(&[ne, hi], ps.iter_mut());
        let tags = |p: &Place| {
            p.names
                .iter()
                .skip(1)
                .map(|n| (n.text.clone(), n.lang.clone().unwrap()))
                .collect::<Vec<_>>()
        };
        assert_eq!(tags(&ps[0]), [("दुबई".to_string(), "ne".to_string())]);
        assert_eq!(tags(&ps[1]), [("बोस्टन".to_string(), "hi".to_string())]);
        assert_eq!(tags(&ps[2]).len(), 0);
        assert!(s.contains("1 places named in `ne`") && s.contains("1 places named in `hi`"));
    }

    #[test]
    fn literals_unescape_and_bad_ones_are_refused() {
        assert_eq!(unquote(r#""a\"b\\c""#).unwrap(), r#"a"b\c"#);
        assert_eq!(unquote("\"\"").unwrap(), "");
        assert!(unquote("bare").is_none());
    }
}
