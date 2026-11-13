//! `forge world --labels <source>…` (TG-14, D-221, D-257): names in a language for the world
//! part's rows. Two kinds of source:
//!
//! - Wikidata labels (`forge/queries/wikidata-labels.rq`, CC0), joined on a row's `wikidata:`
//!   or `geonames:` source key; the language is the source's `query_params.LANG`.
//! - A CLDR locale file (`common/main/<lang>.xml`, Unicode-3.0): its territory names, joined on
//!   a country row's ISO 3166-1 alpha-2 code; the language is the file's own identity. A
//!   territory's `alt` forms (short, variant) are kept as alternate names, so `अमेरिका` finds
//!   the United States.
//!
//! The sources are a chain: a row takes its names from the first source in the order given
//! that labels it, so CLDR, then Wikidata `ne`, then `hi` gives a country CLDR's maintained
//! name, a place Wikidata's Nepali label, and a Hindi one (truly tagged `hi`) only where there
//! is no Nepali. World-general (P16): no language or country is named in code.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use teigeodata_format::names::NameType;

use super::{Origin, Place, PlaceName};
use crate::lock::Lock;

const ENTITY: &str = "<http://www.wikidata.org/entity/";

/// The key prefix of a country's ISO 3166-1 alpha-2 code, as CLDR's territory names are joined.
const ISO: &str = "iso:";

/// One label source: its language, each key's label, and each key's alternate names.
pub(super) struct Labels {
    id: String,
    lang: String,
    origin: Origin,
    by_key: HashMap<String, String>,
    alts: HashMap<String, Vec<String>>,
}

impl Labels {
    /// The source's language.
    pub(super) fn lang(&self) -> &str {
        &self.lang
    }
}

/// Reads each source, in the chain's order: a SPARQL snapshot of Wikidata labels, or a CLDR
/// locale file (an `http` source whose URL ends in `.xml`).
pub(super) fn read(root: &Path, lock: &Lock, ids: &[String]) -> Result<Vec<Labels>, String> {
    ids.iter()
        .map(|id| {
            let s = lock.get(id)?;
            let path = lock.path(root, id)?;
            let at = |e: String| format!("{id}: {e}");
            if s.kind == "sparql" {
                let lang = s
                    .query_params
                    .get("LANG")
                    .cloned()
                    .ok_or_else(|| at("a label source names its LANG".into()))?;
                Ok(Labels {
                    id: id.clone(),
                    lang,
                    origin: Origin::Wikidata,
                    by_key: read_tsv(&path).map_err(at)?,
                    alts: HashMap::new(),
                })
            } else if Path::new(&s.url)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("xml"))
            {
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| at(format!("{}: {e}", path.display())))?;
                let ldml = read_ldml(&text).map_err(at)?;
                Ok(Labels {
                    id: id.clone(),
                    lang: ldml.lang,
                    origin: Origin::Standard,
                    by_key: ldml.names,
                    alts: ldml.alts,
                })
            } else {
                Err(at(format!(
                    "not a label source: kind `{}`, url {}",
                    s.kind, s.url
                )))
            }
        })
        .collect()
}

/// A CLDR locale file's language and territory names.
#[derive(Debug)]
struct Ldml {
    lang: String,
    names: HashMap<String, String>,
    alts: HashMap<String, Vec<String>>,
}

/// Reads a CLDR LDML file's `<identity>` language and its `<territories>`: each
/// `<territory type="XX">` under `iso:XX`, its `alt` forms as alternates. Only two-letter codes
/// (countries; CLDR's numeric regions and `EU`-like groupings never match a row anyway).
/// Entries marked `draft="unconfirmed"` or `"provisional"` are left out, as CLDR's own
/// resolution does by default.
fn read_ldml(text: &str) -> Result<Ldml, String> {
    let identity = section(text, "identity").ok_or("no <identity>")?;
    let lang = identity
        .split("<language ")
        .nth(1)
        .and_then(|t| attr(t, "type"))
        .ok_or("no identity language")?
        .to_string();
    let block = section(text, "territories").ok_or("no <territories>")?;
    let mut names = HashMap::new();
    let mut alts: HashMap<String, Vec<String>> = HashMap::new();
    for item in block.split("<territory ").skip(1) {
        let (open, rest) = item.split_once('>').ok_or("an unclosed <territory>")?;
        let body = rest
            .split_once("</territory>")
            .ok_or("a <territory> without its end tag")?
            .0;
        let code = attr(open, "type").ok_or("a <territory> without a type")?;
        if code.len() != 2 || !code.bytes().all(|b| b.is_ascii_uppercase()) {
            continue;
        }
        if matches!(attr(open, "draft"), Some("unconfirmed" | "provisional")) {
            continue;
        }
        let name = unescape_xml(body.trim())?;
        if name.is_empty() {
            continue;
        }
        let key = format!("{ISO}{code}");
        if attr(open, "alt").is_some() {
            alts.entry(key).or_default().push(name);
        } else if names.insert(key, name).is_some() {
            return Err(format!("territory {code} named twice"));
        }
    }
    if names.is_empty() {
        return Err("no territory names".into());
    }
    Ok(Ldml { lang, names, alts })
}

/// The text between `<tag>` and `</tag>`.
fn section<'t>(text: &'t str, tag: &str) -> Option<&'t str> {
    let start = text.find(&format!("<{tag}>"))? + tag.len() + 2;
    let len = text.get(start..)?.find(&format!("</{tag}>"))?;
    text.get(start..start + len)
}

/// An attribute's value in an opening tag's text (`type="NP" alt="short"`).
fn attr<'t>(tag: &'t str, name: &str) -> Option<&'t str> {
    let pat = format!("{name}=\"");
    let from = tag
        .match_indices(&pat)
        .find(|(i, _)| {
            *i == 0
                || tag
                    .as_bytes()
                    .get(i - 1)
                    .is_some_and(u8::is_ascii_whitespace)
        })?
        .0
        + pat.len();
    let rest = tag.get(from..)?;
    rest.get(..rest.find('"')?)
}

/// XML character data: the five predefined entities and numeric references; any other `&` is
/// refused.
fn unescape_xml(v: &str) -> Result<String, String> {
    let mut out = String::with_capacity(v.len());
    let mut rest = v;
    while let Some(i) = rest.find('&') {
        out.push_str(rest.get(..i).unwrap_or_default());
        let tail = rest.get(i + 1..).unwrap_or_default();
        let end = tail
            .find(';')
            .ok_or_else(|| format!("a bare `&` in `{v}`"))?;
        let ent = tail.get(..end).unwrap_or_default();
        let c = match ent {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            _ => ent
                .strip_prefix("#x")
                .map(|h| u32::from_str_radix(h, 16))
                .or_else(|| ent.strip_prefix('#').map(str::parse))
                .and_then(Result::ok)
                .and_then(char::from_u32)
                .ok_or_else(|| format!("an unknown entity `&{ent};` in `{v}`"))?,
        };
        out.push(c);
        rest = tail.get(end + 1..).unwrap_or_default();
    }
    out.push_str(rest);
    Ok(out)
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
/// summary's lines: rows named by each source. Each place comes with its country's ISO
/// 3166-1 alpha-2 code (either case) when it is a country row, the key CLDR's names are joined
/// on.
pub(super) fn apply<'p>(
    chain: &[Labels],
    places: impl Iterator<Item = (&'p mut Place, Option<[u8; 2]>)>,
) -> String {
    let mut named: BTreeMap<usize, usize> = BTreeMap::new();
    for (p, iso) in places {
        let iso = iso.and_then(|c| {
            let c = c.map(|b| b.to_ascii_uppercase());
            std::str::from_utf8(&c).ok().map(|c| format!("{ISO}{c}"))
        });
        let hit = chain.iter().enumerate().find_map(|(i, l)| {
            p.sources
                .iter()
                .chain(&iso)
                .find_map(|k| l.by_key.get_key_value(k))
                .map(|(k, t)| (i, l, k, t))
        });
        let Some((i, l, key, text)) = hit else {
            continue;
        };
        let alts = l.alts.get(key).map_or(&[][..], Vec::as_slice);
        let mut added = false;
        for (text, kind) in
            std::iter::once((text, None)).chain(alts.iter().map(|a| (a, Some(NameType::Alt))))
        {
            let name = PlaceName {
                text: text.clone(),
                lang: Some(l.lang.clone()),
                kind,
                origin: l.origin,
            };
            if super::pack::fits(text) && !p.names.iter().any(|n| n.same(&name)) {
                p.names.push(name);
                added = true;
            }
        }
        if added {
            *named.entry(i).or_default() += 1;
        }
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
    use super::{Labels, apply, read_ldml, unescape_xml, unquote};
    use crate::build::{Place, PlaceName};
    use std::collections::HashMap;
    use teigeodata_format::names::{NameType, Origin};

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
                origin: Origin::Wikidata,
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
            origin: Origin::Wikidata,
            by_key: pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect::<HashMap<_, _>>(),
            alts: HashMap::new(),
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
        let s = apply(&[ne, hi], ps.iter_mut().map(|p| (p, None)));
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

    const LDML: &str = r#"<?xml version="1.0" encoding="UTF-8" ?>
<ldml>
	<identity>
		<version number="$Revision$"/>
		<language type="ne"/>
	</identity>
	<localeDisplayNames>
		<languages>
			<language type="ne">नेपाली</language>
		</languages>
		<territories>
			<territory type="001">विश्व</territory>
			<territory type="NP">नेपाल</territory>
			<territory type="US">संयुक्त राज्य</territory>
			<territory type="US" alt="short">अमेरिका</territory>
			<territory type="XA" draft="unconfirmed">नक्कली</territory>
			<territory type="TT">त्रिनिडाड &amp; टोबागो</territory>
		</territories>
	</localeDisplayNames>
</ldml>"#;

    /// D-257: a CLDR file gives its own language, two-letter territories under `iso:`, alt forms
    /// apart, unconfirmed entries and numeric regions left out, entities decoded.
    #[test]
    fn a_cldr_file_gives_its_language_and_territory_names() {
        let l = read_ldml(LDML).unwrap();
        assert_eq!(l.lang, "ne");
        let mut names: Vec<_> = l
            .names
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                ("iso:NP", "नेपाल"),
                ("iso:TT", "त्रिनिडाड & टोबागो"),
                ("iso:US", "संयुक्त राज्य")
            ]
        );
        assert_eq!(l.alts.get("iso:US").unwrap(), &["अमेरिका".to_string()]);
        assert!(read_ldml("<ldml><identity><language type=\"ne\"/></identity></ldml>").is_err());
        let twice = LDML.replace("type=\"001\"", "type=\"NP\"");
        assert!(read_ldml(&twice).unwrap_err().contains("NP named twice"));
    }

    #[test]
    fn xml_text_unescapes_and_bad_entities_are_refused() {
        assert_eq!(
            unescape_xml("a &lt;b&gt; &#2344;&#x0947;").unwrap(),
            "a <b> ने"
        );
        assert!(unescape_xml("a & b").is_err());
        assert!(unescape_xml("&nbsp;").is_err());
    }

    /// D-257: CLDR first names a country by its code, with its alt form as an alternate, and the
    /// Wikidata label of the same language no longer adds a second name; a non-country row (no
    /// code) still takes Wikidata's.
    #[test]
    fn cldr_names_a_country_by_its_code_and_wikidata_the_rest() {
        let l = read_ldml(LDML).unwrap();
        let cldr = Labels {
            id: "cldr-ne".into(),
            lang: l.lang,
            origin: Origin::Standard,
            by_key: l.names,
            alts: l.alts,
        };
        let wd = labels(
            "ne",
            &[("wikidata:Q30", "संयुक्त राज्य अमेरिका"), ("geonames:1", "दुबई")],
        );
        let mut us = place(&["ne:USA", "wikidata:Q30"]);
        us.kind = teigeodata_core::PlaceKind::Country;
        let mut ps = [us, place(&["geonames:1"])];
        // Lower case, as `Namespace::country` gives it.
        let codes = [Some(*b"us"), None];
        let s = apply(&[cldr, wd], ps.iter_mut().zip(codes));
        let got = |p: &Place| {
            p.names
                .iter()
                .skip(1)
                .map(|n| (n.text.clone(), n.kind, n.origin))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            got(&ps[0]),
            [
                ("संयुक्त राज्य".to_string(), None, Origin::Standard),
                ("अमेरिका".to_string(), Some(NameType::Alt), Origin::Standard),
            ]
        );
        assert_eq!(got(&ps[1]), [("दुबई".to_string(), None, Origin::Wikidata)]);
        assert!(s.contains("1 places named in `ne` from cldr-ne"), "{s}");
    }
}
