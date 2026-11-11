//! ISO 3166 codes of countries and units (feature `admin`, D-197): [`Engine::iso_code`] and
//! [`Engine::by_iso`].
//!
//! A place's code comes from the first layer, in precedence order, whose pack has a row for its
//! ID with a code: a part that holds a province only as a stub carries the province's code too,
//! and a pack without `ISO_CODES` falls through to one with it.

use teigeodata_core::IsoCode;
use teigeodata_format::Kind;

use crate::{Engine, Error, Place};

impl Engine {
    /// The ISO 3166 code of `place`: the ISO 3166-1 alpha-2 code of a country (`NP`), the
    /// ISO 3166-2 code of a subdivision (`NP-P3`); `None` for a place without one. With
    /// [`ancestors`](Self::ancestors) or [`admin_at`](Self::admin_at), ask it of each entry:
    ///
    /// ```no_run
    /// # fn f(engine: &teigeodata_engine::Engine, place: &teigeodata_engine::Place) -> Result<(), teigeodata_engine::Error> {
    /// let mut chain = Vec::new();
    /// engine.ancestors(place, &mut chain)?;
    /// for p in &chain {
    ///     if let Some(code) = engine.iso_code(p)? {
    ///         println!("{}", code.as_str()); // "NP-P3", then "NP"
    ///     }
    /// }
    /// # Ok(()) }
    /// ```
    ///
    /// O(log rows) per layer that holds the ID; no allocation.
    ///
    /// # Errors
    /// [`Error::Integrity`] if a section it reads is corrupt; [`Error::MissingComponent`] if a
    /// layer holding the place has codes but its `admin` component is not installed.
    pub fn iso_code(&self, place: &Place) -> Result<Option<IsoCode>, Error> {
        for l in &self.layers {
            if !l.may_hold(place.id) {
                continue;
            }
            l.ensure(Kind::IDMAP)?;
            let v = l.views();
            let Some(row) = v.ids.as_ref().and_then(|m| m.row(place.id)) else {
                continue;
            };
            if v.pack.section(Kind::ISO_CODES).is_none() {
                continue;
            }
            l.ensure(Kind::ISO_CODES)?;
            if let Some(code) = v.iso.as_ref().and_then(|t| t.get(u32::try_from(row).ok()?)) {
                return Ok(Some(code));
            }
        }
        Ok(None)
    }

    /// The place whose ISO 3166 code is `code` (`NP`, `NP-P3`), as the winning layer answers
    /// for its ID; `None` if no layer has the code. A scan of each layer's codes in precedence
    /// order: a few thousand values for the world baseline.
    ///
    /// # Errors
    /// [`Error::Integrity`] if a section it reads is corrupt; [`Error::MissingComponent`] if a
    /// layer has codes but its `admin` component is not installed.
    pub fn by_iso(&self, code: IsoCode) -> Result<Option<Place>, Error> {
        for (i, l) in self.layers.iter().enumerate() {
            if l.views().pack.section(Kind::ISO_CODES).is_none() {
                continue;
            }
            l.ensure(Kind::ISO_CODES)?;
            let Some(row) = l.views().iso.as_ref().and_then(|t| t.row_of(code)) else {
                continue;
            };
            let Some(p) = self.place(i, row as usize)? else {
                continue;
            };
            // The winning layer's answer for the ID, if it still has this code there.
            if let Some(w) = self.winning(i, p)?
                && self.iso_code(&w)? == Some(code)
            {
                return Ok(Some(w));
            }
        }
        Ok(None)
    }
}
