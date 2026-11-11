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
use teigeodata_format::search::{Search, substring_from_names};
use teigeodata_format::{Kind, Section};

/// The substring keys of one name text: its fold's word suffixes (D-199).
fn keys(text: &str, out: &mut dyn FnMut(&str)) {
    teigeodata_names::substring_keys(&teigeodata_names::fold(text), out);
}

/// The SUBSTRING section of a pack whose NAMES and SEARCH are `inputs` (kind, item count,
/// bytes; other kinds are ignored).
///
/// # Errors
/// A message when NAMES or SEARCH is missing or malformed, or the section does not encode.
pub fn substring(inputs: &[(Kind, u64, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let get = |k: Kind| {
        inputs
            .iter()
            .find(|i| i.0 == k)
            .ok_or_else(|| format!("no {k} section"))
    };
    let (n, s) = (get(Kind::NAMES)?, get(Kind::SEARCH)?);
    let names =
        Names::open(&Section::unpacked(Kind::NAMES, n.1, &n.2)).map_err(|e| e.to_string())?;
    let search =
        Search::open(&Section::unpacked(Kind::SEARCH, s.1, &s.2)).map_err(|e| e.to_string())?;
    let importance: Vec<u8> = (0..names.len())
        .map(|r| u32::try_from(r).map(|r| search.importance(r)))
        .collect::<Result<_, _>>()
        .map_err(|_| "too many rows".to_owned())?;
    substring_from_names(&names, &importance, &keys).map_err(|e| e.to_string())
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
