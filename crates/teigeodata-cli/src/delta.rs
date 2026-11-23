//! `delta`: the places that changed between two releases of a pack, as a pack read above the
//! older one (D-391).
//!
//! A place changed when anything the engine answers about it differs between the two packs:
//! its record with every name (point, kind, level, parent, timezone, population, elevation,
//! ISO code), its class, its outline at full resolution, and its search importance and unit
//! type. The delta keeps the newer pack's rows of the changed places, with their ancestors,
//! timezone areas and redirect survivors (so the rows it keeps refer only to rows it keeps),
//! re-encoded by `teigeodata_format::extract` (D-145); and REDIRECTS for each ID the newer
//! pack retired or forgot since the older one. Its manifest is the newer pack's, with
//! `delta_of` naming the older release. Read as a layer above the older pack, it hides the
//! older copy of every ID it knows (the engine's precedence, the newer release first), so the
//! two answer as the newer pack does.
//!
//! The sections a delta cannot carry are keyed by rows or by area rather than by place
//! (`DEM`, `SUBSTRING`, `BLOBS`, `ARCS_OVERVIEW`; `REGION` and `POSTAL` when what they say of a
//! place changed; `TZ_RULES` when a zone is gone), and so are the search rules that shape
//! stored keys (droppable suffixes, the phonetic key): when any of them differs, the delta is
//! refused, and the newer pack is installed whole or by section. The query-only rules travel:
//! the engine answers a delta's base by the delta's rules (D-394).

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

use teigeodata_core::PlaceId;
use teigeodata_core::table::Table;
use teigeodata_engine::{Engine, Layer, LayerOptions, Outline, Place};
use teigeodata_format::extract;
use teigeodata_format::hierarchy::Hierarchy;
use teigeodata_format::idmap::IdMap;
use teigeodata_format::place_tz::PlaceTz;
use teigeodata_format::postal::{self, Postal};
use teigeodata_format::redirects::{self, Fate, Redirects};
use teigeodata_format::region::{self, Options as RegionOptions, Region};
use teigeodata_format::search::Search;
use teigeodata_format::search::rules::Rule;
use teigeodata_format::tz_rules::TzRules;
use teigeodata_format::{Encoding, Kind, Pack, Part, Tag, Writer};
use teigeodata_server::output::{self, Bufs, Item};

use crate::Fail;
use crate::pack::{open, read};

/// Sections a delta re-encodes from the kept rows (`extract::rows`) or writes itself.
const CARRIED: [Kind; 15] = [
    Kind::IDMAP,
    Kind::PLACES,
    Kind::HIERARCHY,
    Kind::NAMES,
    Kind::SEARCH,
    Kind::PLACE_INDEX,
    Kind::ARCS,
    Kind::POLYGONS,
    Kind::POLY_INDEX,
    Kind::ADMIN_REGION,
    Kind::ELEVATION,
    Kind::PLACE_TZ,
    Kind::ISO_CODES,
    Kind::COMPONENTS,
    Kind::REDIRECTS,
];

/// What a delta holds.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Plan {
    /// Rows of the newer pack whose place changed or is new.
    pub(crate) changed: Vec<u32>,
    /// Rows of the newer pack whose outline changed or is new: the only rows with admin
    /// geometry in the delta; every other kept row's outline is read from the older pack.
    pub(crate) outlined: Vec<u32>,
    /// Every row the delta keeps: the changed ones and the rows they refer to; increasing.
    pub(crate) keep: Vec<u32>,
    /// IDs the newer pack retired or forgot since the older one: the survivor, or none.
    pub(crate) retired: Vec<(PlaceId, Option<PlaceId>)>,
    /// Sections not keyed by row that changed, carried whole as the newer pack stores them.
    pub(crate) whole: Vec<Kind>,
}

/// Sections not keyed by row that a delta carries whole when they change: the engine reads
/// them from the first layer that holds the key (a zone name), so the delta's copy answers for
/// every key it holds, provided it holds every key the older copy did.
const WHOLE: [Kind; 1] = [Kind::TZ_RULES];

/// Sections keyed by row that a newer release renumbers whenever a place is added or removed:
/// a delta leaves them out when they mean the same place by place, as the older pack's copy
/// then answers through the delta (each place read from the layer that wins its ID).
const BY_MEANING: [Kind; 2] = [Kind::REGION, Kind::POSTAL];

type Ids<'p, 'a> = (&'p IdMap<'a>, &'p IdMap<'a>);

/// Whether section `kind` means the same in both packs once rows are read as IDs: every
/// column alike but the ones naming rows, and those naming the same places in the same order.
fn same_meaning(
    kind: Kind,
    (old, new): (&Pack<'_>, &Pack<'_>),
    (ids_old, ids_new): Ids<'_, '_>,
) -> Result<bool, teigeodata_format::Error> {
    let (Some(a), Some(b)) = (old.section(kind), new.section(kind)) else {
        return Ok(false);
    };
    let bad = |_| teigeodata_format::Error::Payload {
        kind: kind.0,
        column: 0,
        problem: "not a column table",
    };
    let (ta, tb) = (
        Table::parse(a.bytes()).map_err(bad)?,
        Table::parse(b.bytes()).map_err(bad)?,
    );
    let rows: &[u16] = if kind == Kind::POSTAL {
        &[postal::RUN_PLACE, postal::BY_PLACE]
    } else {
        &[region::PLACE]
    };
    let cols = |t: &Table<'_>| t.ids().collect::<Vec<_>>();
    if cols(&ta) != cols(&tb)
        || ta
            .ids()
            .filter(|c| !rows.contains(c))
            .any(|c| ta.column_raw(c) != tb.column_raw(c))
    {
        return Ok(false);
    }
    let id = |m: &IdMap<'_>, row: Option<u32>| row.and_then(|r| m.id(r as usize));
    if kind == Kind::POSTAL {
        let (pa, pb) = (
            Postal::open(&a, ids_old.len())?,
            Postal::open(&b, ids_new.len())?,
        );
        Ok(pa.records() == pb.records()
            && (0..pa.records())
                .all(|r| id(ids_old, pa.place_of(r)) == id(ids_new, pb.place_of(r))))
    } else {
        let (ra, rb) = (Region::open(&a)?, Region::open(&b)?);
        Ok(ra.regions() == rb.regions()
            && (0..ra.regions())
                .filter_map(|r| u32::try_from(r).ok())
                .all(|r| id(ids_old, ra.place(r)) == id(ids_new, rb.place(r))))
    }
}

/// What differs between the packs that a delta cannot carry, by name (a note when the search
/// rules differ); and the [`WHOLE`] kinds it carries as stored.
fn uncarried(
    old: &Pack<'_>,
    new: &Pack<'_>,
    ids: Ids<'_, '_>,
) -> Result<(Vec<String>, Vec<Kind>), teigeodata_format::Error> {
    let mut out = Vec::new();
    let mut whole = Vec::new();
    let kinds: BTreeSet<u32> = old
        .sections()
        .chain(new.sections())
        .filter(|s| s.kind() != Kind::BLOCK_HASHES)
        .map(|s| s.kind().0)
        .collect();
    for k in kinds {
        let kind = Kind(k);
        let parts = |p: &Pack<'_>| -> Vec<(Part, [u8; 32])> {
            p.sections()
                .filter(|s| s.kind() == kind)
                .map(|s| (s.part(), *s.hash()))
                .collect()
        };
        let (a, b) = (parts(old), parts(new));
        // A kind in parts (names by language, ADR-0040) is not re-encoded by part here.
        let parted = a.iter().chain(&b).any(|(p, _)| *p != Part::BASE);
        if WHOLE.contains(&kind) {
            if a != b {
                if !parted && !b.is_empty() && keys_kept(old, new)? {
                    whole.push(kind);
                } else {
                    out.push(format!("{kind} (a key the older release holds is gone)"));
                }
            }
            continue;
        }
        if BY_MEANING.contains(&kind) && a != b && !parted {
            if !same_meaning(kind, (old, new), ids)? {
                out.push(format!("{kind} (what it says of a place changed)"));
            }
            continue;
        }
        if (!CARRIED.contains(&kind) && a != b) || (parted && CARRIED.contains(&kind)) {
            out.push(kind.to_string());
        }
    }
    // The rules that shape stored keys (droppable suffixes, the phonetic key) must agree: the
    // older pack's keys stay in use for the places the delta leaves to it. The other rules act
    // on the query only, and the delta's own replace the older pack's (D-394).
    let rules = |p: &Pack<'_>| -> Result<BTreeSet<String>, teigeodata_format::Error> {
        Ok(search_of(p)?
            .map(|x| {
                x.rules()
                    .iter()
                    .filter(|r| matches!(r, Rule::Drop(_) | Rule::Phonetic(_)))
                    .map(|r| format!("{r:?}"))
                    .collect()
            })
            .unwrap_or_default())
    };
    let (ra, rb) = (rules(old)?, rules(new)?);
    if ra != rb {
        let only = |x: &BTreeSet<String>, y: &BTreeSet<String>| {
            x.difference(y).cloned().collect::<Vec<_>>().join("; ")
        };
        out.push(format!(
            "SEARCH's key rules (only before: {}; only after: {})",
            only(&ra, &rb),
            only(&rb, &ra)
        ));
    }
    Ok((out, whole))
}

/// Whether the newer pack's timezone rules hold every zone the older one's do.
fn keys_kept(old: &Pack<'_>, new: &Pack<'_>) -> Result<bool, teigeodata_format::Error> {
    let zones = |p: &Pack<'_>| -> Result<BTreeSet<String>, teigeodata_format::Error> {
        Ok(match p.section(Kind::TZ_RULES) {
            Some(s) => TzRules::open(&s)?.names().map(str::to_string).collect(),
            None => BTreeSet::new(),
        })
    };
    Ok(zones(old)?.is_subset(&zones(new)?))
}

/// Everything the engine answers about `place` but its outline, with its search importance and
/// unit type, as bytes to compare; and its outline at full resolution (`None` without one).
fn fingerprint(
    e: &Engine,
    search: Option<&Search<'_>>,
    place: Place,
    bufs: &mut Bufs,
    outline: &mut Outline,
) -> Result<(Vec<u8>, Option<String>), Fail> {
    let bad = |e: teigeodata_engine::Error| Fail::Refused(e.to_string());
    let mut item = Item::from(place);
    item.all_names = true;
    let record = output::record(e, &item, bufs, true).map_err(bad)?;
    let mut out = serde_json::to_vec(&record).map_err(|e| Fail::Refused(e.to_string()))?;
    out.extend_from_slice(e.class(&place).map_err(bad)?.unwrap_or("").as_bytes());
    if let Some(s) = search {
        out.extend_from_slice(&[s.importance(place.row), s.unit(place.row)]);
    }
    let shape = e
        .boundary(&place, 0, outline)
        .map_err(bad)?
        .map(|_| format!("{:?}|{:?}", outline.points, outline.rings));
    Ok((out, shape))
}

fn search_of<'a>(p: &Pack<'a>) -> Result<Option<Search<'a>>, teigeodata_format::Error> {
    p.section(Kind::SEARCH)
        .map(|s| Search::open(&s))
        .transpose()
}

fn need<'a>(p: &Pack<'a>, path: &Path) -> Result<teigeodata_format::Section<'a>, Fail> {
    p.section(Kind::IDMAP)
        .ok_or_else(|| Fail::Refused(format!("{}: no IDMAP section", path.display())))
}

fn engine_of(bytes: &[u8], path: &Path) -> Result<Engine, Fail> {
    let layer = Layer::from_bytes(bytes.to_vec(), LayerOptions::default())
        .map_err(|e| Fail::of(path.display(), e))?;
    let mut e = Engine::new();
    e.add(layer);
    Ok(e)
}

/// What the delta from `old` to `new` holds; refused when the packs differ in what a delta
/// cannot carry, or are not two releases of one pack.
pub(crate) fn plan(
    (old_path, old_bytes): (&Path, &[u8]),
    (new_path, new_bytes): (&Path, &[u8]),
) -> Result<Plan, Fail> {
    let (old, new) = (open(old_path, old_bytes)?, open(new_path, new_bytes)?);
    let (mo, mn) = (old.manifest(), new.manifest());
    if (mo.address(), mo.component()) != (mn.address(), mn.component()) {
        return Err(Fail::Refused(format!(
            "{} and {} are not releases of one pack ({}.{} and {}.{})",
            old_path.display(),
            new_path.display(),
            mo.address(),
            mo.component(),
            mn.address(),
            mn.component()
        )));
    }
    if teigeodata_format::release_cmp(mo.release(), mn.release()).is_ge() {
        return Err(Fail::Refused(format!(
            "release {} is not newer than {}",
            mn.release(),
            mo.release()
        )));
    }
    if mo.delta_of().is_some() || mn.delta_of().is_some() {
        return Err(Fail::Refused("a delta is made from two whole packs".into()));
    }
    let bad_new = |e| Fail::of(new_path.display(), e);
    let bad_old = |e| Fail::of(old_path.display(), e);
    new.verify_sections().map_err(bad_new)?;
    old.verify_sections().map_err(bad_old)?;
    let (io, inew) = (need(&old, old_path)?, need(&new, new_path)?);
    let (ids_old, ids_new) = (
        IdMap::open(&io).map_err(bad_old)?,
        IdMap::open(&inew).map_err(bad_new)?,
    );
    let (not, whole) = uncarried(&old, &new, (&ids_old, &ids_new)).map_err(bad_new)?;
    if !not.is_empty() {
        return Err(Fail::Refused(format!(
            "a delta cannot carry what changed in {}: install {} whole or by section",
            not.join(", "),
            mn.release()
        )));
    }
    let (eo, en) = (
        engine_of(old_bytes, old_path)?,
        engine_of(new_bytes, new_path)?,
    );
    let (so, sn) = (
        search_of(&old).map_err(bad_old)?,
        search_of(&new).map_err(bad_new)?,
    );
    let (mut bufs_old, mut bufs_new) = (Bufs::default(), Bufs::default());
    let (mut oa, mut ob) = (Outline::default(), Outline::default());
    let bad = |e: teigeodata_engine::Error| Fail::Refused(e.to_string());
    let (mut changed, mut outlined) = (Vec::new(), Vec::new());
    for row in 0..ids_new.len() {
        let Some(id) = ids_new.id(row) else { continue };
        let Some(now) = en.get(id).map_err(bad)? else {
            continue;
        };
        let r = u32::try_from(row).map_err(|_| Fail::Refused("too many rows".into()))?;
        let (record, shape) = fingerprint(&en, sn.as_ref(), now, &mut bufs_new, &mut ob)?;
        let before = match ids_old.row(id).and_then(|_| eo.get(id).ok().flatten()) {
            Some(was) => Some(fingerprint(&eo, so.as_ref(), was, &mut bufs_old, &mut oa)?),
            None => None,
        };
        let (record_differs, shape_differs) = match &before {
            Some((rec, sh)) => (*rec != record, *sh != shape),
            None => (true, true),
        };
        // An outline removed cannot be said by a layer above: the older one would show through.
        if shape.is_none() && before.as_ref().is_some_and(|(_, sh)| sh.is_some()) {
            return Err(Fail::Refused(format!(
                "a delta cannot carry what changed in {id}: its outline was removed; install {} whole",
                mn.release()
            )));
        }
        if record_differs || shape_differs {
            changed.push(r);
        }
        if shape_differs && shape.is_some() {
            outlined.push(r);
        }
    }
    let retired = retired(&old, &new, &ids_old, &ids_new).map_err(bad_new)?;
    let keep = closure(&new, &ids_new, &changed, &retired).map_err(bad_new)?;
    Ok(Plan {
        changed,
        outlined,
        keep,
        retired,
        whole,
    })
}

/// The IDs the newer pack retired or forgot since the older one, with their survivors: each
/// older ID the newer pack no longer holds as current, and each newer redirect the older pack
/// resolves otherwise; sorted.
fn retired(
    old: &Pack<'_>,
    new: &Pack<'_>,
    ids_old: &IdMap<'_>,
    ids_new: &IdMap<'_>,
) -> Result<Vec<(PlaceId, Option<PlaceId>)>, teigeodata_format::Error> {
    let rn = new
        .section(Kind::REDIRECTS)
        .map(|s| Redirects::open(&s))
        .transpose()?;
    let ro = old
        .section(Kind::REDIRECTS)
        .map(|s| Redirects::open(&s))
        .transpose()?;
    let fate_new = |id: PlaceId| match rn.as_ref().and_then(|r| r.get(id)) {
        Some(Fate::Redirect(s)) => Some(s),
        _ => None,
    };
    let mut retired: Vec<(PlaceId, Option<PlaceId>)> = Vec::new();
    for row in 0..ids_old.len() {
        if let Some(id) = ids_old.id(row)
            && ids_new.row(id).is_none()
        {
            retired.push((id, fate_new(id)));
        }
    }
    if let Some(r) = &rn {
        for (id, fate) in r.iter() {
            let survivor = match fate {
                Fate::Redirect(s) => Some(s),
                Fate::Tombstone => None,
            };
            let before = match redirects::resolve(ids_old, ro.as_ref(), id) {
                redirects::Resolved::Moved(s, _) => Some(Some(s)),
                redirects::Resolved::Retired => Some(None),
                _ => None,
            };
            if before != Some(survivor) && !retired.iter().any(|(r, _)| *r == id) {
                retired.push((id, survivor));
            }
        }
    }
    retired.sort_unstable();
    Ok(retired)
}

/// The changed rows with every row they refer to: ancestors, timezone areas, and the
/// survivors of retired IDs; increasing.
fn closure(
    new: &Pack<'_>,
    ids: &IdMap<'_>,
    changed: &[u32],
    retired: &[(PlaceId, Option<PlaceId>)],
) -> Result<Vec<u32>, teigeodata_format::Error> {
    let hierarchy = new
        .section(Kind::HIERARCHY)
        .map(|s| Hierarchy::open(&s))
        .transpose()?;
    let zones = new
        .section(Kind::PLACE_TZ)
        .map(|s| PlaceTz::open(&s))
        .transpose()?;
    let mut keep = vec![false; ids.len()];
    let mark = |row: usize, keep: &mut Vec<bool>| {
        let mut at = Some(row);
        while let Some(r) = at {
            match keep.get_mut(r) {
                Some(k) if !*k => *k = true,
                _ => break,
            }
            at = hierarchy.as_ref().and_then(|h| h.parent(r));
        }
    };
    let survivors = retired
        .iter()
        .filter_map(|(_, s)| s.and_then(|s| ids.row(s)));
    for row in changed.iter().map(|&r| r as usize).chain(survivors) {
        mark(row, &mut keep);
        if let Some(z) = zones.as_ref().and_then(|z| z.get(row)) {
            mark(z as usize, &mut keep);
        }
    }
    Ok(keep
        .iter()
        .enumerate()
        .filter(|(_, k)| **k)
        .filter_map(|(r, _)| u32::try_from(r).ok())
        .collect())
}

/// The delta pack's bytes for `plan`, from the newer pack.
pub(crate) fn write(
    new: &Pack<'_>,
    old_release: &str,
    plan: &Plan,
) -> Result<Vec<u8>, teigeodata_format::Error> {
    let mut bodies = extract::rows_outlined(
        new,
        &plan.keep,
        &plan.outlined,
        0,
        &RegionOptions::default(),
    )?;
    if !plan.retired.is_empty() {
        let n = plan.retired.len() as u64;
        bodies.push((
            Kind::REDIRECTS,
            Encoding::TABLE,
            n,
            redirects::encode(&plan.retired)?,
        ));
    }
    let m = new.manifest();
    let mut w = Writer::new(m.address(), m.component(), m.release())?;
    for (tag, value) in m.entries() {
        if ![Tag::ADDRESS, Tag::COMPONENT, Tag::RELEASE, Tag::DELTA_OF].contains(&tag) {
            w.manifest(tag, value)?;
        }
    }
    w.manifest(Tag::DELTA_OF, old_release.as_bytes())?;
    for kind in &plan.whole {
        if let Some(s) = new.section(*kind) {
            bodies.push((*kind, s.encoding(), s.item_count(), s.bytes().to_vec()));
        }
    }
    let refs: Vec<(Kind, Encoding, u64, &[u8])> = bodies
        .iter()
        .map(|(k, e, n, b)| (*k, *e, *n, b.as_slice()))
        .collect();
    w.pack(&refs, None)
}

pub(crate) fn run(old: &Path, new: &Path, to: &Path, out: &mut dyn Write) -> Result<(), Fail> {
    let (ob, nb) = (read(old)?, read(new)?);
    let plan = plan((old, &ob), (new, &nb))?;
    let (op, np) = (open(old, &ob)?, open(new, &nb)?);
    let file =
        write(&np, op.manifest().release(), &plan).map_err(|e| Fail::of(new.display(), e))?;
    // Written beside the target, then renamed: a cut-off run leaves no half pack.
    let tmp = to.with_extension("tgd.part");
    std::fs::write(&tmp, &file).map_err(|e| Fail::of(tmp.display(), e))?;
    std::fs::rename(&tmp, to).map_err(|e| Fail::of(to.display(), e))?;
    writeln!(
        out,
        "wrote {}: {} bytes, {} changed places ({} outlines), {} rows kept, {} IDs retired; {} over {}",
        to.display(),
        file.len(),
        plan.changed.len(),
        plan.outlined.len(),
        plan.keep.len(),
        plan.retired.len(),
        np.manifest().release(),
        op.manifest().release()
    )?;
    Ok(())
}
