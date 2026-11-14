//! The store's internals behind [`crate::Store`] and [`crate::Job`]: the job thread (D-274),
//! the blobs of its results, and the options' parsing
//! ([store API](https://github.com/teispace/teigeodata/blob/main/docs/02-research/notes/store-api.md)).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use teigeodata_blob::{Blob, Column};
use teigeodata_engine::{Layer, LayerOptions};
use teigeodata_format::Part;
use teigeodata_format::component::{Component, Components};
use teigeodata_sync::catalog::Catalog;
use teigeodata_sync::client::{self, Client, ClientError, Done, Pending, Want};
use teigeodata_sync::fetch::{Downloads, Ureq};
use teigeodata_sync::store::{Health, Record};
use teigeodata_sync::watch::{Watch, Watched};

use crate::{Engine, Error, ErrorCode, InstallOptions, Result};

/// The transport every store uses: HTTPS, watched by the job running.
pub(crate) type Net = Watched<Ureq>;

impl From<ClientError> for Error {
    fn from(e: ClientError) -> Self {
        match e {
            ClientError::Network(m) => Self::new(ErrorCode::Network, m),
            ClientError::Refused(m) => Self::new(ErrorCode::Refused, m),
            ClientError::Cancelled => Self::new(ErrorCode::Cancelled, "cancelled"),
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic inside a job is caught and reported; the state it leaves is the store's own,
    // which recovers from its journal.
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn blob(columns: &[(&str, Column<'_>)]) -> Result<Blob> {
    Blob::encode(columns).map_err(|e| Error::new(ErrorCode::Internal, e.to_string()))
}

/// A store's shared state: one client, so its jobs run one at a time.
pub(crate) struct Inner {
    pub(crate) client: Mutex<Client<Net>>,
    pub(crate) damaged: String,
}

impl Inner {
    pub(crate) fn open(root: &str) -> Result<Self> {
        let (client, damaged) = Client::open_with(root.as_ref(), Watched::new(Ureq::new()))?;
        Ok(Self {
            client: Mutex::new(client),
            damaged: damaged.join(","),
        })
    }

    pub(crate) fn client(&self) -> MutexGuard<'_, Client<Net>> {
        lock(&self.client)
    }
}

/// A job's shared state: its progress, and its result once it ends.
pub(crate) struct JobInner {
    pub(crate) watch: Arc<Watch>,
    result: Mutex<Option<Result<Blob>>>,
    ended: Condvar,
}

impl JobInner {
    /// The result, once the job has ended.
    pub(crate) fn result(&self) -> Result<Blob> {
        let mut r = lock(&self.result);
        loop {
            if let Some(r) = r.as_ref() {
                return r.clone();
            }
            r = self.ended.wait(r).unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn finish(&self, r: Result<Blob>) {
        *lock(&self.result) = Some(r);
        self.ended.notify_all();
        // After the result: a caller that sees the job ended finds its result.
        self.watch.end();
    }
}

/// Runs `work` on a thread of its own with the store's client, watched (D-274).
pub(crate) fn spawn(
    inner: Arc<Inner>,
    work: impl FnOnce(&mut Client<Net>) -> Result<Blob> + Send + 'static,
) -> Arc<JobInner> {
    let job = Arc::new(JobInner {
        watch: Arc::new(Watch::new()),
        result: Mutex::new(None),
        ended: Condvar::new(),
    });
    let ours = job.clone();
    let spawned = std::thread::Builder::new()
        .name("teigeodata-job".into())
        .spawn(move || {
            let r = catch_unwind(AssertUnwindSafe(|| {
                let mut c = inner.client();
                c.net().watch(Some(ours.watch.clone()));
                let r = work(&mut c);
                c.net().watch(None);
                r
            }))
            .unwrap_or_else(|_| {
                Err(Error::new(
                    ErrorCode::Internal,
                    "a panic inside a teigeodata job was stopped",
                ))
            });
            ours.finish(r);
        });
    if let Err(e) = spawned {
        job.finish(Err(Error::new(
            ErrorCode::Internal,
            format!("no thread for the job: {e}"),
        )));
    }
    job
}

/// The catalog paths of a comma-separated list of packs (`np`, `np.dem`, `np.places.tgd`).
pub(crate) fn paths(packs: &str) -> Vec<String> {
    packs
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(client::path_of)
        .collect()
}

/// What [`InstallOptions`] asks of each pack.
pub(crate) fn want(o: &InstallOptions) -> Result<Want> {
    let invalid = |m: String| Error::new(ErrorCode::InvalidArgument, m);
    let list = |s: &str| -> Vec<String> {
        s.split(',')
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let mut components = Vec::new();
    for name in list(&o.components) {
        components.push(
            Component::from_name(&name)
                .ok_or_else(|| invalid(format!("`{name}` is not a component")))?,
        );
    }
    let languages = if o.languages.trim().is_empty() {
        None
    } else {
        let mut keys = Vec::new();
        for tag in list(&o.languages) {
            keys.push(
                Part::language(&tag)
                    .ok_or_else(|| invalid(format!("`{tag}` is not a language tag")))?,
            );
        }
        Some(keys)
    };
    let outline = match o.outline_level {
        None => None,
        Some(level) => Some((
            u8::try_from(level)
                .map_err(|_| invalid(format!("outline level {level} is above 255")))?,
            o.outline_tolerance_m,
        )),
    };
    Ok(Want {
        components,
        languages,
        outline,
    })
}

fn health(h: &Health) -> &'static str {
    match h {
        Health::Good => "good",
        Health::Length => "length",
        Health::Head => "head",
        Health::Footer => "footer",
        Health::Sections(_) => "sections",
    }
}

fn names(set: Components) -> String {
    set.iter()
        .map(Component::name)
        .collect::<Vec<_>>()
        .join(",")
}

/// The verified catalog against the store: one row per pack.
pub(crate) fn catalog(c: &mut Client<Net>) -> Result<Blob> {
    let t = c.offline()?;
    let (cat, _) = Catalog::trusted(&t);
    let rows: Vec<_> = cat.entries().map(|(_, e)| e).collect();
    let path: Vec<String> = rows.iter().map(|e| e.path()).collect();
    let path: Vec<&str> = path.iter().map(String::as_str).collect();
    let release: Vec<&str> = rows.iter().map(|e| e.release.as_str()).collect();
    let all: Vec<_> = rows.iter().map(|e| e.size_of(Components::all())).collect();
    let download: Vec<u64> = all.iter().map(|s| s.wire).collect();
    let disk: Vec<u64> = all.iter().map(|s| s.disk).collect();
    let components: Vec<String> = rows
        .iter()
        .map(|e| {
            e.sizes
                .keys()
                .map(|c| c.name())
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect();
    let components: Vec<&str> = components.iter().map(String::as_str).collect();
    let licence: Vec<&str> = rows.iter().map(|e| e.licence.as_str()).collect();
    let state: Vec<&str> = rows.iter().map(|e| c.state(e).name()).collect();
    blob(&[
        ("path", Column::Utf8(&path)),
        ("release", Column::Dict(&release)),
        ("download", Column::U64(&download)),
        ("disk", Column::U64(&disk)),
        ("components", Column::Dict(&components)),
        ("licence", Column::Dict(&licence)),
        ("state", Column::Dict(&state)),
    ])
}

/// Each installed pack and its health.
pub(crate) fn status(c: &mut Client<Net>, deep: bool) -> Result<Blob> {
    let st = c.status(deep)?;
    let path: Vec<String> = st.packs.iter().map(|(r, _)| r.key()).collect();
    let path: Vec<&str> = path.iter().map(String::as_str).collect();
    let release: Vec<&str> = st.packs.iter().map(|(r, _)| r.release.as_str()).collect();
    let bytes: Vec<u64> = st.packs.iter().map(|(r, _)| r.pack_len).collect();
    let pinned: Vec<u8> = st.packs.iter().map(|(r, _)| u8::from(r.pinned)).collect();
    let asked: Vec<u8> = st.packs.iter().map(|(r, _)| u8::from(r.asked)).collect();
    let health: Vec<&str> = st.packs.iter().map(|(_, h)| health(h)).collect();
    let components: Vec<String> = st.packs.iter().map(|(r, _)| names(r.set)).collect();
    let components: Vec<&str> = components.iter().map(String::as_str).collect();
    blob(&[
        ("path", Column::Utf8(&path)),
        ("release", Column::Dict(&release)),
        ("bytes", Column::U64(&bytes)),
        ("pinned", Column::U8(&pinned)),
        ("asked", Column::U8(&asked)),
        ("health", Column::Dict(&health)),
        ("components", Column::Dict(&components)),
    ])
}

/// The packs an install or update reached, dependencies first.
pub(crate) fn done(rows: &[Done]) -> Result<Blob> {
    let path: Vec<&str> = rows.iter().map(|d| d.path.as_str()).collect();
    let release: Vec<&str> = rows.iter().map(|d| d.release.as_str()).collect();
    let n = |f: fn(&teigeodata_sync::install::Installed) -> u64| -> Vec<u64> {
        rows.iter()
            .map(|d| d.installed.as_ref().map_or(0, f))
            .collect()
    };
    let (fetched, reused, kept) = (n(|i| i.fetched), n(|i| i.reused), n(|i| i.kept));
    let changed: Vec<u8> = rows
        .iter()
        .map(|d| u8::from(d.installed.is_some()))
        .collect();
    blob(&[
        ("path", Column::Utf8(&path)),
        ("release", Column::Dict(&release)),
        ("fetched", Column::U64(&fetched)),
        ("reused", Column::U64(&reused)),
        ("kept", Column::U64(&kept)),
        ("changed", Column::U8(&changed)),
    ])
}

/// What a platform still has to download after a pass (D-184): one row per missing range of
/// an asset, with the file to save it as. No rows: the pass installed every pack.
pub(crate) fn pending(packs: &[Pending]) -> Result<Blob> {
    let rows = packs.iter().map(|p| p.ranges.len()).sum();
    let urls: Vec<String> = packs.iter().map(|p| p.urls.join("\n")).collect();
    let (mut pack, mut url, mut length) = (
        Vec::with_capacity(rows),
        Vec::with_capacity(rows),
        Vec::with_capacity(rows),
    );
    let (mut start, mut end, mut file) = (
        Vec::with_capacity(rows),
        Vec::with_capacity(rows),
        Vec::with_capacity(rows),
    );
    for (p, u) in packs.iter().zip(&urls) {
        for r in &p.ranges {
            pack.push(p.pack.as_str());
            url.push(u.as_str());
            length.push(p.length);
            start.push(r.start);
            end.push(r.end);
            file.push(
                p.dir
                    .join(Downloads::name(r))
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    let file: Vec<&str> = file.iter().map(String::as_str).collect();
    blob(&[
        ("pack", Column::Dict(&pack)),
        ("urls", Column::Dict(&url)),
        ("length", Column::U64(&length)),
        ("start", Column::U64(&start)),
        ("end", Column::U64(&end)),
        ("file", Column::Utf8(&file)),
    ])
}

/// The packs removed.
pub(crate) fn removed(gone: &[Record]) -> Result<Blob> {
    let path: Vec<String> = gone.iter().map(Record::key).collect();
    let path: Vec<&str> = path.iter().map(String::as_str).collect();
    let release: Vec<&str> = gone.iter().map(|r| r.release.as_str()).collect();
    blob(&[
        ("path", Column::Utf8(&path)),
        ("release", Column::Dict(&release)),
    ])
}

/// A refresh: the catalog's timestamp version, and what was fetched.
pub(crate) fn refresh(c: &mut Client<Net>, countries: &[String]) -> Result<Blob> {
    let r = c.refresh(countries)?;
    let version = r.trusted.timestamp().map_or(0, |t| t.common.version);
    let files = u64::try_from(r.fetched.len()).unwrap_or(u64::MAX);
    blob(&[
        ("timestamp", Column::U64(&[version])),
        ("files", Column::U64(&[files])),
        ("bytes", Column::U64(&[r.bytes])),
    ])
}

/// A repair: the pack's health before, and the bytes fetched (0 when it was whole).
pub(crate) fn repaired(c: &mut Client<Net>, path: &str) -> Result<Blob> {
    let r = c.repair(path)?;
    blob(&[
        ("path", Column::Utf8(&[path])),
        ("health", Column::Dict(&[health(&r.health)])),
        ("fetched", Column::U64(&[r.fetched.unwrap_or(0)])),
    ])
}

/// The engine over the installed packs, each opened with its installed components and parts.
pub(crate) fn engine(c: &Client<Net>) -> Result<Engine> {
    let engine = Engine::new();
    for r in c.packs() {
        let path = c.pack_path(r)?;
        let options = LayerOptions {
            installed: Some(r.set),
            installed_parts: r.parts,
            ..LayerOptions::default()
        };
        engine.add(Layer::open(&path, options)?);
    }
    Ok(engine)
}

/// A place and the catalog's packs that cover it.
struct Covered<'a> {
    id: Option<teigeodata_core::PlaceId>,
    name: Option<String>,
    lat: f64,
    lon: f64,
    countries: String,
    packs: Vec<&'a teigeodata_sync::catalog::Entry>,
}

fn country_codes(codes: &[[u8; 2]]) -> String {
    codes
        .iter()
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect::<Vec<_>>()
        .join(",")
}

/// One row per place and pack; a place no pack covers has one row with the pack's columns
/// null.
fn covered(c: &Client<Net>, places: &[Covered<'_>]) -> Result<Blob> {
    let mut rows: Vec<(&Covered<'_>, Option<&teigeodata_sync::catalog::Entry>)> = Vec::new();
    for p in places {
        if p.packs.is_empty() {
            rows.push((p, None));
        }
        rows.extend(p.packs.iter().map(|&e| (p, Some(e))));
    }
    let ids: Vec<String> = rows
        .iter()
        .map(|(p, _)| p.id.map(|i| i.to_string()).unwrap_or_default())
        .collect();
    let id: Vec<&str> = ids.iter().map(String::as_str).collect();
    let has_id: Vec<bool> = rows.iter().map(|(p, _)| p.id.is_some()).collect();
    let name: Vec<&str> = rows
        .iter()
        .map(|(p, _)| p.name.as_deref().unwrap_or_default())
        .collect();
    let has_name: Vec<bool> = rows.iter().map(|(p, _)| p.name.is_some()).collect();
    let lat: Vec<f64> = rows.iter().map(|(p, _)| p.lat).collect();
    let lon: Vec<f64> = rows.iter().map(|(p, _)| p.lon).collect();
    let countries: Vec<&str> = rows.iter().map(|(p, _)| p.countries.as_str()).collect();
    let paths: Vec<String> = rows
        .iter()
        .map(|(_, e)| {
            e.map(teigeodata_sync::catalog::Entry::path)
                .unwrap_or_default()
        })
        .collect();
    let pack: Vec<&str> = paths.iter().map(String::as_str).collect();
    let has_pack: Vec<bool> = rows.iter().map(|(_, e)| e.is_some()).collect();
    let release: Vec<&str> = rows
        .iter()
        .map(|(_, e)| e.map_or("", |e| e.release.as_str()))
        .collect();
    let size: Vec<(u64, u64)> = rows
        .iter()
        .map(|(_, e)| {
            e.map_or((0, 0), |e| {
                let s = e.size_of(Components::all());
                (s.wire, s.disk)
            })
        })
        .collect();
    let download: Vec<u64> = size.iter().map(|s| s.0).collect();
    let disk: Vec<u64> = size.iter().map(|s| s.1).collect();
    let state: Vec<&str> = rows
        .iter()
        .map(|(_, e)| e.map_or("", |e| c.state(e).name()))
        .collect();
    let (id, name) = (Column::Utf8(&id), Column::Utf8(&name));
    let (pack, release) = (Column::Utf8(&pack), Column::Dict(&release));
    let (download, disk, state) = (
        Column::U64(&download),
        Column::U64(&disk),
        Column::Dict(&state),
    );
    blob(&[
        ("id", Column::Nullable(&id, &has_id)),
        ("name", Column::Nullable(&name, &has_name)),
        ("lat", Column::F64(&lat)),
        ("lon", Column::F64(&lon)),
        ("countries", Column::Dict(&countries)),
        ("pack", Column::Nullable(&pack, &has_pack)),
        ("release", Column::Nullable(&release, &has_pack)),
        ("download", Column::Nullable(&download, &has_pack)),
        ("disk", Column::Nullable(&disk, &has_pack)),
        ("state", Column::Nullable(&state, &has_pack)),
    ])
}

fn name_of(
    engine: &teigeodata_engine::Engine,
    place: &teigeodata_engine::Place,
    buf: &mut [u8],
) -> Result<Option<String>> {
    Ok(engine.name(place, buf)?.map(str::to_owned))
}

/// The places named `query` in `engine`, with the packs that cover each.
pub(crate) fn find(
    c: &mut Client<Net>,
    engine: &teigeodata_engine::Engine,
    query: &str,
    limit: usize,
) -> Result<Blob> {
    let t = c.offline()?;
    let (cat, _) = Catalog::trusted(&t);
    let found = cat.find(engine, query, limit)?;
    let mut buf = [0_u8; 1024];
    let mut places = Vec::with_capacity(found.len());
    for f in found {
        let id = teigeodata_core::PlaceId::from_raw(f.candidate.id);
        let name = match id.map(|id| engine.get(id)).transpose()?.flatten() {
            Some(place) => name_of(engine, &place, &mut buf)?,
            None => None,
        };
        places.push(Covered {
            id,
            name,
            lat: f.candidate.point.lat_degrees(),
            lon: f.candidate.point.lon_degrees(),
            countries: country_codes(&f.candidate.countries),
            packs: f.packs,
        });
    }
    covered(c, &places)
}

/// The countries within `radius_m` of `at` in `engine`, with the packs that cover `at`.
pub(crate) fn locate(
    c: &mut Client<Net>,
    engine: &teigeodata_engine::Engine,
    at: teigeodata_core::Point<teigeodata_core::E7>,
    radius_m: u32,
) -> Result<Blob> {
    let t = c.offline()?;
    let (cat, _) = Catalog::trusted(&t);
    let mut countries = Vec::new();
    engine.countries_near(at, radius_m, &mut countries)?;
    let mut buf = [0_u8; 1024];
    let mut places = Vec::with_capacity(countries.len());
    for country in &countries {
        let Some(cc) = country.id.namespace().country() else {
            continue;
        };
        places.push(Covered {
            id: Some(country.id),
            name: name_of(engine, country, &mut buf)?,
            lat: country.point.lat_degrees(),
            lon: country.point.lon_degrees(),
            countries: country_codes(&[cc]),
            packs: cat.packs_at(cc, at).collect(),
        });
    }
    covered(c, &places)
}
