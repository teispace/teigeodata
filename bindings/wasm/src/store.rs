//! The browser's store (D-375), written by hand: the store's client core
//! (`teigeodata_sync::session`, the one the command line runs) over a host object that holds
//! the files (a worker's Origin Private File System) and the network (`fetch`). Results are
//! the native store's blobs (`teigeodata_api::rows`). Built in the `store` variant only.
//!
//! The host's file calls may return promises; an open file is synchronous, the shape of a
//! `FileSystemSyncAccessHandle`. One call runs at a time: a second call while one runs throws
//! `Refused` (a store worker queues its messages).

use std::cell::RefCell;
use std::future::Future;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::ops::Range;
use std::rc::Rc;

use js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
use teigeodata_api::rows;
use teigeodata_sync::fetch::{AsyncTransport, FetchError, Got, content_range};
use teigeodata_sync::policy::ClientError;
use teigeodata_sync::session::{self, Session};
use teigeodata_sync::store::{AsyncFs, StoreFile, render_journal};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, future_to_promise};

use crate::{bool_field, error, invalid, str_field, thrown, u32_field};

#[wasm_bindgen(typescript_custom_section)]
const STORE_TYPES: &str = r#"
/** An open file of the store's host: the shape of a `FileSystemSyncAccessHandle`. */
export interface StoreFileHandle {
  read(buffer: Uint8Array, options: { at: number }): number;
  write(buffer: Uint8Array, options: { at: number }): number;
  truncate(length: number): void;
  getSize(): number;
  flush(): void;
  close(): void;
}

/** A ranged response: status 206, its `Content-Range` and `ETag` headers, and its body. */
export interface StoreRangeResponse {
  status: number;
  body: Uint8Array;
  contentRange?: string | null;
  etag?: string | null;
}

/**
 * The files and the network of a browser store. Paths are relative to the store's root and
 * use `/`. Each call may return a promise.
 */
export interface StoreHost {
  /** Opens a file for reading and writing, creating it if missing, never truncating it. */
  open(path: string): StoreFileHandle | Promise<StoreFileHandle>;
  /** A whole file, or `null` if it does not exist. */
  read(path: string): Uint8Array | null | Promise<Uint8Array | null>;
  /** A file's length, or `null` if it does not exist. */
  len(path: string): number | null | Promise<number | null>;
  /** Moves `from` over `to`, replacing it. */
  rename(from: string, to: string): void | Promise<void>;
  /** Removes a file (a missing one is not an error); throws while a reader holds it. */
  remove(path: string): void | Promise<void>;
  /** Creates a directory if missing. */
  createDir(path: string): void | Promise<void>;
  /** Makes a directory's entries durable (`""` is the root); OPFS does this already. */
  syncDir(path: string): void | Promise<void>;
  /** The names of the files in a directory; none for one that does not exist. */
  list(path: string): string[] | Promise<string[]>;
  /** A ranged GET of bytes `start` to `end` (exclusive), with `Accept-Encoding: identity`. */
  get(url: string, start: number, end: number): Promise<StoreRangeResponse>;
  /** A whole GET of at most `max` bytes (a metadata file): its status and body. */
  fetch(url: string, max: number): Promise<{ status: number; body: Uint8Array }>;
}

/** What `Store.install` takes of each pack, as the native store's `InstallOptions`. */
export interface StoreInstallOptions {
  /** Components, comma-separated (`point,names,search`); empty for every one. */
  components?: string;
  /** Languages besides the base names, comma-separated BCP 47 tags; empty for every one. */
  languages?: string;
  /** The deepest admin level of the one outline part installed. */
  outlineLevel?: number;
  /** The tolerance in metres of that outline part; 0 is full resolution. */
  outlineToleranceM?: number;
  /** Keeps the packs when room is made. */
  pin?: boolean;
}
"#;

/// The message of a thrown JavaScript value.
fn message(e: &JsValue) -> String {
    e.dyn_ref::<js_sys::Error>()
        .map(|e| String::from(e.message()))
        .or_else(|| e.as_string())
        .unwrap_or_else(|| format!("{e:?}"))
}

fn io_error(e: &JsValue) -> io::Error {
    io::Error::other(message(e))
}

/// `target[name]`, a function.
fn method(target: &JsValue, name: &str) -> Result<Function, JsValue> {
    Reflect::get(target, &name.into())?
        .dyn_into::<Function>()
        .map_err(|_| invalid(&format!("the store host has no method `{name}`")))
}

/// `target[name](...args)` at once.
fn call_now(target: &JsValue, name: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
    let args: Array = args.iter().collect();
    method(target, name)?.apply(target, &args)
}

/// `target[name](...args)`, awaited when it returns a promise.
async fn call(target: &JsValue, name: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
    let r = call_now(target, name, args)?;
    match r.dyn_into::<Promise>() {
        Ok(p) => JsFuture::from(p).await,
        Err(v) => Ok(v),
    }
}

/// A JavaScript number as a byte offset or length.
fn whole(v: &JsValue) -> Option<u64> {
    let n = v.as_f64()?;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "checked whole and within 2^53 first"
    )]
    (n >= 0.0 && n.fract() == 0.0 && n <= 9_007_199_254_740_991.0).then_some(n as u64)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "offsets and lengths are below 2^53"
)]
fn num(n: u64) -> JsValue {
    JsValue::from_f64(n as f64)
}

/// `{ at }` for a file handle's read or write.
fn at(pos: u64) -> JsValue {
    let o = Object::new();
    // Setting a property on a fresh object cannot fail.
    let _ = Reflect::set(&o, &"at".into(), &num(pos));
    o.into()
}

/// An open file of the host (a sync access handle), closed when dropped.
struct HostFile {
    handle: JsValue,
    pos: u64,
}

impl HostFile {
    fn size(&self) -> io::Result<u64> {
        let n = call_now(&self.handle, "getSize", &[]).map_err(|e| io_error(&e))?;
        whole(&n).ok_or_else(|| io::Error::other("getSize did not return a length"))
    }
}

impl Read for HostFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        let a = Uint8Array::new_with_length(len);
        let n = call_now(&self.handle, "read", &[a.clone().into(), at(self.pos)])
            .map_err(|e| io_error(&e))?;
        let n = whole(&n)
            .and_then(|n| u32::try_from(n).ok())
            .filter(|&n| n <= len)
            .ok_or_else(|| io::Error::other("read did not return a length"))?;
        let got = n as usize;
        let dst = buf
            .get_mut(..got)
            .ok_or_else(|| io::Error::other("read returned more than asked"))?;
        a.subarray(0, n).copy_to(dst);
        self.pos += u64::from(n);
        Ok(got)
    }
}

impl Write for HostFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let a = Uint8Array::from(buf);
        let n =
            call_now(&self.handle, "write", &[a.into(), at(self.pos)]).map_err(|e| io_error(&e))?;
        let n = whole(&n)
            .and_then(|n| usize::try_from(n).ok())
            .filter(|&n| n <= buf.len())
            .ok_or_else(|| io::Error::other("write did not return a length"))?;
        self.pos += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Seek for HostFile {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let bad = || io::Error::new(io::ErrorKind::InvalidInput, "a seek before the start");
        self.pos = match to {
            SeekFrom::Start(p) => p,
            SeekFrom::End(o) => self.size()?.checked_add_signed(o).ok_or_else(bad)?,
            SeekFrom::Current(o) => self.pos.checked_add_signed(o).ok_or_else(bad)?,
        };
        Ok(self.pos)
    }
}

impl StoreFile for HostFile {
    fn set_len(&mut self, len: u64) -> io::Result<()> {
        call_now(&self.handle, "truncate", &[num(len)]).map_err(|e| io_error(&e))?;
        Ok(())
    }

    fn sync(&mut self) -> io::Result<()> {
        call_now(&self.handle, "flush", &[]).map_err(|e| io_error(&e))?;
        Ok(())
    }
}

impl Drop for HostFile {
    fn drop(&mut self) {
        // A sync access handle locks its file until closed; a failed close leaves it to GC.
        let _ = call_now(&self.handle, "close", &[]);
    }
}

/// The host's files.
struct HostFs(JsValue);

impl HostFs {
    async fn unit(&self, name: &str, args: &[JsValue]) -> io::Result<()> {
        call(&self.0, name, args).await.map_err(|e| io_error(&e))?;
        Ok(())
    }
}

impl AsyncFs for HostFs {
    type File = HostFile;

    async fn open(&mut self, path: &str) -> io::Result<HostFile> {
        let handle = call(&self.0, "open", &[path.into()])
            .await
            .map_err(|e| io_error(&e))?;
        Ok(HostFile { handle, pos: 0 })
    }

    async fn read(&mut self, path: &str) -> io::Result<Option<Vec<u8>>> {
        let v = call(&self.0, "read", &[path.into()])
            .await
            .map_err(|e| io_error(&e))?;
        if v.is_null() || v.is_undefined() {
            return Ok(None);
        }
        let a = v
            .dyn_into::<Uint8Array>()
            .map_err(|_| io::Error::other("read did not return a Uint8Array"))?;
        Ok(Some(a.to_vec()))
    }

    async fn len(&mut self, path: &str) -> io::Result<Option<u64>> {
        let v = call(&self.0, "len", &[path.into()])
            .await
            .map_err(|e| io_error(&e))?;
        if v.is_null() || v.is_undefined() {
            return Ok(None);
        }
        whole(&v)
            .map(Some)
            .ok_or_else(|| io::Error::other("len did not return a length"))
    }

    async fn rename(&mut self, from: &str, to: &str) -> io::Result<()> {
        self.unit("rename", &[from.into(), to.into()]).await
    }

    async fn remove(&mut self, path: &str) -> io::Result<()> {
        self.unit("remove", &[path.into()]).await
    }

    async fn create_dir(&mut self, path: &str) -> io::Result<()> {
        self.unit("createDir", &[path.into()]).await
    }

    async fn sync_dir(&mut self, path: &str) -> io::Result<()> {
        self.unit("syncDir", &[path.into()]).await
    }

    async fn list(&mut self, path: &str) -> io::Result<Vec<String>> {
        let v = call(&self.0, "list", &[path.into()])
            .await
            .map_err(|e| io_error(&e))?;
        let a = v
            .dyn_into::<Array>()
            .map_err(|_| io::Error::other("list did not return an array"))?;
        a.iter()
            .map(|n| {
                n.as_string()
                    .ok_or_else(|| io::Error::other("list returned a name that is no string"))
            })
            .collect()
    }
}

/// The host's network.
struct HostNet(JsValue);

fn transport(e: &JsValue) -> FetchError {
    FetchError::Transport(message(e))
}

/// A response's `status` and `body`.
fn response(v: &JsValue) -> Result<(u16, Uint8Array), FetchError> {
    let bad = FetchError::Response("the host's response has no status and body");
    let status = Reflect::get(v, &"status".into())
        .ok()
        .and_then(|s| whole(&s))
        .and_then(|s| u16::try_from(s).ok())
        .ok_or(FetchError::Response("the host's response has no status"))?;
    let body = Reflect::get(v, &"body".into())
        .ok()
        .and_then(|b| b.dyn_into::<Uint8Array>().ok())
        .ok_or(bad)?;
    Ok((status, body))
}

fn header(v: &JsValue, name: &str) -> Option<String> {
    Reflect::get(v, &name.into())
        .ok()
        .and_then(|h| h.as_string())
}

impl AsyncTransport for HostNet {
    async fn get(
        &mut self,
        url: &str,
        range: Range<u64>,
        out: &mut Vec<u8>,
    ) -> Result<Got, FetchError> {
        out.clear();
        if range.is_empty() {
            return Err(FetchError::Response("an empty range"));
        }
        let v = call(
            &self.0,
            "get",
            &[url.into(), num(range.start), num(range.end)],
        )
        .await
        .map_err(|e| transport(&e))?;
        let (status, body) = response(&v)?;
        if status != 206 {
            return Err(FetchError::Status(status));
        }
        let total = content_range(header(&v, "contentRange").as_deref(), &range)?;
        if u64::from(body.length()) != range.end - range.start {
            return Err(FetchError::Response("the body is not the range's length"));
        }
        *out = body.to_vec();
        Ok(Got {
            etag: header(&v, "etag"),
            total,
        })
    }

    async fn fetch(&mut self, url: &str, max: u64, out: &mut Vec<u8>) -> Result<(), FetchError> {
        out.clear();
        let v = call(&self.0, "fetch", &[url.into(), num(max)])
            .await
            .map_err(|e| transport(&e))?;
        let (status, body) = response(&v)?;
        if status != 200 {
            return Err(FetchError::Status(status));
        }
        if u64::from(body.length()) > max {
            return Err(FetchError::Response("longer than allowed"));
        }
        *out = body.to_vec();
        Ok(())
    }
}

type Core = Session<HostFs, HostNet>;

/// Unix seconds, from the host's clock.
#[allow(
    clippy::cast_possible_truncation,
    reason = "milliseconds since 1970 fit an i64 in seconds"
)]
fn now() -> i64 {
    (js_sys::Date::now() / 1000.0) as i64
}

fn client(e: ClientError) -> JsValue {
    error(e.into())
}

/// A comma-separated list of country codes or packs, as the native store takes them.
fn countries(list: &str) -> Vec<String> {
    list.split(',')
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A store of installed packs in a browser: the native store's client (journal, recovery,
/// signed catalog, resumable installs) over a host's files and network (`StoreHost`; the
/// package's store worker gives it the Origin Private File System and `fetch`). Packs are
/// named as the native store names them (`np`, `np.dem`, `np.places.tgd`, comma-separated),
/// and results are its blobs.
#[wasm_bindgen]
pub struct Store {
    core: Rc<RefCell<Option<Core>>>,
    damaged: String,
}

impl core::fmt::Debug for Store {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Store").finish_non_exhaustive()
    }
}

impl Store {
    /// Runs `work` on the store's core and gives the core back, as a promise of its result.
    fn run<F, Fut>(&self, work: F) -> Promise
    where
        F: FnOnce(Core) -> Fut + 'static,
        Fut: Future<Output = (Core, Result<JsValue, JsValue>)> + 'static,
    {
        let Some(core) = self.core.borrow_mut().take() else {
            return Promise::reject(&thrown(
                "Refused",
                "the store is busy: one call runs at a time",
            ));
        };
        let slot = Rc::clone(&self.core);
        future_to_promise(async move {
            let (core, r) = work(core).await;
            *slot.borrow_mut() = Some(core);
            r
        })
    }
}

/// A blob's bytes, as the engine's queries return them.
fn bytes(b: Result<teigeodata_api::Blob, teigeodata_api::Error>) -> Result<JsValue, JsValue> {
    let b = b.map_err(error)?;
    Ok(Uint8Array::from(b.into_vec().as_slice()).into())
}

#[wasm_bindgen]
impl Store {
    /// Sets up a store in `host`: its metadata mirrors `repo` and asset mirrors `assets`, and
    /// the trusted root (`root.json`) that becomes its trust anchor once it verifies by itself.
    ///
    /// # Errors
    ///
    /// `Refused` if the root does not verify or the files cannot be written.
    pub async fn init(
        #[wasm_bindgen(unchecked_param_type = "StoreHost")] host: JsValue,
        repo: Vec<String>,
        assets: Vec<String>,
        #[wasm_bindgen(js_name = trustedRoot)] trusted_root: Vec<u8>,
    ) -> Result<(), JsValue> {
        session::init(&mut HostFs(host), &repo, &assets, &trusted_root, now())
            .await
            .map_err(client)
    }

    /// Opens the store in `host`, finishing or undoing an install a crash cut short.
    ///
    /// # Errors
    ///
    /// `Refused` if it is not set up or its journal cannot be read.
    pub async fn open(
        #[wasm_bindgen(unchecked_param_type = "StoreHost")] host: JsValue,
    ) -> Result<Store, JsValue> {
        let net = HostNet(host.clone());
        let (core, damaged) = Session::open(HostFs(host), net).await.map_err(client)?;
        Ok(Self {
            core: Rc::new(RefCell::new(Some(core))),
            damaged: damaged.join(","),
        })
    }

    /// The packs found damaged when the store opened, comma-separated: each needs `repair`.
    pub fn damaged(&self) -> String {
        self.damaged.clone()
    }

    /// The installed packs as the journal lists them (`JournalRecord`s, for a reader's
    /// `installedSections` and `Engine.openInstalled`).
    ///
    /// # Errors
    ///
    /// `Refused` while another call runs.
    #[wasm_bindgen(unchecked_return_type = "JournalRecord[]")]
    pub fn packs(&self) -> Result<JsValue, JsValue> {
        let core = self.core.borrow();
        let core = core
            .as_ref()
            .ok_or_else(|| thrown("Refused", "the store is busy: one call runs at a time"))?;
        let text = String::from_utf8_lossy(&render_journal(core.packs())).into_owned();
        let journal = js_sys::JSON::parse(&text)?;
        Reflect::get(&journal, &"packs".into())
    }

    /// The verified catalog against the store, offline: one row per pack (`path`, `release`,
    /// `download`, `disk`, `components`, `licence`, `state`).
    #[wasm_bindgen(unchecked_return_type = "Promise<Uint8Array>")]
    pub fn catalog(&self) -> Promise {
        self.run(|mut c| async move {
            let r = match c.offline(now()).await {
                Ok(t) => bytes(rows::catalog(&t, c.packs())),
                Err(e) => Err(client(e)),
            };
            (c, r)
        })
    }

    /// Each installed pack and its health, from a full check when `deep`.
    #[wasm_bindgen(unchecked_return_type = "Promise<Uint8Array>")]
    pub fn status(&self, deep: bool) -> Promise {
        self.run(move |mut c| async move {
            let r = match c.status(deep, now()).await {
                Ok(s) => bytes(rows::status(&s)),
                Err(e) => Err(client(e)),
            };
            (c, r)
        })
    }

    /// Refreshes the catalog from the metadata mirrors, with the roles of `countries`
    /// (comma-separated codes) and of every country installed.
    #[wasm_bindgen(unchecked_return_type = "Promise<Uint8Array>")]
    pub fn refresh(&self, countries_list: &str) -> Promise {
        let want = countries(countries_list);
        self.run(move |mut c| async move {
            let r = match c.refresh(&want, now()).await {
                Ok(x) => bytes(rows::refresh(&x)),
                Err(e) => Err(client(e)),
            };
            (c, r)
        })
    }

    /// Installs `packs` with what `options` asks added to what is installed, dependencies
    /// first: one row per pack reached.
    #[wasm_bindgen(unchecked_return_type = "Promise<Uint8Array>")]
    pub fn install(
        &self,
        packs: &str,
        #[wasm_bindgen(unchecked_optional_param_type = "StoreInstallOptions")] options: Option<
            JsValue,
        >,
    ) -> Promise {
        let o = options.unwrap_or(JsValue::UNDEFINED);
        let parsed = (|| -> Result<_, JsValue> {
            let want = rows::want(
                &str_field(&o, "components")?.unwrap_or_default(),
                &str_field(&o, "languages")?.unwrap_or_default(),
                u32_field(&o, "outlineLevel")?,
                u32_field(&o, "outlineToleranceM")?.unwrap_or_default(),
            )
            .map_err(error)?;
            Ok((want, bool_field(&o, "pin")?.unwrap_or_default()))
        })();
        let (want, pin) = match parsed {
            Ok(p) => p,
            Err(e) => return Promise::reject(&e),
        };
        let paths = rows::paths(packs);
        self.run(move |mut c| async move {
            let r = match c.install(&paths, &want, pin, now()).await {
                Ok(report) => bytes(rows::done(&report.done)),
                Err(e) => Err(client(e)),
            };
            (c, r)
        })
    }

    /// Updates `packs` (every installed one when empty) to the catalog's releases, keeping
    /// what is installed of each.
    #[wasm_bindgen(unchecked_return_type = "Promise<Uint8Array>")]
    pub fn update(&self, packs: &str) -> Promise {
        let paths = rows::paths(packs);
        self.run(move |mut c| async move {
            let r = match c.update(&paths, now()).await {
                Ok(done) => bytes(rows::done(&done)),
                Err(e) => Err(client(e)),
            };
            (c, r)
        })
    }

    /// Removes `pack` and the dependencies nothing else needs: the packs removed.
    #[wasm_bindgen(unchecked_return_type = "Promise<Uint8Array>")]
    pub fn remove(&self, pack: &str) -> Promise {
        let paths = rows::paths(pack);
        self.run(move |mut c| async move {
            let mut gone = Vec::new();
            for p in &paths {
                match c.remove(p).await {
                    Ok(g) => gone.extend(g),
                    Err(e) => return (c, Err(client(e))),
                }
            }
            (c, bytes(rows::removed(&gone)))
        })
    }

    /// Pins `pack` (kept when room is made), or unpins it.
    #[wasm_bindgen(unchecked_return_type = "Promise<void>")]
    pub fn pin(&self, pack: &str, on: bool) -> Promise {
        let paths = rows::paths(pack);
        self.run(move |mut c| async move {
            for p in &paths {
                if let Err(e) = c.pin(p, on).await {
                    return (c, Err(client(e)));
                }
            }
            (c, Ok(JsValue::UNDEFINED))
        })
    }

    /// Checks `pack` in full and, unless it is whole, fetches what it lacks.
    #[wasm_bindgen(unchecked_return_type = "Promise<Uint8Array>")]
    pub fn repair(&self, pack: &str) -> Promise {
        let path = rows::paths(pack).into_iter().next().unwrap_or_default();
        self.run(move |mut c| async move {
            let r = match c.repair(&path, now()).await {
                Ok(x) => bytes(rows::repaired(&path, &x)),
                Err(e) => Err(client(e)),
            };
            (c, r)
        })
    }
}

/// The journal's pack list (`JournalRecord`s) from its bytes, for a reader beside the store
/// worker: what `Store.packs` returns, read from the `journal` file.
///
/// # Errors
///
/// `Corrupt` if the bytes are not a journal.
#[wasm_bindgen(js_name = parseJournal, unchecked_return_type = "JournalRecord[]")]
pub fn parse_journal(bytes: &[u8]) -> Result<JsValue, JsValue> {
    let packs = teigeodata_sync::store::parse_journal(bytes)
        .map_err(|e| thrown("Corrupt", &format!("the journal: {e}")))?;
    let text = String::from_utf8_lossy(&render_journal(&packs)).into_owned();
    let journal = js_sys::JSON::parse(&text)?;
    Reflect::get(&journal, &"packs".into())
}
