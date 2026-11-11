//! Postal codes (feature `postal`, D-200): [`Engine::postal_lookup`], [`Engine::postal_prefix`],
//! [`Engine::postal_codes_of`] and [`Engine::postal_at`], over each pack's `POSTAL` section
//! (D-090, D-105).
//!
//! A code answers from the first layer by precedence whose `POSTAL` holds it, with every place
//! it names there, each as the layer that wins its ID answers it. A place's codes come from
//! the first layer holding the place's ID whose `POSTAL` has records for it, so a part that
//! carries no codes falls through to the pack that does. A point's codes are those of the
//! deepest admin unit holding it ([`Engine::admin_at`]) that an **admin-aligned** scheme gives
//! codes to: only there does a unit's code cover the whole unit.

use core::ops::ControlFlow;

use teigeodata_format::Kind;
use teigeodata_format::postal::{CodeKind, Postal};
use teigeodata_format::search::fst::MAX_KEY;

use crate::layer::Layer;
use crate::{Engine, Error, LayerId, Place};

/// The bytes a [`PostalCode`] holds inline: its scheme's name and its code.
const INLINE: usize = 64;

/// A postal code and the place it names, from a postal query.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PostalCode {
    /// The place it names, as the layer that wins its ID answers it.
    pub place: Place,
    /// What the code stands for there: the place's area, a part of it, its post office.
    pub kind: CodeKind,
    /// The first valid day (days since 1970-01-01), its own or its scheme's.
    pub valid_from: Option<u32>,
    /// The last valid day, its own or its scheme's.
    pub valid_to: Option<u32>,
    /// The layer whose `POSTAL` gave the code.
    pub layer: LayerId,
    text: [u8; INLINE],
    scheme_len: u8,
    len: u8,
    replaced_by: Option<(u32, LayerId)>,
}

impl PostalCode {
    /// The scheme's name (`np`, `np-1991`, `gb`).
    #[must_use]
    pub fn scheme(&self) -> &str {
        self.text
            .get(..usize::from(self.scheme_len))
            .and_then(|b| core::str::from_utf8(b).ok())
            .unwrap_or_default()
    }

    /// The code in its canonical form (`44600`, `SW1A 1AA`).
    #[must_use]
    pub fn code(&self) -> &str {
        self.text
            .get(usize::from(self.scheme_len)..usize::from(self.len))
            .and_then(|b| core::str::from_utf8(b).ok())
            .unwrap_or_default()
    }

    /// Whether the code is valid on `day` (days since 1970-01-01).
    #[must_use]
    pub fn valid_on(&self, day: u32) -> bool {
        self.valid_from.is_none_or(|f| f <= day) && self.valid_to.is_none_or(|t| day <= t)
    }

    /// Whether a later code replaces this one: [`Engine::postal_replacement`] names it.
    #[must_use]
    pub const fn is_replaced(&self) -> bool {
        self.replaced_by.is_some()
    }
}

/// A malformed `POSTAL`: a key without its separator or longer than a [`PostalCode`] holds.
const fn malformed(problem: &'static str) -> Error {
    Error::Format(teigeodata_format::Error::Payload {
        kind: Kind::POSTAL.0,
        column: 0,
        problem,
    })
}

/// Key `ordinal`'s scheme and code, inline.
fn key_text(postal: &Postal<'_>, ordinal: u32) -> Result<([u8; INLINE], u8, u8), Error> {
    let mut buf = [0_u8; MAX_KEY];
    let (scheme, code) = postal
        .code(ordinal, &mut buf)
        .ok_or_else(|| malformed("a postal key has no scheme"))?;
    let (s, n) = (scheme.len(), scheme.len() + code.len());
    let mut text = [0_u8; INLINE];
    let too_long = || malformed("a postal key is longer than 64 bytes");
    let (s8, n8) = (
        u8::try_from(s).map_err(|_| too_long())?,
        u8::try_from(n).map_err(|_| too_long())?,
    );
    text.get_mut(..s)
        .ok_or_else(too_long)?
        .copy_from_slice(scheme.as_bytes());
    text.get_mut(s..n)
        .ok_or_else(too_long)?
        .copy_from_slice(code.as_bytes());
    Ok((text, s8, n8))
}

impl Engine {
    /// The installed `POSTAL` of layer `l`, checked; `None` for a pack without one or with
    /// one not installed (named by the query if no layer answers).
    fn postal_of(l: &Layer) -> Result<Option<&Postal<'_>>, Error> {
        if l.views().pack.section(Kind::POSTAL).is_none() || !l.views().installed(Kind::POSTAL) {
            return Ok(None);
        }
        l.ensure(Kind::POSTAL)?;
        l.ensure(Kind::PLACES)?;
        l.ensure(Kind::IDMAP)?;
        Ok(l.views().postal.as_ref())
    }

    /// Pushes the records of key `ordinal` of layer `i` into `out`.
    fn push_records(
        &self,
        i: usize,
        postal: &Postal<'_>,
        ordinal: u32,
        out: &mut Vec<PostalCode>,
    ) -> Result<(), Error> {
        let key = key_text(postal, ordinal)?;
        for r in postal.records_of(ordinal) {
            self.push_record(i, postal, key, r, out)?;
        }
        Ok(())
    }

    /// Pushes record `r` of layer `i`, of key `key`, into `out`, its place as the layer that
    /// wins its ID answers it.
    fn push_record(
        &self,
        i: usize,
        postal: &Postal<'_>,
        (text, scheme_len, len): ([u8; INLINE], u8, u8),
        r: usize,
        out: &mut Vec<PostalCode>,
    ) -> Result<(), Error> {
        let Some(l) = self.layers.get(i) else {
            return Ok(());
        };
        let record = postal
            .record(r)
            .ok_or_else(|| malformed("a postal record is out of range"))?;
        let Some(p) = self.place(i, record.place as usize)? else {
            return Ok(());
        };
        let Some(place) = self.winning(i, p)? else {
            return Ok(());
        };
        out.push(PostalCode {
            place,
            kind: record.kind,
            valid_from: record.valid_from,
            valid_to: record.valid_to,
            layer: l.id(),
            text,
            scheme_len,
            len,
            replaced_by: record.redirect.map(|o| (o, l.id())),
        });
        Ok(())
    }

    /// The places postal code `code` names, into `out`: in scheme `scheme` (`np`, `gb`) or,
    /// with `None`, in every scheme whose rule accepts the input. The code may be typed with or
    /// without separators, in any case its scheme folds, in full-width forms or another
    /// script's digits (`४४६००` finds `44600`); a form no template of the scheme accepts finds
    /// nothing. Each code answers from the first layer by precedence that holds it. Retired
    /// codes are returned too, with their validity: filter with [`PostalCode::valid_on`].
    ///
    /// # Errors
    /// [`Error::Integrity`] if a section it reads is corrupt; [`Error::MissingComponent`]
    /// (`postal`) if no installed layer holds the code and one has codes that were not
    /// installed.
    pub fn postal_lookup(
        &self,
        code: &str,
        scheme: Option<&str>,
        out: &mut Vec<PostalCode>,
    ) -> Result<(), Error> {
        out.clear();
        // The keys found so far, as text: a code held by two layers answers from the first.
        let mut seen: Vec<([u8; INLINE], u8)> = Vec::new();
        for (i, l) in self.layers.iter().enumerate() {
            let Some(postal) = Self::postal_of(l)? else {
                continue;
            };
            let mut ordinals = [0_u32; 8];
            let mut n = 0;
            postal.lookup(code, scheme, |o| {
                if let Some(slot) = ordinals.get_mut(n) {
                    *slot = o;
                    n += 1;
                    ControlFlow::Continue(())
                } else {
                    ControlFlow::Break(())
                }
            });
            for &o in ordinals.get(..n).unwrap_or_default() {
                let (text, _, len) = key_text(postal, o)?;
                if seen.iter().any(|&(t, l)| l == len && t == text) {
                    continue;
                }
                seen.push((text, len));
                self.push_records(i, postal, o, out)?;
            }
        }
        if out.is_empty() {
            self.name_missing_postal()?;
        }
        Ok(())
    }

    /// Up to `limit` codes that start with `input`, with their places, into `out`:
    /// autocomplete, in code order per scheme and layer. Each code answers from the first layer
    /// by precedence that holds it. An empty input names nothing.
    ///
    /// # Errors
    /// As [`postal_lookup`](Self::postal_lookup).
    pub fn postal_prefix(
        &self,
        input: &str,
        scheme: Option<&str>,
        limit: usize,
        out: &mut Vec<PostalCode>,
    ) -> Result<(), Error> {
        out.clear();
        let mut seen: Vec<([u8; INLINE], u8)> = Vec::new();
        for (i, l) in self.layers.iter().enumerate() {
            if seen.len() >= limit {
                break;
            }
            let Some(postal) = Self::postal_of(l)? else {
                continue;
            };
            let mut failed = None;
            postal.prefix(input, scheme, |o| {
                if seen.len() >= limit {
                    return ControlFlow::Break(());
                }
                let step = key_text(postal, o).and_then(|(text, _, len)| {
                    if seen.iter().any(|&(t, l)| l == len && t == text) {
                        return Ok(());
                    }
                    seen.push((text, len));
                    self.push_records(i, postal, o, out)
                });
                match step {
                    Ok(()) => ControlFlow::Continue(()),
                    Err(e) => {
                        failed = Some(e);
                        ControlFlow::Break(())
                    }
                }
            });
            if let Some(e) = failed {
                return Err(e);
            }
        }
        if out.is_empty() {
            self.name_missing_postal()?;
        }
        Ok(())
    }

    /// The postal codes of `place`, into `out`, in code order: from the first layer by
    /// precedence that holds its ID and has codes for it. An admin unit's area code and its
    /// parts (a Nepal local level's code and its wards), a settlement's post office.
    ///
    /// # Errors
    /// [`Error::Integrity`] if a section it reads is corrupt; [`Error::MissingComponent`]
    /// (`postal`) if no installed layer gives the place codes and one holding it has codes
    /// that were not installed.
    pub fn postal_codes_of(&self, place: &Place, out: &mut Vec<PostalCode>) -> Result<(), Error> {
        out.clear();
        let mut missing = None;
        for (i, l) in self.layers.iter().enumerate() {
            if !l.may_hold(place.id) {
                continue;
            }
            if l.views().pack.section(Kind::POSTAL).is_some() && !l.views().installed(Kind::POSTAL)
            {
                missing.get_or_insert(i);
                continue;
            }
            l.ensure(Kind::IDMAP)?;
            let Some(row) = l.views().ids.as_ref().and_then(|m| m.row(place.id)) else {
                continue;
            };
            let Some(postal) = Self::postal_of(l)? else {
                continue;
            };
            let Ok(row) = u32::try_from(row) else {
                continue;
            };
            let mut failed = None;
            postal.records_at(row, |r| {
                let step = postal
                    .key_of(r)
                    .ok_or_else(|| malformed("a postal record has no key"))
                    .and_then(|o| key_text(postal, o))
                    .and_then(|key| self.push_record(i, postal, key, r, out));
                match step {
                    Ok(()) => ControlFlow::Continue(()),
                    Err(e) => {
                        failed = Some(e);
                        ControlFlow::Break(())
                    }
                }
            });
            if let Some(e) = failed {
                return Err(e);
            }
            if !out.is_empty() {
                return Ok(());
            }
        }
        if let Some(i) = missing
            && let Some(l) = self.layers.get(i)
        {
            l.ensure(Kind::POSTAL)?;
        }
        Ok(())
    }

    /// The postal codes of the point `at`, into `out`: those of the deepest admin unit holding
    /// it ([`admin_at`](Self::admin_at)) that an admin-aligned scheme gives codes to, with its
    /// parts (`Part`: a ward's code; the point is in one of them, which the pack does not
    /// say). Returns that unit; `None` where no unit holding the point has such codes.
    ///
    /// # Errors
    /// As [`admin_at`](Self::admin_at) and [`postal_codes_of`](Self::postal_codes_of).
    #[cfg(feature = "admin-index")]
    pub fn postal_at(
        &self,
        at: teigeodata_core::Point<teigeodata_core::E7>,
        s: &mut crate::Scratch,
        out: &mut Vec<PostalCode>,
    ) -> Result<Option<Place>, Error> {
        out.clear();
        let mut units = Vec::new();
        self.admin_at(at, s, &mut units)?;
        let mut codes = Vec::new();
        for unit in units.iter().rev() {
            self.postal_codes_of(unit, &mut codes)?;
            codes.retain(|c| self.aligned(c));
            if !codes.is_empty() {
                out.append(&mut codes);
                return Ok(Some(*unit));
            }
        }
        Ok(None)
    }

    /// The code that replaces `code`, if any, as its layer holds it.
    ///
    /// # Errors
    /// As [`postal_lookup`](Self::postal_lookup).
    pub fn postal_replacement(
        &self,
        code: &PostalCode,
        out: &mut Vec<PostalCode>,
    ) -> Result<(), Error> {
        out.clear();
        let Some((ordinal, layer)) = code.replaced_by else {
            return Ok(());
        };
        let Some(i) = self.layers.iter().position(|l| l.id() == layer) else {
            return Ok(());
        };
        let Some(l) = self.layers.get(i) else {
            return Ok(());
        };
        let Some(postal) = Self::postal_of(l)? else {
            return Ok(());
        };
        self.push_records(i, postal, ordinal, out)
    }

    /// Whether `code`'s scheme is admin-aligned in the layer that gave it.
    #[cfg(feature = "admin-index")]
    fn aligned(&self, code: &PostalCode) -> bool {
        self.layers
            .iter()
            .find(|l| l.id() == code.layer)
            .and_then(|l| l.views().postal.as_ref())
            .and_then(|p| p.scheme(code.scheme()))
            .is_some_and(|s| s.admin_aligned())
    }

    /// Names the `postal` component of the first layer that has codes not installed.
    fn name_missing_postal(&self) -> Result<(), Error> {
        for l in &self.layers {
            if l.views().pack.section(Kind::POSTAL).is_some() && !l.views().installed(Kind::POSTAL)
            {
                l.ensure(Kind::POSTAL)?;
            }
        }
        Ok(())
    }
}
