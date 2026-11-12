//! Every section's full check, in one place: `tgd verify --deep`, and any tool that must know a
//! pack is whole beyond its hashes. Opening a section checks its structure in O(columns);
//! `check` reads every value, O(rows). Sections that read others are checked against them, as
//! the engine opens them: `PLACE_INDEX` on PLACES, POLYGONS on ARCS and the row count,
//! `POLY_INDEX` on POLYGONS and ARCS, REDIRECTS on IDMAP, POSTAL on the row count, ELEVATION,
//! `PLACE_TZ` and HIERARCHY one row per place, `ISO_CODES` on the row count, `ARCS_OVERVIEW` against ARCS, `ADMIN_REGION` on
//! ARCS when it reads its vertices from them; a section's block-hash list is proved against
//! its hash. BLOBS frames are decoded with the `zstd` feature. No allocation.
//!
//! The section hashes are not checked here: [`Pack::open_verified`] or
//! [`Pack::verify_sections`] does that, and should come first.

use crate::arcs::Arcs;
use crate::blobs::Blobs;
use crate::components::Components;
use crate::dem::Dem;
use crate::elevation::Elevations;
use crate::hierarchy::Hierarchy;
use crate::idmap::IdMap;
use crate::iso_codes::IsoCodes;
use crate::names::Names;
use crate::place_index::PlaceIndex;
use crate::place_tz::PlaceTz;
use crate::places::Places;
use crate::poly_index::PolyIndex;
use crate::polygons::Polygons;
use crate::postal::Postal;
use crate::redirects::Redirects;
use crate::region::Region;
use crate::search::Search;
use crate::{Error, Kind, Pack, Section};

fn needs(kind: Kind, problem: &'static str) -> Error {
    Error::Payload {
        kind: kind.0,
        column: 0,
        problem,
    }
}

/// Checks every section of `pack` in table order, calling `each` with its kind and the
/// result: `Ok(true)` checked, `Ok(false)` a kind this reader has no check for (an unknown
/// kind, or `BLOCK_HASHES`, which is proved with the section it covers), `Err` the first
/// problem found in it.
pub fn sections(pack: &Pack<'_>, mut each: impl FnMut(Kind, Result<bool, Error>)) {
    let get = |k| pack.section(k);
    let places = get(Kind::PLACES).map(|s| Places::open(&s));
    let rows = match places {
        Some(Ok(p)) => Some(p.len()),
        _ => None,
    };
    let ids = get(Kind::IDMAP).and_then(|s| IdMap::open(&s).ok());
    let arcs = get(Kind::ARCS).and_then(|s| Arcs::open(&s).ok());
    let polygons = get(Kind::POLYGONS).and_then(|s| Polygons::open(&s).ok());
    let one_per_place = |kind: Kind, len: usize| match rows {
        Some(r) if r == len => Ok(()),
        Some(_) => Err(needs(kind, "not one row per PLACES row")),
        None => Err(needs(kind, "no PLACES to count its rows")),
    };
    for s in pack.sections() {
        let kind = s.kind();
        let checked = (|| -> Result<bool, Error> {
            if let Some(b) = pack.part_blocks(kind, s.part())? {
                b.prove()?;
            }
            match kind {
                Kind::IDMAP => IdMap::open(&s)?.check()?,
                Kind::PLACES => Places::open(&s)?.check()?,
                Kind::ELEVATION => {
                    let h = Elevations::open(&s)?;
                    h.check()?;
                    one_per_place(kind, h.len())?;
                }
                Kind::PLACE_TZ => {
                    let t = PlaceTz::open(&s)?;
                    t.check()?;
                    one_per_place(kind, t.len())?;
                }
                Kind::HIERARCHY => {
                    let h = Hierarchy::open(&s)?;
                    h.check()?;
                    one_per_place(kind, h.len())?;
                }
                Kind::ISO_CODES => {
                    let r = rows.ok_or(needs(kind, "no PLACES to count rows"))?;
                    IsoCodes::open(&s, r)?.check()?;
                }
                Kind::REDIRECTS => {
                    let ids = ids.ok_or(needs(kind, "no IDMAP to check against"))?;
                    Redirects::open(&s)?.check(&ids)?;
                }
                Kind::NAMES => {
                    let n = Names::open(&s)?;
                    n.check()?;
                    one_per_place(kind, n.len())?;
                }
                Kind::SEARCH | Kind::SUBSTRING if !s.part().is_base() => {
                    // A part borrows its base part's importance (ADR-0040).
                    let base = get(kind).ok_or(needs(kind, "a part without its base part"))?;
                    Search::open_part(&s, &Search::open(&base)?)?.check()?;
                }
                Kind::SEARCH | Kind::SUBSTRING => Search::open(&s)?.check()?,
                Kind::PLACE_INDEX => {
                    let p = places
                        .and_then(Result::ok)
                        .ok_or(needs(kind, "no PLACES to index"))?;
                    PlaceIndex::open(&s, p)?.check()?;
                }
                Kind::REGION => Region::open(&s)?.check()?,
                Kind::POSTAL => {
                    let r = rows.ok_or(needs(kind, "no PLACES to count rows"))?;
                    Postal::open(&s, r)?.check()?;
                }
                Kind::DEM | Kind::DEM_COARSE => Dem::open(&s)?.check()?,
                Kind::COMPONENTS => Components::open(&s)?.check()?,
                Kind::BLOBS => {
                    let b = Blobs::open(&s)?;
                    b.check()?;
                    #[cfg(feature = "zstd")]
                    b.decoder()?.check()?;
                }
                _ => return outlines(&s, arcs.as_ref(), polygons.as_ref(), rows),
            }
            Ok(true)
        })();
        each(kind, checked);
    }
}

/// Checks an outline section `s` (`ADMIN_REGION`, ARCS, `ARCS_OVERVIEW`, POLYGONS,
/// `POLY_INDEX`) against the pack's ARCS, POLYGONS and row count: `Ok(false)` for any other
/// kind.
fn outlines(
    s: &Section<'_>,
    arcs: Option<&Arcs<'_>>,
    polygons: Option<&Polygons<'_>>,
    rows: Option<usize>,
) -> Result<bool, Error> {
    let kind = s.kind();
    match kind {
        Kind::ADMIN_REGION => match arcs {
            Some(a) => Region::open_on_arcs(s, a)?.check()?,
            None => Region::open(s)?.check()?,
        },
        Kind::ARCS => Arcs::open(s)?.check()?,
        Kind::ARCS_OVERVIEW => {
            let o = Arcs::open(s)?;
            o.check()?;
            let full = arcs.ok_or(needs(kind, "no ARCS to check against"))?;
            o.check_overview(full)?;
        }
        Kind::POLYGONS => {
            let a = arcs.ok_or(needs(kind, "no ARCS to draw from"))?;
            let r = rows.ok_or(needs(kind, "no PLACES to name units"))?;
            Polygons::open(s)?.check(a, r)?;
        }
        Kind::POLY_INDEX => {
            let p = polygons.ok_or(needs(kind, "no POLYGONS to index"))?;
            let a = arcs.ok_or(needs(kind, "no ARCS to index"))?;
            PolyIndex::open(s, p)?.check(p, a)?;
        }
        _ => return Ok(false),
    }
    Ok(true)
}
