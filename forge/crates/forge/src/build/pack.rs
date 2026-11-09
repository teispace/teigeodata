//! Rows, IDs and sections of a built pack, then the read-back: every section is opened and
//! checked by the reader, and spot queries are compared with the input.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::ops::ControlFlow;
use std::path::Path;

use teigeodata_core::{E6, E7, Namespace, PlaceId, PlaceKind, Point, hilbert};
use teigeodata_format::elevation::Elevations as Heights;
use teigeodata_format::hierarchy::{self, Hierarchy};
use teigeodata_format::idmap::{self, IdMap};
use teigeodata_format::names::{self, NameRecord, Names, TEXT_BUFFER};
use teigeodata_format::place_index::{self, Neighbour, PlaceIndex};
use teigeodata_format::places::{self, PlaceColumns, Places};
use teigeodata_format::postal::{self, Postal};
use teigeodata_format::redirects::{self, Fate, Redirects};
use teigeodata_format::region::{self, Options, Region};
use teigeodata_format::search::rules::Rule;
use teigeodata_format::search::{self, EXACT, Maps, Search};
use teigeodata_format::{Bbox, Encoding, Kind, Pack, Tag, Writer};
use teigeodata_names::{SearchMap, fold, search_keys};

use super::ids::{Assignment, Registry, assign};
use super::{Args, Place, Profile, Zones, tags};

/// Whether a name can be stored: something is left after the search fold (not "?" or "-"),
/// and it is short enough that even escaped byte by byte (two codes per byte) it stays within
/// `names::MAX_CODES`.
pub(super) fn fits(text: &str) -> bool {
    text.len() <= names::MAX_CODES / 2 && !fold(text).is_empty()
}

/// Children per `PLACE_INDEX` box (D-082).
pub(super) const NODE: usize = 16;

pub(super) struct Input<'a> {
    pub(super) root: &'a Path,
    pub(super) args: &'a Args,
    pub(super) places: &'a [Place],
    pub(super) profile: &'a Profile,
    pub(super) zones: &'a Zones,
    /// (min lon, min lat, max lon, max lat).
    pub(super) coverage: (i32, i32, i32, i32),
    pub(super) file: &'a Path,
    /// The ID registry the build starts from, and where the updated one goes.
    pub(super) registry: &'a Registry,
    pub(super) ids_file: &'a Path,
    /// The admin geometry of the `topology` stage, when the build adds it.
    pub(super) geometry: Option<&'a super::geometry::Geometry>,
    /// The postal codes, when the country has them and the build may read them.
    pub(super) postal: Option<&'a super::postal::Postal>,
    /// Place elevations, one per place in `places`, when the build samples a DEM.
    pub(super) elevation: Option<&'a super::elevation::Elevations>,
    /// The sources' notices, one per line (D-126).
    pub(super) attribution: &'a str,
    /// The timezone each source record states, by record key (D-138): the independent check
    /// of `PLACE_TZ`.
    pub(super) stated_tz: &'a BTreeMap<String, String>,
}

/// The search importance of a place, 0 to 130 as in spike S3 (D-052), from what every source
/// has: kind, admin level and population. Provisional: ranking is calibrated on the golden set
/// in step 3.6.
///
/// A former unit (D-110) ranks below every current place: a search for a name means today's
/// place, and a former unit's population is that of a unit which no longer exists.
fn importance(p: &Place) -> u8 {
    if p.flags & places::FLAG_FORMER != 0 {
        return FORMER;
    }
    let base: u32 = match p.kind {
        PlaceKind::Country => 100,
        PlaceKind::Admin => match p.admin_level {
            1 => 90,
            2 => 70,
            3 => 45,
            _ => 40,
        },
        PlaceKind::Settlement => {
            30 + u32::from(RANK_BONUS.get(usize::from(p.rank)).copied().unwrap_or(0))
        }
        PlaceKind::Locality => 10,
        _ => 1,
    };
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "log10 of a population is below 11"
    )]
    let pop = p
        .population
        .filter(|&n| n > 0)
        .map_or(0, |n| ((n as f64).log10() * 5.0) as u32);
    u8::try_from((base + pop).min(255)).unwrap_or(u8::MAX)
}

/// A settlement's importance over the base 30 by its rank (`sources::settlement_rank`): none,
/// village, town, city, national capital (D-124). Calibrated on the golden set: a city's record
/// (44) beats the municipality of its name (40 + population); a town or village earns nothing,
/// since lifting them pushed prefix and ambiguous queries onto small places.
const RANK_BONUS: [u8; 5] = [0, 0, 0, 14, 24];

/// The importance of every former unit: below a settlement with no population, above a locality.
const FORMER: u8 = 25;

fn e6(p: (i32, i32)) -> Result<Point<E6>, String> {
    Point::<E7>::from_units(p.1, p.0)
        .map(Point::to_e6)
        .ok_or_else(|| format!("point {p:?} is off the grid"))
}

/// The PLACES section of the rows.
fn places_section(rows: &[&Place], points: &[Point<E6>]) -> Result<Vec<u8>, String> {
    let kinds: Vec<PlaceKind> = rows.iter().map(|p| p.kind).collect();
    let levels: Vec<u8> = rows.iter().map(|p| p.admin_level).collect();
    let flags: Vec<u32> = rows.iter().map(|p| p.flags).collect();
    let cols = PlaceColumns {
        points,
        kinds: &kinds,
        admin_levels: if levels.iter().any(|&l| l != 0) {
            &levels
        } else {
            &[]
        },
        flags: &flags,
        ..PlaceColumns::default()
    };
    places::encode(&cols).map_err(|e| e.to_string())
}

/// `PLACE_TZ` (D-134): each place's timezone area, the one the linked REGION `body` holds at
/// the place's stored point, so the area of a place and the area at its point agree.
fn place_tz_section(
    body: &[u8],
    zones: usize,
    points: &[Point<E6>],
) -> Result<Option<Body>, String> {
    let e = |e: teigeodata_format::Error| e.to_string();
    let s = teigeodata_format::Section::unpacked(Kind::REGION, zones as u64, body);
    let r = Region::open(&s).map_err(e)?;
    Ok(teigeodata_format::place_tz::encode_from(&r, points)
        .map_err(e)?
        .map(|b| (Kind::PLACE_TZ, Encoding::TABLE, points.len() as u64, b)))
}

pub(super) fn write(input: &Input<'_>) -> Result<String, String> {
    let places = input.places;
    // Rows: Hilbert order of the stored (1e-6°) point, ties by key.
    let mut order: Vec<(u64, &str, usize)> = Vec::with_capacity(places.len());
    for (i, p) in places.iter().enumerate() {
        order.push((hilbert(e6(p.point)?), p.key.as_str(), i));
    }
    order.sort_unstable();
    let rows: Vec<&Place> = order.iter().filter_map(|o| places.get(o.2)).collect();
    let row_of: BTreeMap<&str, u32> = rows
        .iter()
        .enumerate()
        .map(|(r, p)| (p.key.as_str(), u32::try_from(r).unwrap_or(u32::MAX)))
        .collect();
    if row_of.len() != rows.len() {
        return Err("two places share a key".into());
    }
    let n = rows.len();

    let points: Vec<Point<E6>> = rows.iter().map(|p| e6(p.point)).collect::<Result<_, _>>()?;
    let places_body = places_section(&rows, &points)?;
    // Heights in their own section (D-109, D-133), absent when no place has one.
    let heights = super::elevation::in_rows(input.elevation, order.iter().map(|o| o.2));
    let heights_body = teigeodata_format::elevation::encode(&heights).map_err(|e| e.to_string())?;

    let parents: Vec<Option<u32>> = rows
        .iter()
        .map(|p| p.parent.as_deref().and_then(|k| row_of.get(k).copied()))
        .collect();
    let hierarchy_body = hierarchy::encode(&parents).map_err(|e| e.to_string())?;

    let (ids, moved, assigned) = allocate(input, &rows)?;
    let idmap_body = idmap::encode(&ids).map_err(|e| e.to_string())?;

    let (names_body, search_body) = name_sections(&rows, input.profile, &input.args.admin)?;
    let index_body = place_index::encode(&points, NODE).map_err(|e| e.to_string())?;

    let mut sections: Vec<Body> = vec![
        (Kind::IDMAP, Encoding::TABLE, n as u64, idmap_body),
        (Kind::PLACES, Encoding::TABLE, n as u64, places_body),
        (Kind::HIERARCHY, Encoding::TABLE, n as u64, hierarchy_body),
        (Kind::NAMES, Encoding::TABLE, n as u64, names_body),
        (Kind::SEARCH, Encoding::TABLE, n as u64, search_body),
        (Kind::PLACE_INDEX, Encoding::TABLE, n as u64, index_body),
    ];
    if let Some(body) = heights_body {
        sections.push((Kind::ELEVATION, Encoding::TABLE, n as u64, body));
    }
    if !input.zones.rings.is_empty() {
        let body =
            region::encode(&input.zones.rings, &Options::default()).map_err(|e| e.to_string())?;
        let zone_rows: Vec<u32> = input
            .zones
            .places
            .iter()
            .map(|p| {
                row_of
                    .get(p.key.as_str())
                    .copied()
                    .ok_or("a zone has no row")
            })
            .collect::<Result<_, _>>()?;
        let body = region::link(&body, &zone_rows).map_err(|e| e.to_string())?;
        if let Some(tz) = place_tz_section(&body, zone_rows.len(), &points)? {
            sections.push(tz);
        }
        sections.push((Kind::REGION, Encoding::TABLE, zone_rows.len() as u64, body));
    }
    if let Some(g) = input.geometry {
        sections.extend(super::geometry::sections(g, &rows, &row_of)?);
    }
    if let Some(p) = input.postal.filter(|p| !p.entries.is_empty()) {
        sections.push(postal_section(p, &row_of, n)?);
    }
    if !moved.is_empty() {
        let body = redirects::encode(&moved).map_err(|e| e.to_string())?;
        sections.push((Kind::REDIRECTS, Encoding::TABLE, moved.len() as u64, body));
    }

    let (bytes, key) = assemble(input, &sections)?;
    // Read back from memory first: a pack that fails its checks is never written, and the
    // pack and its registry each replace the old file in one rename.
    let mut report = String::new();
    verify(
        &bytes,
        &rows,
        (&ids, &moved),
        input,
        key.as_ref(),
        &mut report,
    )?;
    crate::make::write_atomic(input.file, &bytes)?;
    crate::make::write_atomic(input.ids_file, assigned.registry.render().as_bytes())?;
    ids_report(&assigned, input, &mut report);
    let _ = writeln!(report, "\n| Section | Bytes |\n|---|---:|");
    for s in &sections {
        let _ = writeln!(report, "| {} | {} |", s.0, s.3.len());
    }
    let _ = writeln!(
        report,
        "| **pack** | **{}** |\n\nBLAKE3 of the pack: `{}`.",
        bytes.len(),
        blake3::hash(&bytes).to_hex()
    );
    Ok(report)
}

/// The POSTAL section: every code links to its unit's row (D-105), with the place index.
fn postal_section(
    p: &super::postal::Postal,
    row_of: &BTreeMap<&str, u32>,
    n: usize,
) -> Result<Body, String> {
    let codes: Vec<postal::Code> = p
        .entries
        .iter()
        .map(|e| {
            let place = row_of.get(e.unit.as_str()).copied().ok_or_else(|| {
                format!("postal code {} names {}, which has no row", e.code, e.unit)
            })?;
            Ok(postal::Code {
                scheme: e.scheme.clone(),
                code: e.code.clone(),
                place,
                kind: e.kind,
                valid_from: None,
                valid_to: None,
                redirect: None,
            })
        })
        .collect::<Result<_, String>>()?;
    let places = u32::try_from(n).map_err(|_| "too many places")?;
    let body = postal::encode(&p.rules, &codes, places, true).map_err(|e| e.to_string())?;
    Ok((Kind::POSTAL, Encoding::TABLE, codes.len() as u64, body))
}

/// IDs: kept through the registry, new ones in row order (D-075, D-094); and this pack's
/// redirects.
#[allow(
    clippy::type_complexity,
    reason = "the IDs, the redirects and the registry outcome"
)]
fn allocate(
    input: &Input<'_>,
    rows: &[&Place],
) -> Result<(Vec<PlaceId>, Vec<(PlaceId, Option<PlaceId>)>, Assignment), String> {
    let cc: [u8; 2] = input
        .args
        .cc
        .as_bytes()
        .try_into()
        .map_err(|_| format!("{}: not a two-letter country code", input.args.cc))?;
    let ns = Namespace::from_country(cc).ok_or("not a country namespace")?;
    let keys: Vec<Vec<String>> = rows.iter().map(|p| p.sources.clone()).collect();
    let assigned = assign(input.registry, &keys, input.args.build_name())?;
    let id = |s: u64| PlaceId::new(ns, s).ok_or("serial out of range");
    let ids: Vec<PlaceId> = assigned
        .serials
        .iter()
        .map(|&s| id(s))
        .collect::<Result<_, _>>()?;
    let moved: Vec<(PlaceId, Option<PlaceId>)> = assigned
        .redirects
        .iter()
        .map(|&(s, t)| Ok((id(s)?, t.map(id).transpose()?)))
        .collect::<Result<_, &str>>()?;
    Ok((ids, moved, assigned))
}

pub(super) type Body = (Kind, Encoding, u64, Vec<u8>);

fn ids_report(a: &Assignment, input: &Input<'_>, report: &mut String) {
    let _ = writeln!(
        report,
        "\nIDs ({}): {} kept, {} new, {} retired into a successor, {} retired with none; {} redirects in the pack. Registry: `{}`.",
        if input.registry.entries.is_empty() {
            "first build"
        } else {
            "from the registry"
        },
        a.kept,
        a.new,
        a.merged,
        a.gone,
        a.redirects.len(),
        input
            .ids_file
            .strip_prefix(input.root)
            .unwrap_or(input.ids_file)
            .display(),
    );
}

/// The pack file: the manifest (coverage box, build ID, `SOURCE_DATE_EPOCH`), the sections,
/// and the signature when a key is given.
fn assemble(
    input: &Input<'_>,
    sections: &[Body],
) -> Result<(Vec<u8>, Option<ed25519_dalek::SigningKey>), String> {
    let mut w =
        Writer::new(&input.args.cc, "places", &input.args.release).map_err(|e| e.to_string())?;
    let (x0, y0, x1, y1) = input.coverage;
    let corner = |lat, lon| Point::<E7>::from_units(lat, lon).ok_or("coverage off the grid");
    let bbox = Bbox {
        min: corner(y0, x0)?,
        max: corner(y1, x1)?,
    };
    w.manifest(Tag::BBOX, &bbox.to_bytes())
        .map_err(|e| e.to_string())?;
    // ADR-0002: the main build is ODbL, the lite build CC BY 4.0.
    let licence = if input.args.lite { "cc-by-4.0" } else { "odbl" };
    w.manifest(Tag::LICENCE, licence.as_bytes())
        .map_err(|e| e.to_string())?;
    if !input.attribution.is_empty() {
        w.manifest(Tag::ATTRIBUTION, input.attribution.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    // The build ID: a hash of the forge version, the pinned sources and the command line.
    let mut h = blake3::Hasher::new();
    h.update(env!("CARGO_PKG_VERSION").as_bytes());
    h.update(&fs::read(input.root.join("data/sources.lock")).map_err(|e| e.to_string())?);
    h.update(format!("{:?}", input.args).as_bytes());
    h.update(input.registry.render().as_bytes());
    let digest = h.finalize();
    let id = u64::from_le_bytes(digest.as_bytes()[..8].try_into().unwrap_or([0; 8]));
    let epoch = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    w.build_id(id).created_unix(epoch);
    let key = match &input.args.key {
        Some(path) => Some(signing_key(&input.root.join(path))?),
        None => None,
    };
    let signer = key.as_ref().map(teigeodata_format::signer);
    let bodies: Vec<(Kind, Encoding, u64, &[u8])> = sections
        .iter()
        .map(|s| (s.0, s.1, s.2, s.3.as_slice()))
        .collect();
    let bytes = w
        .pack(
            &bodies,
            signer.as_ref().map(|s| s as &teigeodata_format::Sign<'_>),
        )
        .map_err(|e| e.to_string())?;
    drop(signer);
    Ok((bytes, key))
}

/// The NAMES and SEARCH bodies of the rows.
fn name_sections(
    rows: &[&Place],
    profile: &Profile,
    admin_source: &str,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let records: Vec<Vec<NameRecord<'_>>> = rows
        .iter()
        .map(|p| {
            p.names
                .iter()
                .filter(|n| fits(&n.text))
                .map(|nm| {
                    let (lang, script) = tags(nm);
                    NameRecord {
                        text: &nm.text,
                        lang,
                        script,
                        kind: nm.kind,
                        ..NameRecord::default()
                    }
                })
                .collect()
        })
        .collect();
    let refs: Vec<&[NameRecord<'_>]> = records.iter().map(Vec::as_slice).collect();
    let names_body = names::encode(&refs).map_err(|e| e.to_string())?;

    let mut maps = Maps::default();
    for (row, p) in rows.iter().enumerate() {
        let row = u32::try_from(row).unwrap_or(u32::MAX);
        for nm in &p.names {
            search_keys(&nm.text, &profile.droppable, profile.phonetic, |m, k| {
                let map = match m {
                    SearchMap::Exact => &mut maps.exact,
                    SearchMap::Loose => &mut maps.loose,
                    SearchMap::Phonetic => &mut maps.phonetic,
                };
                // Keys longer than the automaton allows are left out of search only.
                if k.len() < 255 {
                    map.entry(k).or_default().insert(row);
                }
            });
        }
    }
    let importance: Vec<u8> = rows.iter().map(|p| importance(p)).collect();
    let rules = query_rules(profile);
    let units: Vec<u8> = rows
        .iter()
        .map(|p| profile.unit_of(admin_source, &p.key, p.admin_level))
        // A type no abbreviation names cannot be asked for: no column value for it.
        .map(|u| {
            let named = usize::from(u)
                .checked_sub(1)
                .and_then(|i| profile.units.get(i))
                .is_some_and(|t| !t.abbreviations.is_empty());
            if named { u } else { 0 }
        })
        .collect();
    let extra = search::Extra {
        rules: (!rules.is_empty()).then_some(rules.as_slice()),
        units: if units.iter().any(|&u| u > 0) {
            &units
        } else {
            &[]
        },
    };
    let search_body =
        search::encode_maps_with(&maps, &importance, &extra).map_err(|e| e.to_string())?;

    Ok((names_body, search_body))
}

/// The country's query rules for SEARCH (D-120): its phonetic key, unit-type abbreviations,
/// droppable suffixes and native generics with each Latin spelling.
fn query_rules(profile: &Profile) -> Vec<Rule<'_>> {
    let mut rules: Vec<Rule<'_>> = Vec::new();
    if let Some(n) = &profile.phonetic_name {
        rules.push(Rule::Phonetic(n));
    }
    for (i, u) in profile.units.iter().enumerate() {
        let Ok(unit) = u8::try_from(i + 1) else { break };
        for a in &u.abbreviations {
            rules.push(Rule::Unit {
                abbreviation: a.folded(),
                unit,
            });
        }
    }
    rules.extend(profile.droppable.iter().map(|d| Rule::Drop(d)));
    for (native, spellings) in &profile.native {
        rules.extend(spellings.iter().map(|s| Rule::Native {
            native,
            spelling: s,
        }));
    }
    rules
}

/// An Ed25519 signing key from a file of 64 hex digits (its 32-byte seed).
pub(super) fn signing_key(path: &Path) -> Result<ed25519_dalek::SigningKey, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let hex = text.trim();
    let mut seed = [0_u8; 32];
    if hex.len() != 64 {
        return Err(format!("{}: expected 64 hex digits", path.display()));
    }
    for (i, b) in seed.iter_mut().enumerate() {
        *b = hex
            .get(2 * i..2 * i + 2)
            .and_then(|d| u8::from_str_radix(d, 16).ok())
            .ok_or_else(|| format!("{}: not hex", path.display()))?;
    }
    Ok(ed25519_dalek::SigningKey::from_bytes(&seed))
}

/// Opens the written pack with the reader, checks every section, and compares spot queries
/// with the input: every place's ID, point, parent, canonical name, exact search and nearest
/// place, and every zone at its own point.
fn verify(
    bytes: &[u8],
    rows: &[&Place],
    (want_ids, moved): (&[PlaceId], &[(PlaceId, Option<PlaceId>)]),
    input: &Input<'_>,
    key: Option<&ed25519_dalek::SigningKey>,
    report: &mut String,
) -> Result<(), String> {
    let e = |e: teigeodata_format::Error| e.to_string();
    let p = Pack::open(bytes).map_err(e)?;
    p.verify_sections().map_err(e)?;
    if let Some(k) = key {
        p.verify_signature(&[k.verifying_key()]).map_err(e)?;
    }
    let section = |kind| p.section(kind).ok_or(format!("no {kind} section"));
    let places = Places::open(&section(Kind::PLACES)?).map_err(e)?;
    places.check().map_err(e)?;
    let hier = Hierarchy::open(&section(Kind::HIERARCHY)?).map_err(e)?;
    hier.check().map_err(e)?;
    let ids = IdMap::open(&section(Kind::IDMAP)?).map_err(e)?;
    ids.check().map_err(e)?;
    for (row, want) in want_ids.iter().enumerate() {
        if ids.id(row) != Some(*want) {
            return Err(format!("row {row}: the ID differs from the registry's"));
        }
    }
    if let Some(s) = p.section(Kind::REDIRECTS) {
        let r = Redirects::open(&s).map_err(e)?;
        r.check(&ids).map_err(e)?;
        for &(from, to) in moved {
            let want = to.map_or(Fate::Tombstone, Fate::Redirect);
            if r.get(from) != Some(want) {
                return Err(format!("{from}: its redirect differs from the registry's"));
            }
        }
    } else if !moved.is_empty() {
        return Err("no REDIRECTS section".into());
    }
    let names = Names::open(&section(Kind::NAMES)?).map_err(e)?;
    names.check().map_err(e)?;
    let search = Search::open(&section(Kind::SEARCH)?).map_err(e)?;
    search.check().map_err(e)?;
    let index = PlaceIndex::open(&section(Kind::PLACE_INDEX)?, places).map_err(e)?;
    index.check().map_err(e)?;

    verify_rows(rows, &places, &hier, &ids, &names, &search, &index)?;
    let geometry_line = match input.geometry {
        Some(g) => Some(super::geometry::verify(&p, rows, g)?),
        None => None,
    };
    let zones_ok = verify_zones(&p, rows, input)?;
    let tz_line = verify_place_tz(&p, &places, rows, input.stated_tz)?;
    let by_kind: BTreeMap<String, usize> = rows.iter().fold(BTreeMap::new(), |mut m, r| {
        *m.entry(format!("{:?}", r.kind)).or_default() += 1;
        m
    });
    let orphans = rows
        .iter()
        .filter(|r| {
            r.parent.is_none() && !matches!(r.kind, PlaceKind::Country | PlaceKind::TimezoneArea)
        })
        .count();
    let distinct: BTreeSet<&str> = rows.iter().map(|r| r.key.as_str()).collect();
    let _ = writeln!(
        report,
        "Read back and checked: every section opens and passes `check`; all {} places keep their ID, point, parent and canonical name, are found by an exact search of that name, and are their own nearest place; {zones_ok} zones hold their own point and link to their row. Places by kind: {by_kind:?}; {orphans} below the country without a parent; {} distinct keys.",
        rows.len(),
        distinct.len(),
    );
    if let Some(line) = tz_line {
        let _ = writeln!(report, "\n{line}");
    }
    if let Some(line) = geometry_line {
        let _ = writeln!(report, "\n{line}");
    }
    let heights = p
        .section(Kind::ELEVATION)
        .map(|s| Heights::open(&s))
        .transpose()
        .map_err(e)?;
    match (input.elevation, heights) {
        (Some(d), h) => {
            let _ = writeln!(report, "\n{}", d.verify(&places, h.as_ref())?);
        }
        (None, None) => {}
        (None, Some(_)) => return Err("an ELEVATION section without a DEM".into()),
    }
    if let Some(s) = p.section(Kind::POSTAL) {
        let line = verify_postal(&s, rows, input)?;
        let _ = writeln!(report, "\n{line}");
    }
    Ok(())
}

/// REGION read back: `check`, and every zone holds its own point and links to its row.
fn verify_zones(p: &Pack<'_>, rows: &[&Place], input: &Input<'_>) -> Result<usize, String> {
    let e = |e: teigeodata_format::Error| e.to_string();
    let mut zones_ok = 0;
    if let Some(s) = p.section(Kind::REGION) {
        let region = Region::open(&s).map_err(e)?;
        region.check().map_err(e)?;
        for (z, place) in input.zones.places.iter().enumerate() {
            let at = Point::<E7>::from_units(place.point.1, place.point.0)
                .ok_or("a zone point is off the grid")?;
            let mut all = [0_u32; 8];
            let k = region.regions_at(at, &mut all);
            let z = u32::try_from(z).unwrap_or(u32::MAX);
            if !all.get(..k).unwrap_or_default().contains(&z) {
                return Err(format!("{}: its own point is not in the zone", place.key));
            }
            let row = region.place(z).ok_or("a zone is not linked to its place")?;
            if rows.get(row as usize).map(|r| r.key.as_str()) != Some(place.key.as_str()) {
                return Err(format!("{}: linked to the wrong row", place.key));
            }
            zones_ok += 1;
        }
    }
    Ok(zones_ok)
}

/// The most source records whose stated timezone may differ from their place's (G-tz,
/// D-138), as a share of the records compared.
const TZ_DIFFER_MAX: f64 = 0.001;

/// `PLACE_TZ` read back (D-134): `check`, one row per place, and every place's area is the
/// area REGION holds at the place's point as read back from PLACES. No REGION, no `PLACE_TZ`.
/// G-tz (D-138): each source record that states a timezone is compared with its place's;
/// at most [`TZ_DIFFER_MAX`] of them may differ.
fn verify_place_tz(
    p: &Pack<'_>,
    places: &Places<'_>,
    rows: &[&Place],
    stated: &BTreeMap<String, String>,
) -> Result<Option<String>, String> {
    let e = |e: teigeodata_format::Error| e.to_string();
    let (tz, zones) = match (p.section(Kind::PLACE_TZ), p.section(Kind::REGION)) {
        (None, _) => return Ok(None),
        (Some(_), None) => return Err("a PLACE_TZ section without REGION".into()),
        (Some(t), Some(r)) => (
            teigeodata_format::place_tz::PlaceTz::open(&t).map_err(e)?,
            Region::open(&r).map_err(e)?,
        ),
    };
    tz.check().map_err(e)?;
    if tz.len() != places.len() {
        return Err(format!(
            "PLACE_TZ has {} rows, PLACES {}",
            tz.len(),
            places.len()
        ));
    }
    let mut with = 0_usize;
    let (mut agree, mut none) = (0_usize, 0_usize);
    let mut differ: Vec<String> = Vec::new();
    for row in 0..places.len() {
        let at = places.point(row).ok_or("a row without a point")?;
        let want = zones.region(at.to_e7()).and_then(|z| zones.place(z));
        let got = tz.get(row);
        if got != want {
            return Err(format!(
                "row {row} at {at}: PLACE_TZ {got:?}, REGION {want:?}"
            ));
        }
        with += usize::from(got.is_some());
        let zone = got
            .and_then(|z| rows.get(z as usize))
            .and_then(|z| z.names.first())
            .map(|n| n.text.as_str());
        let place = rows.get(row).ok_or("a row without its place")?;
        for k in &place.sources {
            let Some(s) = stated.get(k) else { continue };
            match zone {
                Some(z) if z == s => agree += 1,
                Some(z) => differ.push(format!("{k} states {s}, its place {z}")),
                None => none += 1,
            }
        }
    }
    let compared = agree + differ.len();
    #[expect(clippy::cast_precision_loss, reason = "counts far below 2^52")]
    let share = differ.len() as f64 / compared.max(1) as f64;
    let mut line = format!(
        "Timezones: all {} places agree with REGION at their stored point ({with} in one of {} areas). G-tz: of {compared} source records that state a timezone and whose place has one, {agree} agree and {} differ ({:.3}%, at most {:.1}%); {none} records' places have none.",
        places.len(),
        tz.zones(),
        differ.len(),
        share * 100.0,
        TZ_DIFFER_MAX * 100.0,
    );
    if !differ.is_empty() {
        let _ = write!(
            line,
            " First: {}.",
            differ
                .iter()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
    if share > TZ_DIFFER_MAX {
        return Err(line);
    }
    Ok(Some(line))
}

/// POSTAL read back: `check`, then every input code is found in its own scheme at its unit's
/// row with its kind, and is among that row's codes in the place index.
fn verify_postal(
    s: &teigeodata_format::Section<'_>,
    rows: &[&Place],
    input: &Input<'_>,
) -> Result<String, String> {
    let e = |e: teigeodata_format::Error| e.to_string();
    let post = Postal::open(s, rows.len()).map_err(e)?;
    post.check().map_err(e)?;
    let entries = input.postal.map_or(&[][..], |p| p.entries.as_slice());
    let row_of: BTreeMap<&str, u32> = rows
        .iter()
        .enumerate()
        .map(|(r, p)| (p.key.as_str(), u32::try_from(r).unwrap_or(u32::MAX)))
        .collect();
    for entry in entries {
        let row = row_of
            .get(entry.unit.as_str())
            .copied()
            .ok_or_else(|| format!("{}: no row", entry.unit))?;
        let mut key = None;
        post.lookup(&entry.code, Some(&entry.scheme), |o| {
            key = Some(o);
            ControlFlow::Break(())
        });
        let o = key.ok_or_else(|| format!("postal {}: not found", entry.code))?;
        let found = post.records_of(o).find(|&r| {
            post.record(r)
                .is_some_and(|x| x.place == row && x.kind == entry.kind)
        });
        let rec = found.ok_or_else(|| format!("postal {}: not at {}", entry.code, entry.unit))?;
        let mut indexed = false;
        post.records_at(row, |r| {
            indexed = r == rec;
            if indexed {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        });
        if !indexed {
            return Err(format!(
                "postal {}: missing from the place index",
                entry.code
            ));
        }
    }
    let schemes: Vec<&str> = post.schemes().into_iter().map(|s| s.name()).collect();
    Ok(format!(
        "POSTAL: {} codes in {} keys, schemes {schemes:?}; every code is found at its unit's row with its kind, and through the place index.",
        post.records(),
        post.keys()
    ))
}

/// Every place keeps its ID, point, parent and canonical name, is found by an exact search of
/// that name, and is its own nearest place.
fn verify_rows(
    rows: &[&Place],
    places: &Places<'_>,
    hier: &Hierarchy<'_>,
    ids: &IdMap<'_>,
    names: &Names<'_>,
    search: &Search<'_>,
    index: &PlaceIndex<'_>,
) -> Result<(), String> {
    let e = |e: teigeodata_format::Error| e.to_string();
    let row_of: BTreeMap<&str, usize> = rows
        .iter()
        .enumerate()
        .map(|(r, p)| (p.key.as_str(), r))
        .collect();
    let mut buf = [0_u8; TEXT_BUFFER];
    let (mut found, mut nearest_self) = (0_usize, 0_usize);
    let mut missed: Vec<String> = Vec::new();
    let mut out = [Neighbour::default(); 1];
    for (row, place) in rows.iter().enumerate() {
        let id = ids.id(row).ok_or("a row has no ID")?;
        if ids.row(id) != Some(row) {
            return Err(format!("row {row}: its ID does not lead back to it"));
        }
        if places.point(row) != Some(e6(place.point)?) {
            return Err(format!("row {row}: the point differs"));
        }
        let want_parent = place.parent.as_deref().and_then(|k| row_of.get(k).copied());
        if hier.parent(row) != want_parent {
            return Err(format!("row {row} ({}): the parent differs", place.key));
        }
        let canonical = names.canonical(row).ok_or("a place has no name")?;
        let text = canonical.text(&mut buf).map_err(e)?;
        let first = place
            .names
            .first()
            .map(|n| n.text.as_str())
            .unwrap_or_default();
        if text != first {
            return Err(format!(
                "row {row}: canonical name {text:?}, expected {first:?}"
            ));
        }
        let mut hit = false;
        if let Some(o) = search.get(EXACT, &fold(first)) {
            search.rows_of(o, |r| {
                hit |= r as usize == row;
                if hit {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            });
        }
        if !hit && missed.len() < 5 {
            missed.push(format!("{} {first:?} (fold {:?})", place.key, fold(first)));
        }
        found += usize::from(hit);
        let at = e6(place.point)?;
        if index.nearest(at, &mut out) == 1 {
            let other = out.first().and_then(|n| places.point(n.row));
            nearest_self += usize::from(other == Some(at));
        }
    }
    if found != rows.len() {
        return Err(format!(
            "{} of {} places are not found by their canonical name: {}",
            rows.len() - found,
            rows.len(),
            missed.join(", ")
        ));
    }
    if nearest_self != rows.len() {
        return Err("a place's nearest place is not at its own point".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{FORMER, importance};
    use crate::build::Place;
    use teigeodata_core::PlaceKind;
    use teigeodata_format::places;

    fn place(kind: PlaceKind, admin_level: u8, rank: u8, population: Option<i64>) -> Place {
        Place {
            key: String::new(),
            kind,
            admin_level,
            flags: 0,
            point: (0, 0),
            names: Vec::new(),
            population,
            rank,
            parent: None,
            sources: Vec::new(),
        }
    }

    #[test]
    fn a_city_outranks_a_municipality_and_a_village_does_not() {
        let settlement = |rank| importance(&place(PlaceKind::Settlement, 0, rank, None));
        let municipality = importance(&place(PlaceKind::Admin, 4, 0, None));
        assert_eq!([0, 1, 2, 3, 4].map(settlement), [30, 30, 30, 44, 54]);
        // A city's record beats the municipality of its name; a town's or village's does not.
        assert!(settlement(3) > 40 && settlement(2) < municipality);
        // An unknown rank adds nothing; population adds 5·log10.
        assert_eq!(settlement(9), 30);
        assert_eq!(
            importance(&place(PlaceKind::Settlement, 0, 3, Some(100_000))),
            44 + 25
        );
        let mut former = place(PlaceKind::Settlement, 0, 4, Some(1_000_000));
        former.flags = places::FLAG_FORMER;
        assert_eq!(importance(&former), FORMER);
    }
}
