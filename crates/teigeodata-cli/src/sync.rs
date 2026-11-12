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

use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use teigeodata_core::E7;
use teigeodata_engine::{Engine, Layer, LayerOptions, Load, Place};
use teigeodata_format::component::{Component, Components};
use teigeodata_sync::catalog::{self, Catalog, Entry};
use teigeodata_sync::fetch::{Downloads, FetchError, Mirrors, Ureq};
use teigeodata_sync::refresh::{self, RefreshError};
use teigeodata_sync::store::{Dir, Health, Store, StoreError};
use teigeodata_sync::trust::Trusted;

use crate::Fail;

/// The store's configuration, written by `init`.
const CONFIG: &str = "config.json";
/// The trust anchor `init` was given.
const ROOT: &str = "root.json";

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

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// A mirror base ends with `/`, so a file name joins it.
fn base(url: &str) -> String {
    if url.ends_with('/') {
        url.to_string()
    } else {
        format!("{url}/")
    }
}

fn network(e: &FetchError) -> bool {
    matches!(e, FetchError::Transport(_) | FetchError::Status(_))
}

impl From<RefreshError> for Fail {
    fn from(e: RefreshError) -> Self {
        match e {
            RefreshError::Network { .. } => Self::Network(e.to_string()),
            e => Self::Refused(e.to_string()),
        }
    }
}

impl From<FetchError> for Fail {
    fn from(e: FetchError) -> Self {
        if network(&e) {
            Self::Network(e.to_string())
        } else {
            Self::Refused(e.to_string())
        }
    }
}

impl From<StoreError> for Fail {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::Fetch(f) => f.into(),
            e => Self::Refused(e.to_string()),
        }
    }
}

/// An open store: its packs, its catalog's trust anchor and mirrors.
pub(crate) struct Opened {
    pub(crate) root: PathBuf,
    pub(crate) store: Store<Dir>,
    shipped: Vec<u8>,
    repo: Vec<String>,
    assets: Vec<String>,
    /// `install --downloads`: the directory a platform downloads assets into (D-184).
    downloads: Option<PathBuf>,
    /// What the packs installed from `downloads` still need, one record per asset.
    pending: Vec<Value>,
}

impl Opened {
    /// The store at `root`, set up by `init`.
    pub(crate) fn open(root: &Path) -> Result<Self, Fail> {
        let at = |p: &str| root.join(p);
        let config = std::fs::read(at(CONFIG)).map_err(|e| {
            Fail::Refused(format!(
                "{}: {e} (run `tgd init` first)",
                at(CONFIG).display()
            ))
        })?;
        let config: Value =
            serde_json::from_slice(&config).map_err(|e| Fail::of(at(CONFIG).display(), e))?;
        let list = |k: &str| -> Vec<String> {
            field(&config, k)
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_str).map(base).collect())
                .unwrap_or_default()
        };
        let shipped = std::fs::read(at(ROOT)).map_err(|e| Fail::of(at(ROOT).display(), e))?;
        let dir = Dir::new(root).map_err(|e| Fail::of(root.display(), e))?;
        let (store, recovered) = Store::open(dir).map_err(|e| Fail::of(root.display(), e))?;
        for d in &recovered.damaged {
            let _ = writeln!(std::io::stderr(), "tgd: {d} is damaged: `tgd repair` it");
        }
        Ok(Self {
            root: root.to_path_buf(),
            store,
            shipped,
            repo: list("repo"),
            assets: list("assets"),
            downloads: None,
            pending: Vec::new(),
        })
    }

    /// The verified catalog, from the store alone.
    fn offline(&mut self) -> Result<Trusted, Fail> {
        Ok(refresh::load(self.store.fs(), &self.shipped, now())?)
    }

    /// The catalog refreshed from the mirrors, with the roles of `want` and of every country
    /// whose pack is installed.
    fn refresh(&mut self, want: &[String]) -> Result<refresh::Refreshed, Fail> {
        let mut want: Vec<String> = want.to_vec();
        for r in self.store.packs() {
            if let Some(cc) = catalog::country_of(&r.address) {
                want.push(String::from_utf8_lossy(&cc).into_owned());
            }
        }
        want.sort();
        want.dedup();
        let refs: Vec<&str> = want.iter().map(String::as_str).collect();
        let mut mirrors = Mirrors::new(self.repo.clone());
        Ok(refresh::refresh(
            self.store.fs(),
            &mut Ureq::new(),
            &mut mirrors,
            &self.shipped,
            now(),
            &refs,
        )?)
    }

    /// The engine over the installed packs, each opened with its installed components and
    /// parts.
    pub(crate) fn engine(&self, extra: &[PathBuf]) -> Result<Engine, Fail> {
        let mut e = Engine::new();
        let dir = Dir::new(&self.root).map_err(|x| Fail::of(self.root.display(), x))?;
        for r in self.store.packs() {
            let path = dir.pack_path(r);
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
}

pub(crate) fn init(
    root: &Path,
    repo: &[String],
    assets: &[String],
    trusted_root: &Path,
    out: &mut dyn Write,
) -> Result<(), Fail> {
    let bytes = std::fs::read(trusted_root).map_err(|e| Fail::of(trusted_root.display(), e))?;
    // The root must verify by itself before it becomes the anchor.
    Trusted::new(&bytes, now()).map_err(|e| Fail::of(trusted_root.display(), e))?;
    std::fs::create_dir_all(root).map_err(|e| Fail::of(root.display(), e))?;
    let config = json!({ "repo": repo, "assets": assets });
    let write = |name: &str, data: &[u8]| {
        let (path, tmp) = (root.join(name), root.join(format!("{name}.tmp")));
        std::fs::write(&tmp, data)
            .and_then(|()| std::fs::rename(&tmp, &path))
            .map_err(|e| Fail::of(path.display(), e))
    };
    write(ROOT, &bytes)?;
    write(CONFIG, &serde_json::to_vec_pretty(&config)?)?;
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
    let mut s = Opened::open(root)?;
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

/// The catalog's path of an argument: `np` is `np.places.tgd`, `np.dem` is `np.dem.tgd`.
fn path_of(arg: &str) -> String {
    match arg.strip_suffix(".tgd") {
        Some(_) => arg.to_string(),
        None => match arg.split_once('.') {
            Some((a, c)) => catalog::path(a, c),
            None => catalog::path(arg, "places"),
        },
    }
}

/// What the catalog says of an entry against the store.
fn state(store: &Store<Dir>, e: &Entry) -> &'static str {
    match store
        .packs()
        .iter()
        .find(|r| r.address == e.address && r.component == e.component)
    {
        None => "available",
        Some(r) if r.head == e.head => "installed",
        Some(_) => "update",
    }
}

fn entry_json(store: &Store<Dir>, e: &Entry) -> Value {
    let all = e.size_of(Components::all());
    json!({
        "path": e.path(),
        "release": e.release,
        "download": all.wire,
        "disk": all.disk,
        "components": e.sizes.keys().map(|c| c.name()).collect::<Vec<_>>(),
        "licence": e.licence,
        "state": state(store, e),
    })
}

pub(crate) fn catalog(root: &Path, as_json: bool, out: &mut dyn Write) -> Result<(), Fail> {
    let mut s = Opened::open(root)?;
    let t = s.offline()?;
    let (c, bad) = Catalog::trusted(&t);
    for (p, e) in &bad {
        let _ = writeln!(std::io::stderr(), "tgd: {p}: {e}");
    }
    let rows: Vec<Value> = c.entries().map(|(_, e)| entry_json(&s.store, e)).collect();
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
    let mut s = Opened::open(root)?;
    let t = s.offline()?;
    let (c, _) = Catalog::trusted(&t);
    let engine = s.engine(with)?;
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
            "packs": f.packs.iter().map(|e| entry_json(&s.store, e)).collect::<Vec<_>>(),
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
    let mut s = Opened::open(root)?;
    let t = s.offline()?;
    let (c, _) = Catalog::trusted(&t);
    let engine = s.engine(with)?;
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
            "packs": c.packs_at(cc, at).map(|e| entry_json(&s.store, e)).collect::<Vec<_>>(),
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

/// The catalog entry at `path` and its asset's length.
fn target(t: &Trusted, path: &str) -> Result<(Entry, u64), Fail> {
    let (_, target) = t
        .trusted_targets()
        .find(|(p, _)| *p == path)
        .ok_or_else(|| Fail::Refused(format!("{path}: not in the catalog")))?;
    let entry = Entry::from_target(path, target).map_err(|e| Fail::of(path, e))?;
    Ok((entry, target.length))
}

/// The catalog for installing `paths`: refreshed for their countries, or the stored one when
/// the network fails (an install then fails at the network itself).
fn catalog_for(s: &mut Opened, paths: &[String]) -> Result<Trusted, Fail> {
    let want: Vec<String> = paths
        .iter()
        .filter_map(|p| catalog::country_of(p.split('.').next().unwrap_or_default()))
        .map(|cc| String::from_utf8_lossy(&cc).into_owned())
        .collect();
    match s.refresh(&want) {
        Ok(r) => Ok(r.trusted),
        Err(Fail::Network(m)) => {
            let _ = writeln!(std::io::stderr(), "tgd: {m}; using the stored catalog");
            s.offline()
        }
        Err(e) => Err(e),
    }
}

/// Installs `path` with `set` (or the installed set, or everything), its dependencies first.
fn install_one(
    s: &mut Opened,
    t: &Trusted,
    path: &str,
    set: Option<Components>,
    asked: bool,
    out: &mut dyn Write,
) -> Result<(), Fail> {
    let (entry, asset_len) = target(t, path)?;
    for dep in &entry.deps {
        if !s.store.packs().iter().any(|r| r.key() == *dep) {
            install_one(s, t, dep, None, false, out)?;
        }
    }
    let installed = s.store.packs().iter().find(|r| r.key() == path).cloned();
    // An install adds to what is there; `None` (an update, a dependency) keeps it.
    let set = match (set, installed.as_ref()) {
        (Some(want), Some(r)) => want.union(r.set),
        (Some(want), None) => want,
        (None, Some(r)) => r.set,
        (None, None) => Components::all(),
    };
    if installed
        .as_ref()
        .is_some_and(|r| r.head == entry.head && r.set == set.closed())
    {
        writeln!(out, "{path} {} is installed", entry.release)?;
        return Ok(());
    }
    let done = if let Some(dir) = s.downloads.clone() {
        // A platform's pass (D-184): the files it downloaded, or what is still missing.
        let at = dir.join(&entry.file);
        let mut parts = Downloads::open(&at, asset_len).map_err(|e| Fail::of(at.display(), e))?;
        let mut mirrors = Mirrors::new(["downloads/"]);
        match s.store.install(
            &mut parts,
            &mut mirrors,
            &entry.file,
            &entry,
            asset_len,
            set,
        ) {
            Err(FetchError::Needs(ranges)) => {
                let urls: Vec<String> = s
                    .assets
                    .iter()
                    .map(|m| format!("{m}{}", entry.file))
                    .collect();
                let parts: Vec<Value> = ranges
                    .iter()
                    .map(|r| json!({ "start": r.start, "end": r.end, "file": Downloads::name(r) }))
                    .collect();
                s.pending.push(json!({
                    "pack": path,
                    "urls": urls,
                    "length": asset_len,
                    "dir": at,
                    "ranges": parts,
                }));
                return Ok(());
            }
            r => r?,
        }
    } else {
        let mut mirrors = Mirrors::new(s.assets.clone());
        s.store.install(
            &mut Ureq::new(),
            &mut mirrors,
            &entry.file,
            &entry,
            asset_len,
            set,
        )?
    };
    if let Some(dir) = &s.downloads {
        // Committed: the downloaded files are no longer needed.
        let _ = std::fs::remove_dir_all(dir.join(&entry.file));
    }
    if !asked && installed.is_none() {
        s.store.mark(&entry.address, &entry.component, false)?;
    }
    writeln!(
        out,
        "{path} {}: {} bytes fetched, {} reused, {} kept from an earlier attempt",
        entry.release, done.fetched, done.reused, done.kept
    )?;
    Ok(())
}

pub(crate) fn install(
    root: &Path,
    packs: &[String],
    components: &[Component],
    pin: bool,
    downloads: Option<&Path>,
    out: &mut dyn Write,
) -> Result<(), Fail> {
    let mut s = Opened::open(root)?;
    s.downloads = downloads.map(Path::to_path_buf);
    let paths: Vec<String> = packs.iter().map(|p| path_of(p)).collect();
    let t = catalog_for(&mut s, &paths)?;
    // No components named: the whole pack.
    let set = Some(if components.is_empty() {
        Components::all()
    } else {
        Components::of(components).closed()
    });
    for p in &paths {
        install_one(&mut s, &t, p, set, true, out)?;
        if pin && s.store.packs().iter().any(|r| r.key() == *p) {
            let (a, c) = split(p);
            s.store.pin(a, c, true)?;
        }
    }
    if s.pending.is_empty() {
        return Ok(());
    }
    serde_json::to_writer_pretty(&mut *out, &s.pending)?;
    writeln!(out)?;
    Err(Fail::Pending)
}

pub(crate) fn update(root: &Path, packs: &[String], out: &mut dyn Write) -> Result<(), Fail> {
    let mut s = Opened::open(root)?;
    let installed: Vec<String> = s
        .store
        .packs()
        .iter()
        .map(teigeodata_sync::store::Record::key)
        .collect();
    let paths: Vec<String> = if packs.is_empty() {
        installed
    } else {
        packs.iter().map(|p| path_of(p)).collect()
    };
    let t = s.refresh(&[])?.trusted;
    let mut updated = 0_usize;
    for p in &paths {
        if !s.store.packs().iter().any(|r| r.key() == *p) {
            return Err(Fail::Refused(format!("{p}: not installed")));
        }
        let (entry, _) = target(&t, p)?;
        if s.store
            .packs()
            .iter()
            .any(|r| r.key() == *p && r.head != entry.head)
        {
            install_one(&mut s, &t, p, None, true, out)?;
            updated += 1;
        }
    }
    if updated == 0 {
        writeln!(out, "everything is up to date")?;
    }
    Ok(())
}

/// `address` and `component` of a catalog path.
fn split(path: &str) -> (&str, &str) {
    let stem = path.strip_suffix(".tgd").unwrap_or(path);
    stem.split_once('.').unwrap_or((stem, "places"))
}

pub(crate) fn remove(root: &Path, pack: &str, out: &mut dyn Write) -> Result<(), Fail> {
    let mut s = Opened::open(root)?;
    let path = path_of(pack);
    let (a, c) = split(&path);
    for r in s.store.remove(a, c)? {
        writeln!(out, "removed {} {}", r.key(), r.release)?;
    }
    Ok(())
}

pub(crate) fn pin(root: &Path, pack: &str, on: bool, out: &mut dyn Write) -> Result<(), Fail> {
    let mut s = Opened::open(root)?;
    let path = path_of(pack);
    let (a, c) = split(&path);
    s.store.pin(a, c, on)?;
    writeln!(out, "{path} {}", if on { "pinned" } else { "unpinned" })?;
    Ok(())
}

pub(crate) fn repair(root: &Path, pack: &str, out: &mut dyn Write) -> Result<(), Fail> {
    let mut s = Opened::open(root)?;
    let path = path_of(pack);
    let (a, c) = split(&path);
    match s.store.verify(a, c, true)? {
        Health::Good => {
            writeln!(out, "{path} is whole")?;
            return Ok(());
        }
        h => writeln!(out, "{path}: {h:?}; repairing")?,
    }
    let release = s
        .store
        .packs()
        .iter()
        .find(|r| r.key() == path)
        .map(|r| r.release.clone())
        .unwrap_or_default();
    let t = match s.offline() {
        Ok(t) if target(&t, &path).is_ok_and(|(e, _)| e.release == release) => t,
        _ => s.refresh(&[])?.trusted,
    };
    let (entry, asset_len) = target(&t, &path)?;
    let mut mirrors = Mirrors::new(s.assets.clone());
    let done = s.store.repair(
        &mut Ureq::new(),
        &mut mirrors,
        &entry.file,
        &entry,
        asset_len,
    )?;
    writeln!(out, "{path} repaired: {} bytes fetched", done.fetched)?;
    Ok(())
}

pub(crate) fn status(
    root: &Path,
    deep: bool,
    as_json: bool,
    out: &mut dyn Write,
) -> Result<(), Fail> {
    let mut s = Opened::open(root)?;
    let t = s.offline().ok();
    let packs: Vec<_> = s.store.packs().to_vec();
    let mut rows = Vec::new();
    let mut bad = 0_usize;
    for r in &packs {
        let health = s.store.verify(&r.address, &r.component, deep)?;
        bad += usize::from(health != Health::Good);
        rows.push(json!({
            "path": r.key(),
            "release": r.release,
            "components": r.set.iter().map(Component::name).collect::<Vec<_>>(),
            "bytes": r.pack_len,
            "pinned": r.pinned,
            "asked": r.asked,
            "health": format!("{health:?}"),
        }));
    }
    let ts = t.as_ref().and_then(|t| t.timestamp());
    let catalog = json!({
        "timestamp": ts.map(|x| x.common.version),
        "expires": ts.map(|x| crate::pack::utc(u64::try_from(x.common.expires).unwrap_or(0))),
    });
    if as_json {
        serde_json::to_writer_pretty(&mut *out, &json!({"catalog": catalog, "packs": rows}))?;
        writeln!(out)?;
    } else {
        match ts {
            Some(x) => writeln!(
                out,
                "catalog: timestamp {}, expires {}",
                x.common.version,
                field(&catalog, "expires").as_str().unwrap_or_default()
            )?,
            None => writeln!(out, "catalog: not refreshed yet")?,
        }
        for r in &rows {
            writeln!(
                out,
                "{:<28} {:<12} {:>12} bytes  {}{}  {}",
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
            )?;
        }
    }
    if bad > 0 {
        return Err(Fail::Refused(format!("{bad} packs need `tgd repair`")));
    }
    Ok(())
}
