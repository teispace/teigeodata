//! Places as one blob (D-275): the columns every place query returns, built without a
//! per-row allocation.

use teigeodata_blob::{Blob, Column};
use teigeodata_engine::{CodeKind, Engine, Error as EngineError, Place, PostalCode};

use crate::{Error, ErrorCode, Result};

/// Longest name decoded, in bytes (a name longer than this is cut by the engine's error).
const NAME: usize = 1024;

/// Strings of one column in one arena: no allocation per row.
#[derive(Default)]
struct Arena {
    text: String,
    ends: Vec<usize>,
    present: Vec<bool>,
}

impl Arena {
    fn push(&mut self, s: Option<&str>) {
        if let Some(s) = s {
            self.text.push_str(s);
        }
        self.ends.push(self.text.len());
        self.present.push(s.is_some());
    }

    fn push_fmt<T: core::fmt::Display>(&mut self, v: Option<&T>) {
        use core::fmt::Write as _;
        if let Some(v) = v {
            // Writing to a String cannot fail.
            let _ = write!(self.text, "{v}");
        }
        self.ends.push(self.text.len());
        self.present.push(v.is_some());
    }

    fn strs(&self) -> Vec<&str> {
        let mut start = 0;
        self.ends
            .iter()
            .map(|&end| {
                let s = self.text.get(start..end).unwrap_or_default();
                start = end;
                s
            })
            .collect()
    }
}

/// A component the layer was installed without reads as absent (D-133).
fn installed<T>(r: core::result::Result<Option<T>, EngineError>) -> Result<Option<T>> {
    match r {
        Err(EngineError::MissingComponent { .. }) => Ok(None),
        r => r.map_err(Error::from),
    }
}

/// The columns of a postal result, one row per code.
#[derive(Default)]
pub(crate) struct Postal {
    code: Arena,
    scheme: Arena,
    kind: Arena,
    from: Vec<u32>,
    has_from: Vec<bool>,
    to: Vec<u32>,
    has_to: Vec<bool>,
}

impl Postal {
    /// The codes' own columns; their places go in [`Places`] in the same order.
    pub(crate) fn of(codes: &[PostalCode]) -> Self {
        let mut out = Self::default();
        for c in codes {
            out.code.push(Some(c.code()));
            out.scheme.push(Some(c.scheme()));
            out.kind.push(Some(match c.kind {
                CodeKind::Area => "area",
                CodeKind::Part => "part",
                CodeKind::PostOffice => "post_office",
                CodeKind::Point => "point",
            }));
            out.from.push(c.valid_from.unwrap_or(0));
            out.has_from.push(c.valid_from.is_some());
            out.to.push(c.valid_to.unwrap_or(0));
            out.has_to.push(c.valid_to.is_some());
        }
        out
    }
}

/// The rows of a place result, with optional per-query columns.
#[derive(Default)]
pub(crate) struct Places {
    id: Arena,
    name: Arena,
    kind: Arena,
    level: Vec<u8>,
    lat: Vec<f64>,
    lon: Vec<f64>,
    elevation: Vec<i16>,
    has_elevation: Vec<bool>,
    parent: Arena,
    tz: Arena,
    iso: Arena,
    /// Search scores, when the query has them.
    pub(crate) score: Option<Vec<i32>>,
    /// Distances in metres and bearings in degrees, when the query has them.
    pub(crate) near: Option<(Vec<f64>, Vec<f64>)>,
    /// The codes of a postal query, one per row.
    pub(crate) postal: Option<Postal>,
}

impl Places {
    /// Appends `place`, reading what the installed components hold.
    pub(crate) fn push(
        &mut self,
        engine: &Engine,
        place: &Place,
        buf: &mut [u8; NAME],
    ) -> Result<()> {
        self.id.push_fmt(Some(&place.id));
        self.name.push(installed(engine.name(place, buf))?);
        self.kind
            .push(place.kind.map(teigeodata_core::PlaceKind::name));
        self.level.push(place.admin_level);
        self.lat.push(place.point.lat_degrees());
        self.lon.push(place.point.lon_degrees());
        let e = installed(engine.elevation(place))?;
        self.elevation.push(e.unwrap_or(0));
        self.has_elevation.push(e.is_some());
        let parent = installed(engine.parent(place))?.map(|p| p.id);
        self.parent.push_fmt(parent.as_ref());
        match installed(engine.tz_of(place))? {
            Some(z) => self.tz.push(installed(engine.name(&z, buf))?),
            None => self.tz.push(None),
        }
        self.iso
            .push_fmt(installed(engine.iso_code(place))?.as_ref());
        Ok(())
    }

    /// Every place of `places`, in order.
    pub(crate) fn of(engine: &Engine, places: impl IntoIterator<Item = Place>) -> Result<Self> {
        let mut out = Self::default();
        let mut buf = [0u8; NAME];
        for p in places {
            out.push(engine, &p, &mut buf)?;
        }
        Ok(out)
    }

    /// The blob: `id`, `name`, `kind` (dictionary), `level` (`u8`), `lat`, `lon` (`f64`),
    /// `elevation` (`i16`, nullable), `parent` (ID, nullable), `tz` and `iso` (dictionaries,
    /// nullable), then `score`, `meters` and `bearing` when the query set them, and for a
    /// postal query `postal` (the code), `scheme`, `code_kind` (`area`, `part`, `post_office`,
    /// `point`), `valid_from` and `valid_to` (days since 1970-01-01, nullable).
    pub(crate) fn blob(&self) -> Result<Blob> {
        let (id, name, kind) = (self.id.strs(), self.name.strs(), self.kind.strs());
        let (parent, tz, iso) = (self.parent.strs(), self.tz.strs(), self.iso.strs());
        let (name_c, kind_c) = (Column::Utf8(&name), Column::Dict(&kind));
        let elev = Column::I16(&self.elevation);
        let (parent_c, tz_c, iso_c) =
            (Column::Utf8(&parent), Column::Dict(&tz), Column::Dict(&iso));
        let mut columns = vec![
            ("id", Column::Utf8(&id)),
            ("name", Column::Nullable(&name_c, &self.name.present)),
            ("kind", Column::Nullable(&kind_c, &self.kind.present)),
            ("level", Column::U8(&self.level)),
            ("lat", Column::F64(&self.lat)),
            ("lon", Column::F64(&self.lon)),
            ("elevation", Column::Nullable(&elev, &self.has_elevation)),
            ("parent", Column::Nullable(&parent_c, &self.parent.present)),
            ("tz", Column::Nullable(&tz_c, &self.tz.present)),
            ("iso", Column::Nullable(&iso_c, &self.iso.present)),
        ];
        if let Some(score) = &self.score {
            columns.push(("score", Column::I32(score)));
        }
        if let Some((meters, bearing)) = &self.near {
            columns.push(("meters", Column::F64(meters)));
            columns.push(("bearing", Column::F64(bearing)));
        }
        let postal = self.postal.as_ref().map(|p| {
            let strs = (p.code.strs(), p.scheme.strs(), p.kind.strs());
            (p, strs)
        });
        let postal_c = postal.as_ref().map(|(p, (_, scheme, kind))| {
            (
                Column::Dict(scheme),
                Column::Dict(kind),
                Column::U32(&p.from),
                Column::U32(&p.to),
            )
        });
        if let (Some((p, (code, _, _))), Some((scheme, kind, from, to))) = (&postal, &postal_c) {
            columns.push(("postal", Column::Utf8(code)));
            columns.push(("scheme", *scheme));
            columns.push(("code_kind", *kind));
            columns.push(("valid_from", Column::Nullable(from, &p.has_from)));
            columns.push(("valid_to", Column::Nullable(to, &p.has_to)));
        }
        Blob::encode(&columns).map_err(|e| Error::new(ErrorCode::Internal, e.to_string()))
    }
}
