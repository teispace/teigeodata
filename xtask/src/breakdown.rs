//! `cargo xtask pack-sizes <pack.tgd> [--langs ne,en]`: where a pack's bytes go (TG-11).
//!
//! Every consumer plans its install from these tables:
//! - each component: bytes on disk (the pack is read in place, so raw is disk), on the wire
//!   (the transport asset, [`crate::wire`]), and both per place;
//! - each column of each section, raw and as zstd level 19 on its own (the column's weight;
//!   frames hold whole sections, so these do not add up to the wire);
//! - names by language: records, places that carry one, UTF-8 and stored bytes; and, with
//!   `--langs`, the NAMES section re-encoded keeping only each place's canonical name and
//!   names in those languages, which is what a profile that names those languages ships;
//! - outlines by admin level (groups coarse to fine): each arc counted once, at the coarsest
//!   level that uses it, so "down to here" is a running total; and by level of detail.
//!
//! Outline bytes per level are the ARCS section's wire bytes shared by vertex count: arcs are
//! one stream, so this is an estimate and says so.

#![allow(
    clippy::cast_precision_loss,
    reason = "byte counts shown as ratios with one or two decimals"
)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;

use teigeodata_core::table::Table;
use teigeodata_format::arcs::Arcs;
use teigeodata_format::names::{NameRecord, Names, TEXT_BUFFER, Tag};
use teigeodata_format::places::Places;
use teigeodata_format::polygons::Polygons;
use teigeodata_format::{Encoding, Kind, Pack};

const LEVEL: i32 = 19;

fn zstd_len(bytes: &[u8]) -> u64 {
    zstd::bulk::compress(bytes, LEVEL).map_or(bytes.len() as u64, |c| c.len() as u64)
}

pub(crate) fn run(args: &[String]) -> Result<String, Vec<String>> {
    let one = |e: String| vec![e];
    let usage = || one("usage: cargo xtask pack-sizes <pack.tgd> [--langs ne,en]".into());
    let langs: Option<Vec<String>> = args
        .iter()
        .position(|a| a == "--langs")
        .map(|i| args.get(i + 1).ok_or_else(usage))
        .transpose()?
        .map(|l| l.split(',').map(str::to_owned).collect());
    let path = args
        .iter()
        .find(|a| !a.starts_with("--") && langs.as_ref().is_none_or(|l| **a != l.join(",")))
        .ok_or_else(usage)?;
    let bytes = fs::read(path).map_err(|e| one(format!("{path}: {e}")))?;
    let pack = Pack::open(&bytes).map_err(|e| one(format!("{path}: {e}")))?;
    let places = pack
        .section(Kind::PLACES)
        .ok_or_else(|| one(format!("{path}: no PLACES")))?;
    let places = Places::open(&places).map_err(|e| one(format!("{path}: {e}")))?;
    let n = places.len().max(1);

    let mut out = String::new();
    let wire = crate::wire::Wire::measure(&bytes).map_err(|e| one(format!("{path}: {e}")))?;
    let _ = writeln!(
        out,
        "## {path}\n\n{} places; file {} B; transport asset {} B\n",
        places.len(),
        bytes.len(),
        wire.asset()
    );
    out.push_str(&wire.report_per_place(n));
    columns(&pack, &mut out);
    names(&pack, langs.as_deref(), n, &mut out).map_err(one)?;
    outlines(&pack, &places, &mut out).map_err(one)?;
    Ok(out)
}

/// Each column of each column-table section, largest first by its zstd bytes.
fn columns(pack: &Pack<'_>, out: &mut String) {
    let mut rows = Vec::new();
    for s in pack.sections() {
        if s.encoding() == Encoding::TABLE
            && let Ok(t) = Table::parse(s.bytes())
        {
            for id in t.ids() {
                if let Some(b) = t.column_bytes(id) {
                    rows.push((s.kind().to_string(), id, b.len() as u64, zstd_len(b)));
                }
            }
        } else {
            rows.push((
                s.kind().to_string(),
                0,
                s.bytes().len() as u64,
                zstd_len(s.bytes()),
            ));
        }
    }
    rows.sort_by(|a, b| b.3.cmp(&a.3).then_with(|| a.0.cmp(&b.0)));
    out.push_str(
        "\n### Columns (zstd 19 each on its own; 0 = the whole section)\n\n\
         | Section | Column | Raw B | zstd B |\n|---|--:|--:|--:|\n",
    );
    for (kind, id, raw, z) in &rows {
        let _ = writeln!(out, "| {kind} | {id} | {raw} | {z} |");
    }
}

/// Names by language, and the section cut to `langs` plus each canonical name.
fn names(
    pack: &Pack<'_>,
    langs: Option<&[String]>,
    places: usize,
    out: &mut String,
) -> Result<(), String> {
    let Some(section) = pack.section(Kind::NAMES) else {
        return Ok(());
    };
    let names = Names::open(&section).map_err(|e| format!("NAMES: {e}"))?;
    // lang → (records, places with one, UTF-8 bytes)
    let mut by: BTreeMap<String, (u64, u64, u64)> = BTreeMap::new();
    // script → (records, places with one, UTF-8 bytes)
    let mut by_script: BTreeMap<String, (u64, u64, u64)> = BTreeMap::new();
    let mut texts: Vec<Vec<(String, Option<Tag>, NameRecordOwned)>> = Vec::new();
    let mut buf = [0_u8; TEXT_BUFFER];
    let keep: Option<Vec<Tag>> =
        langs.map(|l| l.iter().map(String::as_str).filter_map(Tag::new).collect());
    for row in 0..names.len() {
        let mut seen: Vec<String> = Vec::new();
        let mut seen_script: Vec<String> = Vec::new();
        let mut kept = Vec::new();
        for (i, name) in names.names(row).enumerate() {
            let text = name
                .text(&mut buf)
                .map_err(|e| format!("NAMES row {row}: {e}"))?;
            let lang = name
                .lang()
                .map_or_else(|| "und".to_owned(), |t| t.to_string());
            let e = by.entry(lang.clone()).or_default();
            e.0 += 1;
            e.2 += text.len() as u64;
            if !seen.contains(&lang) {
                e.1 += 1;
                seen.push(lang);
            }
            let script = name
                .script()
                .map_or_else(|| "none".to_owned(), |t| t.to_string());
            let e = by_script.entry(script.clone()).or_default();
            e.0 += 1;
            e.2 += text.len() as u64;
            if !seen_script.contains(&script) {
                e.1 += 1;
                seen_script.push(script);
            }
            if let Some(k) = &keep
                && (i == 0 || name.lang().is_some_and(|t| k.contains(&t)))
            {
                kept.push((
                    text.to_owned(),
                    name.lang(),
                    NameRecordOwned {
                        script: name.script(),
                        kind: name.kind().map(teigeodata_format::names::NameType::code),
                        valid: name.valid(),
                        rule: name.rule(),
                    },
                ));
            }
        }
        if keep.is_some() {
            texts.push(kept);
        }
    }
    let total = section.bytes().len() as u64;
    let _ = writeln!(
        out,
        "\n### Names by language\n\nNAMES: {} records, {total} B raw, {} B zstd 19, {:.1} B per place\n\n\
",
        names.records(),
        zstd_len(section.bytes()),
        total as f64 / places as f64
    );
    tally(out, "Language", by, places);
    tally(out, "Script", by_script, places);
    if let Some(l) = langs {
        let records: Vec<Vec<NameRecord<'_>>> = texts
            .iter()
            .map(|p| {
                p.iter()
                    .map(|(t, lang, o)| NameRecord {
                        text: t,
                        lang: *lang,
                        script: o.script,
                        kind: o
                            .kind
                            .and_then(teigeodata_format::names::NameType::from_code),
                        valid_from: o.valid.0,
                        valid_to: o.valid.1,
                        rule: o.rule,
                    })
                    .collect()
            })
            .collect();
        let refs: Vec<&[NameRecord<'_>]> = records.iter().map(Vec::as_slice).collect();
        let cut = teigeodata_format::names::encode(&refs).map_err(|e| format!("NAMES cut: {e}"))?;
        let _ = writeln!(
            out,
            "\nCut to the canonical name plus {}: {} records, {} B raw, {} B zstd 19, {:.1} B per place\n",
            l.join(", "),
            records.iter().map(Vec::len).sum::<usize>(),
            cut.len(),
            zstd_len(&cut),
            cut.len() as f64 / places as f64
        );
    }
    Ok(())
}

/// One table of name records grouped by `what` (language or script), most places first.
fn tally(out: &mut String, what: &str, by: BTreeMap<String, (u64, u64, u64)>, places: usize) {
    let mut rows: Vec<_> = by.into_iter().collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.1.1));
    let _ = writeln!(
        out,
        "\n| {what} | Records | Places with one | Share of places | UTF-8 B |\n|---|--:|--:|--:|--:|"
    );
    for (key, (records, with, utf8)) in &rows {
        let _ = writeln!(
            out,
            "| {key} | {records} | {with} | {:.1}% | {utf8} |",
            100.0 * *with as f64 / places as f64
        );
    }
}

struct NameRecordOwned {
    script: Option<Tag>,
    kind: Option<u32>,
    valid: (Option<i32>, Option<i32>),
    rule: u32,
}

/// Outlines by admin group and by level of detail.
fn outlines(pack: &Pack<'_>, places: &Places<'_>, out: &mut String) -> Result<(), String> {
    let (Some(a), Some(p)) = (pack.section(Kind::ARCS), pack.section(Kind::POLYGONS)) else {
        return Ok(());
    };
    let arcs = Arcs::open(&a).map_err(|e| format!("ARCS: {e}"))?;
    let polys = Polygons::open(&p).map_err(|e| format!("POLYGONS: {e}"))?;
    let arcs_wire = zstd_len(a.bytes());
    let total = arcs.vertices().max(1);
    let mut first_group = vec![usize::MAX; arcs.len()];
    let _ = writeln!(
        out,
        "\n### Outlines by admin level\n\nARCS: {} arcs, {} vertices, {} B raw, {arcs_wire} B zstd 19. \
         Each arc counts once, at the coarsest level using it; bytes are ARCS' zstd bytes shared by vertices (an estimate).\n\n\
         | Group | Admin level | Polygons | New vertices | Down to here: vertices | Down to here: B (est.) |\n|--:|--:|--:|--:|--:|--:|",
        arcs.len(),
        arcs.vertices(),
        a.bytes().len()
    );
    let mut running = 0_usize;
    for g in 0..polys.groups() {
        let range = polys.group(g).unwrap_or(0..0);
        let level = range
            .clone()
            .next()
            .and_then(|q| polys.row(q))
            .and_then(|r| places.admin_level(r as usize));
        let mut new = 0_usize;
        for q in range.clone() {
            for part in polys.parts(q).unwrap_or(0..0) {
                for ring in polys.rings(part).unwrap_or(0..0) {
                    for (arc, _) in polys.refs(ring) {
                        if let Some(f) = first_group.get_mut(arc)
                            && *f == usize::MAX
                        {
                            *f = g;
                            new += arcs.range(arc).map_or(0, |r| r.len());
                        }
                    }
                }
            }
        }
        running += new;
        let _ = writeln!(
            out,
            "| {g} | {} | {} | {new} | {running} | {} |",
            level.map_or_else(|| "-".to_owned(), |l| l.to_string()),
            range.len(),
            (arcs_wire as f64 * running as f64 / total as f64).round()
        );
    }
    if arcs.has_levels() {
        out.push_str(
            "\n### Outlines by level of detail\n\n| LOD | Tolerance cm | Vertices kept | Share |\n|--:|--:|--:|--:|\n",
        );
        let mut at = [0_usize; 256];
        for v in 0..arcs.vertices() {
            if let Some(c) = at.get_mut(usize::from(arcs.level(v).unwrap_or(0))) {
                *c += 1;
            }
        }
        for lod in 0..=u8::MAX {
            let Some(tol) = arcs.tolerance_cm(lod) else {
                break;
            };
            let kept: usize = at.iter().skip(usize::from(lod)).sum();
            let _ = writeln!(
                out,
                "| {lod} | {tol} | {kept} | {:.1}% |",
                100.0 * kept as f64 / total as f64
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use teigeodata_format::names::{NameRecord, Tag, encode};
    use teigeodata_format::{Encoding, Kind, Pack, Writer};

    #[test]
    fn the_names_cut_keeps_the_canonical_name_and_the_asked_languages() {
        let tag = |c| Tag::new(c);
        let rec = |text, lang| NameRecord {
            text,
            lang: tag(lang),
            script: None,
            ..NameRecord::default()
        };
        let a = [
            rec("Pokhara", "und"),
            rec("पोखरा", "ne"),
            rec("Pokhara", "fr"),
        ];
        let b = [rec("Lahachowk", "und"), rec("Lahachowk", "de")];
        let names = encode(&[&a, &b]).unwrap();
        let pack = Writer::new("zz", "places", "2026.11.0")
            .unwrap()
            .pack(&[(Kind::NAMES, Encoding::TABLE, 2, &names)], None)
            .unwrap();
        let pack = Pack::open(&pack).unwrap();
        let mut out = String::new();
        super::names(&pack, Some(&["ne".to_owned()]), 2, &mut out).unwrap();
        // Pokhara keeps two records (canonical, ne), Lahachowk one (canonical).
        assert!(out.contains("plus ne: 3 records"), "{out}");
        assert!(out.contains("| fr | 1 | 1 | 50.0% |"), "{out}");
        out.clear();
        super::names(
            &pack,
            Some(&["ne".to_owned(), "fr".to_owned()]),
            2,
            &mut out,
        )
        .unwrap();
        assert!(out.contains("plus ne, fr: 4 records"), "{out}");
    }
}

/// Derive-on-device timing (the embedded-layer note): SUBSTRING rebuilt from the pack's own
/// NAMES and SEARCH importance, timed and compared with the shipped section.
/// `TGD_PACK=<pack> cargo test --release -p xtask -- --ignored derive --nocapture`
#[cfg(test)]
mod derive {
    use std::time::Instant;

    use teigeodata_format::names::Names;
    use teigeodata_format::search::{Search, substring_from_names};
    use teigeodata_format::{Kind, Pack};
    use teigeodata_names::{fold, substring_keys};

    #[test]
    #[ignore = "needs TGD_PACK"]
    fn derive_substring_from_names() {
        let bytes = std::fs::read(std::env::var("TGD_PACK").unwrap()).unwrap();
        let pack = Pack::open(&bytes).unwrap();
        let names = Names::open(&pack.section(Kind::NAMES).unwrap()).unwrap();
        let search = Search::open(&pack.section(Kind::SEARCH).unwrap()).unwrap();
        let t0 = Instant::now();
        let importance: Vec<u8> = (0..names.len())
            .map(|r| search.importance(u32::try_from(r).unwrap()))
            .collect();
        let keys = |text: &str, out: &mut dyn FnMut(&str)| substring_keys(&fold(text), out);
        let body = substring_from_names(&names, &importance, &keys).unwrap();
        let t1 = Instant::now();
        let shipped = pack.section(Kind::SUBSTRING).unwrap().bytes();
        println!(
            "derived in {:?}: {} B, shipped {} B, equal {}",
            t1 - t0,
            body.len(),
            shipped.len(),
            body == shipped
        );
        assert_eq!(
            body, shipped,
            "the derived SUBSTRING differs from the shipped one"
        );
    }
}
