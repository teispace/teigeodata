//! The store commands (D-182): `init`, `refresh`, `catalog`, `find`, `locate`, `install`,
//! `update`, `remove`, `pin`, `repair` and `status`, over one store directory:
//!
//! ```text
//! <store>/config.json   the metadata and asset mirrors, from `init`
//! <store>/root.json     the trusted root `init` was given: the trust anchor
//! <store>/meta/         the verified catalog (`sync::refresh`, D-179)
//! <store>/journal, packs/, staging/   the installed packs (`sync::store`, D-171)
//! ```
//!
//! A network failure exits 3, so a script can tell "try again later" from a refusal (1). The
//! catalog works offline once refreshed: `catalog`, `find`, `locate` and `status` never touch
//! the network.

use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use teigeodata_core::E7;
use teigeodata_engine::{Engine, Layer, LayerOptions, Load, Place};
use teigeodata_format::Part;
use teigeodata_format::component::{Component, Components, Parts};
use teigeodata_sync::catalog::{self, Catalog, Entry};
pub(crate) use teigeodata_sync::client::Want;
use teigeodata_sync::client::{self as client, Client, ClientError, language, path_of};
use teigeodata_sync::fetch::Downloads;
use teigeodata_sync::store::Health;

use crate::Fail;

/// The store directory: `--store`, else `TEIGEODATA_STORE`, else the platform's data directory.
pub(crate) fn default_store() -> PathBuf {
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let home = env("HOME").unwrap_or_else(|| PathBuf::from("."));
    let base = if cfg!(target_os = "macos") {
        home.join("Library/Application Support")
    } else if cfg!(windows) {
        env("LOCALAPPDATA").unwrap_or(home)
    } else {
        env("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share"))
    };
    base.join("teigeodata")
}

/// A JSON object's field, `null` when absent.
fn field<'a>(v: &'a Value, key: &str) -> &'a Value {
    v.get(key).unwrap_or(&Value::Null)
}

impl From<ClientError> for Fail {
    fn from(e: ClientError) -> Self {
        match e {
            ClientError::Network(m) => Self::Network(m),
            ClientError::Refused(m) => Self::Refused(m),
        }
    }
}

/// The store at `root`, set up by `init`; a pack found damaged is reported on standard error.
pub(crate) fn open(root: &Path) -> Result<Client, Fail> {
    let (c, damaged) = Client::open(root)?;
    for d in &damaged {
        let _ = writeln!(std::io::stderr(), "tgd: {d} is damaged: `tgd repair` it");
    }
    Ok(c)
}

/// The engine over the installed packs, each opened with its installed components and parts,
/// and the packs `extra`.
pub(crate) fn engine(c: &Client, extra: &[PathBuf]) -> Result<Engine, Fail> {
    let mut e = Engine::new();
    for r in c.packs() {
        let path = c.pack_path(r)?;
        let options = LayerOptions {
            installed: Some(r.set),
            installed_parts: r.parts,
            ..LayerOptions::default()
        };
        e.add(Layer::open(&path, options).map_err(|x| Fail::of(path.display(), x))?);
    }
    for p in extra {
        let options = LayerOptions {
            load: Load::Read,
            ..LayerOptions::default()
        };
        e.add(Layer::open(p, options).map_err(|x| Fail::of(p.display(), x))?);
    }
    Ok(e)
}

pub(crate) fn init(
    root: &Path,
    repo: &[String],
    assets: &[String],
    trusted_root: &Path,
    out: &mut dyn Write,
) -> Result<(), Fail> {
    let bytes = std::fs::read(trusted_root).map_err(|e| Fail::of(trusted_root.display(), e))?;
    client::init(root, repo, assets, &bytes)?;
    writeln!(
        out,
        "store {} set up: {} metadata and {} asset mirrors",
        root.display(),
        repo.len(),
        assets.len()
    )?;
    Ok(())
}

pub(crate) fn refresh(root: &Path, countries: &[String], out: &mut dyn Write) -> Result<(), Fail> {
    let mut s = open(root)?;
    let r = s.refresh(countries)?;
    let ts = r.trusted.timestamp().map_or(0, |t| t.common.version);
    if r.fetched.len() <= 1 {
        writeln!(out, "the catalog is current (timestamp {ts})")?;
    } else {
        writeln!(
            out,
            "the catalog is at timestamp {ts}: fetched {} ({} bytes)",
            r.fetched.join(", "),
            r.bytes
        )?;
    }
    Ok(())
}

fn entry_json(c: &Client, e: &Entry) -> Value {
    let all = e.size_of(Components::all());
    json!({
        "path": e.path(),
        "release": e.release,
        "download": all.wire,
        "disk": all.disk,
        "components": e.sizes.keys().map(|c| c.name()).collect::<Vec<_>>(),
        "parts": e.parts.iter().map(part_json).collect::<Vec<_>>(),
        "licence": e.licence,
        "state": c.state(e).name(),
    })
}

/// A catalog part for people and `--json`: its component, bytes, and its language or its
/// outline's level, level of detail and tolerance.
fn part_json(p: &catalog::PartSize) -> Value {
    let (component, download, disk) = (p.component.name(), p.size.wire, p.size.disk);
    p.part.outline_levels().map_or_else(
        || json!({"component": component, "language": language(p.part), "download": download, "disk": disk}),
        |(level, lod)| json!({"component": component, "level": level, "lod": lod, "tolerance_m": p.tolerance_m, "download": download, "disk": disk}),
    )
}

/// The parts a record holds, for people and `--json`: `null` for every one.
fn parts_json(p: Parts) -> Value {
    json!({
        "languages": p.language_parts().map(|l| l.iter().map(|&x| language(x)).collect::<Vec<_>>()),
        "outline": p.outline_part().map(|o| o.outline_levels().map_or_else(
            || json!("base"),
            |(level, lod)| json!({"level": level, "lod": lod}),
        )),
    })
}

/// The parts a record holds, for people: nothing for every part, else ` names en,*` and
/// ` outlines to level 1 at lod 2` or ` outlines whole`.
fn parts_text(p: Parts) -> String {
    let mut t = String::new();
    if let Some(l) = p.language_parts() {
        let l: Vec<String> = l.iter().map(|&x| language(x)).collect();
        let _ = write!(t, "  names {}", l.join(","));
    }
    match p.outline_part().map(Part::outline_levels) {
        Some(Some((level, lod))) => {
            let _ = write!(t, "  outlines to level {level} at lod {lod}");
        }
        Some(None) => t.push_str("  outlines whole"),
        None => {}
    }
    t
}

pub(crate) fn catalog(root: &Path, as_json: bool, out: &mut dyn Write) -> Result<(), Fail> {
    let mut s = open(root)?;
    let t = s.offline()?;
    let (c, bad) = Catalog::trusted(&t);
    for (p, e) in &bad {
        let _ = writeln!(std::io::stderr(), "tgd: {p}: {e}");
    }
    let rows: Vec<Value> = c.entries().map(|(_, e)| entry_json(&s, e)).collect();
    if as_json {
        serde_json::to_writer_pretty(&mut *out, &rows)?;
        writeln!(out)?;
        return Ok(());
    }
    writeln!(
        out,
        "{:<28} {:<12} {:>12} {:>12}  state",
        "pack", "release", "download", "disk"
    )?;
    for r in &rows {
        writeln!(
            out,
            "{:<28} {:<12} {:>12} {:>12}  {}",
            field(r, "path").as_str().unwrap_or_default(),
            field(r, "release").as_str().unwrap_or_default(),
            field(r, "download"),
            field(r, "disk"),
            field(r, "state").as_str().unwrap_or_default()
        )?;
    }
    if rows.is_empty() {
        writeln!(
            out,
            "(empty: `tgd refresh --countries <cc>` lists a country's packs)"
        )?;
    }
    Ok(())
}

pub(crate) fn find(
    root: &Path,
    query: &str,
    limit: usize,
    with: &[PathBuf],
    as_json: bool,
    out: &mut dyn Write,
) -> Result<(), Fail> {
    let mut s = open(root)?;
    let t = s.offline()?;
    let (c, _) = Catalog::trusted(&t);
    let engine = engine(&s, with)?;
    let found = c.find(&engine, query, limit)?;
    let mut buf = [0_u8; 1024];
    let mut rows = Vec::new();
    for f in &found {
        let id = teigeodata_core::PlaceId::from_raw(f.candidate.id);
        let place = id.and_then(|id| engine.get(id).ok().flatten());
        let name = match &place {
            Some(p) => engine.name(p, &mut buf)?.map(str::to_owned),
            None => None,
        };
        rows.push(json!({
            "id": id.map(|i| i.to_string()),
            "name": name,
            "lat": f.candidate.point.lat_degrees(),
            "lon": f.candidate.point.lon_degrees(),
            "countries": f.candidate.countries.iter().map(|c| String::from_utf8_lossy(c).into_owned()).collect::<Vec<_>>(),
            "packs": f.packs.iter().map(|e| entry_json(&s, e)).collect::<Vec<_>>(),
        }));
    }
    print_places(&rows, as_json, out)?;
    if rows.is_empty() {
        return Err(Fail::Refused("no result".into()));
    }
    Ok(())
}

fn print_places(rows: &[Value], as_json: bool, out: &mut dyn Write) -> Result<(), Fail> {
    if as_json {
        serde_json::to_writer_pretty(&mut *out, rows)?;
        writeln!(out)?;
        return Ok(());
    }
    for r in rows {
        writeln!(
            out,
            "{}  {},{}  {}",
            field(r, "name").as_str().unwrap_or("(no name)"),
            field(r, "lat"),
            field(r, "lon"),
            field(r, "id").as_str().unwrap_or_default()
        )?;
        for p in field(r, "packs").as_array().into_iter().flatten() {
            writeln!(
                out,
                "  {} {} ({} bytes, {})",
                field(p, "path").as_str().unwrap_or_default(),
                field(p, "release").as_str().unwrap_or_default(),
                field(p, "download"),
                field(p, "state").as_str().unwrap_or_default()
            )?;
        }
    }
    Ok(())
}

pub(crate) fn locate(
    root: &Path,
    at: teigeodata_core::Point<E7>,
    radius_m: u32,
    with: &[PathBuf],
    as_json: bool,
    out: &mut dyn Write,
) -> Result<(), Fail> {
    let mut s = open(root)?;
    let t = s.offline()?;
    let (c, _) = Catalog::trusted(&t);
    let engine = engine(&s, with)?;
    let mut countries: Vec<Place> = Vec::new();
    engine.countries_near(at, radius_m, &mut countries)?;
    let mut buf = [0_u8; 1024];
    let mut rows = Vec::new();
    for country in &countries {
        let Some(cc) = country.id.namespace().country() else {
            continue;
        };
        rows.push(json!({
            "id": country.id.to_string(),
            "name": engine.name(country, &mut buf)?,
            "lat": country.point.lat_degrees(),
            "lon": country.point.lon_degrees(),
            "countries": [String::from_utf8_lossy(&cc)],
            "packs": c.packs_at(cc, at).map(|e| entry_json(&s, e)).collect::<Vec<_>>(),
        }));
    }
    print_places(&rows, as_json, out)?;
    if rows.is_empty() {
        return Err(Fail::Refused(
            "no country there: give a world pack with --pack, or install one".into(),
        ));
    }
    Ok(())
}

pub(crate) fn install(
    root: &Path,
    packs: &[String],
    want: &Want,
    pin: bool,
    downloads: Option<&Path>,
    out: &mut dyn Write,
) -> Result<(), Fail> {
    let mut s = open(root)?;
    let paths: Vec<String> = packs.iter().map(|p| path_of(p)).collect();
    let report = s.install(&paths, want, pin, downloads);
    let report = report.map_err(|e| match e {
        ClientError::Refused(m) if m.starts_with("at most") => {
            Fail::Refused(format!("--names: {m}"))
        }
        e => e.into(),
    })?;
    if let Some(m) = &report.stale_catalog {
        let _ = writeln!(std::io::stderr(), "tgd: {m}; using the stored catalog");
    }
    print_done(&report.done, out)?;
    if report.pending.is_empty() {
        return Ok(());
    }
    let pending: Vec<Value> = report
        .pending
        .iter()
        .map(|p| {
            let ranges: Vec<Value> = p
                .ranges
                .iter()
                .map(|r| json!({ "start": r.start, "end": r.end, "file": Downloads::name(r) }))
                .collect();
            json!({
                "pack": p.pack,
                "urls": p.urls,
                "length": p.length,
                "dir": p.dir,
                "ranges": ranges,
            })
        })
        .collect();
    serde_json::to_writer_pretty(&mut *out, &pending)?;
    writeln!(out)?;
    Err(Fail::Pending)
}

/// One line per pack an install or update reached.
fn print_done(done: &[client::Done], out: &mut dyn Write) -> Result<(), Fail> {
    for d in done {
        match &d.installed {
            None => writeln!(out, "{} {} is installed", d.path, d.release)?,
            Some(i) => writeln!(
                out,
                "{} {}: {} bytes fetched, {} reused, {} kept from an earlier attempt",
                d.path, d.release, i.fetched, i.reused, i.kept
            )?,
        }
    }
    Ok(())
}

pub(crate) fn update(root: &Path, packs: &[String], out: &mut dyn Write) -> Result<(), Fail> {
    let mut s = open(root)?;
    let paths: Vec<String> = packs.iter().map(|p| path_of(p)).collect();
    let done = s.update(&paths)?;
    print_done(&done, out)?;
    if done.is_empty() {
        writeln!(out, "everything is up to date")?;
    }
    Ok(())
}

pub(crate) fn remove(root: &Path, pack: &str, out: &mut dyn Write) -> Result<(), Fail> {
    let mut s = open(root)?;
    for r in s.remove(&path_of(pack))? {
        writeln!(out, "removed {} {}", r.key(), r.release)?;
    }
    Ok(())
}

pub(crate) fn pin(root: &Path, pack: &str, on: bool, out: &mut dyn Write) -> Result<(), Fail> {
    let mut s = open(root)?;
    let path = path_of(pack);
    s.pin(&path, on)?;
    writeln!(out, "{path} {}", if on { "pinned" } else { "unpinned" })?;
    Ok(())
}

pub(crate) fn repair(root: &Path, pack: &str, out: &mut dyn Write) -> Result<(), Fail> {
    let mut s = open(root)?;
    let path = path_of(pack);
    let r = s.repair(&path)?;
    match r.fetched {
        None => writeln!(out, "{path} is whole")?,
        Some(fetched) => {
            writeln!(out, "{path}: {:?}; repairing", r.health)?;
            writeln!(out, "{path} repaired: {fetched} bytes fetched")?;
        }
    }
    Ok(())
}

pub(crate) fn status(
    root: &Path,
    deep: bool,
    as_json: bool,
    out: &mut dyn Write,
) -> Result<(), Fail> {
    let mut s = open(root)?;
    let st = s.status(deep)?;
    let packs: Vec<_> = st.packs.iter().map(|(r, _)| r.clone()).collect();
    let mut rows = Vec::new();
    let mut bad = 0_usize;
    for (r, health) in &st.packs {
        bad += usize::from(*health != Health::Good);
        rows.push(json!({
            "path": r.key(),
            "release": r.release,
            "components": r.set.iter().map(Component::name).collect::<Vec<_>>(),
            "parts": parts_json(r.parts),
            "bytes": r.pack_len,
            "pinned": r.pinned,
            "asked": r.asked,
            "health": format!("{health:?}"),
        }));
    }
    let ts = st.catalog;
    let catalog = json!({
        "timestamp": ts.map(|(version, _)| version),
        "expires": ts.map(|(_, expires)| crate::pack::utc(u64::try_from(expires).unwrap_or(0))),
    });
    if as_json {
        serde_json::to_writer_pretty(&mut *out, &json!({"catalog": catalog, "packs": rows}))?;
        writeln!(out)?;
    } else {
        match ts {
            Some((version, _)) => writeln!(
                out,
                "catalog: timestamp {}, expires {}",
                version,
                field(&catalog, "expires").as_str().unwrap_or_default()
            )?,
            None => writeln!(out, "catalog: not refreshed yet")?,
        }
        for (r, rec) in rows.iter().zip(&packs) {
            writeln!(
                out,
                "{:<28} {:<12} {:>12} bytes  {}{}  {}{}",
                field(r, "path").as_str().unwrap_or_default(),
                field(r, "release").as_str().unwrap_or_default(),
                field(r, "bytes"),
                field(r, "health").as_str().unwrap_or_default(),
                if field(r, "pinned") == &Value::Bool(true) {
                    ", pinned"
                } else {
                    ""
                },
                field(r, "components")
                    .as_array()
                    .map(|c| c
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(","))
                    .unwrap_or_default(),
                parts_text(rec.parts),
            )?;
        }
    }
    if bad > 0 {
        return Err(Fail::Refused(format!("{bad} packs need `tgd repair`")));
    }
    Ok(())
}
