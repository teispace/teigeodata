//! Indexes built on the device instead of downloaded (D-213, feature `derive`).
//!
//! A component that is a pure function of others a device already holds can be built there:
//! [`substring`] writes the SUBSTRING section from NAMES and SEARCH with the same builder the
//! forge uses (`teigeodata_format::search::substring_from_names`) and the same key rule
//! (`teigeodata_names::substring_keys` of the fold), so the result is the signed section byte
//! for byte. With the store, [`derive_installed`] adds it to an installed pack:
//! `teigeodata_sync::store::Store::derive` checks the bytes against the hash the pack's head
//! lists and installs them as a download would.
//!
//! About 0.4 s and a few MB for Nepal's 96,000 places on a laptop core (measured 2026-11-11).

use teigeodata_format::names::Names;
use teigeodata_format::search::{Search, substring_from_names, substring_part_from_names};
use teigeodata_format::{Kind, Part, Section};

/// The substring keys of one name text: its fold's word suffixes (D-199).
fn keys(text: &str, out: &mut dyn FnMut(&str)) {
    teigeodata_names::substring_keys(&teigeodata_names::fold(text), out);
}

/// The SUBSTRING sections of a pack whose NAMES and SEARCH are `inputs` (kind, part, item
/// count, bytes; other kinds are ignored): one part per NAMES part, in part order (ADR-0040).
/// Every part ranks by the base SEARCH part's importance.
///
/// # Errors
/// A message when the base NAMES or SEARCH is missing, a section is malformed, or a section
/// does not encode.
pub fn substring(inputs: &[(Kind, Part, u64, Vec<u8>)]) -> Result<Vec<(Part, Vec<u8>)>, String> {
    let err = |e: teigeodata_format::Error| e.to_string();
    let s = inputs
        .iter()
        .find(|i| i.0 == Kind::SEARCH && i.1.is_base())
        .ok_or("no SEARCH section")?;
    let search = Search::open(&Section::unpacked(Kind::SEARCH, s.2, &s.3)).map_err(err)?;
    let mut importance = Vec::new();
    let mut out = Vec::new();
    let mut names: Vec<_> = inputs.iter().filter(|i| i.0 == Kind::NAMES).collect();
    names.sort_by_key(|i| i.1);
    if names.first().is_none_or(|n| !n.1.is_base()) {
        return Err("no NAMES section".to_owned());
    }
    for n in names {
        let names = Names::open(&Section::unpacked(Kind::NAMES, n.2, &n.3)).map_err(err)?;
        if importance.is_empty() {
            importance = (0..names.len())
                .map(|r| u32::try_from(r).map(|r| search.importance(r)))
                .collect::<Result<_, _>>()
                .map_err(|_| "too many rows".to_owned())?;
        }
        let body = if n.1.is_base() {
            substring_from_names(&names, &importance, &keys)
        } else {
            substring_part_from_names(&names, &importance, &keys)
        };
        out.push((n.1, body.map_err(err)?));
    }
    Ok(out)
}

/// Adds SUBSTRING to the installed pack `address`/`component` of `store`, built here.
///
/// # Errors
/// The store's ([`teigeodata_sync::store::StoreError`]).
#[cfg(feature = "store")]
pub fn derive_installed<D: teigeodata_sync::store::Fs>(
    store: &mut teigeodata_sync::store::Store<D>,
    address: &str,
    component: &str,
) -> Result<(), teigeodata_sync::store::StoreError> {
    store.derive(
        address,
        component,
        teigeodata_format::component::Component::Substring,
        substring,
    )
}
