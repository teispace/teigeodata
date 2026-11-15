//! `cargo xtask accuracy <cc> <pack.tgd> [--registry <tsv>] [--places <tsv>] [--write]`: a country's
//! `ACCURACY.md`, measured on the shipped pack against the labelled sample
//! (`tests/golden/<cc>-places.csv`, D-209, [accuracy labels](../../docs/02-research/notes/accuracy-labels.md)).
//!
//! Each labelled source record is found in the pack through the build's ID registry (the
//! bridge from source keys to IDs, following retired serials to their survivors). For each:
//!
//! - **horizontal**: the distance from the pack's point to the label, by stratum (class and
//!   terrain), against the D-062 targets (P90 ≤ 500 m lowland, ≤ 150 m hill);
//! - **vertical, from position**: the pack's elevation of the place against the terrain height
//!   the same pack gives at the label. Both are read from one DEM, so the difference is the
//!   part of the vertical error that comes from where the point is; the DEM's own error is
//!   measured against ICESat-2 in S6;
//! - **agreement**: where a second labeller marked the same place blind
//!   (`<cc>-places-pass2.csv`), how often the two passes agree there is a settlement and how
//!   far apart their marks are: the noise of the labels themselves.
//!
//! `--write` writes `data/countries/<cc>/ACCURACY.md` for a main pack and `ACCURACY-lite.md`
//! for a lite one; without it the page is printed. `--places` also writes each labelled place's
//! point, label and distance, to compare two builds place by place.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use teigeodata_core::{E7, Namespace, PlaceId, Point};
use teigeodata_engine::{Engine, Layer, LayerOptions, Scratch};

const USAGE: &str =
    "usage: cargo xtask accuracy <cc> <pack.tgd> [--registry <tsv>] [--places <tsv>] [--write]";

/// The D-062 horizontal targets (P90, metres) by terrain.
const LOWLAND_M: f64 = 500.0;
const HILL_M: f64 = 150.0;
/// The vertical target (P90, metres, D-062).
const VERTICAL_M: f64 = 50.0;

/// One labelled row of the sample.
struct Row {
    id: String,
    class: String,
    terrain: String,
    /// The label, or `None` where the labeller saw no settlement.
    label: Option<(f64, f64)>,
    /// The locator cell's corner (the source point truncated to 0.01°), where the sample has
    /// one: the labeller's chip is centred on the middle of that cell.
    locator: Option<(f64, f64)>,
}

/// Half the side of the chip the labeller saw, metres: 2 km square, 4 km for a town
/// ([accuracy labels](../../docs/02-research/notes/accuracy-labels.md)).
fn chip_half_m(class: &str) -> f64 {
    if class == "town" { 2_000.0 } else { 1_000.0 }
}

/// Whether `at` lies outside the chip of `r`: no label could be near it.
fn outside_chip(r: &Row, at: (f64, f64)) -> bool {
    let Some((lat, lon)) = r.locator else {
        return false;
    };
    let centre = (lat + 0.005, lon + 0.005);
    let half = chip_half_m(&r.class);
    let north = meters((at.0, centre.1), centre);
    let east = meters((centre.0, at.1), centre);
    north > half || east > half
}

/// Splits one CSV line (fields quoted when they hold a comma or a quote).
fn split(line: &str) -> Vec<String> {
    let (mut out, mut cur, mut quoted) = (Vec::new(), String::new(), false);
    let mut chars = line.chars().peekable();
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

/// The rows a labeller has seen (`labeller` set), with the label columns at `lat` and `lon`.
fn rows(file: &Path, lat: usize, lon: usize, labeller: usize) -> Result<Vec<Row>, String> {
    let text = fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate().skip(1) {
        let f = split(line);
        let get = |k: usize| f.get(k).map_or("", |s| s.trim());
        if get(labeller).is_empty() {
            continue;
        }
        let label = match (get(lat), get(lon)) {
            ("", "") => None,
            (a, b) => match (a.parse::<f64>(), b.parse::<f64>()) {
                (Ok(a), Ok(b)) if a.is_finite() && b.is_finite() => Some((a, b)),
                _ => return Err(format!("{}: line {}: bad label", file.display(), i + 1)),
            },
        };
        // The sample's locator columns (4, 5); a second pass's file has none.
        let locator = match (lat, get(4).parse::<f64>(), get(5).parse::<f64>()) {
            (7, Ok(a), Ok(b)) if a.is_finite() && b.is_finite() => Some((a, b)),
            _ => None,
        };
        out.push(Row {
            id: get(0).to_owned(),
            class: get(2).to_owned(),
            terrain: get(3).to_owned(),
            label,
            locator,
        });
    }
    Ok(out)
}

/// Source key → live serial of `build`, from a registry (following `>N` retirements).
fn registry(file: &Path, build: &str) -> Result<HashMap<String, u64>, String> {
    let text = fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let (mut keys, mut state) = (HashMap::new(), HashMap::new());
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let mut f = line.split('\t');
        let (Some(serial), Some(st), Some(ks)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        let Ok(serial) = serial.parse::<u64>() else {
            continue;
        };
        state.insert(serial, st.to_owned());
        for k in ks.split(' ') {
            keys.insert(k.to_owned(), serial);
        }
    }
    let live = |mut s: u64| {
        // A chain of retirements is short; the bound keeps a corrupt registry from looping.
        for _ in 0..64 {
            match state.get(&s) {
                Some(st) if st.starts_with('>') => s = st.get(1..)?.parse().ok()?,
                Some(st) if st.split(',').any(|b| b == build) => return Some(s),
                _ => return None,
            }
        }
        None
    };
    Ok(keys
        .into_iter()
        .filter_map(|(k, s)| Some((k, live(s)?)))
        .collect())
}

/// Metres between two (lat, lon) points in degrees, on the mean Earth sphere.
fn meters(a: (f64, f64), b: (f64, f64)) -> f64 {
    teigeodata_engine::meters_between(a.0, a.1, b.0, b.1)
}

/// The `at` quantile (nearest rank) of `v`; NaN when empty.
fn q(v: &mut [f64], at: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "an index into a short list"
    )]
    let i = ((at * v.len() as f64).ceil() as usize).clamp(1, v.len()) - 1;
    v.get(i).copied().unwrap_or(f64::NAN)
}

#[allow(clippy::cast_precision_loss, reason = "counts")]
fn pct(n: usize, of: usize) -> f64 {
    n as f64 * 100.0 / of.max(1) as f64
}

/// Distances by stratum, horizontal and vertical.
#[derive(Default)]
struct Stratum {
    horizontal: Vec<f64>,
    vertical: Vec<f64>,
    none: usize,
}

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, Vec<String>> {
    let (mut cc, mut pack, mut reg, mut places, mut write) = (None, None, None, None, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--write" => write = true,
            "--registry" => reg = Some(PathBuf::from(it.next().ok_or(vec![USAGE.into()])?)),
            "--places" => places = Some(PathBuf::from(it.next().ok_or(vec![USAGE.into()])?)),
            p if !p.starts_with("--") && cc.is_none() => cc = Some(p.to_owned()),
            p if !p.starts_with("--") && pack.is_none() => pack = Some(PathBuf::from(p)),
            _ => return Err(vec![USAGE.into()]),
        }
    }
    let (Some(cc), Some(pack)) = (cc, pack) else {
        return Err(vec![USAGE.into()]);
    };
    let (page, each) = page(root, &cc, &pack, reg.as_deref()).map_err(|e| vec![e])?;
    if let Some(out) = places {
        fs::write(&out, each).map_err(|e| vec![format!("{}: {e}", out.display())])?;
    }
    if write {
        // The main build's page is `ACCURACY.md`; the lite build's `ACCURACY-lite.md`.
        let file = if page.contains(", lite build") {
            "ACCURACY-lite.md"
        } else {
            "ACCURACY.md"
        };
        let out = root.join("data/countries").join(&cc).join(file);
        fs::write(&out, &page).map_err(|e| vec![format!("{}: {e}", out.display())])?;
        return Ok(format!("accuracy: wrote {}", out.display()));
    }
    Ok(page)
}

/// The page for `cc` measured on `pack`, and a TSV of each labelled place (`--places`).
#[allow(clippy::too_many_lines, reason = "one page, written top to bottom")]
fn page(
    root: &Path,
    cc: &str,
    pack: &Path,
    reg: Option<&Path>,
) -> Result<(String, String), String> {
    let name = pack
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    // `np-places-main.tgd` → build `main`, registry `np-ids-main.tsv` beside it.
    let build = name
        .strip_suffix(".tgd")
        .and_then(|s| s.rsplit('-').next())
        .filter(|b| *b == "main" || *b == "lite")
        .ok_or_else(|| format!("{name}: not a <cc>-places-<main|lite>.tgd pack"))?
        .to_owned();
    let reg = reg.map_or_else(
        || pack.with_file_name(format!("{cc}-ids-{build}.tsv")),
        Path::to_path_buf,
    );
    let serials = registry(&reg, &build)?;
    let ns = <[u8; 2]>::try_from(cc.as_bytes())
        .ok()
        .and_then(Namespace::from_country)
        .ok_or_else(|| format!("{cc}: not a two-letter country code"))?;
    let layer = Layer::open(pack, LayerOptions::default()).map_err(|e| format!("{name}: {e}"))?;
    let manifest = format!("{} {}", layer.address(), layer.release());
    let mut engine = Engine::new();
    engine.add(layer);
    let mut s = Scratch::default();

    let golden = root.join("tests/golden");
    let first = rows(&golden.join(format!("{cc}-places.csv")), 7, 8, 9)?;
    let mut by: BTreeMap<String, Stratum> = BTreeMap::new();
    let (mut missing, mut no_height, mut outside) = (Vec::new(), 0_usize, Vec::new());
    let mut each = String::from("id\tclass\tterrain\tlat\tlon\tlabel_lat\tlabel_lon\tmeters\n");
    for r in &first {
        let st = by.entry(format!("{}, {}", r.class, r.terrain)).or_default();
        let Some(label) = r.label else {
            st.none += 1;
            continue;
        };
        let place = serials
            .get(&r.id)
            .and_then(|&s| PlaceId::new(ns, s))
            .map(|id| engine.get(id))
            .transpose()
            .map_err(|e| format!("{}: {e}", r.id))?
            .flatten();
        let Some(place) = place else {
            missing.push(r.id.clone());
            continue;
        };
        let at = (place.point.lat_degrees(), place.point.lon_degrees());
        let m = meters(at, label);
        if outside_chip(r, at) {
            outside.push(format!("`{}` ({m:.0} m)", r.id));
        }
        st.horizontal.push(m);
        let _ = writeln!(
            each,
            "{}\t{}\t{}\t{:.6}\t{:.6}\t{:.6}\t{:.6}\t{m:.0}",
            r.id, r.class, r.terrain, at.0, at.1, label.0, label.1
        );
        let here = engine
            .elevation(&place)
            .map_err(|e| format!("{}: {e}", r.id))?;
        let there = Point::<E7>::from_degrees(label.0, label.1)
            .ok()
            .map(|p| engine.elevation_at(p, &mut s))
            .transpose()
            .map_err(|e| format!("{}: {e}", r.id))?
            .flatten();
        match (here, there) {
            (Some(h), Some(t)) => st.vertical.push((f64::from(h) - t.meters).abs()),
            _ => no_height += 1,
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "{} labelled places are not in {name} through {}: {}",
            missing.len(),
            reg.display(),
            missing.join(", ")
        ));
    }

    let labelled: usize = by.values().map(|s| s.horizontal.len()).sum();
    let none: usize = by.values().map(|s| s.none).sum();
    let mut p = String::new();
    let _ = writeln!(
        p,
        "<!-- Generated by `cargo xtask accuracy {cc} {name} --write`; do not edit. -->\n\n\
         # Accuracy: {cc}, {build} build\n\n\
         Measured on `{name}` ({manifest}) against the labelled sample of \
         [`tests/golden/{cc}-places.csv`](../../../tests/golden/{cc}-places.csv): {} places drawn \
         by class and terrain, {labelled} marked with a settlement, {none} marked as showing \
         none.\n",
        first.len()
    );
    let _ = writeln!(
        p,
        "## How the truth was made\n\n\
         The labels were made **by AI agents, not by people with local knowledge** \
         ([accuracy labels](../../../docs/02-research/notes/accuracy-labels.md), D-209). Each \
         agent saw a Copernicus Sentinel-2 image (10 m pixels) of the place's surroundings with \
         the points of open building-footprint datasets on top, and a hint of the place's \
         kilometre cell, and marked the centre of the built-up area carrying the name. Nothing \
         from the forge's own output was shown. There are no names on the images, so where two \
         clusters are alike the label is a judgement; the agreement of a second, blind pass \
         below is the measure of that noise. A people-made set replaces this one when it \
         exists.\n"
    );

    let _ = writeln!(
        p,
        "## Horizontal\n\n\
         Distance from the pack's point to the label, metres. Target (D-062): P90 ≤ \
         {LOWLAND_M:.0} m in the lowland, ≤ {HILL_M:.0} m in the hills.\n\n\
         | Stratum | Labelled | P50 | P90 | Within target | No settlement seen |\n\
         |---|---:|---:|---:|---:|---:|"
    );
    let mut all = Vec::new();
    for (stratum, st) in &mut by {
        let target = if stratum.ends_with("lowland") {
            LOWLAND_M
        } else {
            HILL_M
        };
        let within = st.horizontal.iter().filter(|&&d| d <= target).count();
        all.extend_from_slice(&st.horizontal);
        let _ = writeln!(
            p,
            "| {stratum} | {} | {:.0} | {:.0} | {:.0}% | {} |",
            st.horizontal.len(),
            q(&mut st.horizontal, 0.5),
            q(&mut st.horizontal, 0.9),
            pct(within, st.horizontal.len()),
            st.none
        );
    }
    let _ = writeln!(
        p,
        "| **all** | {} | {:.0} | {:.0} | | {none} |\n",
        all.len(),
        q(&mut all, 0.5),
        q(&mut all, 0.9)
    );

    let mut vall = Vec::new();
    let _ = writeln!(
        p,
        "## Vertical\n\n\
         The pack's elevation of each place against the terrain height the same pack gives at \
         the label, metres: the part of the vertical error that comes from the point's \
         position, since both are read from one DEM. The DEM's own error against ICESat-2 is \
         measured separately ([S6](../../../docs/02-research/benches/s6-elevation.md)). Target \
         (D-062): P90 ≤ {VERTICAL_M:.0} m for the two together.\n\n\
         | Stratum | Places | P50 | P90 | Within {VERTICAL_M:.0} m |\n|---|---:|---:|---:|---:|"
    );
    for (stratum, st) in &mut by {
        let within = st.vertical.iter().filter(|&&d| d <= VERTICAL_M).count();
        vall.extend_from_slice(&st.vertical);
        let _ = writeln!(
            p,
            "| {stratum} | {} | {:.0} | {:.0} | {:.0}% |",
            st.vertical.len(),
            q(&mut st.vertical, 0.5),
            q(&mut st.vertical, 0.9),
            pct(within, st.vertical.len())
        );
    }
    let vwithin = vall.iter().filter(|&&d| d <= VERTICAL_M).count();
    let _ = writeln!(
        p,
        "| **all** | {} | {:.0} | {:.0} | {:.0}% |\n",
        vall.len(),
        q(&mut vall, 0.5),
        q(&mut vall, 0.9),
        pct(vwithin, vall.len())
    );
    if no_height > 0 {
        let _ = writeln!(
            p,
            "{no_height} labelled places have no elevation in the pack or no terrain at the label, and are left out of this table.\n"
        );
    }

    let second = golden.join(format!("{cc}-places-pass2.csv"));
    if second.exists() {
        p.push_str(&agreement(&first, &rows(&second, 1, 2, 3)?));
    }
    let _ = writeln!(
        p,
        "## Reading these numbers\n\n\
         - A label is itself uncertain by about the agreement distances above, so a P90 below \
           them cannot be shown with this set; the hill target of {HILL_M:.0} m is finer than \
           the labels.\n\
         - The same noise enters the vertical table: on a hillside a label a few hundred \
           metres off reads a terrain height tens of metres off, so the hill rows are an upper \
           bound on the error from position.\n\
         - Places marked as showing no settlement (scattered houses, no buildings in the \
           footprint data) are not scored: the sample says nothing about where those names \
           belong.\n\
         - The sample is small per stratum; a stratum of under 10 places is a hint, not a \
           measurement."
    );
    // The chip is centred on the source record's own point: a build that moves a place out of
    // it is scored against a label that could not be near its point.
    let _ = write!(
        p,
        "- Each labeller's image was centred on the source record's own point (2 km across, \
         4 km for a town), so a label always lies near that point. A build that moves a place \
         further, to where another source puts it, is scored as wrong even where it is right. "
    );
    if outside.is_empty() {
        let _ = writeln!(p, "No point of this pack lies outside its image.");
    } else {
        let _ = writeln!(
            p,
            "{} of this pack's points lie outside the image their label was made on: {}.",
            outside.len(),
            outside.join(", ")
        );
    }
    Ok((p, each))
}

/// The agreement of the two passes on the rows both saw.
fn agreement(first: &[Row], second: &[Row]) -> String {
    let one: HashMap<&str, Option<(f64, f64)>> =
        first.iter().map(|r| (r.id.as_str(), r.label)).collect();
    let (mut d, mut both_none, mut split_presence) = (Vec::new(), 0_usize, 0_usize);
    for r in second {
        let Some(a) = one.get(r.id.as_str()) else {
            continue;
        };
        match (a, r.label) {
            (Some(a), Some(b)) => d.push(meters(*a, b)),
            (None, None) => both_none += 1,
            _ => split_presence += 1,
        }
    }
    let seen = d.len() + both_none + split_presence;
    let both = d.len();
    let (w150, w500) = (
        d.iter().filter(|&&x| x <= HILL_M).count(),
        d.iter().filter(|&&x| x <= LOWLAND_M).count(),
    );
    format!(
        "## Agreement of two blind passes\n\n\
         {seen} places were labelled twice, the second time by another agent on mirrored \
         chips with a moved centre. Both marked a settlement for {both} ({:.0}%), both marked \
         none for {both_none}, and they disagreed on whether there is one for \
         {split_presence} ({:.0}%).\n\n\
         | Both marked | n | P50 (m) | P90 (m) | Within {HILL_M:.0} m | Within {LOWLAND_M:.0} m |\n\
         |---|---:|---:|---:|---:|---:|\n\
         | distance between the two marks | {both} | {:.0} | {:.0} | {:.0}% | {:.0}% |\n\n",
        pct(both, seen),
        pct(split_presence, seen),
        q(&mut d, 0.5),
        q(&mut d, 0.9),
        pct(w150, both),
        pct(w500, both)
    )
}

#[cfg(test)]
mod tests {
    use super::{Row, agreement, outside_chip, q, registry, split};

    fn row(id: &str, label: Option<(f64, f64)>) -> Row {
        Row {
            id: id.into(),
            class: "hamlet".into(),
            terrain: "hill".into(),
            label,
            locator: Some((27.70, 85.30)),
        }
    }

    #[test]
    fn a_point_outside_the_labellers_image_is_found() {
        // The image is centred on (27.705, 85.305), 2 km across for a hamlet, 4 km for a town.
        let mut r = row("a", None);
        assert!(!outside_chip(&r, (27.705, 85.305)));
        // 0.008983° of latitude is 1 km: 900 m north is inside, 1.1 km north is not.
        assert!(!outside_chip(&r, (27.705 + 0.008_085, 85.305)));
        assert!(outside_chip(&r, (27.705 + 0.009_881, 85.305)));
        // East and west count the same way; a town's image is twice as wide.
        assert!(outside_chip(&r, (27.705, 85.305 - 0.012)));
        r.class = "town".into();
        assert!(!outside_chip(&r, (27.705, 85.305 - 0.012)));
        // A row without a locator (a second pass) is never outside.
        r.locator = None;
        assert!(!outside_chip(&r, (0.0, 0.0)));
    }

    #[test]
    fn agreement_counts_presence_and_distance() {
        let at = Some((27.7, 85.3));
        // 0.000899° of latitude is 100 m.
        let first = [row("a", at), row("b", None), row("c", at), row("d", at)];
        let second = [
            row("a", Some((27.700_899, 85.3))),
            row("b", None),
            row("c", None),
            row("x", at),
        ];
        let out = agreement(&first, &second);
        assert!(out.contains("3 places were labelled twice"), "{out}");
        assert!(out.contains("settlement for 1 (33%)"), "{out}");
        assert!(out.contains("none for 1,"), "{out}");
        assert!(out.contains("one for 1 (33%)"), "{out}");
        assert!(out.contains("| 1 | 100 | 100 | 100% | 100% |"), "{out}");
    }

    #[test]
    fn the_registry_follows_retirements_to_the_build() {
        let dir = std::env::temp_dir().join(format!("xtask-accuracy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("ids.tsv");
        std::fs::write(
            &file,
            "# teigeodata id registry 1\n\
             1\tlite,main\tgeonames:1 wikidata:Q1\n\
             2\t>1\tgeonames:2\n\
             3\tmain\tgeonames:3\n\
             4\tx\tgeonames:4\n",
        )
        .unwrap();
        let lite = registry(&file, "lite").unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(lite.get("wikidata:Q1"), Some(&1));
        assert_eq!(
            lite.get("geonames:2"),
            Some(&1),
            "a retired serial gives its survivor"
        );
        assert_eq!(lite.get("geonames:3"), None, "main only");
        assert_eq!(lite.get("geonames:4"), None, "a tombstone");
    }

    #[test]
    fn csv_and_quantiles() {
        assert_eq!(
            split("a,\"b, c\",\"say \"\"hi\"\"\""),
            ["a", "b, c", "say \"hi\""]
        );
        let mut v = [5.0, 1.0, 3.0, 2.0, 4.0];
        assert_eq!((q(&mut v, 0.5), q(&mut v, 0.9)), (3.0, 5.0));
        assert!(q(&mut [], 0.5).is_nan());
    }
}
