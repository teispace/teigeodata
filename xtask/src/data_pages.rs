//! `cargo xtask data-pages`: the docs site's coverage page and one page per country, generated
//! from `data/countries/<cc>/` and the pack `site/data.json` pins (phase 7.5,
//! [research](../../docs/02-research/notes/docs-data-pages.md)).
//!
//! The pack is read through `tgd export` (every place, streamed) and `tgd inspect --json`; the
//! countries are the pack's ID prefixes and the folders of `data/countries/` (P16: no country is
//! named here). `--check` fails when a page differs from what this writes, or when a country
//! page has no folder behind it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::io::{BufRead as _, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::bindings::escape_mdx;
use crate::data::Profile;
use crate::docs_examples::{Data, pinned, read};
use crate::lexicon::{Kind, LexiconFile, Status};
use crate::measured::Record;
use crate::sources::{LITE, Lock};

const OUT: &str = "site/content/docs/data";

/// One line of `tgd export`.
#[derive(Deserialize)]
struct Row {
    id: String,
    name: String,
    kind: String,
    admin_level: u8,
    #[serde(default)]
    elevation: Option<f64>,
    #[serde(default)]
    timezone: Option<String>,
    #[serde(default)]
    iso_code: Option<String>,
    #[serde(default)]
    parent: Option<String>,
}

/// What the pack holds for one country.
#[derive(Default)]
struct Country {
    /// The name and ISO code of its country row.
    name: Option<String>,
    iso: Option<String>,
    places: u64,
    /// Places by kind.
    kinds: BTreeMap<String, u64>,
    /// Places with a height, by kind.
    heights_by_kind: BTreeMap<String, u64>,
    /// Admin units (and the country, level 0) by level: units, with an ISO code.
    levels: BTreeMap<u8, (u64, u64)>,
    /// Heights in whole metres, for the range and the median.
    heights: Vec<i32>,
    zones: BTreeMap<String, u64>,
}

struct Args {
    check: bool,
    fetch: bool,
    pack: Option<PathBuf>,
    tgd: Option<PathBuf>,
}

fn parse(args: &[String]) -> Result<Args, Vec<String>> {
    let mut out = Args {
        check: false,
        fetch: false,
        pack: None,
        tgd: None,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--check" => out.check = true,
            "--fetch" => out.fetch = true,
            "--pack" => out.pack = it.next().map(PathBuf::from),
            "--tgd" => out.tgd = it.next().map(PathBuf::from),
            other => {
                return Err(vec![format!(
                    "unknown argument {other}; usage: cargo xtask data-pages [--check] [--fetch] [--pack <path>] [--tgd <path>]"
                )]);
            }
        }
    }
    Ok(out)
}

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, Vec<String>> {
    let args = parse(args)?;
    let data = crate::docs_examples::data(root)?;
    let pack = match args.pack {
        Some(p) => p,
        None => pinned(root, &data, args.fetch)?,
    };
    let tgd = args.tgd.unwrap_or_else(|| root.join("target/release/tgd"));
    if !tgd.is_file() {
        return Err(vec![format!(
            "{} is missing: build it with `cargo build --release -p teigeodata-cli`",
            tgd.display()
        )]);
    }
    let countries = read_pack(&tgd, &pack)?;
    let (file_bytes, components) = components(&tgd, &pack)?;
    let lock: Lock = toml::from_str(&read(&root.join("data/sources.lock"))?)
        .map_err(|e| vec![format!("data/sources.lock: {e}")])?;

    let mut pages: BTreeMap<PathBuf, String> = BTreeMap::new();
    let mut folders = Vec::new();
    for entry in fs::read_dir(root.join("data/countries"))
        .map_err(|e| vec![format!("data/countries: {e}")])?
        .flatten()
    {
        let cc = entry.file_name().to_string_lossy().into_owned();
        if entry.path().join("profile.yaml").is_file() {
            folders.push(cc);
        }
    }
    folders.sort();
    let mut titles = Vec::new();
    for cc in &folders {
        let dir = root.join("data/countries").join(cc);
        let profile: Profile = serde_saphyr::from_str(&read(&dir.join("profile.yaml"))?)
            .map_err(|e| vec![format!("data/countries/{cc}/profile.yaml: {e}")])?;
        let lexicon: Option<LexiconFile> = match dir.join("lexicon.yaml") {
            p if p.is_file() => Some(
                serde_saphyr::from_str(&read(&p)?)
                    .map_err(|e| vec![format!("data/countries/{cc}/lexicon.yaml: {e}")])?,
            ),
            _ => None,
        };
        let held = countries.get(cc.as_str());
        let page = country_page(cc, &profile, lexicon.as_ref(), held, &lock, &data);
        titles.push((profile.name.clone(), cc.clone()));
        pages.insert(
            root.join(OUT).join("countries").join(format!("{cc}.mdx")),
            page,
        );
    }
    titles.sort();
    let meta = format!(
        "{{\n  \"title\": \"Countries\",\n  \"pages\": [{}]\n}}\n",
        titles
            .iter()
            .map(|(_, cc)| format!("\"{cc}\""))
            .collect::<Vec<_>>()
            .join(", ")
    );
    pages.insert(root.join(OUT).join("countries/meta.json"), meta);
    let pages_of: BTreeSet<&str> = folders.iter().map(String::as_str).collect();
    pages.insert(
        root.join(OUT).join("coverage.mdx"),
        coverage_page(&countries, (file_bytes, &components), &pages_of, &data),
    );

    pages.insert(root.join(OUT).join("measured.mdx"), measured_page(root)?);
    write_pages(root, &pages, args.check)
}

/// Writes the pages that changed, or with `check` lists them; a country page with no folder
/// behind it is stale either way.
fn write_pages(
    root: &Path,
    pages: &BTreeMap<PathBuf, String>,
    check: bool,
) -> Result<String, Vec<String>> {
    // Country pages with no folder behind them are stale.
    let mut problems = Vec::new();
    if let Ok(dir) = fs::read_dir(root.join(OUT).join("countries")) {
        for entry in dir.flatten() {
            let path = entry.path();
            if !pages.contains_key(&path) {
                problems.push(format!(
                    "{}: no data/countries folder writes it; remove it",
                    rel(root, &path)
                ));
            }
        }
    }
    let mut written = 0;
    for (path, text) in pages {
        let current = fs::read_to_string(path).unwrap_or_default();
        if current == *text {
            continue;
        }
        if check {
            problems.push(format!(
                "{} differs from what `cargo xtask data-pages` writes",
                rel(root, path)
            ));
        } else {
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir).map_err(|e| vec![format!("{}: {e}", dir.display())])?;
            }
            fs::write(path, text).map_err(|e| vec![format!("{}: {e}", path.display())])?;
            written += 1;
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    Ok(if check {
        format!("data pages ok: {} files up to date", pages.len())
    } else {
        format!("data pages: wrote {written} of {} files", pages.len())
    })
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Streams `tgd export` into per-country counts, keyed by the ID's country prefix.
fn read_pack(tgd: &Path, pack: &Path) -> Result<BTreeMap<String, Country>, Vec<String>> {
    let mut child = Command::new(tgd)
        .arg("export")
        .arg(pack)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| vec![format!("{}: {e}", tgd.display())])?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| vec!["tgd export: no output".to_owned()])?;
    let mut rows = Vec::new();
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|e| vec![format!("tgd export: {e}")])?;
        let row: Row =
            serde_json::from_str(&line).map_err(|e| vec![format!("tgd export: {e}: {line}")])?;
        rows.push(row);
    }
    let done = child
        .wait_with_output()
        .map_err(|e| vec![format!("tgd export: {e}")])?;
    if !done.status.success() {
        return Err(vec![format!(
            "tgd export {} failed: {}",
            pack.display(),
            String::from_utf8_lossy(&done.stderr).trim()
        )]);
    }
    tally(&rows)
}

/// The country code of an ID (`tgd:np/1xhb` is `np`).
fn namespace(id: &str) -> Option<&str> {
    id.strip_prefix("tgd:")
        .and_then(|r| r.split_once('/'))
        .map(|(cc, _)| cc)
}

/// Counts the rows per country. A place's country is its country ancestor in the hierarchy
/// (D-237: an ID keeps the code of its first build, which can name the other side of a
/// disputed border); a place with none counts under its ID's code.
fn tally(rows: &[Row]) -> Result<BTreeMap<String, Country>, Vec<String>> {
    let index: std::collections::HashMap<&str, usize> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| (r.id.as_str(), i))
        .collect();
    let mut countries: BTreeMap<String, Country> = BTreeMap::new();
    for row in rows {
        // At most a few levels; the bound stops a cycle.
        let mut at = row;
        for _ in 0..32 {
            if at.kind == "country" {
                break;
            }
            match at
                .parent
                .as_deref()
                .and_then(|p| index.get(p))
                .and_then(|&i| rows.get(i))
            {
                Some(up) => at = up,
                None => break,
            }
        }
        let owner = if at.kind == "country" {
            &at.id
        } else {
            &row.id
        };
        let Some(cc) = namespace(owner) else {
            return Err(vec![format!(
                "tgd export: an ID with no country: {}",
                row.id
            )]);
        };
        let c = countries.entry(cc.to_owned()).or_default();
        c.places += 1;
        *c.kinds.entry(row.kind.clone()).or_default() += 1;
        if row.kind == "country" {
            c.name = Some(row.name.clone());
            c.iso.clone_from(&row.iso_code);
        }
        if row.kind == "country" || row.kind == "admin" {
            let level = c.levels.entry(row.admin_level).or_default();
            level.0 += 1;
            level.1 += u64::from(row.iso_code.is_some());
        }
        if let Some(h) = row.elevation {
            *c.heights_by_kind.entry(row.kind.clone()).or_default() += 1;
            #[allow(clippy::cast_possible_truncation, reason = "heights are within ±11 km")]
            c.heights.push(h.round() as i32);
        }
        if let Some(z) = &row.timezone {
            *c.zones.entry(z.clone()).or_default() += 1;
        }
    }
    for c in countries.values_mut() {
        c.heights.sort_unstable();
    }
    Ok(countries)
}

/// What the pages read of `tgd inspect --json`.
#[derive(Deserialize)]
struct Inspect {
    bytes: u64,
    components: BTreeMap<String, u64>,
}

/// The pack's components and their bytes, from `tgd inspect --json`.
fn components(tgd: &Path, pack: &Path) -> Result<(u64, BTreeMap<String, u64>), Vec<String>> {
    let out = Command::new(tgd)
        .args(["inspect", "--json"])
        .arg(pack)
        .output()
        .map_err(|e| vec![format!("{}: {e}", tgd.display())])?;
    if !out.status.success() {
        return Err(vec![format!("tgd inspect {} failed", pack.display())]);
    }
    let inspect: Inspect = serde_json::from_slice(&out.stdout)
        .map_err(|e| vec![format!("tgd inspect --json: {e}")])?;
    Ok((inspect.bytes, inspect.components))
}

/// A number with thousands separators.
fn n(v: u64) -> String {
    let s = v.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A share as a percentage with one decimal.
fn pct(part: u64, whole: u64) -> String {
    if whole == 0 {
        return "0%".to_owned();
    }
    #[allow(clippy::cast_precision_loss, reason = "counts far below 2^52")]
    let p = part as f64 * 100.0 / whole as f64;
    format!("{p:.1}%")
}

/// Text for a table cell: MDX-escaped, with `|` escaped.
fn cell(text: &str) -> String {
    escape_mdx(text).replace('|', "\\|")
}

/// The kinds of place in the order the pages list them, with their plural names.
const KINDS: [(&str, &str); 5] = [
    ("country", "Country"),
    ("admin", "Admin units"),
    ("settlement", "Settlements"),
    ("locality", "Localities"),
    ("natural", "Natural features"),
];

fn kind_name(kind: &str) -> String {
    KINDS
        .iter()
        .find(|(k, _)| *k == kind)
        .map_or_else(|| kind.to_owned(), |(_, name)| (*name).to_owned())
}

fn generated(out: &mut String, data: &Data, from: &str) {
    let _ = write!(
        out,
        "<Callout title=\"Generated from the data\">\nThis page is written by `cargo xtask data-pages` from {from} and the pack the site's\nexamples run on (`{}`, data release `{}`), and CI fails when it differs. It changes with\neach release.\n</Callout>\n\n",
        data.file, data.release
    );
}

fn coverage_page(
    countries: &BTreeMap<String, Country>,
    (file_bytes, components): (u64, &BTreeMap<String, u64>),
    pages: &BTreeSet<&str>,
    data: &Data,
) -> String {
    let mut out = String::new();
    let total: u64 = countries.values().map(|c| c.places).sum();
    let zones = countries
        .values()
        .map(|c| c.kinds.get("timezone_area").copied().unwrap_or(0))
        .sum::<u64>();
    let with_country = countries
        .values()
        .filter(|c| c.kinds.keys().any(|k| k != "timezone_area"))
        .count();
    let _ = write!(
        out,
        "---\ntitle: Coverage\ndescription: Every country in the released pack, with its places by kind and admin level, read from the pack itself.\n---\n\n"
    );
    generated(&mut out, data, "`data/countries/`");
    let _ = write!(
        out,
        "`{}` holds **{} places**: {} in {} countries and territories, and {} timezone areas. Countries with rules of their own in\n[`data/countries`](https://github.com/teispace/teigeodata/tree/main/data/countries) have a\npage of their own; the rest are covered by the world sources alone.\n\n## Inside the pack\n\n| Component | Bytes |\n|---|--:|\n",
        data.file,
        n(total),
        n(total - zones),
        n(u64::try_from(with_country).unwrap_or(u64::MAX)),
        n(zones)
    );
    for (name, bytes) in components {
        let label = if name == "-" {
            "head, seek table, block hashes".to_owned()
        } else {
            format!("`{name}`")
        };
        let _ = writeln!(out, "| {label} | {} |", n(*bytes));
    }
    let _ = write!(
        out,
        "| **the file**, with the alignment between sections | **{}** |\n\nWhat each component answers is in [packs](/docs/concepts/packs#components).\n\n## Countries\n\n| Country | Code | Places | Admin units | Settlements | Other | With a height |\n|---|---|--:|--:|--:|--:|--:|\n",
        n(file_bytes)
    );
    let mut rows: Vec<(&str, &str, &Country)> = countries
        .iter()
        .filter_map(|(cc, c)| c.name.as_deref().map(|name| (name, cc.as_str(), c)))
        .collect();
    rows.sort_by(|a, b| a.0.cmp(b.0).then(a.1.cmp(b.1)));
    let unnamed: Vec<(&str, &Country)> = countries
        .iter()
        .filter(|(_, c)| c.name.is_none() && c.kinds.keys().any(|k| k != "timezone_area"))
        .map(|(cc, c)| (cc.as_str(), c))
        .collect();
    for (name, cc, c) in rows {
        let kind = |k: &str| c.kinds.get(k).copied().unwrap_or(0);
        let admin = kind("admin");
        let settlements = kind("settlement");
        let other = c.places - admin - settlements - kind("country");
        let shown = if pages.contains(cc) {
            format!("[{}](/docs/data/countries/{cc})", cell(name))
        } else {
            cell(name)
        };
        let _ = writeln!(
            out,
            "| {shown} | `{}` | {} | {} | {} | {} | {} |",
            c.iso.as_deref().unwrap_or(cc),
            n(c.places),
            n(admin),
            n(settlements),
            n(other),
            pct(u64::try_from(c.heights.len()).unwrap_or(u64::MAX), c.places)
        );
    }
    if !unnamed.is_empty() {
        out.push_str("\nSome places reach no country in the hierarchy: they are counted under their ID's code.\n\n| Code | Places | Admin units | Settlements |\n|---|--:|--:|--:|\n");
        for (cc, c) in unnamed {
            let kind = |k: &str| c.kinds.get(k).copied().unwrap_or(0);
            let _ = writeln!(
                out,
                "| `{}` | {} | {} | {} |",
                cc.to_ascii_uppercase(),
                n(c.places),
                n(kind("admin")),
                n(kind("settlement"))
            );
        }
    }
    out
}

fn country_page(
    cc: &str,
    profile: &Profile,
    lexicon: Option<&LexiconFile>,
    held: Option<&Country>,
    lock: &Lock,
    data: &Data,
) -> String {
    let mut out = String::new();
    let name = &profile.name;
    // Front matter as JSON strings, which YAML reads as they are.
    let _ = write!(
        out,
        "---\ntitle: {}\ndescription: {}\n---\n\n",
        serde_json::Value::from(name.as_str()),
        serde_json::Value::from(format!(
            "What teigeodata holds for {name}: its places, admin levels, heights, timezones, names and sources."
        ))
    );
    generated(
        &mut out,
        data,
        &format!(
            "[`data/countries/{cc}`](https://github.com/teispace/teigeodata/tree/main/data/countries/{cc})"
        ),
    );
    let empty = Country::default();
    let c = held.unwrap_or(&empty);
    let _ = write!(
        out,
        "## In the pack\n\n`{}` holds **{} places** in {name}: those whose country in the hierarchy is {name},\nas [disputed borders](/docs/concepts/accuracy#boundaries-and-perspective) are drawn. Their IDs begin `tgd:{cc}/`,\nexcept places a disputed border moved, which keep the ID they were first given.\n\n| Kind | Places | With a height |\n|---|--:|--:|\n",
        data.file,
        n(c.places)
    );
    let mut kinds: Vec<&String> = c.kinds.keys().collect();
    kinds.sort_by_key(|k| {
        KINDS
            .iter()
            .position(|(x, _)| x == k)
            .unwrap_or(KINDS.len())
    });
    for k in kinds {
        let count = c.kinds.get(k).copied().unwrap_or(0);
        let heights = c.heights_by_kind.get(k).copied().unwrap_or(0);
        let _ = writeln!(
            out,
            "| {} | {} | {} |",
            kind_name(k),
            n(count),
            pct(heights, count)
        );
    }
    admin_levels(&mut out, profile, c);
    heights_and_zones(&mut out, c);
    names(&mut out, cc, profile, lexicon);
    sources(&mut out, cc, name, lock);
    out
}

fn admin_levels(out: &mut String, profile: &Profile, c: &Country) {
    out.push_str("\n## Admin levels\n\nThe ladder is the country's own, from its profile. The counts are the admin places of each level\nin the pack: the current units, and former ones kept so their names still resolve (only current\nunits have an ISO code).\n\n| Level | Unit | Local name | In the pack | With an ISO 3166 code |\n|--:|---|---|--:|--:|\n");
    let levels: BTreeSet<u8> = profile
        .admin_levels
        .iter()
        .map(|l| l.level)
        .chain(c.levels.keys().copied())
        .chain(std::iter::once(0))
        .collect();
    for level in levels {
        let (unit, local) = if level == 0 {
            ("Country".to_owned(), String::new())
        } else {
            profile
                .admin_levels
                .iter()
                .find(|l| l.level == level)
                .map_or_else(
                    || (format!("level {level}"), String::new()),
                    |l| {
                        (
                            l.english.clone(),
                            l.native
                                .iter()
                                .map(|x| x.text.as_str())
                                .collect::<Vec<_>>()
                                .join(", "),
                        )
                    },
                )
        };
        let (units, iso) = c.levels.get(&level).copied().unwrap_or((0, 0));
        let _ = writeln!(
            out,
            "| {level} | {} | {} | {} | {} |",
            cell(&unit),
            cell(&local),
            n(units),
            n(iso)
        );
    }
}

fn heights_and_zones(out: &mut String, c: &Country) {
    out.push_str("\n## Heights and timezones\n\n");
    if let (Some(lo), Some(hi)) = (c.heights.first(), c.heights.last()) {
        let median = c.heights.get(c.heights.len() / 2).copied().unwrap_or(0);
        let _ = write!(
            out,
            "{} of {} places have a height, in metres above EGM2008: the lowest {lo} m, the median\n{median} m, the highest {hi} m. How heights are found and how good they are: [accuracy](/docs/concepts/accuracy).\n\n",
            n(u64::try_from(c.heights.len()).unwrap_or(u64::MAX)),
            n(c.places)
        );
    } else {
        out.push_str("No place has a height yet.\n\n");
    }
    if c.zones.is_empty() {
        out.push_str("No place has a timezone yet.\n");
    } else {
        out.push_str("| Timezone | Places |\n|---|--:|\n");
        for (zone, count) in &c.zones {
            let _ = writeln!(out, "| `{zone}` | {} |", n(*count));
        }
    }
}

fn names(out: &mut String, cc: &str, profile: &Profile, lexicon: Option<&LexiconFile>) {
    let _ = write!(
        out,
        "\n## Names\n\nLanguages (BCP 47): {}. Scripts (ISO 15924): {}.\n",
        profile
            .languages
            .iter()
            .map(|l| format!("`{l}`"))
            .collect::<Vec<_>>()
            .join(", "),
        profile
            .scripts
            .iter()
            .map(|s| format!("`{s}`"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    if !profile.romanise.is_empty() || !profile.transliterate.is_empty() {
        let _ = writeln!(
            out,
            "Names with no Latin spelling are romanised by {}; names missing in a script are written by {} and marked as transliterations, never canonical.",
            list_or_none(&profile.romanise),
            list_or_none(&profile.transliterate)
        );
    }
    if let Some(lexicon) = lexicon {
        let accepted = lexicon
            .terms
            .iter()
            .filter(|t| t.status == Status::Accepted)
            .count();
        let _ = write!(
            out,
            "\n### Lexicon\n\nThe spellings of generic terms in names, each with its evidence in\n[`lexicon.yaml`](https://github.com/teispace/teigeodata/blob/main/data/countries/{cc}/lexicon.yaml):\n{} terms, {accepted} accepted. A name is written with the canonical spelling, and search finds\nit by every variant.\n\n| Term | Local | Variants | Meaning | Kind | Status |\n|---|---|---|---|---|---|\n",
            lexicon.terms.len()
        );
        for t in &lexicon.terms {
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {} | {} |",
                cell(&t.canonical),
                cell(
                    &t.native
                        .iter()
                        .map(|x| x.text.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                cell(&t.variants.join(", ")),
                cell(&t.meaning),
                match t.kind {
                    Kind::Generic => "generic",
                    Kind::AdminSuffix => "admin suffix",
                    Kind::StreetType => "street type",
                    Kind::Qualifier => "qualifier",
                },
                match t.status {
                    Status::Accepted => "accepted",
                    Status::Proposed => "proposed",
                }
            );
        }
    }
}

fn sources(out: &mut String, cc: &str, name: &str, lock: &Lock) {
    let upper = cc.to_ascii_uppercase();
    let mut sources: Vec<_> = lock
        .sources
        .iter()
        .filter(|s| s.countries.contains(&upper))
        .collect();
    sources.sort_by(|a, b| a.title.cmp(&b.title));
    let _ = write!(
        out,
        "\n## Sources\n\nThe sources pinned for {name} in\n[`data/sources.lock`](https://github.com/teispace/teigeodata/blob/main/data/sources.lock), by version and SHA-256. The\nmain build may read any of them that its recipe names; the lite build only those under an open\nlicence that asks no more than attribution, by the class the gate reads (`CLEAN_ROOM.md`). The world sources every country\nshares are on the [data page](/docs/data#sources).\n\n| Source | Version | Licence | Class | Lite build |\n|---|---|---|---|---|\n"
    );
    for s in sources {
        let lite = if LITE.contains(&s.licence_class.as_str()) {
            "may read it"
        } else {
            "no"
        };
        // The licence's name, before its notes.
        let licence = s
            .licence
            .split(['(', ';'])
            .next()
            .unwrap_or_default()
            .trim();
        let _ = writeln!(
            out,
            "| [{}]({}) | {} | {} | `{}` | {lite} |",
            cell(&s.title),
            s.url.replace('{', "%7B").replace('}', "%7D"),
            cell(&s.version),
            cell(licence),
            s.licence_class
        );
    }
}

fn list_or_none(ids: &[String]) -> String {
    if ids.is_empty() {
        "none".to_owned()
    } else {
        ids.iter()
            .map(|i| format!("`{i}`"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The measured page: every record in `site/data/measured/`, in its order.
fn measured_page(root: &Path) -> Result<String, Vec<String>> {
    let dir = root.join(crate::measured::DIR);
    let mut records = Vec::new();
    for entry in fs::read_dir(&dir)
        .map_err(|e| vec![format!("{}: {e}", dir.display())])?
        .flatten()
    {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            let record: Record = serde_json::from_str(&read(&path)?)
                .map_err(|e| vec![format!("{}: {e}", rel(root, &path))])?;
            records.push(record);
        }
    }
    records.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.title.cmp(&b.title)));
    let mut out = String::from(
        "---\ntitle: \"Measured\"\ndescription: \"Sizes and speed as the gates measured them, each with its date, machine and commit.\"\n---\n\n",
    );
    out.push_str(
        "<Callout title=\"Generated from the gates\">\nThis page is written by `cargo xtask data-pages` from the records in\n[`site/data/measured`](https://github.com/teispace/teigeodata/tree/main/site/data/measured), each\nwritten by the gate that measured it (`--record`). Every figure says when, where and at which\ncommit it was measured: compare a timing only with timings from the same machine, and read one\nfrom a busy machine (a high load average) as a ceiling, not a typical value.\n</Callout>\n\nThe bytes of each component of the released pack are on the [coverage page](/docs/data/coverage);\nhow good the data is, and how it is gated, is on [accuracy](/docs/concepts/accuracy).\n",
    );
    for r in &records {
        let commit = r.commit.trim_end_matches("+changes");
        let commit = if r.commit.ends_with("+changes") {
            format!("`{commit}` with uncommitted changes")
        } else {
            format!("[`{commit}`](https://github.com/teispace/teigeodata/commit/{commit})")
        };
        let _ = write!(
            out,
            "\n## {}\n\n{}\n\nMeasured {} on {} (load average {}), commit {commit}, by `{}`.\n\n{}\n",
            cell(&r.title),
            r.intro,
            r.date,
            cell(&r.machine),
            r.load,
            r.command,
            r.table
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{cell, n, pct};

    #[test]
    fn numbers_group_by_thousands() {
        assert_eq!(n(0), "0");
        assert_eq!(n(999), "999");
        assert_eq!(n(1000), "1,000");
        assert_eq!(n(172_339), "172,339");
        assert_eq!(n(15_641_408), "15,641,408");
    }

    #[test]
    fn shares_round_to_one_decimal_and_survive_zero() {
        assert_eq!(pct(0, 0), "0%");
        assert_eq!(pct(1, 3), "33.3%");
        assert_eq!(pct(7287, 8091), "90.1%");
    }

    #[test]
    fn cells_escape_mdx_and_table_pipes() {
        assert_eq!(cell("a | b"), "a \\| b");
        assert_eq!(cell("x < y {z}"), "x &lt; y \\{z\\}");
        assert_eq!(cell("`{kept}`"), "`{kept}`");
    }
}
