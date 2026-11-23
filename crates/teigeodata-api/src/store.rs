//! The store's internals behind [`crate::Store`] and [`crate::Job`]: the job thread (D-274),
//! the blobs of its results, and the options' parsing
//! ([store API](https://github.com/teispace/teigeodata/blob/main/docs/02-research/notes/store-api.md)).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use teigeodata_blob::{Blob, Column};
use teigeodata_engine::{Layer, LayerOptions};
use teigeodata_format::component::Components;
use teigeodata_sync::catalog::Catalog;
use teigeodata_sync::client::{Client, Pending, Want};
use teigeodata_sync::fetch::{Downloads, Ureq};
use teigeodata_sync::watch::{Watch, Watched};

use crate::rows::blob;
pub(crate) use crate::rows::{done, paths, removed};
use crate::{Engine, Error, ErrorCode, InstallOptions, Result, rows};

/// The transport every store uses: HTTPS, watched by the job running.
pub(crate) type Net = Watched<Ureq>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic inside a job is caught and reported; the state it leaves is the store's own,
    // which recovers from its journal.
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What [`InstallOptions`] asks of each pack.
pub(crate) fn want(o: &InstallOptions) -> Result<Want> {
    rows::want(
        &o.components,
        &o.languages,
        o.outline_level,
        o.outline_tolerance_m,
    )
}

/// The verified catalog against the store: one row per pack.
pub(crate) fn catalog(c: &mut Client<Net>) -> Result<Blob> {
    let t = c.offline()?;
    rows::catalog(&t, &c.current())
}

/// Each installed pack and its health.
pub(crate) fn status(c: &mut Client<Net>, deep: bool) -> Result<Blob> {
    rows::status(&c.status(deep)?)
}

/// A refresh: the catalog's timestamp version, and what was fetched.
pub(crate) fn refresh(c: &mut Client<Net>, countries: &[String]) -> Result<Blob> {
    rows::refresh(&c.refresh(countries)?)
}

/// A repair: the pack's health before, and the bytes fetched (0 when it was whole).
pub(crate) fn repaired(c: &mut Client<Net>, path: &str) -> Result<Blob> {
    rows::repaired(path, &c.repair(path)?)
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

    /// Stores the result and ends the watch as one step under the result's lock: a caller that
    /// sees the job ended finds its result, and one that gets the result sees the job ended.
    fn finish(&self, r: Result<Blob>) {
        let mut slot = lock(&self.result);
        *slot = Some(r);
        self.watch.end();
        drop(slot);
        self.ended.notify_all();
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

/// The engine over the installed packs, each opened with its installed components and parts,
/// and each installed delta above its base (D-395).
pub(crate) fn engine(c: &Client<Net>) -> Result<Engine> {
    let engine = Engine::new();
    let packs = c.packs().iter().map(|r| (c.pack_path(r), r.selection()));
    let deltas = c.deltas().iter().map(|d| (c.delta_path(d), d.selection()));
    for (path, sel) in packs.chain(deltas) {
        let options = LayerOptions {
            installed: Some(sel.set),
            installed_parts: sel.parts,
            ..LayerOptions::default()
        };
        engine.add(Layer::open(&path?, options)?);
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
