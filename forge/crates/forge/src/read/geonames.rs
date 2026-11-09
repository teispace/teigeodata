//! `read/geonames`: a `GeoNames` country dump (`<CC>.zip`), its alternate names
//! (`alternateNamesV2.zip`, streamed and filtered to the country's IDs) and the hierarchy
//! (`hierarchy.zip`). Format: the TSV tables of the dump's `readme.txt`. Every line is split
//! into a fixed array, so reading allocates nothing per line beyond the rows it keeps.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::record::{Link, Name, Output, Record, e7};

/// The first `N` tab-separated fields of a line; missing fields are empty.
fn fields<const N: usize>(line: &str) -> [&str; N] {
    let mut it = line.split('\t');
    std::array::from_fn(|_| it.next().unwrap_or(""))
}

/// Reads a country dump plus, when given, the alternate names and the hierarchy.
pub(crate) fn read(
    country_zip: &Path,
    alternate_zip: Option<&Path>,
    hierarchy_zip: Option<&Path>,
) -> Result<Output, String> {
    let mut out = Output::default();
    let mut ids: BTreeSet<u64> = BTreeSet::new();
    let entry = country_entry(country_zip)?;
    for_each_line(country_zip, &entry, |line| {
        main_row(line, &mut ids, &mut out);
    })?;
    if let Some(zip) = alternate_zip {
        for_each_line(zip, "alternateNamesV2.txt", |line| {
            alternate_row(line, &ids, &mut out);
        })?;
    }
    if let Some(zip) = hierarchy_zip {
        for_each_line(zip, "hierarchy.txt", |line| {
            // parentId, childId, type
            let [parent, child] = fields::<2>(line);
            if let (Ok(p), Ok(c)) = (parent.parse::<u64>(), child.parse::<u64>())
                && ids.contains(&c)
            {
                out.links.push(Link {
                    id: format!("geonames:{c}"),
                    rel: "parent".into(),
                    target: format!("geonames:{p}"),
                });
            }
        })?;
    }
    Ok(out)
}

/// geonameid, name, asciiname, alternatenames, latitude, longitude, feature class, feature
/// code, country code, cc2, admin1–4, population, elevation, dem, timezone, modification date.
fn main_row(line: &str, ids: &mut BTreeSet<u64>, out: &mut Output) {
    let [
        gid,
        name,
        _,
        _,
        lat,
        lon,
        fclass,
        fcode,
        cc,
        _,
        a1,
        a2,
        a3,
        a4,
        pop,
        ele,
        dem,
    ] = fields::<17>(line);
    let Ok(gid) = gid.parse::<u64>() else { return };
    ids.insert(gid);
    let id = format!("geonames:{gid}");
    let kind = match fclass {
        "P" => "populated_place",
        "A" => "admin",
        _ => "other",
    };
    out.records.push(Record {
        id: id.clone(),
        class: format!("{fclass}.{fcode}"),
        kind: kind.to_string(),
        name: name.to_string(),
        lat_e7: lat.parse().ok().and_then(e7),
        lon_e7: lon.parse().ok().and_then(e7),
        elevation_m: ele.parse().ok(),
        // GeoNames writes -9999 where its DEM has no value.
        dem_m: dem.parse::<f32>().ok().filter(|d| *d > -9000.0),
        population: pop.parse().ok().filter(|p| *p > 0),
        dissolved: None,
    });
    out.names.push(Name {
        id: id.clone(),
        text: name.to_string(),
        lang: String::new(),
        kind: "name".into(),
    });
    // The admin codes are the country's parent keys: kept as stated, resolved later.
    for (level, code) in [(1, a1), (2, a2), (3, a3), (4, a4)] {
        if !code.is_empty() {
            out.links.push(Link {
                id: id.clone(),
                rel: "parent".into(),
                target: format!("geonames-admin{level}:{cc}.{code}"),
            });
        }
    }
}

/// alternateNameId, geonameid, isolanguage, name, isPreferredName, isShortName, isColloquial,
/// isHistoric, from, to.
fn alternate_row(line: &str, ids: &BTreeSet<u64>, out: &mut Output) {
    let [_, gid, lang, text, preferred, short, colloquial, historic] = fields::<8>(line);
    let Ok(gid) = gid.parse::<u64>() else { return };
    if !ids.contains(&gid) {
        return;
    }
    let id = format!("geonames:{gid}");
    let same_as = |target: String| Link {
        id: id.clone(),
        rel: "same_as".into(),
        target,
    };
    match lang {
        // Not names: links and codes.
        "link" => return out.links.push(same_as(text.to_string())),
        "wkdt" => return out.links.push(same_as(format!("wikidata:{text}"))),
        // A city's IATA code and a stated abbreviation are what people type (KTM for
        // Kathmandu): short names with no language. Airports carry codes too, but never
        // become places.
        "iata" | "abbr" => {
            return out.names.push(Name {
                id,
                text: text.to_string(),
                lang: String::new(),
                kind: "short".into(),
            });
        }
        "post" | "icao" | "faac" | "tcid" | "unlc" | "fr_1793" => return,
        _ => {}
    }
    let kind = if historic == "1" {
        "old"
    } else if colloquial == "1" {
        "colloquial"
    } else if short == "1" {
        "short"
    } else if preferred == "1" {
        "official"
    } else {
        "alternate"
    };
    out.names.push(Name {
        id,
        text: text.to_string(),
        lang: lang.to_string(),
        kind: kind.into(),
    });
}

/// The country table inside a dump: the one `.txt` entry that is not `readme.txt`.
fn country_entry(zip: &Path) -> Result<String, String> {
    let file = File::open(zip).map_err(|e| format!("{}: {e}", zip.display()))?;
    let archive = zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", zip.display()))?;
    let mut found: Vec<String> = archive
        .file_names()
        .filter_map(Result::ok)
        .filter(|n| n.ends_with(".txt") && n != "readme.txt")
        .map(std::borrow::Cow::into_owned)
        .collect();
    found.sort_unstable();
    match found.as_slice() {
        [one] => Ok(one.clone()),
        _ => Err(format!(
            "{}: expected one country table, found {found:?}",
            zip.display()
        )),
    }
}

/// Calls `f` for every line of one entry of a zip archive, streaming, reusing one buffer.
fn for_each_line(zip: &Path, entry: &str, mut f: impl FnMut(&str)) -> Result<(), String> {
    let file = File::open(zip).map_err(|e| format!("{}: {e}", zip.display()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", zip.display()))?;
    let reader = archive
        .by_name(entry)
        .map_err(|e| format!("{}: {entry}: {e}", zip.display()))?;
    let mut lines = BufReader::with_capacity(1 << 20, reader);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let n = lines
            .read_until(b'\n', &mut buf)
            .map_err(|e| format!("{entry}: {e}"))?;
        if n == 0 {
            break;
        }
        // GeoNames is UTF-8; a broken line is skipped rather than guessed.
        if let Ok(s) = std::str::from_utf8(&buf) {
            f(s.trim_end_matches(['\n', '\r']));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Output, alternate_row, fields, main_row};
    use std::collections::BTreeSet;

    #[test]
    fn reads_rows() {
        assert_eq!(fields::<3>("a\tb"), ["a", "b", ""]);
        let mut ids = BTreeSet::new();
        let mut out = Output::default();
        main_row(
            "1283240\tKathmandu\tKathmandu\tKTM\t27.70169\t85.3206\tP\tPPLC\tNP\t\t3\t26\t\t\t1442271\t\t1317\tAsia/Kathmandu\t2024-01-01",
            &mut ids,
            &mut out,
        );
        let r = out.records.first().cloned().unwrap_or_default();
        assert_eq!((r.lat_e7, r.lon_e7), (Some(277_016_900), Some(853_206_000)));
        assert_eq!((r.dem_m, r.population), (Some(1317.0), Some(1_442_271)));
        alternate_row("9\t1283240\ten\tYen\t\t\t1\t", &ids, &mut out);
        alternate_row("10\t1283240\twkdt\tQ3037\t\t\t\t", &ids, &mut out);
        alternate_row("11\t999\ten\tOther\t\t\t\t", &ids, &mut out);
        alternate_row("12\t1283240\tiata\tKTM\t\t\t\t", &ids, &mut out);
        alternate_row("13\t1283240\ticao\tVNKT\t\t\t\t", &ids, &mut out);
        assert!(
            out.names
                .iter()
                .any(|n| n.text == "Yen" && n.kind == "colloquial")
        );
        assert!(out.links.iter().any(|l| l.target == "wikidata:Q3037"));
        assert!(!out.names.iter().any(|n| n.text == "Other"));
        assert!(
            out.names
                .iter()
                .any(|n| n.text == "KTM" && n.kind == "short" && n.lang.is_empty())
        );
        assert!(!out.names.iter().any(|n| n.text == "VNKT"));
    }
}
