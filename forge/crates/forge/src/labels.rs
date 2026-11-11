//! `forge labels`: the hand-labelled truth set for populated places (Q-005, D-062,
//! [settlement-core](../../../../docs/02-research/notes/settlement-core.md) §3.4).
//!
//! ```text
//! forge labels sample <cc> <points-source> <count-per-stratum> <seed>
//! forge labels score <cc> <points run> <buildings run>
//! ```
//!
//! - `sample` draws places from one points source's populated places, stratified by class
//!   (hamlet, village, town, from the source class) and terrain (the source's stated DEM height
//!   below 400 m, a proxy for slope below 5° until the DEM section lands), and writes
//!   `tests/golden/<cc>-places.csv` for labellers. The draw is the lowest BLAKE3 of
//!   (seed, record ID) per stratum, so it is the same on every run and needs no random number
//!   generator. Every fifth row is marked for a second labeller. The locator is the source
//!   point truncated to 0.01° (about 1 km; the place lies in the cell north-east of it): enough
//!   to find the place, too coarse to anchor the label. Nothing from the forge's own outputs is shown, so the labels stay independent of
//!   the core they judge (the D-060 rule).
//! - `score` reads the labelled rows (`label_lat`, `label_lon` filled) and reports, per
//!   stratum, how far the source point and the consensus point of the `core` run are from the
//!   label: P50, P90 and the share within the D-062 target (500 m in the lowland stratum, 150 m
//!   in the hills).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use arrow_array::{Array as _, Float32Array, Int32Array, StringArray};

use crate::record::{column, e7, for_each_batch};

/// The terrain split: a stated DEM height below this is "lowland" (proxy for slope < 5°).
const LOWLAND_M: f32 = 400.0;
const HEADER: &str =
    "id,name,class,terrain,locator_lat,locator_lon,second,label_lat,label_lon,labeller,notes";

/// The class of a source class, for sampling (hamlet, village, town).
fn class_of(class: &str) -> Option<&'static str> {
    Some(match class {
        "P.PPLL" | "place=hamlet" | "place=isolated_dwelling" => "hamlet",
        "P.PPL" | "place=village" => "village",
        "P.PPLA" | "P.PPLA2" | "P.PPLA3" | "P.PPLA4" | "place=town" => "town",
        _ => return None,
    })
}

/// A CSV field: quoted when it holds a comma, a quote or a line break.
pub(crate) fn field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Splits one CSV line (quotes as written by [`field`]).
pub(crate) fn split(line: &str) -> Vec<String> {
    let (mut out, mut cur, mut quoted, mut chars) =
        (Vec::new(), String::new(), false, line.chars().peekable());
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            ('"', _) => quoted = !quoted,
            (',', false) => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// One drawn row: its draw key (BLAKE3 of seed and record ID) and its CSV text.
type Draw = ([u8; 32], String);
/// One labelled row: record ID, class, terrain, label point (lon, lat in 1e-7°).
type Label = (String, String, String, (i32, i32));

/// `forge labels sample …`.
pub(crate) fn sample(
    root: &Path,
    cc: &str,
    source: &str,
    per: usize,
    seed: &str,
) -> Result<String, String> {
    // (stratum) → [(draw key, row)]
    let mut strata: BTreeMap<(String, &'static str), Vec<Draw>> = BTreeMap::new();
    let path = root
        .join("data/build/read")
        .join(source)
        .join("records.parquet");
    for_each_batch(&path, |b| {
        let id = column::<StringArray>(b, "id")?;
        let name = column::<StringArray>(b, "name")?;
        let class = column::<StringArray>(b, "class")?;
        let lat = column::<Int32Array>(b, "lat_e7")?;
        let lon = column::<Int32Array>(b, "lon_e7")?;
        let dem = column::<Float32Array>(b, "dem_m")?;
        for i in 0..b.num_rows() {
            let Some(c) = class_of(class.value(i)) else {
                continue;
            };
            if lat.is_null(i) || lon.is_null(i) || dem.is_null(i) || name.value(i).is_empty() {
                continue;
            }
            let terrain = if dem.value(i) < LOWLAND_M {
                "lowland"
            } else {
                "hill"
            };
            let round = |v: i32| f64::from(v / 100_000) / 100.0;
            let row = format!(
                "{},{},{},{terrain},{:.2},{:.2}",
                field(id.value(i)),
                field(name.value(i)),
                c,
                round(lat.value(i)),
                round(lon.value(i)),
            );
            let mut h = blake3::Hasher::new();
            h.update(seed.as_bytes());
            h.update(&[0]);
            h.update(id.value(i).as_bytes());
            strata
                .entry((c.to_string(), terrain))
                .or_default()
                .push((*h.finalize().as_bytes(), row));
        }
        Ok(())
    })?;
    let mut out = format!("{HEADER}\n");
    let mut report = format!(
        "# labels: {cc} sample from {source}, seed `{seed}`\n\n| Stratum | Pool | Drawn |\n|---|---:|---:|\n"
    );
    let mut n = 0_usize;
    for ((class, terrain), mut rows) in strata {
        rows.sort_unstable();
        let pool = rows.len();
        for (_, row) in rows.into_iter().take(per) {
            n += 1;
            let second = if n.is_multiple_of(5) { "yes" } else { "" };
            let _ = writeln!(out, "{row},{second},,,,");
        }
        let _ = writeln!(
            report,
            "| {class}, {terrain} | {pool} | {} |",
            pool.min(per)
        );
    }
    let file = root.join("tests/golden").join(format!("{cc}-places.csv"));
    if file.exists() {
        return Err(format!(
            "{}: exists; labels are never overwritten (delete it deliberately to redraw)",
            file.display()
        ));
    }
    fs::write(&file, &out).map_err(|e| format!("{}: {e}", file.display()))?;
    let _ = writeln!(
        report,
        "\n{n} places → {}.",
        file.strip_prefix(root).unwrap_or(&file).display()
    );
    Ok(report)
}

/// `forge labels score …`.
pub(crate) fn score(
    root: &Path,
    cc: &str,
    points_run: &str,
    buildings_run: &str,
) -> Result<String, String> {
    let file = root.join("tests/golden").join(format!("{cc}-places.csv"));
    let labels = read_labels(&file)?;
    if labels.is_empty() {
        return Ok(format!(
            "# labels: {cc}\n\nNo labelled rows yet in {}.\n",
            file.display()
        ));
    }
    // Record → cluster, cluster → (source point of the record, consensus point).
    let build = root.join("data/build");
    let mut cluster_of: BTreeMap<String, String> = BTreeMap::new();
    for_each_batch(
        &build
            .join("conflate")
            .join(points_run)
            .join("clusters.parquet"),
        |b| {
            let id = column::<StringArray>(b, "id")?;
            let cl = column::<StringArray>(b, "cluster")?;
            for i in 0..b.num_rows() {
                cluster_of.insert(id.value(i).to_string(), cl.value(i).to_string());
            }
            Ok(())
        },
    )?;
    let mut consensus: BTreeMap<String, (i32, i32)> = BTreeMap::new();
    for_each_batch(
        &build
            .join("core")
            .join(points_run)
            .join(buildings_run)
            .join("points.parquet"),
        |b| {
            let cl = column::<StringArray>(b, "cluster")?;
            let lon = column::<Int32Array>(b, "lon_e7")?;
            let lat = column::<Int32Array>(b, "lat_e7")?;
            for i in 0..b.num_rows() {
                consensus.insert(cl.value(i).to_string(), (lon.value(i), lat.value(i)));
            }
            Ok(())
        },
    )?;
    let mut source_pt: BTreeMap<String, (i32, i32)> = BTreeMap::new();
    for s in points_run.split('+') {
        for_each_batch(&build.join("read").join(s).join("records.parquet"), |b| {
            let id = column::<StringArray>(b, "id")?;
            let lon = column::<Int32Array>(b, "lon_e7")?;
            let lat = column::<Int32Array>(b, "lat_e7")?;
            for i in 0..b.num_rows() {
                if !lon.is_null(i) && !lat.is_null(i) {
                    source_pt.insert(id.value(i).to_string(), (lon.value(i), lat.value(i)));
                }
            }
            Ok(())
        })?;
    }
    let dist = |a: (i32, i32), b: (i32, i32)| {
        let k = 111_195.08;
        let lat = f64::from(a.1) * 1e-7;
        let dx = f64::from(b.0 - a.0) * 1e-7 * k * lat.to_radians().cos();
        let dy = f64::from(b.1 - a.1) * 1e-7 * k;
        dx.hypot(dy)
    };
    // stratum → (source distances, consensus distances)
    let mut by: BTreeMap<String, (Vec<f64>, Vec<f64>)> = BTreeMap::new();
    for (id, class, terrain, label) in &labels {
        let e = by.entry(format!("{class}, {terrain}")).or_default();
        if let Some(&p) = source_pt.get(id) {
            e.0.push(dist(*label, p));
        }
        if let Some(&p) = cluster_of.get(id).and_then(|c| consensus.get(c)) {
            e.1.push(dist(*label, p));
        }
    }
    let mut out = format!(
        "# labels: {cc}, {} labelled places\n\nDistance to the label, metres. Target (D-062): P90 ≤ 500 m lowland, ≤ 150 m hill.\n\n| Stratum | n | Source P50 | Source P90 | Consensus P50 | Consensus P90 | Within target |\n|---|---:|---:|---:|---:|---:|---:|\n",
        labels.len()
    );
    for (stratum, (mut s, mut c)) in by {
        let target = if stratum.ends_with("lowland") {
            500.0
        } else {
            150.0
        };
        #[allow(clippy::cast_precision_loss, reason = "counts")]
        let within = c.iter().filter(|&&d| d <= target).count() as f64 / c.len().max(1) as f64;
        let _ = writeln!(
            out,
            "| {stratum} | {} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0}% |",
            c.len(),
            q(&mut s, 0.5),
            q(&mut s, 0.9),
            q(&mut c, 0.5),
            q(&mut c, 0.9),
            within * 100.0
        );
    }
    Ok(out)
}

/// Reads the labelled rows of a labels file (rows without a label are skipped).
fn read_labels(file: &Path) -> Result<Vec<Label>, String> {
    let text = fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut lines = text.lines();
    if lines.next() != Some(HEADER) {
        return Err(format!("{}: header must be `{HEADER}`", file.display()));
    }
    // id → (stratum, label point)
    let mut labels: Vec<Label> = Vec::new();
    for (i, line) in lines.enumerate() {
        let f = split(line);
        let get = |k: usize| f.get(k).map_or("", String::as_str);
        if get(7).is_empty() || get(8).is_empty() {
            continue;
        }
        let deg = |v: &str| v.trim().parse::<f64>().ok().and_then(e7);
        let (Some(lat), Some(lon)) = (deg(get(7)), deg(get(8))) else {
            return Err(format!(
                "{}: line {}: bad label_lat/label_lon",
                file.display(),
                i + 2
            ));
        };
        labels.push((
            get(0).to_string(),
            get(2).to_string(),
            get(3).to_string(),
            (lon, lat),
        ));
    }
    Ok(labels)
}

fn q(v: &mut [f64], at: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    let last = u32::try_from(v.len() - 1).unwrap_or(u32::MAX);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "an index"
    )]
    let i = (f64::from(last) * at).round() as usize;
    v.get(i).copied().unwrap_or(f64::NAN)
}

#[cfg(test)]
mod tests {
    use super::{field, q, split};

    #[test]
    fn csv_round_trips_and_quantiles() {
        for s in ["Lahachowk", "a,b", "say \"hi\"", ""] {
            let line = format!("{},x", field(s));
            assert_eq!(split(&line), vec![s.to_string(), "x".to_string()], "{s:?}");
        }
        let mut v = vec![5.0, 1.0, 3.0, 2.0, 4.0];
        assert_eq!((q(&mut v, 0.5), q(&mut v, 0.9)), (3.0, 5.0));
        assert!(q(&mut [], 0.5).is_nan());
    }
}
