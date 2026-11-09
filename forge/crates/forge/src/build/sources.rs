//! Reading the stage outputs a build consumes: records, names, clusters and core points.

use super::PlaceName;
use crate::record::column;
use crate::record::for_each_batch;
use arrow_array::Array;
use arrow_array::Int64Array;
use arrow_array::StringArray;
use std::collections::BTreeMap;
use std::path::Path;
use teigeodata_format::names::NameType;

/// A settlement's rank from its source class: 4 a national capital, 3 a city or the seat of a
/// first-level unit, 2 a town or the seat of a second- or third-level unit, 1 a village or the
/// seat of a fourth-level unit, 0 anything else (hamlets, localities, plain `PPL`). Source
/// vocabularies only (OSM `place=*`, `GeoNames` feature codes, Wikidata classes): no country's
/// rule (P16).
pub(super) fn settlement_rank(class: &str) -> u8 {
    // A Wikidata class cell lists every P31 value, joined by `|`.
    class
        .split('|')
        .map(|c| match c {
            "P.PPLC" | "Q5119" => 4,
            "place=city" | "P.PPLA" | "Q515" | "Q1637706" => 3,
            "place=town" | "P.PPLA2" | "P.PPLA3" | "Q3957" => 2,
            "place=village" | "P.PPLA4" | "Q532" => 1,
            _ => 0,
        })
        .max()
        .unwrap_or(0)
}

/// A record of a points source, as the build needs it.
#[derive(Clone, Debug)]
pub(super) struct Rec {
    pub(super) kind: String,
    /// The source's class (`A.ADM3`, `place=village`).
    pub(super) class: String,
    pub(super) name: String,
    pub(super) point: Option<(i32, i32)>,
    pub(super) population: Option<i64>,
}

/// The consensus points of a `core` run, by cluster.
pub(super) fn core_points(
    build: &Path,
    run: &[String],
    buildings: &str,
) -> Result<BTreeMap<String, (i32, i32)>, String> {
    let path = build
        .join("core")
        .join(run.join("+"))
        .join(buildings)
        .join("points.parquet");
    let mut out = BTreeMap::new();
    for_each_batch(&path, |b| {
        let cl = column::<StringArray>(b, "cluster")?;
        let lon = column::<arrow_array::Int32Array>(b, "lon_e7")?;
        let lat = column::<arrow_array::Int32Array>(b, "lat_e7")?;
        for i in 0..b.num_rows() {
            out.insert(cl.value(i).to_string(), (lon.value(i), lat.value(i)));
        }
        Ok(())
    })
    .map_err(|e| format!("{}: {e}; run forge core first", path.display()))?;
    Ok(out)
}

pub(super) fn read_records(build: &Path, source: &str) -> Result<BTreeMap<String, Rec>, String> {
    let mut out = BTreeMap::new();
    for_each_batch(
        &build.join("read").join(source).join("records.parquet"),
        |b| {
            let ids = column::<StringArray>(b, "id")?;
            let kinds = column::<StringArray>(b, "kind")?;
            let classes = column::<StringArray>(b, "class")?;
            let names = column::<StringArray>(b, "name")?;
            let lat = column::<arrow_array::Int32Array>(b, "lat_e7")?;
            let lon = column::<arrow_array::Int32Array>(b, "lon_e7")?;
            let pop = column::<Int64Array>(b, "population")?;
            for i in 0..b.num_rows() {
                let point =
                    (lat.is_valid(i) && lon.is_valid(i)).then(|| (lon.value(i), lat.value(i)));
                out.insert(
                    ids.value(i).to_string(),
                    Rec {
                        kind: kinds.value(i).to_string(),
                        class: if classes.is_valid(i) {
                            classes.value(i).to_string()
                        } else {
                            String::new()
                        },
                        name: names.value(i).to_string(),
                        point,
                        population: pop.is_valid(i).then(|| pop.value(i)),
                    },
                );
            }
            Ok(())
        },
    )?;
    Ok(out)
}

pub(super) fn read_names(
    build: &Path,
    source: &str,
) -> Result<BTreeMap<String, Vec<PlaceName>>, String> {
    let mut out: BTreeMap<String, Vec<PlaceName>> = BTreeMap::new();
    for_each_batch(
        &build.join("read").join(source).join("names.parquet"),
        |b| {
            let ids = column::<StringArray>(b, "id")?;
            let text = column::<StringArray>(b, "text")?;
            let lang = column::<StringArray>(b, "lang")?;
            let kind = column::<StringArray>(b, "kind")?;
            for i in 0..b.num_rows() {
                out.entry(ids.value(i).to_string())
                    .or_default()
                    .push(PlaceName {
                        text: text.value(i).to_string(),
                        lang: Some(lang.value(i))
                            .filter(|l| !l.is_empty())
                            .map(str::to_string),
                        kind: name_type(kind.value(i)),
                    });
            }
            Ok(())
        },
    )?;
    Ok(out)
}

/// A source's name kind as a NAMES type (`None` for the plain primary name).
pub(super) fn name_type(kind: &str) -> Option<NameType> {
    match kind {
        "official" => Some(NameType::Official),
        "alias" | "alternate" | "short" => Some(NameType::Alt),
        "colloquial" => Some(NameType::Colloquial),
        "old" => Some(NameType::Historical),
        _ => None,
    }
}

/// A cluster's members, (source index, record ID), by cluster; a source the build may not read
/// contributes nothing.
pub(super) fn read_clusters(
    path: &Path,
    records: &[BTreeMap<String, Rec>],
) -> Result<BTreeMap<String, Vec<(usize, String)>>, String> {
    let mut clusters: BTreeMap<String, Vec<(usize, String)>> = BTreeMap::new();
    for_each_batch(path, |b| {
        let ids = column::<StringArray>(b, "id")?;
        let cl = column::<StringArray>(b, "cluster")?;
        for i in 0..b.num_rows() {
            let id = ids.value(i);
            if let Some(src) = records.iter().position(|r| r.contains_key(id)) {
                clusters
                    .entry(cl.value(i).to_string())
                    .or_default()
                    .push((src, id.to_string()));
            }
        }
        Ok(())
    })?;
    Ok(clusters)
}

#[cfg(test)]
mod tests {
    use super::settlement_rank;

    #[test]
    fn a_settlement_ranks_by_its_highest_class() {
        let cases = [
            ("place=city", 3),
            ("place=town", 2),
            ("place=village", 1),
            ("place=hamlet", 0),
            ("P.PPLC", 4),
            ("P.PPLA", 3),
            ("P.PPLA3", 2),
            ("P.PPLA4", 1),
            ("P.PPL", 0),
            // A Wikidata cell lists every class; the highest counts.
            ("Q532|Q1637706|Q486972", 3),
            ("Q3957|Q5119", 4),
            ("", 0),
        ];
        for (class, rank) in cases {
            assert_eq!(settlement_rank(class), rank, "{class}");
        }
    }
}
