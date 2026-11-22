//! `Engine.openSections`, written by hand: it takes a list of buffers, which the IDL does not
//! carry (D-370). For a reader that holds a pack's head and only the sections it read, such as
//! the browser store's (note §5b).

use js_sys::{Array, Float64Array, Uint8Array};
use wasm_bindgen::prelude::*;

use crate::{Engine, error, field, invalid, open_options_of};

#[wasm_bindgen]
impl Engine {
    /// Adds a pack from its head and some of its sections, each in its own buffer: `head` is
    /// the file's first bytes, at least its head frame; `fileLen` the file's length; `offsets`
    /// each section's offset in the file and `sections` its bytes, in the same order. Set
    /// `options.components` (and `languages`) to what the sections hold. Returns its layer ID.
    ///
    /// # Errors
    ///
    /// `Corrupt` if the head is not a valid pack head or a section is not one of its sections
    /// (or, verified eagerly, does not match its hash); `InvalidArgument` for mismatched lists.
    #[wasm_bindgen(js_name = openSections)]
    pub fn open_sections(
        &self,
        head: &[u8],
        #[wasm_bindgen(js_name = fileLen)] file_len: f64,
        offsets: &Float64Array,
        #[wasm_bindgen(unchecked_param_type = "Uint8Array[]")] sections: &Array,
        #[wasm_bindgen(js_name = options, unchecked_optional_param_type = "OpenOptions")]
        options: Option<JsValue>,
    ) -> Result<u32, JsValue> {
        let file_len = whole(file_len).ok_or_else(|| invalid("fileLen must be a whole number"))?;
        let list = pairs(offsets, sections)?;
        let options = open_options_of(&options.unwrap_or(JsValue::UNDEFINED))?;
        self.0
            .open_sections(head, file_len, list, &options)
            .map_err(error)
    }
}

#[wasm_bindgen]
impl Engine {
    /// Adds a pack the browser store installed, as its journal `record` says (`set`, `parts`,
    /// `pack_len`), from its head and the sections [`installedSections`] named, read from its
    /// file: lazy by section, the rest never enters memory.
    ///
    /// # Errors
    ///
    /// As `openSections`; `InvalidArgument` for a record that is not a journal record.
    #[wasm_bindgen(js_name = openInstalled)]
    pub fn open_installed(
        &self,
        head: &[u8],
        #[wasm_bindgen(unchecked_param_type = "JournalRecord")] record: &JsValue,
        offsets: &Float64Array,
        #[wasm_bindgen(unchecked_param_type = "Uint8Array[]")] sections: &Array,
        #[wasm_bindgen(js_name = options, unchecked_optional_param_type = "OpenOptions")]
        options: Option<JsValue>,
    ) -> Result<u32, JsValue> {
        let r = Record::of(record)?;
        let want = r.selection()?;
        let list = pairs(offsets, sections)?;
        let options = open_options_of(&options.unwrap_or(JsValue::UNDEFINED))?;
        self.0
            .open_installed(head, r.len, list, want, &options)
            .map_err(error)
    }
}

/// The sections a pack the browser store installed holds, as its journal `record` says, from
/// a `prefix` of its file: `[offset, length, offset, length, ...]`, or `undefined` when the
/// prefix does not hold the whole head yet (read twice as much).
///
/// # Errors
///
/// `Corrupt` if the prefix is not a pack head; `InvalidArgument` for a bad record.
#[wasm_bindgen(js_name = installedSections)]
pub fn installed_sections(
    prefix: &[u8],
    #[wasm_bindgen(unchecked_param_type = "JournalRecord")] record: &JsValue,
) -> Result<Option<Float64Array>, JsValue> {
    let r = Record::of(record)?;
    let ranges =
        teigeodata_api::Engine::installed_sections(prefix, r.len, r.selection()?).map_err(error)?;
    #[allow(
        clippy::cast_precision_loss,
        reason = "offsets and lengths of a pack stay below 2^53"
    )]
    Ok(ranges.map(|r| {
        let flat: Vec<f64> = r.iter().flat_map(|&(o, l)| [o as f64, l as f64]).collect();
        Float64Array::from(flat.as_slice())
    }))
}

/// The fields of a journal record a reader needs.
struct Record {
    set: Vec<String>,
    languages: Option<Vec<u32>>,
    outline: Option<u32>,
    len: u64,
}

impl Record {
    fn of(record: &JsValue) -> Result<Self, JsValue> {
        let bad = || invalid("not a journal record");
        let set: Vec<String> = Array::from(&field(record, "set")?.ok_or_else(bad)?)
            .iter()
            .map(|v| v.as_string().ok_or_else(bad))
            .collect::<Result<_, _>>()?;
        let len = field(record, "pack_len")?
            .and_then(|v| v.as_f64())
            .and_then(whole)
            .ok_or_else(bad)?;
        let key = |v: JsValue| {
            v.as_f64()
                .and_then(whole)
                .and_then(|k| u32::try_from(k).ok())
                .ok_or_else(bad)
        };
        let (mut languages, mut outline) = (None, None);
        if let Some(p) = field(record, "parts")? {
            if let Some(l) = field(&p, "languages")? {
                languages = Some(Array::from(&l).iter().map(key).collect::<Result<_, _>>()?);
            }
            outline = field(&p, "outline")?.map(key).transpose()?;
        }
        Ok(Self {
            set,
            languages,
            outline,
            len,
        })
    }

    fn selection(&self) -> Result<teigeodata_api::StoreSelection, JsValue> {
        teigeodata_api::Engine::store_selection(&self.set, self.languages.as_deref(), self.outline)
            .map_err(error)
    }
}

/// Sections held apart: each one's file offset and bytes.
type Sections = Vec<(u64, Box<[u8]>)>;

/// Offsets and buffers as the engine takes them.
fn pairs(offsets: &Float64Array, sections: &Array) -> Result<Sections, JsValue> {
    let offsets = offsets.to_vec();
    if offsets.len() != sections.length() as usize {
        return Err(invalid("offsets and sections must have the same length"));
    }
    offsets
        .into_iter()
        .zip(sections.iter())
        .map(|(at, s)| {
            let at = whole(at).ok_or_else(|| invalid("an offset must be a whole number"))?;
            let bytes = s
                .dyn_into::<Uint8Array>()
                .map_err(|_| invalid("each section must be a Uint8Array"))?;
            Ok((at, bytes.to_vec().into_boxed_slice()))
        })
        .collect()
}

#[wasm_bindgen(typescript_custom_section)]
const JOURNAL_RECORD: &str = r#"
/** A pack as the store's journal records it: the fields a reader of the store uses. */
export interface JournalRecord {
  /** The components installed. */
  set: string[];
  /** The pack file's length. */
  pack_len: number;
  /** The parts installed, when not every one: language parts' keys and the outline part's. */
  parts?: { languages?: number[]; outline?: number };
  /** The file's name under `packs/`. */
  file?: string;
}
"#;

/// A JavaScript number as a whole `u64` below 2^53.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "checked whole, non-negative and below 2^53 first"
)]
fn whole(n: f64) -> Option<u64> {
    (n.fract() == 0.0 && (0.0..9_007_199_254_740_992.0).contains(&n)).then_some(n as u64)
}
