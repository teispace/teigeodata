//! A pack opened from its head and some of its sections (D-370): for a reader that holds only
//! what it read, such as the browser store's (note §5b). Rust and WebAssembly only: it takes a
//! list of buffers, which the IDL does not carry, so it lives outside `lib.rs`.

use teigeodata_engine::Layer;
use teigeodata_format::component::{Component, Components, Parts, Selection};
use teigeodata_format::{Error as FormatError, PackHead, Part};

use crate::{Engine, Error, ErrorCode, OpenOptions, Result};

impl Engine {
    /// Adds a pack from its head and some of its sections, each in its own buffer
    /// ([`Layer::from_sections`]): `head` is the file's first bytes, at least its head frame;
    /// `file_len` the file's length; `sections` each section's file offset and bytes. Set
    /// `options.components` (and the parts) to what the sections hold. Returns its layer ID,
    /// as [`open_file`](Self::open_file).
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Corrupt`](crate::ErrorCode::Corrupt) if the head is not a valid pack head,
    /// a section is not one of its sections or (verified eagerly) does not match its hash;
    /// [`ErrorCode::InvalidArgument`](crate::ErrorCode::InvalidArgument) for an outline level
    /// above 255.
    pub fn open_sections(
        &self,
        head: &[u8],
        file_len: u64,
        sections: Vec<(u64, Box<[u8]>)>,
        options: &OpenOptions,
    ) -> Result<u32> {
        let layer = Layer::from_sections(head, file_len, sections, options.layer()?)?;
        Ok(self.add(layer))
    }

    /// Adds a pack the store installed as `want` (its journal record's components and parts)
    /// from its head and the sections [`installed_sections`](Self::installed_sections) names,
    /// as [`open_sections`](Self::open_sections) with `want` as what is installed.
    ///
    /// # Errors
    ///
    /// As [`open_sections`](Self::open_sections).
    pub fn open_installed(
        &self,
        head: &[u8],
        file_len: u64,
        sections: Vec<(u64, Box<[u8]>)>,
        want: Selection,
        options: &OpenOptions,
    ) -> Result<u32> {
        let mut o = options.layer()?;
        o.installed = Some(want.set.closed());
        o.installed_parts = want.parts;
        let layer = Layer::from_sections(head, file_len, sections, o)?;
        Ok(self.add(layer))
    }

    /// The sections (file offset, length) a pack of `file_len` bytes installed as `want` holds,
    /// from a `prefix` of its file: the ones the store wrote, by the store's own rule
    /// ([`PackHead::holds`]). `None` when `prefix` does not hold the whole head yet (read more:
    /// twice as much).
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Corrupt`] if the prefix is not a pack head.
    pub fn installed_sections(
        prefix: &[u8],
        file_len: u64,
        want: Selection,
    ) -> Result<Option<Vec<(u64, u64)>>> {
        let h = match PackHead::parse(prefix, file_len) {
            Ok(h) => h,
            Err(FormatError::Truncated) if (prefix.len() as u64) < file_len => return Ok(None),
            Err(e) => return Err(Error::new(ErrorCode::Corrupt, e.to_string())),
        };
        let holds = h.holds(want);
        Ok(Some(
            h.sections()
                .filter(|s| s.length > 0 && holds(s.kind, s.part))
                .map(|s| (s.offset, s.length))
                .collect(),
        ))
    }

    /// What the store installed of a pack, from its journal record's fields: `set` (component
    /// names), the language parts' keys (`parts.languages`; `None` for every one) and the
    /// outline part's key (`parts.outline`).
    ///
    /// # Errors
    ///
    /// [`ErrorCode::InvalidArgument`] for a name that is no component, or too many languages.
    pub fn store_selection(
        set: &[String],
        languages: Option<&[u32]>,
        outline: Option<u32>,
    ) -> Result<Selection> {
        let bad = |m: String| Error::new(ErrorCode::InvalidArgument, m);
        let mut components = Components::NONE;
        for name in set {
            let c = Component::from_name(name)
                .ok_or_else(|| bad(format!("`{name}` is not a component")))?;
            components = components.union(Components::of(&[c]));
        }
        let mut parts = Parts::ALL;
        if let Some(keys) = languages {
            let keys: Vec<Part> = keys.iter().copied().map(Part).collect();
            parts = parts
                .languages(&keys)
                .ok_or_else(|| bad(format!("{} languages is too many", keys.len())))?;
        }
        if let Some(k) = outline {
            parts = parts.outline(Part(k));
        }
        Ok(Selection {
            set: components,
            parts,
        })
    }
}
