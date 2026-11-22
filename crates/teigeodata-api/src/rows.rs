//! The store's results as blobs, and its arguments parsed: one set of rules for the native
//! [`crate::Store`] and the browser's store (`teigeodata-wasm`'s `store` variant, D-375),
//! which runs the same client core ([`teigeodata_sync::session`]) over the Origin Private File
//! System. Rust-only: the IDL reads `lib.rs`.

use teigeodata_blob::{Blob, Column};
use teigeodata_format::Part;
use teigeodata_format::component::{Component, Components};
use teigeodata_sync::catalog::Catalog;
use teigeodata_sync::meta::Refreshed;
use teigeodata_sync::policy::{ClientError, Done, PackState, Want, path_of};
use teigeodata_sync::session::{Repaired, Status};
use teigeodata_sync::store::{Health, Record};
use teigeodata_sync::trust::Trusted;

use crate::{Error, ErrorCode, Result};

impl From<ClientError> for Error {
    fn from(e: ClientError) -> Self {
        match e {
            ClientError::Network(m) => Self::new(ErrorCode::Network, m),
            ClientError::Refused(m) => Self::new(ErrorCode::Refused, m),
            ClientError::Cancelled => Self::new(ErrorCode::Cancelled, "cancelled"),
            other => Self::new(ErrorCode::Internal, other.to_string()),
        }
    }
}

pub(crate) fn blob(columns: &[(&str, Column<'_>)]) -> Result<Blob> {
    Blob::encode(columns).map_err(|e| Error::new(ErrorCode::Internal, e.to_string()))
}

/// The catalog paths of a comma-separated list of packs (`np`, `np.dem`, `np.places.tgd`).
pub fn paths(packs: &str) -> Vec<String> {
    packs
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(path_of)
        .collect()
}

/// What an install asks of each pack: `components` and `languages` comma-separated, and the
/// one outline part's deepest level and tolerance (as `InstallOptions`).
///
/// # Errors
///
/// [`ErrorCode::InvalidArgument`] for a name that is no component or no language tag, or an
/// outline level above 255.
pub fn want(
    components: &str,
    languages: &str,
    outline_level: Option<u32>,
    outline_tolerance_m: u32,
) -> Result<Want> {
    let invalid = |m: String| Error::new(ErrorCode::InvalidArgument, m);
    let list = |s: &str| -> Vec<String> {
        s.split(',')
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let mut named = Vec::new();
    for name in list(components) {
        named.push(
            Component::from_name(&name)
                .ok_or_else(|| invalid(format!("`{name}` is not a component")))?,
        );
    }
    let languages = if languages.trim().is_empty() {
        None
    } else {
        let mut keys = Vec::new();
        for tag in list(languages) {
            keys.push(
                Part::language(&tag)
                    .ok_or_else(|| invalid(format!("`{tag}` is not a language tag")))?,
            );
        }
        Some(keys)
    };
    let outline = match outline_level {
        None => None,
        Some(level) => Some((
            u8::try_from(level)
                .map_err(|_| invalid(format!("outline level {level} is above 255")))?,
            outline_tolerance_m,
        )),
    };
    Ok(Want {
        components: named,
        languages,
        outline,
    })
}

fn health(h: &Health) -> &'static str {
    h.name()
}

fn names(set: Components) -> String {
    set.iter()
        .map(Component::name)
        .collect::<Vec<_>>()
        .join(",")
}

/// The verified catalog against the store: one row per pack.
///
/// # Errors
///
/// [`ErrorCode::Internal`] if the blob cannot be encoded.
pub fn catalog(t: &Trusted, packs: &[Record]) -> Result<Blob> {
    let (cat, _) = Catalog::trusted(t);
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
    let state: Vec<&str> = rows
        .iter()
        .map(|e| PackState::of(e, packs).name())
        .collect();
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
///
/// # Errors
///
/// [`ErrorCode::Internal`] if the blob cannot be encoded.
pub fn status(st: &Status) -> Result<Blob> {
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
///
/// # Errors
///
/// [`ErrorCode::Internal`] if the blob cannot be encoded.
pub fn done(rows: &[Done]) -> Result<Blob> {
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

/// The packs removed.
///
/// # Errors
///
/// [`ErrorCode::Internal`] if the blob cannot be encoded.
pub fn removed(gone: &[Record]) -> Result<Blob> {
    let path: Vec<String> = gone.iter().map(Record::key).collect();
    let path: Vec<&str> = path.iter().map(String::as_str).collect();
    let release: Vec<&str> = gone.iter().map(|r| r.release.as_str()).collect();
    blob(&[
        ("path", Column::Utf8(&path)),
        ("release", Column::Dict(&release)),
    ])
}

/// A refresh: the catalog's timestamp version, and what was fetched.
///
/// # Errors
///
/// [`ErrorCode::Internal`] if the blob cannot be encoded.
pub fn refresh(r: &Refreshed) -> Result<Blob> {
    let version = r.trusted.timestamp().map_or(0, |t| t.common.version);
    let files = u64::try_from(r.fetched.len()).unwrap_or(u64::MAX);
    blob(&[
        ("timestamp", Column::U64(&[version])),
        ("files", Column::U64(&[files])),
        ("bytes", Column::U64(&[r.bytes])),
    ])
}

/// A repair: the pack's health before, and the bytes fetched (0 when it was whole).
///
/// # Errors
///
/// [`ErrorCode::Internal`] if the blob cannot be encoded.
pub fn repaired(path: &str, r: &Repaired) -> Result<Blob> {
    blob(&[
        ("path", Column::Utf8(&[path])),
        ("health", Column::Dict(&[health(&r.health)])),
        ("fetched", Column::U64(&[r.fetched.unwrap_or(0)])),
    ])
}
