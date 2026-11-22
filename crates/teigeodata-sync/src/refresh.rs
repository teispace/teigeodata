//! The catalog refresh (D-178) for the native store: the core in [`crate::meta`] run over an
//! [`Fs`] and a [`Transport`], with the verified metadata kept in the store under `meta/`, so
//! every app and the CLI share one tested path ([the CLI note](https://github.com/teispace/teigeodata/blob/main/docs/02-research/notes/cli.md#5-what-we-build-ourselves-and-why)).
//!
//! Both are synchronous, so the core's futures are ready at once and [`meta::block_on`] runs
//! them (D-369). The walk, the layout and the commit order are [`crate::meta`]'s, the writes
//! [`meta::StoreIo`]'s, as in a browser.

use alloc::vec::Vec;
use core::ops::Range;

use crate::fetch::{FetchError, Got, Mirrors, Ready, Transport};
use crate::meta::{self, StoreIo};
pub use crate::meta::{RefreshError, Refreshed};
use crate::store::{Fs, SyncFs};
use crate::trust::Trusted;

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
    let (mut fs, mut net) = (SyncFs(fs), Ready(transport));
    let mut io = StoreIo {
        fs: &mut fs,
        net: &mut net,
    };
    meta::block_on(meta::refresh(&mut io, mirrors, shipped, now, want))
}

/// The stored metadata, with no network: see [`meta::load`].
///
/// # Errors
/// As [`meta::load`].
pub fn load<D: Fs>(fs: &mut D, shipped: &[u8], now: i64) -> Result<Trusted, RefreshError> {
    let (mut fs, mut none) = (SyncFs(fs), NoNetwork);
    let mut net = Ready(&mut none);
    let mut io = StoreIo {
        fs: &mut fs,
        net: &mut net,
    };
    meta::block_on(meta::load(&mut io, shipped, now))
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
