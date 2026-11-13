//! `forge embed --profile <name>`: an embedded layer built from a consumer's profile (TG-12,
//! D-213).
//!
//! A profile is a data file, `data/embed/<name>.yaml`, never code. It names:
//! - `build`: `lite` or `main`;
//! - `components`: what the layer carries (the [component](teigeodata_format::component) names;
//!   each brings what it needs);
//! - `languages`: the name languages every place keeps beside its canonical name, each a BCP 47
//!   language with an optional script (`ne-Deva` keeps Nepali in Devanagari, not a
//!   romanisation tagged `ne`); empty keeps every name;
//! - `machine_names`: `keep` (the default) or `drop` the names a program wrote (a
//!   romanisation, a transliteration or a donor label, D-227) beside the canonical name, which
//!   is always kept; every kept name carries its origin, so an app can show machine names apart;
//! - `countries`: per country code, `rows: slice` (the country's baseline slice, `forge
//!   baseline`) or `rows: all` (every place of its pack, with the pack's IDs, so installing the
//!   pack later changes nothing a person chose), and an optional outline `tolerance_m`;
//! - `world`: the world part's city list (`geonames-cities5000` for cities of 5,000 or more,
//!   TG-13), the default list when absent;
//! - `wire_kb`: the layer's budget on the wire (the transport asset, zstd 19).
//!
//! The world part comes first, then the countries by code; a later part's row replaces an
//! earlier one of the same ID ([`extract::merge`]). SUBSTRING, when asked for, is rebuilt from
//! the merged NAMES with the format's one builder, the same bytes a device derives.
//! `dem-coarse` carries every part's coarse terrain grid, codes copied (D-225).
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
type Keyer<'a> = Box<dyn Fn(&str, super::Origin, u32, &mut search::Maps) + 'a>;

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
    /// Whether the names a program wrote stay beside the canonical name (D-256).
    #[serde(default)]
    machine_names: Machine,
    #[serde(default)]
    countries: BTreeMap<String, Country>,
    /// The world part's city list (`forge world --cities`); the default list when absent.
    #[serde(default)]
    world: Option<String>,
    /// The world part's label chain (`forge world --labels`, D-221).
    #[serde(default)]
    world_labels: Vec<String>,
    /// The countries whose districts and towns join the world part (`forge world --towns`,
    /// D-259).
    #[serde(default)]
    world_towns: Vec<String>,
    /// The timezone source whose tzdb release compiles `tz-rules` (D-224); the world part's.
    #[serde(default)]
    tz: Option<String>,
    wire_kb: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Country {
    rows: Rows,
    #[serde(default)]
    tolerance_m: Option<u32>,
}

/// `machine_names`: what becomes of the names a program wrote.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Machine {
    /// Kept, marked by their origin.
    #[default]
    Keep,
    /// Left out, but for a canonical name.
    Drop,
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
    let (path, p) = read_profile(root, name)?;
    let build = p.build.as_str();
    let wanted = components(&p.components).map_err(|e| format!("{}: {e}", path.display()))?;
    let inputs = inputs(root, &p, build)?;
    let packs: Vec<Pack<'_>> = inputs
        .iter()
        .map(|i| Pack::open_verified(&i.bytes).map_err(|e| format!("{}: {e}", i.name)))
        .collect::<Result<_, _>>()?;
    let langs: Vec<Wanted> = p
        .languages
        .iter()
        .map(|l| {
            language(l).ok_or_else(|| {
                format!(
                    "{}: `{l}` is not a language or language-Script tag",
                    path.display()
                )
            })
        })
        .collect::<Result<_, _>>()?;
    let profiles: Vec<Profile> = inputs
        .iter()
        .map(|i| {
            i.cc.as_deref()
                .map_or_else(|| Ok(Profile::default()), |cc| Profile::read(root, cc))
        })
        .collect::<Result<_, _>>()?;
    let keep_name =
        |_: u32, i: usize, n: &NameRecord<'_>| keeps_name(&langs, p.machine_names, i, n);
    let keyers: Vec<Keyer<'_>> = profiles
        .iter()
        .map(|pr| {
            Box::new(
                move |text: &str, origin, row: u32, maps: &mut search::Maps| {
                    super::pack::add_keys(maps, (text, origin), row, pr);
                },
            ) as Keyer<'_>
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
    // Each part's outlines on the decimal grid its level allows (TG-16, D-229).
    let snappers = snappers(&packs, &lods);
    let runs: Vec<_> = snappers
        .iter()
        .map(|s| move |a: &mut [Vec<teigeodata_core::Point<teigeodata_core::E7>>]| s.run(a))
        .collect();
    let rewrites: Vec<extract::ArcRewrite<'_>> = runs
        .iter()
        .map(|r| extract::ArcRewrite { run: r })
        .collect();
    let list: Vec<Part<'_, '_>> = packs
        .iter()
        .zip(&keeps)
        .zip(&lods)
        .zip(&rules)
        .zip(&rewrites)
        .map(|((((pack, keep), &lod), rule), rewrite)| Part {
            pack,
            keep,
            lod,
            names: (!langs.is_empty() || p.machine_names == Machine::Drop).then_some(rule),
            arcs: Some(rewrite),
        })
        .collect();
    let mut sections =
        extract::merge(&list, &super::baseline::region_options()).map_err(|e| e.to_string())?;
    sections.retain(|s| Component::of(s.0).is_none_or(|c| wanted.contains(c)));
    sections.extend(coarse_terrain(&packs, wanted)?);
    let tzdb = derived(root, &p, wanted, &mut sections)?;
    refuse_missing(&path, &p, &sections)?;
    let outlines = inputs
        .iter()
        .zip(&snappers)
        .fold(String::new(), |mut o, (i, s)| {
            let _ = writeln!(o, "- `{}`: {}", i.name, s.report().line(s.k));
            o
        });
    finish(
        root,
        name,
        &p,
        (&inputs, &packs),
        &sections,
        (key, tzdb.as_deref()),
        (t, &outlines),
    )
}

/// The profile `data/embed/<name>.yaml` and its path; its build is `lite` or `main`.
fn read_profile(root: &Path, name: &str) -> Result<(std::path::PathBuf, File), String> {
    let path = root.join("data/embed").join(format!("{name}.yaml"));
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let p: File = serde_saphyr::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    match p.build.as_str() {
        "lite" | "main" => Ok((path, p)),
        other => Err(format!(
            "{}: build `{other}`, not lite or main",
            path.display()
        )),
    }
}

/// Each part's snap at its level of detail (TG-16, D-229).
fn snappers(packs: &[Pack<'_>], lods: &[u8]) -> Vec<crate::topo::quantize::Snapper> {
    packs
        .iter()
        .zip(lods)
        .map(|(pk, &lod)| {
            let arcs = pk.section(Kind::ARCS).and_then(|s| Arcs::open(&s).ok());
            crate::topo::quantize::Snapper::at(arcs.as_ref(), lod)
        })
        .collect()
}

/// A component the profile names must be in the layer: never silently left out.
fn refuse_missing(path: &Path, p: &File, sections: &[extract::Body]) -> Result<(), String> {
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
    Ok(())
}

/// `DEM_COARSE` of the layer when the profile asks for `dem-coarse` (D-225): every part's coarse
/// grids in one section, the tile codes copied, later parts' grids first so a part's own grid
/// answers in the halo it shares with an earlier one (as a later part's rows win).
fn coarse_terrain(packs: &[Pack<'_>], wanted: Components) -> Result<Option<extract::Body>, String> {
    if !wanted.contains(Component::DemCoarse) {
        return Ok(None);
    }
    let dems: Vec<teigeodata_format::dem::Dem<'_>> = packs
        .iter()
        .rev()
        .filter_map(|p| p.section(Kind::DEM_COARSE))
        .map(|s| teigeodata_format::dem::Dem::open(&s).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    Ok(teigeodata_format::dem::concat(&dems)
        .map_err(|e| format!("DEM_COARSE: {e}"))?
        .map(|(body, n)| (Kind::DEM_COARSE, Encoding::TABLE, n, body)))
}

/// The sections the layer derives instead of merging: SUBSTRING from its names (D-218) and
/// `TZ_RULES` from tzdb (D-224); returns the tzdb release when rules were compiled.
fn derived(
    root: &Path,
    p: &File,
    wanted: Components,
    sections: &mut Vec<extract::Body>,
) -> Result<Option<String>, String> {
    if wanted.contains(Component::Substring) {
        let b = substring(sections)?;
        sections.push(b);
    }
    if !wanted.contains(Component::TzRules) {
        return Ok(None);
    }
    let tz = p.tz.as_deref().unwrap_or(super::world::DEFAULT_TZ);
    let (version, body) = tz_rules(root, tz, sections)?;
    sections.push(body);
    Ok(Some(version))
}

/// `TZ_RULES` for every timezone area of the merged layer (D-224): their zones compiled from
/// the pinned tzdb release, each linked to its area's row, and G-tzrules run on the section.
pub(super) fn tz_rules(
    root: &Path,
    tz: &str,
    sections: &[extract::Body],
) -> Result<(String, extract::Body), String> {
    let body = |k: Kind| {
        sections
            .iter()
            .find(|s| s.0 == k)
            .ok_or(format!("tz-rules needs {k}"))
    };
    let (pl, nm) = (body(Kind::PLACES)?, body(Kind::NAMES)?);
    let places = Places::open(&teigeodata_format::Section::unpacked(
        Kind::PLACES,
        pl.2,
        &pl.3,
    ))
    .map_err(|e| e.to_string())?;
    let names = Names::open(&teigeodata_format::Section::unpacked(
        Kind::NAMES,
        nm.2,
        &nm.3,
    ))
    .map_err(|e| e.to_string())?;
    let mut buf = vec![0; teigeodata_format::names::TEXT_BUFFER];
    // One link per zone name: a later part's area (a country pack's, finer than the world
    // part's) replaces an earlier one of the same zone.
    let mut zones: BTreeMap<String, u32> = BTreeMap::new();
    for row in 0..places.len() {
        if places.kind(row) != Some(teigeodata_core::PlaceKind::TimezoneArea) {
            continue;
        }
        let name = names
            .canonical(row)
            .ok_or_else(|| format!("timezone area row {row} has no name"))?;
        let text = name.text(&mut buf).map_err(|e| e.to_string())?;
        let row = u32::try_from(row).map_err(|_| "too many rows")?;
        zones.insert(text.to_owned(), row);
    }
    if zones.is_empty() {
        return Err("tz-rules: the layer has no timezone areas".into());
    }
    let lock = crate::lock::Lock::read(&root.join("data/sources.lock"))?;
    let linked: Vec<(&str, u32)> = zones.iter().map(|(n, r)| (n.as_str(), *r)).collect();
    let list: Vec<&str> = linked.iter().map(|z| z.0).collect();
    let compiled = super::tz_rules::compile_names(root, &lock, tz, &list)?;
    let b = super::tz_rules::section(&compiled, &linked)?;
    let rules = teigeodata_format::tz_rules::TzRules::open(&teigeodata_format::Section::unpacked(
        Kind::TZ_RULES,
        linked.len() as u64,
        &b,
    ))
    .map_err(|e| e.to_string())?;
    super::tz_rules::check(&rules, Some(&compiled.version), &linked, &compiled)?;
    Ok((
        compiled.version.clone(),
        (Kind::TZ_RULES, Encoding::TABLE, linked.len() as u64, b),
    ))
}

/// Writes the layer, runs its gates and writes its summary.
fn finish(
    root: &Path,
    name: &str,
    p: &File,
    (inputs, packs): (&[Input], &[Pack<'_>]),
    sections: &[extract::Body],
    key: (Option<&str>, Option<&str>),
    (t, outlines): (Instant, &str),
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
    let mut report = report(
        name,
        p,
        inputs,
        &out,
        &m,
        replaced,
        super::embed::rows_of(sections),
    );
    let _ = write!(report, "\nOutlines:\n\n{outlines}");
    let names_in = coverage(p, inputs, sections)?;
    if !names_in.is_empty() {
        let _ = write!(
            report,
            "\nPlaces with a name in each language:\n\n{names_in}"
        );
    }
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

/// A profile language: a BCP 47 language, and its script when named (`ne-Deva`), so a name
/// tagged `ne` but written in Latin (a romanisation) is not taken as a Nepali name (D-215).
struct Wanted {
    lang: Tag,
    script: Option<Tag>,
}

impl Wanted {
    fn keeps(&self, n: &NameRecord<'_>) -> bool {
        self.matches(n.lang, n.script)
    }

    fn matches(&self, lang: Option<Tag>, script: Option<Tag>) -> bool {
        lang == Some(self.lang) && self.script.is_none_or(|s| script == Some(s))
    }
}

/// Whether a place's name record `n`, its `i`th, stays in the layer: the canonical name always;
/// another when its language is wanted (any, when none is named) and, under `drop`, a person
/// wrote it.
fn keeps_name(langs: &[Wanted], machine: Machine, i: usize, n: &NameRecord<'_>) -> bool {
    i == 0
        || ((langs.is_empty() || langs.iter().any(|w| w.keeps(n)))
            && !(machine == Machine::Drop
                && teigeodata_format::names::Origin::from_code(n.rule).is_machine()))
}

/// How complete each profile language is: for each country with a pack in the profile, and
/// for the rest of the layer, the places (timezone areas left out) with a name in it, as a
/// Markdown table; empty when the profile keeps every name.
fn coverage(p: &File, inputs: &[Input], sections: &[extract::Body]) -> Result<String, String> {
    use teigeodata_format::idmap::IdMap;
    let langs: Vec<Wanted> = p.languages.iter().filter_map(|l| language(l)).collect();
    if langs.is_empty() {
        return Ok(String::new());
    }
    let section = |k: Kind| {
        sections
            .iter()
            .find(|s| s.0 == k)
            .map(|b| teigeodata_format::Section::unpacked(k, b.2, &b.3))
            .ok_or(format!("coverage needs {k}"))
    };
    let err = |e: teigeodata_format::Error| e.to_string();
    let places = Places::open(&section(Kind::PLACES)?).map_err(err)?;
    let names = Names::open(&section(Kind::NAMES)?).map_err(err)?;
    let ids = IdMap::open(&section(Kind::IDMAP)?).map_err(err)?;
    let own: Vec<&str> = inputs.iter().filter_map(|i| i.cc.as_deref()).collect();
    // Group 0 is the rest of the layer; group g > 0 is own[g - 1].
    let mut counts = vec![(0_usize, vec![0_usize; langs.len()]); own.len() + 1];
    for row in 0..places.len() {
        if places.kind(row) == Some(teigeodata_core::PlaceKind::TimezoneArea) {
            continue;
        }
        let cc = ids
            .id(row)
            .and_then(|id| id.namespace().country())
            .map(|c| String::from_utf8_lossy(&c).into_owned());
        let g = cc
            .and_then(|c| own.iter().position(|o| *o == c))
            .map_or(0, |i| i + 1);
        let Some((total, with)) = counts.get_mut(g) else {
            continue;
        };
        *total += 1;
        for (w, n) in langs.iter().zip(with.iter_mut()) {
            if names.names(row).any(|r| w.matches(r.lang(), r.script())) {
                *n += 1;
            }
        }
    }
    let mut s = format!(
        "| Places | Rows | {} |\n|---|--:|{}\n",
        p.languages.join(" | "),
        "--:|".repeat(langs.len())
    );
    let rows = counts.iter().enumerate();
    for (g, (total, with)) in rows.clone().skip(1).chain(rows.take(1)) {
        let label = g
            .checked_sub(1)
            .and_then(|i| own.get(i))
            .map_or_else(|| "the rest".to_owned(), |c| format!("`{c}`"));
        let cells: Vec<String> = with
            .iter()
            .map(|&n| {
                #[allow(clippy::cast_precision_loss, reason = "a share for display")]
                let pct = if *total == 0 {
                    0.0
                } else {
                    100.0 * n as f64 / *total as f64
                };
                format!("{n} ({pct:.1}%)")
            })
            .collect();
        let _ = writeln!(s, "| {label} | {total} | {} |", cells.join(" | "));
    }
    Ok(s)
}

/// `ne` or `ne-Deva`; `None` for anything else.
fn language(code: &str) -> Option<Wanted> {
    let mut it = code.split('-');
    let lang = Tag::new(it.next()?)?;
    let script = match it.next() {
        None => None,
        Some(s) if s.len() == 4 => Some(Tag::new(s)?),
        Some(_) => return None,
    };
    it.next().is_none().then_some(Wanted { lang, script })
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
    let variant = super::world::variant(
        p.world.as_deref().unwrap_or(super::world::DEFAULT_CITIES),
        &p.world_labels,
        &p.world_towns,
    );
    let world = root.join(format!(
        "data/build/baseline/world/world-baseline-{build}{variant}.tgd"
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
        match (p.languages.is_empty(), p.machine_names) {
            (true, Machine::Keep) => "every name".to_owned(),
            (true, Machine::Drop) => "every name a person wrote".to_owned(),
            (false, m) => format!(
                "the canonical name plus {}{}",
                p.languages.join(", "),
                if m == Machine::Drop {
                    ", machine names left out"
                } else {
                    ""
                }
            ),
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

#[cfg(test)]
mod tests {
    use super::{Machine, keeps_name, language};
    use teigeodata_format::names::{NameRecord, Origin, Tag};

    #[test]
    fn machine_names_are_kept_or_dropped_but_never_the_canonical_name() {
        let rec = |origin: Origin| NameRecord {
            text: "पोखरा",
            lang: Tag::new("ne"),
            script: Tag::new("Deva"),
            rule: origin.code(),
            ..NameRecord::default()
        };
        let (person, machine) = (rec(Origin::Official), rec(Origin::Transliteration));
        let ne = [language("ne-Deva").unwrap()];
        for langs in [&ne[..], &[]] {
            assert!(keeps_name(langs, Machine::Keep, 1, &machine));
            assert!(!keeps_name(langs, Machine::Drop, 1, &machine));
            assert!(keeps_name(langs, Machine::Drop, 1, &person));
            assert!(keeps_name(langs, Machine::Drop, 0, &machine), "canonical");
        }
        assert!(!keeps_name(
            &[language("en").unwrap()],
            Machine::Keep,
            1,
            &person
        ));
    }

    #[test]
    fn a_language_with_its_script_keeps_only_that_script() {
        let rec = |text, lang, script| NameRecord {
            text,
            lang: Tag::new(lang),
            script: Tag::new(script),
            ..NameRecord::default()
        };
        let deva = rec("पोखरा", "ne", "Deva");
        let roman = rec("Pokharā", "ne", "Latn");
        let ne = language("ne-Deva").unwrap();
        assert!(ne.keeps(&deva));
        assert!(
            !ne.keeps(&roman),
            "a romanisation tagged ne is not a Nepali name"
        );
        let any = language("ne").unwrap();
        assert!(any.keeps(&deva) && any.keeps(&roman));
        assert!(!language("en").unwrap().keeps(&deva));
        assert!(language("ne-Deva-x").is_none() && language("ne-De").is_none());
    }
}
