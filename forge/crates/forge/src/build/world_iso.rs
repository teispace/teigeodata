//! `forge world --iso <validity> <codes>` (D-271): the ISO 3166 code of every world country row
//! and of every unit whose current ISO 3166-2 code the sources agree on, for the `ISO_CODES`
//! section (D-197).
//!
//! A country row that owns its alpha-2 code (D-257) takes it. A unit takes a code from, in
//! order, Wikidata's best-rank P300 on one of its items (`forge/queries/wikidata-iso3166-2.rq`),
//! then its own source's code (Natural Earth's `iso_3166_2`), each only when CLDR's subdivision
//! validity list marks it **regular** and its alpha-2 part is the row's country. A source's
//! stale code (India's `IN-OR`, Nepal's 14 zones) is deprecated in CLDR and never ships; two
//! regular codes for one unit, or one code for two units, leave them uncoded, reported.
//! World-general (P16): no country or code named.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;

use teigeodata_core::{IsoCode, PlaceKind};

use teigeodata_format::extract::Body;
use teigeodata_format::{Encoding, Kind};

use super::Place;
use super::world_labels::unquote;

/// CLDR's regular subdivision codes.
#[derive(Debug, Default)]
pub(super) struct Validity {
    regular: BTreeSet<IsoCode>,
    deprecated: usize,
}

/// Reads CLDR's `common/validity/subdivision.xml`: the `regular` list, ranges (`npp1~7`)
/// expanded.
pub(super) fn validity(path: &Path) -> Result<Validity, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = Validity::default();
    let mut rest = text.as_str();
    while let Some(start) = rest.find("<id type='subdivision' idStatus='") {
        let tail = rest.get(start + 33..).unwrap_or_default();
        let status = tail.split('\'').next().unwrap_or_default();
        let body_at = tail.find('>').ok_or("an unclosed <id>")? + 1;
        let body = tail.get(body_at..).unwrap_or_default();
        let end = body.find("</id>").ok_or("an <id> without </id>")?;
        let ids = body.get(..end).unwrap_or_default();
        rest = body.get(end..).unwrap_or_default();
        let ids = strip_comments(ids);
        for token in ids.split_whitespace() {
            for id in expand(token)? {
                match status {
                    "regular" => {
                        out.regular.insert(cldr_code(&id)?);
                    }
                    "deprecated" => out.deprecated += 1,
                    _ => {}
                }
            }
        }
    }
    if out.regular.is_empty() {
        return Err(format!("{}: no regular subdivision codes", path.display()));
    }
    Ok(out)
}

fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("<!--") {
        out.push_str(rest.get(..i).unwrap_or_default());
        rest = rest
            .get(i..)
            .and_then(|r| r.find("-->").and_then(|j| r.get(j + 3..)))
            .unwrap_or_default();
    }
    out.push_str(rest);
    out
}

/// `npp1~7` is `npp1` to `npp7`: the last character runs to the one after `~`.
fn expand(token: &str) -> Result<Vec<String>, String> {
    let Some((first, last)) = token.split_once('~') else {
        return Ok(vec![token.to_string()]);
    };
    let (stem, from) = first
        .char_indices()
        .last()
        .and_then(|(i, c)| first.get(..i).map(|s| (s, c)))
        .ok_or_else(|| format!("bad range `{token}`"))?;
    let mut to = last.chars();
    let (Some(to), None) = (to.next(), to.next()) else {
        return Err(format!("bad range `{token}`"));
    };
    if from > to {
        return Err(format!("bad range `{token}`"));
    }
    Ok((from..=to).map(|c| format!("{stem}{c}")).collect())
}

/// CLDR's `inup` is ISO's `IN-UP`.
fn cldr_code(id: &str) -> Result<IsoCode, String> {
    let (cc, sub) = (id.get(..2), id.get(2..));
    let (Some(cc), Some(sub)) = (cc, sub) else {
        return Err(format!("bad subdivision `{id}`"));
    };
    format!("{cc}-{sub}")
        .parse()
        .map_err(|e| format!("bad subdivision `{id}`: {e:?}"))
}

/// Wikidata's codes by source key (`wikidata:Q…`).
pub(super) fn wikidata(path: &Path) -> Result<HashMap<String, Vec<String>>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut lines = text.lines();
    if lines.next().map(|h| h.split('\t').collect::<Vec<_>>()) != Some(vec!["?q", "?code"]) {
        return Err(format!("{}: unexpected header", path.display()));
    }
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for line in lines {
        let (Some(q), Some(code)) = line
            .split_once('\t')
            .map(|(q, c)| (unquote(q), unquote(c)))
            .unwrap_or_default()
        else {
            return Err(format!("not a row: `{line}`"));
        };
        out.entry(format!("wikidata:{q}")).or_default().push(code);
    }
    Ok(out)
}

/// One row as the assignment sees it.
pub(super) struct Input<'a> {
    pub(super) place: &'a Place,
    /// Its country (lower case), when it has one.
    pub(super) country: Option<[u8; 2]>,
    /// The country row that owns the alpha-2 code (D-257).
    pub(super) owns_country_code: bool,
    /// Its own source's code (Natural Earth's `iso_3166_2`), as given.
    pub(super) source_code: Option<&'a str>,
}

/// Each coded row's key and code, and the list they were checked against.
#[derive(Debug)]
pub(crate) struct Codes {
    by_key: BTreeMap<String, IsoCode>,
    valid: Validity,
}

impl Codes {
    /// The `ISO_CODES` body of the rows `row_of` numbers, gated (G-iso) before it ships; none
    /// without codes.
    pub(super) fn section(
        &self,
        row_of: &BTreeMap<&str, u32>,
        places: &[&Place],
        country: impl Fn(u32) -> Option<[u8; 2]>,
    ) -> Result<Option<Body>, String> {
        let mut rows: Vec<(u32, IsoCode)> = self
            .by_key
            .iter()
            .filter_map(|(k, &c)| row_of.get(k.as_str()).map(|&r| (r, c)))
            .collect();
        rows.sort_unstable();
        if rows.len() != self.by_key.len() {
            return Err("an ISO code on a row the part does not hold".into());
        }
        gate(&rows, places, country, &self.valid)?;
        let body = teigeodata_format::iso_codes::encode(&rows).map_err(|e| e.to_string())?;
        Ok(body.map(|b| (Kind::ISO_CODES, Encoding::TABLE, rows.len() as u64, b)))
    }
}

/// Each coded row's key and code, and the report's lines.
pub(super) fn assign(
    rows: &[Input<'_>],
    valid: Validity,
    wikidata: &HashMap<String, Vec<String>>,
) -> (Codes, String) {
    // Each code's claimants: row and whether Wikidata gave it (true) or the row's source.
    let mut claims: BTreeMap<IsoCode, Vec<(usize, bool)>> = BTreeMap::new();
    let (mut countries, mut from_wd, mut from_source, mut units) = (0, 0, 0, 0);
    let (mut two, mut foreign, mut none) = (0, 0, 0);
    let mut stale: Vec<String> = Vec::new();
    let parse = |c: &str| c.parse::<IsoCode>().ok().filter(|c| !c.is_country());
    for (i, r) in rows.iter().enumerate() {
        let Some(cc) = r.country else { continue };
        let up = cc.map(|b| b.to_ascii_uppercase());
        match r.place.kind {
            PlaceKind::Country if r.owns_country_code => {
                if let Ok(code) = IsoCode::from_bytes(&up) {
                    claims.entry(code).or_default().push((i, true));
                    countries += 1;
                }
            }
            PlaceKind::Admin => {
                units += 1;
                let wd: BTreeSet<IsoCode> = r
                    .place
                    .sources
                    .iter()
                    .filter_map(|k| wikidata.get(k))
                    .flatten()
                    .filter_map(|c| parse(c))
                    .collect();
                let src: BTreeSet<IsoCode> = r.source_code.and_then(parse).into_iter().collect();
                // A source's current codes of the row's country.
                let current = |set: &BTreeSet<IsoCode>| -> Vec<IsoCode> {
                    set.iter()
                        .copied()
                        .filter(|c| valid.regular.contains(c) && c.country() == up)
                        .collect()
                };
                let (found, by_wd) = match current(&wd) {
                    v if v.is_empty() => (current(&src), false),
                    v => (v, true),
                };
                match found.as_slice() {
                    [code] => {
                        claims.entry(*code).or_default().push((i, by_wd));
                        if by_wd {
                            from_wd += 1;
                        } else {
                            from_source += 1;
                        }
                    }
                    [_, _, ..] => two += 1,
                    [] if wd.iter().chain(&src).any(|c| valid.regular.contains(c)) => {
                        foreign += 1;
                    }
                    [] if wd.is_empty() && src.is_empty() => none += 1,
                    [] => {
                        let codes: Vec<&str> = wd.iter().chain(&src).map(IsoCode::as_str).collect();
                        stale.push(format!("{} {}", r.place.key, codes.join("/")));
                    }
                }
            }
            _ => {}
        }
    }
    let mut out = BTreeMap::new();
    let mut shared = Vec::new();
    for (code, who) in claims {
        // Several rows: the one Wikidata files the code under, when it is one, else none.
        let by_wd: Vec<usize> = who.iter().filter(|w| w.1).map(|w| w.0).collect();
        let owner = match (who.as_slice(), by_wd.as_slice()) {
            ([(i, _)], _) | (_, [i]) => Some(*i),
            _ => None,
        };
        match owner.and_then(|i| rows.get(i)) {
            Some(r) => {
                out.insert(r.place.key.clone(), code);
            }
            None => shared.push(format!("{code} ({})", who.len())),
        }
    }
    let coded_units = out.len().saturating_sub(countries);
    let report = format!(
        "- ISO 3166 codes (D-271): {countries} countries; {coded_units} of {units} units \
         ({from_wd} by Wikidata, {from_source} by their source), checked against CLDR's {} \
         current codes; left uncoded: {none} with no code in either source, {} with only \
         stale codes{}, {foreign} with another country's code, {two} with two current codes, \
         {} codes claimed by more than one row{}.\n",
        valid.regular.len(),
        stale.len(),
        examples(&stale),
        shared.len(),
        examples(&shared),
    );
    (Codes { by_key: out, valid }, report)
}

/// How many examples a report line names.
const SAMPLE: usize = 12;

/// ` (a, b, …, n more)`, or nothing for none.
fn examples(v: &[String]) -> String {
    if v.is_empty() {
        return String::new();
    }
    let head: Vec<&str> = v.iter().take(SAMPLE).map(String::as_str).collect();
    match v.len().saturating_sub(SAMPLE) {
        0 => format!(" ({})", head.join(", ")),
        more => format!(" ({}, {more} more)", head.join(", ")),
    }
}

/// Gate G-iso on the world part's codes: every subdivision code regular in CLDR and carrying
/// its row's country, every country code on a country row, no code twice.
pub(super) fn gate(
    codes: &[(u32, IsoCode)],
    places: &[&Place],
    country: impl Fn(u32) -> Option<[u8; 2]>,
    valid: &Validity,
) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for &(row, code) in codes {
        let p = places
            .get(row as usize)
            .ok_or_else(|| format!("{code}: row {row} out of range"))?;
        if !seen.insert(code) {
            return Err(format!("G-iso: {code} on two rows"));
        }
        let cc = country(row).map(|c| c.map(|b| b.to_ascii_uppercase()));
        if cc != Some(code.country()) {
            return Err(format!(
                "G-iso: {code} on {}, a row of another country",
                p.key
            ));
        }
        if code.is_country() {
            if p.kind != PlaceKind::Country {
                return Err(format!("G-iso: country code {code} on {}", p.key));
            }
        } else if !valid.regular.contains(&code) {
            return Err(format!("G-iso: {code} on {} is not current in CLDR", p.key));
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests")]
mod tests {
    use super::{Input, assign, gate, validity, wikidata};
    use crate::build::{Place, PlaceKind, PlaceName};
    use std::fs;
    use teigeodata_core::IsoCode;
    use teigeodata_format::names::Origin;

    fn place(kind: PlaceKind, key: &str, refs: &[&str]) -> Place {
        Place {
            key: key.into(),
            kind,
            admin_level: u8::from(kind == PlaceKind::Admin),
            flags: 0,
            point: (0, 0),
            names: vec![PlaceName {
                text: key.into(),
                lang: None,
                kind: None,
                origin: Origin::GeoNames,
            }],
            population: None,
            rank: 0,
            parent: None,
            sources: refs.iter().map(|k| (*k).to_string()).collect(),
        }
    }

    #[test]
    fn units_take_only_current_codes_of_their_country() {
        let dir = std::env::temp_dir().join(format!("forge-iso-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let cldr = dir.join("subdivision.xml");
        fs::write(
            &cldr,
            "<supplementalData><idValidity>\n\
             <id type='subdivision' idStatus='regular'>\t<!-- 9 items -->\n\
             \t\tinod inuk inup\n\t\tnpp1~7 <!-- seven -->\n</id>\n\
             <id type='subdivision' idStatus='deprecated'>\n\t\tinor inut npba\n</id>\n\
             </idValidity></supplementalData>\n",
        )
        .unwrap();
        let valid = validity(&cldr).unwrap();
        assert_eq!(valid.regular.len(), 10);
        assert!(valid.regular.contains(&"NP-P7".parse().unwrap()));
        let wd = dir.join("wd.tsv");
        fs::write(
            &wd,
            "?q\t?code\n\"Q1\"\t\"IN-OD\"\n\"Q1\"\t\"IN-OR\"\n\"Q2\"\t\"NP-BA\"\n\
             \"Q3\"\t\"IN-UP\"\n\"Q3\"\t\"IN-UK\"\n\"Q4\"\t\"NP-P1\"\n\"Q5\"\t\"IN-UP\"\n",
        )
        .unwrap();
        let codes = wikidata(&wd).unwrap();
        let rows = [
            place(PlaceKind::Country, "in", &[]),
            place(PlaceKind::Country, "in-claim", &[]),
            place(PlaceKind::Admin, "odisha", &["wikidata:Q1"]),
            place(PlaceKind::Admin, "zone", &["wikidata:Q2"]),
            place(PlaceKind::Admin, "two", &["wikidata:Q3"]),
            place(PlaceKind::Admin, "abroad", &["wikidata:Q4"]),
            place(PlaceKind::Admin, "up", &["wikidata:Q5"]),
            place(PlaceKind::Admin, "uttarakhand", &[]),
            place(PlaceKind::Admin, "stale-source", &[]),
        ];
        let src = [
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some("IN-UK"),
            Some("IN-UT"),
        ];
        let cc = |i: usize| if i == 3 { *b"np" } else { *b"in" };
        let input: Vec<Input<'_>> = rows
            .iter()
            .enumerate()
            .map(|(i, p)| Input {
                place: p,
                country: Some(cc(i)),
                owns_country_code: i == 0,
                source_code: src[i],
            })
            .collect();
        let (out, report) = assign(&input, validity(&cldr).unwrap(), &codes);
        let got: Vec<(&str, &str)> = out
            .by_key
            .iter()
            .map(|(k, c)| (k.as_str(), c.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("in", "IN"),
                ("odisha", "IN-OD"),
                ("up", "IN-UP"),
                ("uttarakhand", "IN-UK")
            ]
        );
        assert!(
            report.contains("2 with only stale codes (zone NP-BA, stale-source IN-UT)"),
            "{report}"
        );
        assert!(report.contains("1 with another country's code"), "{report}");
        assert!(report.contains("1 with two current codes"), "{report}");

        // The gate: a planted stale code, a code on another country's row, a duplicate.
        let places: Vec<&Place> = rows.iter().collect();
        let country = |r: u32| Some(cc(r as usize));
        let ok: Vec<(u32, IsoCode)> =
            vec![(0, "IN".parse().unwrap()), (2, "IN-OD".parse().unwrap())];
        gate(&ok, &places, country, &valid).unwrap();
        for bad in [
            vec![(2, "IN-OR".parse().unwrap())],
            vec![(3, "IN-UP".parse().unwrap())],
            vec![(2, "IN-OD".parse().unwrap()), (4, "IN-OD".parse().unwrap())],
            vec![(2, "IN".parse().unwrap())],
        ] {
            assert!(gate(&bad, &places, country, &valid).is_err(), "{bad:?}");
        }
        fs::remove_dir_all(&dir).unwrap();
    }
}
