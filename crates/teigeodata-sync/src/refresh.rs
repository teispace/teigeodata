//! The catalog refresh (D-178) for the native store: the core in [`crate::meta`] run over an
//! [`Fs`] and a [`Transport`], with the verified metadata kept in the store under `meta/`, so
//! every app and the CLI share one tested path ([the CLI note](https://github.com/teispace/teigeodata/blob/main/docs/02-research/notes/cli.md#5-what-we-build-ourselves-and-why)).
//!
//! Both are synchronous, so the core's futures are ready at once and [`meta::block_on`] runs
//! them (D-369). The walk, the layout and the commit order are [`crate::meta`]'s. Each file is
//! written to `meta/tmp`, synced, renamed over its name and its directory synced.

use alloc::string::String;
use alloc::vec::Vec;
use core::future::{Future, ready};
use core::ops::Range;
use std::io::{self, Write as _};

use crate::fetch::{FetchError, Got, Mirrors, Transport};
use crate::meta::{self, MetaIo};
pub use crate::meta::{RefreshError, Refreshed};
use crate::store::{Fs, StoreFile};
use crate::trust::Trusted;

const TMP: &str = "meta/tmp";

/// Refreshes the catalog in the store `fs` from `mirrors` over `transport`: see
/// [`meta::refresh`].
///
/// # Errors
/// As [`meta::refresh`].
pub fn refresh<D: Fs, T: Transport>(
    fs: &mut D,
    transport: &mut T,
    mirrors: &mut Mirrors,
    shipped: &[u8],
    now: i64,
    want: &[&str],
) -> Result<Refreshed, RefreshError> {
    let mut io = SyncIo { fs, transport };
    meta::block_on(meta::refresh(&mut io, mirrors, shipped, now, want))
}

/// The stored metadata, with no network: see [`meta::load`].
///
/// # Errors
/// As [`meta::load`].
pub fn load<D: Fs>(fs: &mut D, shipped: &[u8], now: i64) -> Result<Trusted, RefreshError> {
    let mut io = SyncIo {
        fs,
        transport: &mut NoNetwork,
    };
    meta::block_on(meta::load(&mut io, shipped, now))
}

/// The native effects: an [`Fs`] and a [`Transport`], each future [`ready`] at once.
struct SyncIo<'a, D: Fs, T: Transport> {
    fs: &'a mut D,
    transport: &'a mut T,
}

impl<D: Fs, T: Transport> MetaIo for SyncIo<'_, D, T> {
    fn read(&mut self, path: &str) -> impl Future<Output = io::Result<Option<Vec<u8>>>> {
        ready(self.fs.read(path))
    }

    fn put(&mut self, path: &str, data: &[u8]) -> impl Future<Output = io::Result<()>> {
        ready(put(self.fs, path, data))
    }

    fn remove(&mut self, path: &str) -> impl Future<Output = io::Result<()>> {
        ready(self.fs.remove(path))
    }

    fn list(&mut self, dir: &str) -> impl Future<Output = io::Result<Vec<String>>> {
        ready(self.fs.list(dir))
    }

    fn create_dir(&mut self, path: &str) -> impl Future<Output = io::Result<()>> {
        ready(self.fs.create_dir(path))
    }

    fn sync_dir(&mut self, path: &str) -> impl Future<Output = io::Result<()>> {
        ready(self.fs.sync_dir(path))
    }

    fn fetch(
        &mut self,
        url: &str,
        max: u64,
        out: &mut Vec<u8>,
    ) -> impl Future<Output = Result<(), FetchError>> {
        ready(self.transport.fetch(url, max, out))
    }
}

/// `data` at `path`, atomically and durably.
fn put<D: Fs>(fs: &mut D, path: &str, data: &[u8]) -> io::Result<()> {
    fs.remove(TMP)?;
    let mut f = fs.open(TMP)?;
    f.set_len(0)?;
    f.write_all(data)?;
    f.sync()?;
    drop(f);
    fs.rename(TMP, path)?;
    let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
    fs.sync_dir(dir)
}

/// The transport of [`load`], which reads only the store.
struct NoNetwork;

impl Transport for NoNetwork {
    fn get(&mut self, _: &str, _: Range<u64>, _: &mut Vec<u8>) -> Result<Got, FetchError> {
        Err(FetchError::Response("offline"))
    }

    fn fetch(&mut self, _: &str, _: u64, _: &mut Vec<u8>) -> Result<(), FetchError> {
        Err(FetchError::Response("offline"))
    }
}
