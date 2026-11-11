//! Rows, IDs and sections of a built pack, then the read-back: every section is opened and
//! checked by the reader, and spot queries are compared with the input.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::ops::ControlFlow;
use std::path::Path;

use teigeodata_core::{E6, E7, IsoCode, Namespace, PlaceId, PlaceKind, Point, hilbert};
use teigeodata_format::elevation::Elevations as Heights;
use teigeodata_format::hierarchy::{self, Hierarchy};
use teigeodata_format::idmap::{self, IdMap};
use teigeodata_format::iso_codes::IsoCodes;
use teigeodata_format::names::{self, NameRecord, Names, TEXT_BUFFER};
use teigeodata_format::place_index::{self, Neighbour, PlaceIndex};
use teigeodata_format::places::{self, PlaceColumns, Places};
use teigeodata_format::postal::{self, Postal};
use teigeodata_format::redirects::{self, Fate, Redirects};
use teigeodata_format::region::{self, Options, Region};
use teigeodata_format::search::rules::Rule;
use teigeodata_format::search::{self, EXACT, Maps, Search};
use teigeodata_format::{Bbox, Encoding, Kind, Pack, Tag, Writer};
use teigeodata_names::{SearchMap, fold, search_keys, substring_keys};

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
    /// The pack's address: the country code, or a split build's part (`np-p3`, D-191).
    pub(super) address: &'a str,
    /// A split build's part takes the country pack's IDs and redirects (D-191); the country
    /// pack allocates them through the registry.
    pub(super) given: Option<&'a Given>,
    pub(super) places: &'a [Place],
    pub(super) profile: &'a Profile,
    pub(super) zones: &'a Zones,
    /// (min lon, min lat, max lon, max lat).
    pub(super) coverage: (i32, i32, i32, i32),
    pub(super) file: &'a Path,
    /// The ID registry the build starts from, and where the updated one goes (none for a part).
    pub(super) registry: &'a Registry,
    pub(super) ids_file: Option<&'a Path>,
    /// The admin geometry of the `topology` stage, when the build adds it.
    pub(super) geometry: Option<&'a super::geometry::Geometry>,
    /// The postal codes, when the country has them and the build may read them.
    pub(super) postal: Option<&'a super::postal::Postal>,
    /// Place elevations, one per place in `places`, when the build samples a DEM.
    pub(super) elevation: Option<&'a super::elevation::Elevations>,
    /// The pack's area: its units' polygons, which its DEM section is masked to (D-192). Empty:
    /// no DEM section.
    pub(super) outline: &'a [&'a crate::geom::Polygon],
    /// The sources' notices, one per line (D-126).
    pub(super) attribution: &'a str,
    /// The timezone each source record states, by record key (D-138): the independent check
    /// of `PLACE_TZ`.
    pub(super) stated_tz: &'a BTreeMap<String, String>,
    /// The country's ISO 3166 codes (D-197), when it has `iso3166.yaml`.
    pub(super) iso: Option<&'a super::iso::Iso>,
    /// The timezones' compiled rules (D-196); a pack with timezone areas needs them.
    pub(super) tz_rules: Option<&'a super::tz_rules::Compiled>,
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

/// A first-level unit ranks just below a city of its own name that it holds (D-158): a search
/// for "Tokyo", "Berlin" or "Seoul" means the city first, then the region (the order of the
/// major geocoders); a region's base importance (90) is above a capital's (54 plus population).
/// A lower unit is left alone: a municipality or district is often the city itself, its
/// point source a duplicate (Nepal's golden set wants the municipality for "Damak" and
/// "Kamalamai"). A city is a settlement of rank [`CITY_RANK`] or more; "its own name" is a
/// shared exact search key; "holds" is the parent chain.
fn below_their_city(
    rows: &[&Place],
    exact: &BTreeMap<String, BTreeSet<u32>>,
    importance: &mut [u8],
) {
    let row_of: BTreeMap<&str, usize> = rows
        .iter()
        .enumerate()
        .map(|(i, p)| (p.key.as_str(), i))
        .collect();
    for set in exact.values().filter(|s| s.len() > 1) {
        for &s in set {
            let s = usize::try_from(s).unwrap_or(usize::MAX);
            let Some(city) = rows.get(s).filter(|p| {
                p.kind == PlaceKind::Settlement
                    && p.rank >= CITY_RANK
                    && p.flags & places::FLAG_FORMER == 0
            }) else {
                continue;
            };
            let below = importance.get(s).map_or(0, |i| i.saturating_sub(1));
            let mut up = city.parent.as_deref();
            // A parent chain is short; the bound only guards a cycle.
            for _ in 0..rows.len().min(64) {
                let Some(&a) = up.and_then(|k| row_of.get(k)) else {
                    break;
                };
                let unit = rows
                    .get(a)
                    .filter(|p| p.kind == PlaceKind::Admin && p.admin_level == 1);
                if unit.is_some()
                    && set.contains(&u32::try_from(a).unwrap_or(u32::MAX))
                    && let Some(i) = importance.get_mut(a)
                {
                    *i = (*i).min(below);
                }
                up = rows.get(a).and_then(|p| p.parent.as_deref());
            }
        }
    }
}

/// The settlement rank of a city (`sources::settlement_rank`: 3 a city or first-level seat, 4
/// a national capital).
const CITY_RANK: u8 = 3;

/// A settlement's importance over the base 30 by its rank (`sources::settlement_rank`): none,
/// village, town, city, national capital (D-124). Calibrated on the golden set: a city's record
/// (44) beats the municipality of its name (40 + population); a town or village earns nothing,
/// since lifting them pushed prefix and ambiguous queries onto small places.
const RANK_BONUS: [u8; 5] = [0, 0, 0, 14, 24];

/// The importance of every former unit: below a settlement with no population, above a locality.
const FORMER: u8 = 25;

pub(super) fn e6(p: (i32, i32)) -> Result<Point<E6>, String> {
    Point::<E7>::from_units(p.1, p.0)
        .map(Point::to_e6)
        .ok_or_else(|| format!("point {p:?} is off the grid"))
}

/// The PLACES section of the rows.
pub(super) fn places_section(rows: &[&Place], points: &[Point<E6>]) -> Result<Vec<u8>, String> {
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

/// Coded rows and their codes, in row order.
type Coded = Vec<(u32, IsoCode)>;

/// Gate G-iso on the rows, the stale-key check on the country pack only, and the `ISO_CODES`
/// section of their codes added to `sections` (D-197): none without codes. Returns the codes.
fn iso_section(
    input: &Input<'_>,
    rows: &[&Place],
    sections: &mut Vec<Body>,
) -> Result<Coded, String> {
    let Some(iso) = input.iso else {
        return Ok(Vec::new());
    };
    let codes = super::iso::gate(iso, rows, input.given.is_none())?;
    let body = teigeodata_format::iso_codes::encode(&codes).map_err(|e| e.to_string())?;
    let k = codes.len() as u64;
    sections.extend(body.map(|b| (Kind::ISO_CODES, Encoding::TABLE, k, b)));
    Ok(codes)
}

/// The timezone sections, when the pack has zone areas: REGION linked to the areas' rows,
/// each place's zone (`PLACE_TZ`) and the zones' rules (`TZ_RULES`, D-196).
fn zone_sections(
    input: &Input<'_>,
    row_of: &BTreeMap<&str, u32>,
    points: &[Point<E6>],
    sections: &mut Vec<Body>,
) -> Result<(), String> {
    if input.zones.rings.is_empty() {
        return Ok(());
    }
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
    if let Some(tz) = place_tz_section(&body, zone_rows.len(), points)? {
        sections.push(tz);
    }
    sections.push((Kind::REGION, Encoding::TABLE, zone_rows.len() as u64, body));
    sections.push(tz_rules_section(input, &zone_rows)?);
    Ok(())
}

/// `TZ_RULES` (D-196): the rules of each timezone area, linked to its PLACES row.
fn tz_rules_section(
    input: &Input<'_>,
    rows: &[u32],
) -> Result<(Kind, Encoding, u64, Vec<u8>), String> {
    let named: Vec<(&str, u32)> = input
        .zones
        .places
        .iter()
        .zip(rows)
        .map(|(p, &row)| {
            super::tz_rules::zone_name(&p.key)
                .map(|n| (n, row))
                .ok_or_else(|| format!("{}: not a zone key", p.key))
        })
        .collect::<Result<_, _>>()?;
    let compiled = input
        .tz_rules
        .ok_or("G-tzrules: a pack with timezone areas and no compiled rules")?;
    let body = super::tz_rules::section(compiled, &named)?;
    Ok((Kind::TZ_RULES, Encoding::TABLE, named.len() as u64, body))
}

/// The IDs a pack was written with, by place key, and its redirects: what a split build's
/// parts are given (D-191).
pub(super) struct Given {
    pub(super) ids: BTreeMap<String, PlaceId>,
    pub(super) moved: Vec<(PlaceId, Option<PlaceId>)>,
}

/// A written pack: its bytes, and the IDs it was written with.
pub(super) struct Built {
    pub(super) bytes: Vec<u8>,
    pub(super) given: Given,
}

pub(super) fn write(input: &Input<'_>) -> Result<(String, Built), String> {
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

    let (ids, moved, assigned) = ids_for(input, &rows)?;
    let idmap_body = idmap::encode(&ids).map_err(|e| e.to_string())?;

    let (names_body, search_body, importance) =
        name_sections(&rows, input.profile, &input.args.admin)?;
    let substring_body = substring_section(&rows, &importance)?;
    let index_body = place_index::encode(&points, NODE).map_err(|e| e.to_string())?;

    let mut sections: Vec<Body> = vec![
        (Kind::IDMAP, Encoding::TABLE, n as u64, idmap_body),
        (Kind::PLACES, Encoding::TABLE, n as u64, places_body),
        (Kind::HIERARCHY, Encoding::TABLE, n as u64, hierarchy_body),
        (Kind::NAMES, Encoding::TABLE, n as u64, names_body),
        (Kind::SEARCH, Encoding::TABLE, n as u64, search_body),
        (Kind::SUBSTRING, Encoding::TABLE, n as u64, substring_body),
        (Kind::PLACE_INDEX, Encoding::TABLE, n as u64, index_body),
    ];
    let iso_codes = iso_section(input, &rows, &mut sections)?;
    terrain(input, heights_body, n, &mut sections)?;
    zone_sections(input, &row_of, &points, &mut sections)?;
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
    verify_iso(&bytes, &iso_codes, &mut report)?;
    crate::make::write_atomic(input.file, &bytes)?;
    if let (Some(assigned), Some(file)) = (&assigned, input.ids_file) {
        crate::make::write_atomic(file, assigned.registry.render().as_bytes())?;
        ids_report(assigned, file, input, &mut report);
    }
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
    let given = Given {
        ids: rows.iter().map(|p| p.key.clone()).zip(ids).collect(),
        moved,
    };
    Ok((report, Built { bytes, given }))
}

/// The rows' IDs and the pack's redirects: given to a split build's part, else allocated
/// through the registry (whose outcome is returned).
#[allow(
    clippy::type_complexity,
    reason = "the IDs, the redirects and the registry outcome"
)]
fn ids_for(
    input: &Input<'_>,
    rows: &[&Place],
) -> Result<
    (
        Vec<PlaceId>,
        Vec<(PlaceId, Option<PlaceId>)>,
        Option<Assignment>,
    ),
    String,
> {
    if let Some(g) = input.given {
        let ids = rows
            .iter()
            .map(|p| {
                g.ids
                    .get(&p.key)
                    .copied()
                    .ok_or_else(|| format!("{}: no ID in the country pack", p.key))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok((ids, g.moved.clone(), None))
    } else {
        let (ids, moved, assigned) = allocate(input, rows)?;
        Ok((ids, moved, Some(assigned)))
    }
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

fn ids_report(a: &Assignment, ids_file: &Path, input: &Input<'_>, report: &mut String) {
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
        ids_file
            .strip_prefix(input.root)
            .unwrap_or(ids_file)
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
        Writer::new(input.address, "places", &input.args.release).map_err(|e| e.to_string())?;
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
    if let Some(r) = input.tz_rules.filter(|_| !input.zones.places.is_empty()) {
        w.manifest(Tag::TZDB, r.version.as_bytes())
            .map_err(|e| e.to_string())?;
    }
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

/// Adds the SEARCH keys of name `text` of row `row`: the one way every build keys a name
/// (packs, and a baseline slice rebuilding its keys from the names it keeps, D-152).
pub(super) fn add_keys(maps: &mut Maps, text: &str, row: u32, profile: &Profile) {
    search_keys(text, &profile.droppable, profile.phonetic, |m, k| {
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

/// The NAMES and SEARCH bodies, and each row's search importance.
pub(super) type NameBodies = (Vec<u8>, Vec<u8>, Vec<u8>);

/// The NAMES and SEARCH bodies of the rows, and each row's search importance.
pub(super) fn name_sections(
    rows: &[&Place],
    profile: &Profile,
    admin_source: &str,
) -> Result<NameBodies, String> {
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
            add_keys(&mut maps, &nm.text, row, profile);
        }
    }
    let mut importance: Vec<u8> = rows.iter().map(|p| importance(p)).collect();
    below_their_city(rows, &maps.exact, &mut importance);
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

    Ok((names_body, search_body, importance))
}

/// The SUBSTRING body of the rows (D-199): the word suffixes of every name the SEARCH keys
/// fold, with SEARCH's importance, so a query from the middle of a word ranks as search does.
/// Keys longer than the automaton allows are left out, as in SEARCH.
pub(super) fn substring_section(rows: &[&Place], importance: &[u8]) -> Result<Vec<u8>, String> {
    let mut keys: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
    for (row, p) in rows.iter().enumerate() {
        let row = u32::try_from(row).unwrap_or(u32::MAX);
        for nm in &p.names {
            let f = fold(&nm.text);
            substring_keys(&f, |k| {
                if k.len() < 255 {
                    match keys.get_mut(k) {
                        Some(set) => {
                            set.insert(row);
                        }
                        None => {
                            keys.insert(k.to_string(), BTreeSet::from([row]));
                        }
                    }
                }
            });
        }
    }
    search::encode_substring(&keys, importance).map_err(|e| e.to_string())
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
    let substring_line = verify_substring(&p, &search, rows)?;
    let geometry_line = match input.geometry {
        Some(g) => Some(super::geometry::verify(&p, rows, g)?),
        None => None,
    };
    let zones_ok = verify_zones(&p, rows, input)?;
    let rules_line = verify_tz_rules(&p, rows, input)?;
    let tz_line = verify_place_tz(&p, &places, rows, input.stated_tz, input.given.is_none())?;
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
    let _ = writeln!(report, "\n{substring_line}");
    if let Some(line) = tz_line {
        let _ = writeln!(report, "\n{line}");
    }
    if let Some(line) = rules_line {
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
            if let Some(s) = p.section(Kind::DEM) {
                let line = verify_dem(&s, &places, h.as_ref())?;
                let _ = writeln!(report, "\n{line}");
            }
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

/// SUBSTRING read back (D-199): `check`, SEARCH's importance row for row, and every place found
/// from the longest word suffix of its canonical name that is a key (a mid-word query).
fn verify_substring(p: &Pack<'_>, search: &Search<'_>, rows: &[&Place]) -> Result<String, String> {
    let e = |e: teigeodata_format::Error| e.to_string();
    let section = p.section(Kind::SUBSTRING).ok_or("no SUBSTRING section")?;
    let sub = Search::open(&section).map_err(e)?;
    sub.check().map_err(e)?;
    if sub.rows() != rows.len() {
        return Err("SUBSTRING: not one row per place".into());
    }
    let mut keyed = 0;
    for (row, place) in rows.iter().enumerate() {
        let r = u32::try_from(row).unwrap_or(u32::MAX);
        if sub.importance(r) != search.importance(r) {
            return Err(format!(
                "row {row}: SUBSTRING's importance differs from SEARCH's"
            ));
        }
        let first = place
            .names
            .first()
            .map(|n| fold(&n.text))
            .unwrap_or_default();
        let mut longest = None;
        substring_keys(&first, |k| {
            if k.len() < 255 && longest.is_none_or(|l: &str| k.len() > l.len()) {
                longest = Some(k);
            }
        });
        let Some(key) = longest else { continue };
        let mut hit = false;
        if let Some(o) = sub.get(search::SUBSTRING, key) {
            sub.rows_of(o, |x| {
                hit |= x == r;
                if hit {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            });
        }
        if !hit {
            return Err(format!(
                "{}: not found from {key:?} in SUBSTRING",
                place.key
            ));
        }
        keyed += 1;
    }
    let bytes = section.bytes().len();
    let tenths = bytes * 10 / rows.len().max(1);
    Ok(format!(
        "SUBSTRING (D-199): {} keys, {bytes} B ({}.{} B per place); all {keyed} places with a word suffix of 3 or more code points in their canonical name are found from its longest one.",
        sub.keys(),
        tenths / 10,
        tenths % 10
    ))
}

/// ELEVATION (one height per row) and, when the build samples a DEM, the DEM masked to the
/// pack's outline (D-192), added to `sections`.
fn terrain(
    input: &Input<'_>,
    heights: Option<Vec<u8>>,
    rows: usize,
    sections: &mut Vec<Body>,
) -> Result<(), String> {
    if let Some(body) = heights {
        sections.push((Kind::ELEVATION, Encoding::TABLE, rows as u64, body));
    }
    if let Some((body, tiles)) = input
        .elevation
        .map(|e| e.masked(input.outline, input.places))
        .transpose()?
        .flatten()
    {
        sections.push((Kind::DEM, Encoding::TABLE, tiles, body));
    }
    Ok(())
}

/// The pack's own DEM read back (D-192): `check`, then every row's height equals the masked
/// DEM at its stored point, so no place's terrain was masked away.
fn verify_dem(
    s: &teigeodata_format::Section<'_>,
    places: &Places<'_>,
    heights: Option<&Heights<'_>>,
) -> Result<String, String> {
    let dem = teigeodata_format::dem::Dem::open(s).map_err(|e| e.to_string())?;
    dem.check().map_err(|e| format!("the pack's DEM: {e}"))?;
    let line = super::elevation::verify(&dem, places, heights)
        .map_err(|e| format!("the pack's DEM: {e}"))?;
    Ok(format!(
        "DEM (D-192): {} tiles in its boxes, {} B, masked to the pack's outline and {}° around it. In it: {line}",
        dem.tiles(),
        s.bytes().len(),
        super::elevation::DEM_HALO_DEG
    ))
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

/// G-tzrules (D-196) on the written pack: a pack with timezone areas has `TZ_RULES` and the
/// manifest's `tzdb`, every area has its rules linked to its row and rebuilt as `zic` wrote
/// them, and the release is the one compiled; a pack without areas has neither.
fn verify_tz_rules(
    p: &Pack<'_>,
    rows: &[&Place],
    input: &Input<'_>,
) -> Result<Option<String>, String> {
    let tzdb = p.manifest().text(Tag::TZDB);
    let Some(s) = p.section(Kind::TZ_RULES) else {
        if input.zones.places.is_empty() && tzdb.is_none() {
            return Ok(None);
        }
        return Err("G-tzrules: timezone areas without TZ_RULES, or a tzdb tag without it".into());
    };
    let compiled = input
        .tz_rules
        .ok_or("G-tzrules: TZ_RULES with nothing compiled")?;
    let row_of: BTreeMap<&str, u32> = rows
        .iter()
        .enumerate()
        .map(|(r, p)| (p.key.as_str(), u32::try_from(r).unwrap_or(u32::MAX)))
        .collect();
    let zones: Vec<(&str, u32)> = input
        .zones
        .places
        .iter()
        .map(|z| {
            let name = super::tz_rules::zone_name(&z.key).ok_or("a zone key")?;
            let row = row_of
                .get(z.key.as_str())
                .copied()
                .ok_or("a zone has no row")?;
            Ok((name, row))
        })
        .collect::<Result<_, &str>>()?;
    let rules = teigeodata_format::tz_rules::TzRules::open(&s).map_err(|e| e.to_string())?;
    super::tz_rules::check(&rules, tzdb, &zones, compiled)?;
    Ok(Some(format!(
        "G-tzrules (D-196): all {} timezone areas have rules from tzdb {} ({} sets, {} transitions, {} B), each linked to its row and rebuilt byte for byte as `zic -b slim` wrote it.",
        zones.len(),
        compiled.version,
        rules.sets(),
        rules.transitions(),
        s.bytes().len()
    )))
}

/// The most source records whose stated timezone may differ from their place's (G-tz,
/// D-138), as a share of the records compared.
const TZ_DIFFER_MAX: f64 = 0.001;

/// `ISO_CODES` read back (D-197): `check`, and every coded row reads back its code and no other
/// row has one.
fn verify_iso(bytes: &[u8], want: &[(u32, IsoCode)], report: &mut String) -> Result<(), String> {
    let err = |e: teigeodata_format::Error| e.to_string();
    let pack = Pack::open(bytes).map_err(err)?;
    let places = Places::open(&pack.section(Kind::PLACES).ok_or("no PLACES")?)
        .map_err(err)?
        .len();
    let Some(section) = pack.section(Kind::ISO_CODES) else {
        return if want.is_empty() {
            Ok(())
        } else {
            Err("no ISO_CODES section".into())
        };
    };
    let t = IsoCodes::open(&section, places).map_err(err)?;
    t.check().map_err(err)?;
    if !t.iter().eq(want.iter().copied()) {
        return Err("ISO_CODES differs from the codes written".into());
    }
    for &(row, code) in want {
        if t.get(row) != Some(code) {
            return Err(format!("row {row}: ISO code {code} does not read back"));
        }
    }
    let countries = want.iter().filter(|c| c.1.is_country()).count();
    let _ = writeln!(
        report,
        "\nISO 3166 codes (G-iso, D-197): {} rows coded ({countries} country, {} units), each read back; {} bytes.",
        want.len(),
        want.len() - countries,
        section.bytes().len()
    );
    Ok(())
}

/// `PLACE_TZ` read back (D-134): `check`, one row per place, and every place's area is the
/// area REGION holds at the place's point as read back from PLACES. No REGION, no `PLACE_TZ`.
/// G-tz (D-138): each source record that states a timezone is compared with its place's;
/// at most [`TZ_DIFFER_MAX`] of them may differ. G-tz is a gate on the country's records, so
/// it fails only the country pack (`gate`): a split build's part (D-191) holds a subset of the
/// same records, reports its share, and is proved equal to the country pack, timezone areas
/// included, by `split::check`.
fn verify_place_tz(
    p: &Pack<'_>,
    places: &Places<'_>,
    rows: &[&Place],
    stated: &BTreeMap<String, String>,
    gate: bool,
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
    if share > TZ_DIFFER_MAX && gate {
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
pub(super) fn verify_rows(
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
    use super::{FORMER, below_their_city, importance};
    use crate::build::Place;
    use std::collections::{BTreeMap, BTreeSet};
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
    fn a_unit_ranks_below_the_city_of_its_name_inside_it() {
        // Region "Tokyo" holds district "Shinjuku", which holds the capital "Tokyo"; region
        // "Kyoto" holds a town "Kyoto"; region "Osaka" does not hold the city "Osaka";
        // municipality "Damak" holds the city "Damak" (a lower unit is left alone).
        let keyed = |key: &str, mut p: Place, parent: Option<&str>| {
            p.key = key.into();
            p.parent = parent.map(Into::into);
            p
        };
        let rows = [
            keyed("r:tokyo", place(PlaceKind::Admin, 1, 0, None), None),
            keyed(
                "d:shinjuku",
                place(PlaceKind::Admin, 2, 0, None),
                Some("r:tokyo"),
            ),
            keyed(
                "c:tokyo",
                place(PlaceKind::Settlement, 0, 4, Some(8_000_000)),
                Some("d:shinjuku"),
            ),
            keyed("r:kyoto", place(PlaceKind::Admin, 1, 0, None), None),
            keyed(
                "c:kyoto",
                place(PlaceKind::Settlement, 0, 2, Some(1_400_000)),
                Some("r:kyoto"),
            ),
            keyed("r:osaka", place(PlaceKind::Admin, 1, 0, None), None),
            keyed(
                "m:damak",
                place(PlaceKind::Admin, 3, 0, Some(1_000_000)),
                None,
            ),
            keyed(
                "c:damak",
                place(PlaceKind::Settlement, 0, 3, Some(75_000)),
                Some("m:damak"),
            ),
            keyed(
                "c:osaka",
                place(PlaceKind::Settlement, 0, 3, Some(2_700_000)),
                None,
            ),
        ];
        let refs: Vec<&Place> = rows.iter().collect();
        let exact: BTreeMap<String, BTreeSet<u32>> = [
            ("tokyo", vec![0, 2]),
            ("shinjuku", vec![1]),
            ("kyoto", vec![3, 4]),
            ("damak", vec![6, 7]),
            ("osaka", vec![5, 8]),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.into_iter().collect()))
        .collect();
        let before: Vec<u8> = refs.iter().map(|p| importance(p)).collect();
        assert!(before[0] > before[2], "the region first without the rule");
        assert!(
            before[6] > before[7],
            "the municipality first without the rule"
        );
        let mut after = before.clone();
        below_their_city(&refs, &exact, &mut after);
        assert_eq!(after[0], before[2] - 1, "the region just below its capital");
        assert_eq!(
            after[1..],
            before[1..],
            "the rest unchanged (a town, a city outside, a municipality)"
        );
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
