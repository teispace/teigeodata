//! `forge embed --profile <name>`: an embedded layer built from a consumer's profile (TG-12,
//! D-213).
//!
//! A profile is a data file, `data/embed/<name>.yaml`, never code. It names:
//! - `build`: `lite` or `main`;
//! - `components`: what the layer carries (the [component](teigeodata_format::component) names;
//!   each brings what it needs);
//! - `languages`: the name languages every place keeps beside its canonical name (empty keeps
//!   every name);
//! - `countries`: per country code, `rows: slice` (the country's baseline slice, `forge
//!   baseline`) or `rows: all` (every place of its pack, with the pack's IDs, so installing the
//!   pack later changes nothing a person chose), and an optional outline `tolerance_m`;
//! - `wire_kb`: the layer's budget on the wire (the transport asset, zstd 19).
//!
//! The world part comes first, then the countries by code; a later part's row replaces an
//! earlier one of the same ID ([`extract::merge`]). SUBSTRING, when asked for, is rebuilt from
//! the merged NAMES with the format's one builder, the same bytes a device derives.
//!
//! Gates: every ID of every part is one row with the last part's point and ISO code; the pack
//! reads back verified with valid geometry; the transport asset fits `wire_kb`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::time::Instant;

use serde::Deserialize;
use teigeodata_format::arcs::Arcs;
use teigeodata_format::component::{Component, Components};
use teigeodata_format::extract::{self, NameRule, Part};
use teigeodata_format::names::{NameRecord, Names, Tag};
use teigeodata_format::places::Places;
use teigeodata_format::search::{self, Search};
use teigeodata_format::{Encoding, Kind, Pack};
use teigeodata_sync::measure::Measured;
use teigeodata_sync::transport;

use super::Profile;

/// A part's search keyer: the key function its pack was built with.
type Keyer<'a> = Box<dyn Fn(&str, u32, &mut search::Maps) + 'a>;

/// A consumer's profile, `data/embed/<name>.yaml`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    /// Who it is for and why, for the summary.
    about: String,
    build: String,
    components: Vec<String>,
    #[serde(default)]
    languages: Vec<String>,
    #[serde(default)]
    countries: BTreeMap<String, Country>,
    wire_kb: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Country {
    rows: Rows,
    #[serde(default)]
    tolerance_m: Option<u32>,
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Rows {
    Slice,
    All,
}

/// One part of the layer: its name, its bytes and how it is cut.
struct Input {
    name: String,
    bytes: Vec<u8>,
    tolerance_m: Option<u32>,
    /// The country whose profile keyed its search; `None` for the world part.
    cc: Option<String>,
}

pub(super) fn run(root: &Path, name: &str, key: Option<&str>) -> Result<String, String> {
    let t = Instant::now();
    let path = root.join("data/embed").join(format!("{name}.yaml"));
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let p: File = serde_saphyr::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let build = match p.build.as_str() {
        "lite" | "main" => p.build.as_str(),
        other => {
            return Err(format!(
                "{}: build `{other}`, not lite or main",
                path.display()
            ));
        }
    };
    let wanted = components(&p.components).map_err(|e| format!("{}: {e}", path.display()))?;
    let inputs = inputs(root, &p, build)?;
    let packs: Vec<Pack<'_>> = inputs
        .iter()
        .map(|i| Pack::open_verified(&i.bytes).map_err(|e| format!("{}: {e}", i.name)))
        .collect::<Result<_, _>>()?;
    let langs: Vec<Tag> = p.languages.iter().filter_map(|l| Tag::new(l)).collect();
    if langs.len() != p.languages.len() {
        return Err(format!(
            "{}: a language is not a BCP 47 tag",
            path.display()
        ));
    }
    let profiles: Vec<Profile> = inputs
        .iter()
        .map(|i| {
            i.cc.as_deref()
                .map_or_else(|| Ok(Profile::default()), |cc| Profile::read(root, cc))
        })
        .collect::<Result<_, _>>()?;
    let keep_name =
        |_: u32, i: usize, n: &NameRecord<'_>| i == 0 || n.lang.is_some_and(|l| langs.contains(&l));
    let keyers: Vec<Keyer<'_>> = profiles
        .iter()
        .map(|pr| {
            Box::new(move |text: &str, row: u32, maps: &mut search::Maps| {
                super::pack::add_keys(maps, text, row, pr);
            }) as Keyer<'_>
        })
        .collect();
    let rules: Vec<NameRule<'_>> = keyers
        .iter()
        .map(|k| NameRule {
            keep: &keep_name,
            keys: k.as_ref(),
        })
        .collect();
    let keeps: Vec<Vec<u32>> = packs.iter().map(all_rows).collect::<Result<_, _>>()?;
    let lods: Vec<u8> = packs
        .iter()
        .zip(&inputs)
        .map(|(pk, i)| lod_of(pk, i.tolerance_m))
        .collect::<Result<_, _>>()?;
    let list: Vec<Part<'_, '_>> = packs
        .iter()
        .zip(&keeps)
        .zip(&lods)
        .zip(&rules)
        .map(|(((pack, keep), &lod), rule)| Part {
            pack,
            keep,
            lod,
            names: (!langs.is_empty()).then_some(rule),
        })
        .collect();
    let mut sections =
        extract::merge(&list, &super::baseline::region_options()).map_err(|e| e.to_string())?;
    sections.retain(|s| Component::of(s.0).is_none_or(|c| wanted.contains(c)));
    if wanted.contains(Component::Substring) {
        sections.push(substring(&sections)?);
    }
    // A component the profile names must be in the layer: never silently left out.
    let missing: Vec<&str> = p
        .components
        .iter()
        .filter(|n| {
            Component::from_name(n)
                .is_some_and(|c| !c.kinds().iter().any(|k| sections.iter().any(|s| s.0 == *k)))
        })
        .map(String::as_str)
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "{}: the layer would lack {} (no part carries it, or the merge does not yet)",
            path.display(),
            missing.join(", ")
        ));
    }
    finish(root, name, &p, (&inputs, &packs), &sections, key, t)
}

/// Writes the layer, runs its gates and writes its summary.
fn finish(
    root: &Path,
    name: &str,
    p: &File,
    (inputs, packs): (&[Input], &[Pack<'_>]),
    sections: &[extract::Body],
    key: Option<&str>,
    t: Instant,
) -> Result<String, String> {
    let build = p.build.as_str();
    let out = super::embed::write(packs, sections, key, root)?;
    super::baseline::check(&out, super::embed::rows_of(sections))?;
    let replaced = super::embed::check_ids(packs, &out)?;
    let asset =
        transport::encode(&out, |d| zstd::bulk::compress(d, 19).ok()).map_err(|e| e.to_string())?;
    let m = Measured::of(&out, &asset).map_err(|e| e.to_string())?;
    let dir = root.join("data/build/embed");
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let file = dir.join(format!("{name}-{build}.tgd"));
    crate::make::write_atomic(&file, &out)?;
    let report = report(
        name,
        p,
        inputs,
        &out,
        &m,
        replaced,
        super::embed::rows_of(sections),
    );
    fs::write(dir.join(format!("summary-{name}-{build}.md")), &report)
        .map_err(|e| e.to_string())?;
    if m.asset > p.wire_kb * 1000 {
        return Err(format!(
            "{report}\nG-wire: the layer is {} B on the wire, over its budget of {} kB",
            m.asset, p.wire_kb
        ));
    }
    Ok(format!(
        "{report}\n→ {} in {:.1?}",
        file.strip_prefix(root).unwrap_or(&file).display(),
        t.elapsed()
    ))
}

/// The set of `names`, each with what it needs.
fn components(names: &[String]) -> Result<Components, String> {
    let list: Vec<Component> = names
        .iter()
        .map(|n| Component::from_name(n).ok_or_else(|| format!("no component `{n}`")))
        .collect::<Result<_, _>>()?;
    Ok(Components::of(&list).closed())
}

/// The world part, then each country by code, as the profile cuts it.
fn inputs(root: &Path, p: &File, build: &str) -> Result<Vec<Input>, String> {
    let read = |f: &Path| fs::read(f).map_err(|e| format!("{}: {e}", f.display()));
    let world = root.join(format!(
        "data/build/baseline/world/world-baseline-{build}.tgd"
    ));
    let mut out = vec![Input {
        name: "world".into(),
        bytes: read(&world)?,
        tolerance_m: None,
        cc: None,
    }];
    for (cc, c) in &p.countries {
        let f = match c.rows {
            Rows::Slice => root.join(format!(
                "data/build/baseline/{cc}/{cc}-baseline-{build}.tgd"
            )),
            Rows::All => root.join(format!("data/build/pack/{cc}/{cc}-places-{build}.tgd")),
        };
        out.push(Input {
            name: format!(
                "{cc} ({})",
                if c.rows == Rows::All { "all" } else { "slice" }
            ),
            bytes: read(&f)?,
            tolerance_m: c.tolerance_m,
            cc: Some(cc.clone()),
        });
    }
    Ok(out)
}

fn all_rows(p: &Pack<'_>) -> Result<Vec<u32>, String> {
    let n = Places::open(&p.section(Kind::PLACES).ok_or("a part without PLACES")?)
        .map_err(|e| e.to_string())?
        .len();
    u32::try_from(n)
        .map(|n| (0..n).collect())
        .map_err(|_| "too many rows".to_owned())
}

/// The level of detail of a part's outlines: the finest within `tolerance_m`, full without one.
fn lod_of(p: &Pack<'_>, tolerance_m: Option<u32>) -> Result<u8, String> {
    match (tolerance_m, p.section(Kind::ARCS)) {
        (Some(m), Some(s)) => {
            let arcs = Arcs::open(&s).map_err(|e| e.to_string())?;
            Ok(extract::lod_within(&arcs, m.saturating_mul(100)))
        }
        _ => Ok(0),
    }
}

/// SUBSTRING of the merged NAMES, ranked with the merged SEARCH's importance.
fn substring(sections: &[extract::Body]) -> Result<extract::Body, String> {
    let body = |k: Kind| {
        sections
            .iter()
            .find(|s| s.0 == k)
            .ok_or(format!("substring needs {k}"))
    };
    let (n, s) = (body(Kind::NAMES)?, body(Kind::SEARCH)?);
    let names = Names::open(&teigeodata_format::Section::unpacked(
        Kind::NAMES,
        n.2,
        &n.3,
    ))
    .map_err(|e| e.to_string())?;
    let search = Search::open(&teigeodata_format::Section::unpacked(
        Kind::SEARCH,
        s.2,
        &s.3,
    ))
    .map_err(|e| e.to_string())?;
    let importance: Vec<u8> = (0..names.len())
        .map(|r| u32::try_from(r).map(|r| search.importance(r)))
        .collect::<Result<_, _>>()
        .map_err(|_| "too many rows")?;
    let b = search::substring_from_names(&names, &importance, &super::pack::substring_keys_of)
        .map_err(|e| e.to_string())?;
    Ok((Kind::SUBSTRING, Encoding::TABLE, n.2, b))
}

fn report(
    name: &str,
    p: &File,
    inputs: &[Input],
    out: &[u8],
    m: &Measured,
    replaced: usize,
    rows: usize,
) -> String {
    let mut s = format!(
        "# Embedded layer `{name}` ({})\n\n{}\n\n",
        p.build,
        p.about.trim()
    );
    let parts: Vec<String> = inputs
        .iter()
        .map(|i| format!("{} ({} B)", i.name, i.bytes.len()))
        .collect();
    let _ = writeln!(
        s,
        "{rows} rows from {}; {replaced} rows replaced by a later part. Names: {}. \
         Disk {} B; wire {} B of {} kB.\n",
        parts.join(", "),
        if p.languages.is_empty() {
            "every name".to_owned()
        } else {
            format!("the canonical name plus {}", p.languages.join(", "))
        },
        out.len(),
        m.asset,
        p.wire_kb
    );
    s.push_str(
        "| Component | Disk B | Wire B | Download with what it needs |\n|---|--:|--:|--:|\n",
    );
    for (&c, &(raw, wire)) in &m.parts {
        let _ = writeln!(
            s,
            "| {} | {raw} | {wire} | {} |",
            c.name(),
            m.wire(Components::of(&[c]))
        );
    }
    let _ = writeln!(
        s,
        "| head, seek table, block hashes | | {} | |\n\nBLAKE3 of the layer: `{}`.",
        m.fixed + m.hashes,
        blake3::hash(out).to_hex()
    );
    s
}
