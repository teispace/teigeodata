//! Test packs for the engine: a model of a pack's places that can be read from any pack, cut
//! into parts by row and written again with the format's writer, and a synthetic model of a
//! small country.

#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::missing_panics_doc,
    clippy::too_many_lines,
    clippy::similar_names,
    clippy::items_after_statements,
    clippy::manual_midpoint,
    unreachable_pub,
    reason = "test support: failures panic with their message; values are small"
)]

use std::collections::{BTreeMap, BTreeSet};

use teigeodata_core::{E6, E7, IsoCode, Namespace, PlaceId, PlaceKind, Point, hilbert};
use teigeodata_format::arcs::Arcs;
use teigeodata_format::elevation::{self, Elevations};
use teigeodata_format::hierarchy::{self, Hierarchy};
use teigeodata_format::idmap::{self, IdMap};
use teigeodata_format::iso_codes::{self, IsoCodes};
use teigeodata_format::names::{self, NameRecord, NameType, Names, Tag as NameTag};
use teigeodata_format::place_index::{self, PlaceIndex};
use teigeodata_format::place_tz;
use teigeodata_format::places::{self, PlaceColumns, Places};
use teigeodata_format::polygons::Polygons;
use teigeodata_format::redirects::{self, Fate, Redirects};
use teigeodata_format::region::{self, Options, Region};
use teigeodata_format::search::{self, EXACT, LOOSE, Search};
use teigeodata_format::{Encoding, Kind, Pack, Section, Tag, Writer};

/// A name record, owned.
#[derive(Clone, Debug)]
pub struct OwnedName {
    pub text: String,
    pub lang: Option<NameTag>,
    pub script: Option<NameTag>,
    pub kind: Option<NameType>,
    pub valid: (Option<i32>, Option<i32>),
    pub rule: u32,
}

/// A region index: its body and the PLACES row of each region.
#[derive(Clone, Debug)]
pub struct RegionModel {
    pub body: Vec<u8>,
    pub rows: Vec<u32>,
}

/// Everything the engine reads from a pack, by row.
#[derive(Clone, Debug)]
pub struct Model {
    pub address: String,
    pub component: String,
    pub release: String,
    pub bbox: Option<Vec<u8>>,
    pub ids: Vec<PlaceId>,
    pub points: Vec<Point<E6>>,
    pub kinds: Vec<PlaceKind>,
    pub levels: Vec<u8>,
    pub classes: Vec<u32>,
    pub flags: Vec<u32>,
    pub elevations: Vec<Option<i16>>,
    pub names: Vec<Vec<OwnedName>>,
    /// Tagged search keys and their rows; empty for no SEARCH.
    pub keys: BTreeMap<Vec<u8>, BTreeSet<u32>>,
    pub importance: Vec<u8>,
    /// SEARCH's query rules, encoded, and each row's unit type (empty for none).
    pub rules: Option<Vec<u8>>,
    pub units: Vec<u8>,
    pub node: usize,
    pub redirects: Vec<(PlaceId, Option<PlaceId>)>,
    pub zones: Option<RegionModel>,
    pub admin: Option<RegionModel>,
    /// ARCS (body, item count), when the admin index reads its vertices from it.
    pub arcs: Option<(Vec<u8>, u64)>,
    /// Each row's parent row (HIERARCHY); empty for none.
    pub parents: Vec<Option<u32>>,
    /// The coded rows and their ISO 3166 codes (`ISO_CODES`), increasing; empty for none.
    pub iso: Vec<(u32, IsoCode)>,
    /// Sections written as they are (POLYGONS, `POLY_INDEX`): their rows are this model's,
    /// so a subset drops them.
    pub raw: Vec<(Kind, u64, Vec<u8>)>,
}

impl Model {
    /// Reads a model from a pack.
    pub fn from_pack(bytes: &[u8]) -> Self {
        let pack = Pack::open(bytes).unwrap();
        let m = pack.manifest();
        let sec = |k| pack.section(k).unwrap();
        let ids = IdMap::open(&sec(Kind::IDMAP)).unwrap();
        let places = Places::open(&sec(Kind::PLACES)).unwrap();
        let n = places.len();
        let names = pack.section(Kind::NAMES).map(|s| Names::open(&s).unwrap());
        let search = pack
            .section(Kind::SEARCH)
            .map(|s| Search::open(&s).unwrap());
        let node = pack
            .section(Kind::PLACE_INDEX)
            .map_or(16, |s| PlaceIndex::open(&s, places).unwrap().node_size());
        let mut keys = BTreeMap::new();
        let mut importance = Vec::new();
        let (mut rules, mut units) = (None, Vec::new());
        if let Some(s) = &search {
            let read: Vec<_> = s.rules().iter().collect();
            if !read.is_empty() {
                rules = Some(search::rules::encode(&read).unwrap());
            }
            units = (0..n as u32).map(|r| s.unit(r)).collect();
            if units.iter().all(|&u| u == 0) {
                units.clear();
            }
            let mut buf = [0_u8; search::fst::MAX_KEY];
            for o in 0..s.keys() {
                let len = s.automaton().key(o, &mut buf).unwrap();
                let mut rows = BTreeSet::new();
                s.rows_of(o, |r| {
                    rows.insert(r);
                    core::ops::ControlFlow::Continue(())
                });
                keys.insert(buf[..len].to_vec(), rows);
            }
            importance = (0..n as u32).map(|r| s.importance(r)).collect();
        }
        let redirects = pack
            .section(Kind::REDIRECTS)
            .map(|s| {
                Redirects::open(&s)
                    .unwrap()
                    .iter()
                    .map(|(old, fate)| {
                        (
                            old,
                            match fate {
                                Fate::Redirect(new) => Some(new),
                                Fate::Tombstone => None,
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let heights = pack
            .section(Kind::ELEVATION)
            .map(|s| Elevations::open(&s).unwrap());
        let arcs_view = pack.section(Kind::ARCS).map(|s| Arcs::open(&s).unwrap());
        let polygons = pack
            .section(Kind::POLYGONS)
            .map(|s| Polygons::open(&s).unwrap());
        let region_of = |kind: Kind| {
            let s = pack.section(kind)?;
            let r = match &arcs_view {
                Some(a) => Region::open_on_arcs(&s, a).unwrap(),
                None => Region::open(&s).unwrap(),
            };
            let rows = (0..r.regions() as u32)
                .map(|z| {
                    r.place(z)
                        .or_else(|| polygons.as_ref().and_then(|p| p.row(z as usize)))
                        .unwrap()
                })
                .collect();
            Some(RegionModel {
                body: s.bytes().to_vec(),
                rows,
            })
        };
        let mut buf = vec![0_u8; 4096];
        Self {
            address: m.address().into(),
            component: m.component().into(),
            release: m.release().into(),
            bbox: m.get(Tag::BBOX).map(<[u8]>::to_vec),
            ids: (0..n).map(|r| ids.id(r).unwrap()).collect(),
            points: (0..n).map(|r| places.point(r).unwrap()).collect(),
            kinds: (0..n).map(|r| places.kind(r).unwrap()).collect(),
            levels: (0..n).map(|r| places.admin_level(r).unwrap()).collect(),
            classes: (0..n).map(|r| places.class(r).unwrap()).collect(),
            flags: (0..n).map(|r| places.flags(r).unwrap()).collect(),
            elevations: (0..n).map(|r| heights.and_then(|h| h.get(r))).collect(),
            names: names.map_or_else(Vec::new, |nm| {
                (0..n)
                    .map(|r| {
                        nm.names(r)
                            .map(|x| OwnedName {
                                text: x.text(&mut buf).unwrap().to_owned(),
                                lang: x.lang(),
                                script: x.script(),
                                kind: x.kind(),
                                valid: x.valid(),
                                rule: x.rule(),
                            })
                            .collect()
                    })
                    .collect()
            }),
            keys,
            importance,
            rules,
            units,
            node,
            redirects,
            zones: region_of(Kind::REGION),
            admin: region_of(Kind::ADMIN_REGION),
            arcs: pack
                .section(Kind::ARCS)
                .map(|s| (s.bytes().to_vec(), s.item_count())),
            parents: pack
                .section(Kind::HIERARCHY)
                .map(|s| {
                    let h = Hierarchy::open(&s).unwrap();
                    (0..n).map(|r| h.parent(r).map(|p| p as u32)).collect()
                })
                .unwrap_or_default(),
            iso: pack
                .section(Kind::ISO_CODES)
                .map(|s| IsoCodes::open(&s, n).unwrap().iter().collect())
                .unwrap_or_default(),
            raw: [Kind::POLYGONS, Kind::POLY_INDEX]
                .into_iter()
                .filter_map(|k| {
                    let s = pack.section(k)?;
                    Some((k, s.item_count(), s.bytes().to_vec()))
                })
                .collect(),
        }
    }

    /// Adds SUBSTRING (D-199) cut from the model's names as the forge cuts it, with SEARCH's
    /// importance, to the sections written as they are (a subset or split drops it).
    pub fn with_substring(&mut self) {
        let mut keys: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
        for (row, names) in self.names.iter().enumerate() {
            for n in names {
                let f = teigeodata_names::fold(&n.text);
                teigeodata_names::substring_keys(&f, |k| {
                    if k.len() < 255 {
                        keys.entry(k.to_string()).or_default().insert(row as u32);
                    }
                });
            }
        }
        let body = search::encode_substring(&keys, &self.importance).unwrap();
        self.raw.retain(|r| r.0 != Kind::SUBSTRING);
        self.raw
            .push((Kind::SUBSTRING, self.ids.len() as u64, body));
    }

    /// The rows every part must hold: the places the region indexes link to.
    pub fn linked_rows(&self) -> BTreeSet<u32> {
        self.zones
            .iter()
            .chain(&self.admin)
            .flat_map(|z| z.rows.iter().copied())
            .collect()
    }

    /// The model of rows `keep` (increasing) under a new address; redirects whose survivor is
    /// kept, and the tombstones `tomb` selects.
    pub fn subset(&self, address: &str, keep: &[u32], tomb: impl Fn(PlaceId) -> bool) -> Self {
        let mut new_of = vec![u32::MAX; self.ids.len()];
        for (new, &old) in keep.iter().enumerate() {
            new_of[old as usize] = new as u32;
        }
        let pick = |v: &[u8]| keep.iter().map(|&r| v[r as usize]).collect::<Vec<_>>();
        let held: BTreeSet<PlaceId> = keep.iter().map(|&r| self.ids[r as usize]).collect();
        let keys = self
            .keys
            .iter()
            .filter_map(|(k, rows)| {
                let rows: BTreeSet<u32> = rows
                    .iter()
                    .map(|&r| new_of[r as usize])
                    .filter(|&r| r != u32::MAX)
                    .collect();
                (!rows.is_empty()).then(|| (k.clone(), rows))
            })
            .collect();
        let relink = |z: &Option<RegionModel>| {
            z.as_ref().map(|z| RegionModel {
                body: z.body.clone(),
                rows: z.rows.iter().map(|&r| new_of[r as usize]).collect(),
            })
        };
        Self {
            address: address.into(),
            component: self.component.clone(),
            release: self.release.clone(),
            bbox: self.bbox.clone(),
            ids: keep.iter().map(|&r| self.ids[r as usize]).collect(),
            points: keep.iter().map(|&r| self.points[r as usize]).collect(),
            kinds: keep.iter().map(|&r| self.kinds[r as usize]).collect(),
            levels: pick(&self.levels),
            classes: keep.iter().map(|&r| self.classes[r as usize]).collect(),
            flags: keep.iter().map(|&r| self.flags[r as usize]).collect(),
            elevations: keep.iter().map(|&r| self.elevations[r as usize]).collect(),
            names: if self.names.is_empty() {
                Vec::new()
            } else {
                keep.iter()
                    .map(|&r| self.names[r as usize].clone())
                    .collect()
            },
            keys,
            importance: if self.importance.is_empty() {
                Vec::new()
            } else {
                pick(&self.importance)
            },
            rules: self.rules.clone(),
            units: if self.units.is_empty() {
                Vec::new()
            } else {
                pick(&self.units)
            },
            node: self.node,
            redirects: self
                .redirects
                .iter()
                .copied()
                .filter(|&(old, new)| match new {
                    Some(s) => held.contains(&s) && tomb(s),
                    None => tomb(old),
                })
                .collect(),
            zones: relink(&self.zones),
            admin: relink(&self.admin),
            arcs: self.arcs.clone(),
            // A parent outside the kept rows leaves a root: the part holds no stub for it.
            parents: if self.parents.is_empty() {
                Vec::new()
            } else {
                keep.iter()
                    .map(|&r| {
                        self.parents[r as usize]
                            .map(|q| new_of[q as usize])
                            .filter(|&q| q != u32::MAX)
                    })
                    .collect()
            },
            iso: self
                .iso
                .iter()
                .filter(|&&(r, _)| new_of[r as usize] != u32::MAX)
                .map(|&(r, c)| (new_of[r as usize], c))
                .collect(),
            raw: Vec::new(),
        }
    }

    /// Writes the pack.
    pub fn encode(&self) -> Vec<u8> {
        let n = self.ids.len() as u64;
        let mut bodies: Vec<(Kind, u64, Vec<u8>)> = vec![
            (Kind::IDMAP, n, idmap::encode(&self.ids).unwrap()),
            (
                Kind::PLACES,
                n,
                places::encode(&PlaceColumns {
                    points: &self.points,
                    kinds: &self.kinds,
                    admin_levels: &self.levels,
                    classes: &self.classes,
                    flags: &self.flags,
                })
                .unwrap(),
            ),
            (
                Kind::PLACE_INDEX,
                n,
                place_index::encode(&self.points, self.node).unwrap(),
            ),
        ];
        if let Some(h) = elevation::encode(&self.elevations).unwrap() {
            bodies.push((Kind::ELEVATION, n, h));
        }
        if !self.names.is_empty() {
            let recs: Vec<Vec<NameRecord<'_>>> = self
                .names
                .iter()
                .map(|list| {
                    list.iter()
                        .map(|x| NameRecord {
                            text: &x.text,
                            lang: x.lang,
                            script: x.script,
                            kind: x.kind,
                            valid_from: x.valid.0,
                            valid_to: x.valid.1,
                            rule: x.rule,
                        })
                        .collect()
                })
                .collect();
            let refs: Vec<&[NameRecord<'_>]> = recs.iter().map(Vec::as_slice).collect();
            bodies.push((Kind::NAMES, n, names::encode(&refs).unwrap()));
        }
        if !self.importance.is_empty() {
            bodies.push((Kind::SEARCH, n, {
                let rules: Option<Vec<_>> = self
                    .rules
                    .as_deref()
                    .map(|r| search::rules::Rules::new(r).iter().collect());
                let extra = search::Extra {
                    rules: rules.as_deref(),
                    units: &self.units,
                };
                search::encode_with(&self.keys, &self.importance, &extra).unwrap()
            }));
        }
        if let Some(z) = &self.zones {
            let body = region::link(&z.body, &z.rows).unwrap();
            let items = z.rows.len() as u64;
            let zones = Region::open(&Section::unpacked(Kind::REGION, items, &body)).unwrap();
            if let Some(tz) = place_tz::encode_from(&zones, &self.points).unwrap() {
                bodies.push((Kind::PLACE_TZ, n, tz));
            }
            bodies.push((Kind::REGION, items, body));
        }
        if let Some((body, items)) = &self.arcs {
            bodies.push((Kind::ARCS, *items, body.clone()));
        }
        if let Some(z) = &self.admin {
            let body = region::link(&z.body, &z.rows).unwrap();
            bodies.push((Kind::ADMIN_REGION, z.rows.len() as u64, body));
        }
        if !self.parents.is_empty() {
            bodies.push((
                Kind::HIERARCHY,
                n,
                hierarchy::encode(&self.parents).unwrap(),
            ));
        }
        if !self.redirects.is_empty() {
            bodies.push((
                Kind::REDIRECTS,
                self.redirects.len() as u64,
                redirects::encode(&self.redirects).unwrap(),
            ));
        }
        if let Some(b) = iso_codes::encode(&self.iso).unwrap() {
            bodies.push((Kind::ISO_CODES, self.iso.len() as u64, b));
        }
        bodies.extend(self.raw.iter().cloned());
        let mut w = Writer::new(&self.address, &self.component, &self.release).unwrap();
        if let Some(b) = &self.bbox {
            w.manifest(Tag::BBOX, b).unwrap();
        }
        let sections: Vec<(Kind, Encoding, u64, &[u8])> = bodies
            .iter()
            .map(|(k, items, b)| (*k, Encoding::TABLE, *items, b.as_slice()))
            .collect();
        w.pack(&sections, None).unwrap()
    }

    /// Cuts the model into `parts` packs: row `r` goes to `part(r)`, every linked row to all
    /// of them, and every part holds the ancestors of its rows (stubs, D-143); redirects go
    /// with their survivor, tombstones by `part` of the retired ID.
    pub fn split(&self, parts: usize, part: impl Fn(u32, PlaceId) -> usize) -> Vec<Vec<u8>> {
        let linked = self.linked_rows();
        let row_of: BTreeMap<PlaceId, u32> =
            (0_u32..).zip(&self.ids).map(|(r, &id)| (id, r)).collect();
        (0..parts)
            .map(|p| {
                let mut keep: BTreeSet<u32> = (0..self.ids.len() as u32)
                    .filter(|&r| linked.contains(&r) || part(r, self.ids[r as usize]) == p)
                    .collect();
                for r in keep.clone() {
                    let mut at = r;
                    while let Some(q) = self.parents.get(at as usize).copied().flatten() {
                        if !keep.insert(q) {
                            break;
                        }
                        at = q;
                    }
                }
                let keep: Vec<u32> = keep.into_iter().collect();
                let owner = |id: PlaceId| {
                    row_of.get(&id).map_or(part(u32::MAX, id), |&r| {
                        if linked.contains(&r) { 0 } else { part(r, id) }
                    }) == p
                };
                self.subset(&format!("{}-p{p}", self.address), &keep, owner)
                    .encode()
            })
            .collect()
    }
}

/// A xorshift generator: deterministic test data.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

const SYLLABLES: &[&str] = &[
    "ka", "ma", "ra", "pur", "gaun", "kot", "tar", "bas", "dhara", "pani", "lek", "deu", "ri",
    "chowk", "khola", "ban", "shi", "na", "la", "ta",
];

/// A random name of two or three syllables, sometimes two words.
pub fn name(r: &mut Rng) -> String {
    let mut s = String::new();
    for i in 0..2 + r.below(2) {
        if i == 2 && r.below(3) == 0 {
            s.push(' ');
        }
        s.push_str(SYLLABLES[r.below(SYLLABLES.len() as u64) as usize]);
    }
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

/// The country box of the synthetic model, on the 1e-7° grid: lat 26–30°, lon 80–88°.
pub const SOUTH: i32 = 260_000_000;
pub const NORTH: i32 = 300_000_000;
pub const WEST: i32 = 800_000_000;
pub const EAST: i32 = 880_000_000;

fn rect(w: i32, s: i32, e: i32, n: i32) -> Vec<Vec<[i32; 2]>> {
    vec![vec![[w, s], [e, s], [e, n], [w, n]]]
}

/// A synthetic country of `n` settlements: a country unit and 4 × 2 districts as admin
/// regions, two timezone areas (west and east), names with repeats, and some redirects and
/// tombstones.
pub fn synthetic(n: usize, seed: u64) -> Model {
    let mut r = Rng(seed | 1);
    let ns = Namespace::from_country(*b"np").unwrap();
    #[derive(Clone)]
    struct Row {
        id: PlaceId,
        point: Point<E6>,
        kind: PlaceKind,
        level: u8,
        names: Vec<String>,
        importance: u8,
        /// The parent, by index before the Hilbert sort.
        parent: Option<usize>,
    }
    let mut serials = BTreeSet::new();
    let mut fresh = |r: &mut Rng| loop {
        let s = 1 + r.below(1 << 30);
        if serials.insert(s) {
            return PlaceId::new(ns, s).unwrap();
        }
    };
    let e6 = |lat: i32, lon: i32| Point::<E7>::from_units(lat, lon).unwrap().to_e6();
    let mut rows: Vec<Row> = Vec::new();
    // The country, the districts and the zones, at their centres.
    rows.push(Row {
        id: fresh(&mut r),
        point: e6((SOUTH + NORTH) / 2, (WEST + EAST) / 2),
        kind: PlaceKind::Country,
        level: 0,
        names: vec!["Synthland".into()],
        importance: 250,
        parent: None,
    });
    let (cw, ch) = ((EAST - WEST) / 4, (NORTH - SOUTH) / 2);
    let mut districts = Vec::new();
    for j in 0..2 {
        for i in 0..4 {
            let (w, s) = (WEST + i * cw, SOUTH + j * ch);
            districts.push(rect(w, s, w + cw, s + ch));
            rows.push(Row {
                id: fresh(&mut r),
                point: e6(s + ch / 2, w + cw / 2),
                kind: PlaceKind::Admin,
                level: 1,
                names: vec![format!("{} District", name(&mut r))],
                importance: 200,
                parent: Some(0),
            });
        }
    }
    let mid = (WEST + EAST) / 2;
    let zone_rects = [rect(WEST, SOUTH, mid, NORTH), rect(mid, SOUTH, EAST, NORTH)];
    for z in 0..2 {
        rows.push(Row {
            id: fresh(&mut r),
            point: e6(
                (SOUTH + NORTH) / 2,
                if z == 0 { WEST + cw } else { EAST - cw },
            ),
            kind: PlaceKind::TimezoneArea,
            level: 0,
            names: vec![format!("Zone {z}")],
            importance: 0,
            parent: None,
        });
    }
    let common: Vec<String> = (0..=n / 20).map(|_| name(&mut r)).collect();
    for _ in 0..n {
        let lat = SOUTH + 1 + r.below((NORTH - SOUTH - 2) as u64) as i32;
        let lon = WEST + 1 + r.below((EAST - WEST - 2) as u64) as i32;
        let first = if r.below(4) == 0 {
            common[r.below(common.len() as u64) as usize].clone()
        } else {
            name(&mut r)
        };
        let mut names = vec![first];
        if r.below(5) == 0 {
            names.push(name(&mut r));
        }
        rows.push(Row {
            id: fresh(&mut r),
            point: e6(lat, lon),
            kind: PlaceKind::Settlement,
            level: 0,
            names,
            importance: r.below(130) as u8,
            // Its district: row 1 + 4 × band + column, as the districts were pushed.
            parent: Some(1 + (4 * ((lat - SOUTH) / ch) + (lon - WEST) / cw) as usize),
        });
    }
    // Rows in Hilbert order, as PLACES requires; ties by ID.
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by_key(|&i| (hilbert(rows[i].point), rows[i].id));
    let new_of: BTreeMap<usize, u32> = order
        .iter()
        .enumerate()
        .map(|(new, &old)| (old, new as u32))
        .collect();
    let rows: Vec<Row> = order.iter().map(|&i| rows[i].clone()).collect();
    let mut keys: BTreeMap<Vec<u8>, BTreeSet<u32>> = BTreeMap::new();
    for (row, p) in rows.iter().enumerate() {
        for nm in &p.names {
            let f = teigeodata_names::fold(nm);
            for (tag, text) in [(EXACT, f.clone()), (LOOSE, f.replace(' ', ""))] {
                let mut k = vec![tag];
                k.extend_from_slice(text.as_bytes());
                keys.entry(k).or_default().insert(row as u32);
            }
        }
    }
    // A loose key keeps only the rows its exact twin lacks (D-086), as the forge writes it.
    let exact: BTreeMap<Vec<u8>, BTreeSet<u32>> = keys
        .iter()
        .filter(|(k, _)| k[0] == EXACT)
        .map(|(k, v)| (k[1..].to_vec(), v.clone()))
        .collect();
    keys.retain(|k, rows| {
        if k[0] == LOOSE
            && let Some(twin) = exact.get(&k[1..])
        {
            rows.retain(|r| !twin.contains(r));
        }
        !rows.is_empty()
    });
    let ids: Vec<PlaceId> = rows.iter().map(|p| p.id).collect();
    let live: Vec<PlaceId> = ids.clone();
    let mut redirects = Vec::new();
    for i in 0..(n / 25 + 2) {
        let old = fresh(&mut r);
        let new = (i % 3 != 0).then(|| live[r.below(live.len() as u64) as usize]);
        redirects.push((old, new));
    }
    let country_row = new_of[&0];
    let district_rows: Vec<u32> = (1..=8).map(|d| new_of[&d]).collect();
    let zone_rows = vec![new_of[&9], new_of[&10]];
    let mut admin_regions = vec![rect(WEST, SOUTH, EAST, NORTH)];
    admin_regions.extend(districts);
    let mut admin_rows = vec![country_row];
    admin_rows.extend(district_rows);
    let bbox = teigeodata_format::Bbox {
        min: Point::<E7>::from_units(SOUTH, WEST).unwrap(),
        max: Point::<E7>::from_units(NORTH, EAST).unwrap(),
    };
    Model {
        address: "np".into(),
        component: "places".into(),
        release: "2026.11.0".into(),
        bbox: Some(bbox.to_bytes().to_vec()),
        points: rows.iter().map(|p| p.point).collect(),
        kinds: rows.iter().map(|p| p.kind).collect(),
        levels: rows.iter().map(|p| p.level).collect(),
        classes: vec![0; rows.len()],
        flags: vec![0; rows.len()],
        elevations: rows
            .iter()
            .map(|p| (p.kind == PlaceKind::Settlement).then_some(100 + (p.id.get() % 4000) as i16))
            .collect(),
        names: rows
            .iter()
            .map(|p| {
                p.names
                    .iter()
                    .map(|t| OwnedName {
                        text: t.clone(),
                        lang: None,
                        script: None,
                        kind: None,
                        valid: (None, None),
                        rule: 0,
                    })
                    .collect()
            })
            .collect(),
        importance: rows.iter().map(|p| p.importance).collect(),
        rules: None,
        units: Vec::new(),
        keys,
        ids,
        node: 8,
        redirects,
        zones: Some(RegionModel {
            body: region::encode(&zone_rects, &Options::default()).unwrap(),
            rows: zone_rows,
        }),
        admin: Some(RegionModel {
            body: region::encode(&admin_regions, &Options::default()).unwrap(),
            rows: admin_rows,
        }),
        arcs: None,
        raw: Vec::new(),
        iso: Vec::new(),
        parents: rows.iter().map(|p| p.parent.map(|q| new_of[&q])).collect(),
    }
}
