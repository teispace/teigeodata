//! Zone rules (D-196, TG-03): the UTC offset in force in a timezone at an instant, the
//! offsets a civil time may have there, and the tzdb release, from the packs' `TZ_RULES`
//! ([ADR-0039]). The rules are read in place: no query allocates or parses text.
//!
//! [ADR-0039]: https://github.com/teispace/teigeodata/blob/main/docs/03-decisions/adr/ADR-0039-tz-rules.md

#[cfg(feature = "timezone-index")]
use teigeodata_core::{E7, Point};
use teigeodata_format::Kind;
use teigeodata_format::Tag;
use teigeodata_format::tz_rules::{Civil, Local, LocalType, Zone};

use crate::{Engine, Error, LayerId, Place};

/// The longest abbreviation a pack may store (RFC 9636's limit, as the format checks it).
const ABBR_MAX: usize = 16;

/// The UTC offset in force in a timezone at an instant, as its rules state it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Offset {
    /// Seconds east of UTC (Kathmandu: 20,700, +05:45).
    pub seconds: i32,
    /// Whether tzdb counts this as daylight saving time (its main form: Dublin's winter is).
    pub is_dst: bool,
    /// The layer whose rules answered.
    pub layer: LayerId,
    abbr: [u8; ABBR_MAX],
    abbr_len: u8,
}

impl Offset {
    fn of(t: &LocalType<'_>, layer: LayerId) -> Self {
        let mut abbr = [0; ABBR_MAX];
        let n = t.abbr.len().min(ABBR_MAX);
        if let (Some(to), Some(from)) = (abbr.get_mut(..n), t.abbr.as_bytes().get(..n)) {
            to.copy_from_slice(from);
        }
        Self {
            seconds: t.offset,
            is_dst: t.is_dst,
            layer,
            abbr,
            abbr_len: u8::try_from(n).unwrap_or(0),
        }
    }

    /// tzdb's abbreviation (`+0545`, `EST`); not a localised name.
    #[must_use]
    pub fn abbr(&self) -> &str {
        self.abbr
            .get(..usize::from(self.abbr_len))
            .and_then(|b| core::str::from_utf8(b).ok())
            .unwrap_or_default()
    }
}

impl Engine {
    /// Zone `zone` (`Asia/Kathmandu`, a tzdb name or link) from the first layer by precedence
    /// whose rules hold it, and that layer.
    fn rules_zone<T>(
        &self,
        zone: &str,
        answer: impl Fn(&Zone<'_>, LayerId) -> Result<T, Error>,
    ) -> Result<Option<T>, Error> {
        for l in &self.layers {
            let Some(rules) = &l.views().tz_rules else {
                continue;
            };
            let Some(z) = rules.zone(zone) else {
                continue;
            };
            l.ensure(Kind::TZ_RULES)?;
            return answer(&z, l.id).map(Some);
        }
        // A layer with rules it was not installed with: name the component.
        for l in &self.layers {
            if !l.views().installed(Kind::TZ_RULES) {
                l.ensure(Kind::TZ_RULES)?;
            }
        }
        Ok(None)
    }

    /// The UTC offset in force in `zone` (a tzdb name or link, `Asia/Kathmandu`) at
    /// `unix_seconds`, from the first layer by precedence whose rules hold the zone (D-196):
    /// its stored transitions, then its POSIX TZ footer for later instants. `None` where no
    /// layer holds the zone. Instants from year −9,999 to 9,999 answer.
    ///
    /// # Errors
    /// [`Error::MissingComponent`] (`tz-rules`) if no installed layer holds the zone and one
    /// has rules that were not installed; [`Error::Integrity`] if the rules are corrupt;
    /// [`Error::Format`] for an instant outside the years ±9,999.
    pub fn tz_offset_at(&self, zone: &str, unix_seconds: i64) -> Result<Option<Offset>, Error> {
        self.rules_zone(zone, |z, layer| {
            Ok(Offset::of(&z.offset_at(unix_seconds)?, layer))
        })
    }

    /// The offsets civil time `civil` may have in `zone` (D-196): [`Local::Unique`],
    /// [`Local::Gap`] (the time is skipped: the offsets before and after) or
    /// [`Local::Overlap`] (it happens twice: the earlier and the later instant's offsets), as
    /// `jiff`'s `AmbiguousOffset` and `java.time`'s `getValidOffsets` answer. Offset `o` is
    /// valid when the offset at `civil − o` is `o`. `None` where no layer holds the zone.
    ///
    /// # Errors
    /// As [`tz_offset_at`](Self::tz_offset_at); [`Error::Format`] for a civil time that is
    /// not a date of the years ±9,999.
    pub fn tz_local_candidates(&self, zone: &str, civil: Civil) -> Result<Option<Local>, Error> {
        self.rules_zone(zone, |z, _| Ok(z.local(civil)?))
    }

    /// The tzdb release (`2026f`) of the first layer by precedence that has zone rules, from
    /// its manifest's `tzdb` tag; `None` without rules.
    #[must_use]
    pub fn tz_version(&self) -> Option<&str> {
        self.layers.iter().find_map(|l| {
            l.views().tz_rules.as_ref()?;
            l.views().pack.manifest().text(Tag::TZDB)
        })
    }

    /// The timezone area holding `at` ([`tz_at`](Self::tz_at)) and the offset in force there
    /// at `unix_seconds`: the area's zone as its own layer's rules link it, answered by the
    /// first layer by precedence that holds that zone. `None` where no area holds the point
    /// or its layer has no rules for it.
    ///
    /// # Errors
    /// As [`tz_at`](Self::tz_at) and [`tz_offset_at`](Self::tz_offset_at).
    #[cfg(feature = "timezone-index")]
    pub fn tz_offset_at_point(
        &self,
        at: Point<E7>,
        unix_seconds: i64,
    ) -> Result<Option<(Place, Offset)>, Error> {
        let Some(area) = self.tz_at(at)? else {
            return Ok(None);
        };
        let Some(name) = self.zone_name(&area)? else {
            return Ok(None);
        };
        Ok(self
            .tz_offset_at(name, unix_seconds)?
            .map(|offset| (area, offset)))
    }

    /// The tzdb name of timezone area `area`, as its layer's rules link it to its row.
    ///
    /// # Errors
    /// [`Error::MissingComponent`] if the layer's rules were not installed;
    /// [`Error::Integrity`] if they are corrupt.
    pub fn zone_name(&self, area: &Place) -> Result<Option<&str>, Error> {
        let Some(l) = self.layer(area.layer) else {
            return Ok(None);
        };
        l.ensure(Kind::TZ_RULES)?;
        Ok(l.views()
            .tz_rules
            .as_ref()
            .and_then(|r| r.zone_of_place(area.row))
            .map(|z| z.name()))
    }
}
