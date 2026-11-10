//! The world part's timezones (D-151): a zone per country, unit and city, from tzdb's
//! `zone.tab` and the city list's own `timezone` column, with no timezone geometry. The engine
//! answers `tz_at` on such a layer by the zone of the deepest admin unit at the point, and
//! `tz_of` by the place's own zone (`PLACE_TZ`).
//!
//! - **A country** with one row in `zone.tab` takes it; with several, the zone of most of its
//!   cities' people; else its first row; a feature without a code (a disputed area), its
//!   cities' zone, else the nearest city's.
//! - **A unit** takes the zone of most of its cities' people. A unit without a city takes its
//!   country's zone when the country has one row, else the zone of the nearest city of its
//!   country, else its country's.
//! - **A city** keeps the zone the list states for it, else its parent's.
//!
//! Every zone is a `zone.tab` ID: a stated ID that tzdb's `backward` file makes a link is
//! replaced by its target, and an ID tzdb does not know fails the build. Ties go to the
//! smallest ID, so the build is deterministic.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use teigeodata_core::PlaceKind;

/// One place for the assignment.
pub(super) struct Item<'a> {
    pub(super) key: &'a str,
    pub(super) kind: PlaceKind,
    pub(super) parent: Option<&'a str>,
    /// A country's ISO 3166-1 code, lower case.
    pub(super) iso2: Option<&'a str>,
    /// (lon, lat) on the 1e-7° grid.
    pub(super) point: (i32, i32),
    pub(super) population: Option<i64>,
    /// The zone the source states for it (a city's `timezone`).
    pub(super) stated: Option<&'a str>,
}

/// tzdb's `zone.tab`: each country's zones in file order, each zone's point, and the links of
/// `backward`.
pub(super) struct Tzdb {
    by_cc: BTreeMap<String, Vec<String>>,
    point: BTreeMap<String, (i32, i32)>,
    links: BTreeMap<String, String>,
}

impl Tzdb {
    /// From the text of `zone.tab` and `backward`.
    pub(super) fn parse(zone_tab: &str, backward: &str) -> Result<Self, String> {
        let mut t = Self {
            by_cc: BTreeMap::new(),
            point: BTreeMap::new(),
            links: BTreeMap::new(),
        };
        for line in zone_tab.lines().filter(|l| !l.starts_with('#')) {
            let mut f = line.split('\t');
            let (Some(cc), Some(at), Some(id)) = (f.next(), f.next(), f.next()) else {
                continue;
            };
            let p = iso6709(at).ok_or_else(|| format!("zone.tab: {id}: bad point {at}"))?;
            t.by_cc
                .entry(cc.to_ascii_lowercase())
                .or_default()
                .push(id.to_string());
            t.point.insert(id.to_string(), p);
        }
        for line in backward.lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            if let ["Link", target, name, ..] = f[..] {
                t.links.insert(name.to_string(), target.to_string());
            }
        }
        if t.point.is_empty() {
            return Err("zone.tab: no zones".into());
        }
        Ok(t)
    }

    /// The `zone.tab` ID of `id`, through `backward`'s links (a link may point to a zone that
    /// `zone.tab` lists under another country's row; it is still that ID).
    fn canonical<'a>(&'a self, id: &'a str) -> Option<&'a str> {
        let mut id = id;
        for _ in 0..4 {
            if self.point.contains_key(id) {
                return Some(id);
            }
            id = self.links.get(id)?;
        }
        None
    }
}

/// An ISO 6709 point `±DDMM[SS]±DDDMM[SS]` as (lon, lat) on the 1e-7° grid.
fn iso6709(s: &str) -> Option<(i32, i32)> {
    let split = s.get(1..)?.find(['+', '-'])? + 1;
    let part = |p: &str, deg_digits: usize| -> Option<i32> {
        let (sign, d) = (p.get(..1)?, p.get(1..)?);
        let deg: i64 = d.get(..deg_digits)?.parse().ok()?;
        let min: i64 = d.get(deg_digits..deg_digits + 2)?.parse().ok()?;
        let sec: i64 = match d.len() - deg_digits {
            2 => 0,
            4 => d.get(deg_digits + 2..)?.parse().ok()?,
            _ => return None,
        };
        let v = (deg * 3600 + min * 60 + sec) * 10_000_000 / 3600;
        i32::try_from(if sign == "-" { -v } else { v }).ok()
    };
    Some((part(s.get(split..)?, 3)?, part(s.get(..split)?, 2)?))
}

/// The assigned zones: the zone IDs used (sorted) with their `zone.tab` points, and each
/// item's zone (an index into them), with a report.
pub(super) struct Assigned {
    pub(super) zones: Vec<(String, (i32, i32))>,
    pub(super) of: Vec<Option<usize>>,
    pub(super) report: String,
    /// People in cities whose stated zone is not the zone of the unit (or country) they hang
    /// from: there `tz_at` would differ from the city's own zone. Over all cities with a zone.
    pub(super) off_share: f64,
}

/// The zone most people of `votes` live in; ties to the smallest ID.
fn majority<'a>(votes: Option<&BTreeMap<&'a str, i64>>) -> Option<&'a str> {
    votes?
        .iter()
        .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
        .map(|(z, _)| *z)
}

/// Assigns a zone to every item (see the module docs).
#[allow(clippy::too_many_lines, reason = "one pass per level, in order")]
pub(super) fn assign(items: &[Item<'_>], tz: &Tzdb) -> Result<Assigned, String> {
    let at: BTreeMap<&str, usize> = items.iter().enumerate().map(|(i, x)| (x.key, i)).collect();
    let country_of = |i: usize| -> Option<usize> {
        let mut i = i;
        for _ in 0..4 {
            let x = items.get(i)?;
            if x.kind == PlaceKind::Country {
                return Some(i);
            }
            i = *at.get(x.parent?)?;
        }
        None
    };
    // Each city's zone as tzdb names it; an unknown ID fails.
    let mut unknown: BTreeMap<&str, usize> = BTreeMap::new();
    let stated: Vec<Option<&str>> = items
        .iter()
        .map(|x| {
            let s = x.stated.filter(|s| !s.is_empty())?;
            let c = tz.canonical(s);
            if c.is_none() {
                *unknown.entry(s).or_default() += 1;
            }
            c
        })
        .collect();
    if !unknown.is_empty() {
        return Err(format!("zones tzdb does not know: {unknown:?}"));
    }
    // Votes: each city's people for its parent and its country.
    let mut votes: Vec<Option<BTreeMap<&str, i64>>> = vec![None; items.len()];
    for (i, x) in items.iter().enumerate() {
        let Some(z) = stated.get(i).copied().flatten() else {
            continue;
        };
        let w = x.population.unwrap_or(0).max(1);
        let parent = x.parent.and_then(|p| at.get(p).copied());
        let country = country_of(i);
        for j in [parent, country.filter(|&c| Some(c) != parent)]
            .into_iter()
            .flatten()
        {
            if let Some(v) = votes.get_mut(j) {
                *v.get_or_insert_default().entry(z).or_default() += w;
            }
        }
    }
    // Cities with a zone: all, and by country, for places without a city of their own.
    let mut cities_of: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let mut all_cities: Vec<usize> = Vec::new();
    for (i, x) in items.iter().enumerate() {
        if x.kind == PlaceKind::Settlement && stated.get(i).copied().flatten().is_some() {
            all_cities.push(i);
            if let Some(c) = country_of(i) {
                cities_of.entry(c).or_default().push(i);
            }
        }
    }
    let mut zone: Vec<Option<&str>> = vec![None; items.len()];
    let mut how: BTreeMap<&str, usize> = BTreeMap::new();
    // Countries.
    for (i, x) in items.iter().enumerate() {
        if x.kind != PlaceKind::Country {
            continue;
        }
        let rows = x.iso2.and_then(|c| tz.by_cc.get(c));
        let (z, why) = match rows.map(Vec::as_slice) {
            Some([one]) => (Some(one.as_str()), "country: its one zone.tab row"),
            _ => match majority(votes.get(i).and_then(Option::as_ref)) {
                Some(z) => (Some(z), "country: its cities' zone"),
                None => match rows.and_then(|r| r.first()) {
                    Some(z) => (Some(z.as_str()), "country: its first zone.tab row"),
                    None => (
                        nearest(x.point, Some(&all_cities), items)
                            .and_then(|j| stated.get(j).copied().flatten()),
                        "country: the nearest city (no code, no city of its own)",
                    ),
                },
            },
        };
        if let Some(slot) = zone.get_mut(i) {
            *slot = z;
        }
        *how.entry(why).or_default() += 1;
    }
    // Units.
    for (i, x) in items.iter().enumerate() {
        if x.kind != PlaceKind::Admin {
            continue;
        }
        let country = country_of(i);
        let one_row = country
            .and_then(|c| items.get(c))
            .and_then(|c| c.iso2)
            .and_then(|c| tz.by_cc.get(c))
            .is_some_and(|r| r.len() == 1);
        let country_zone = country.and_then(|c| zone.get(c).copied().flatten());
        let (z, why) = if let Some(z) = majority(votes.get(i).and_then(Option::as_ref)) {
            (Some(z), "unit: its cities' zone")
        } else if one_row {
            (country_zone, "unit: its country's one zone")
        } else if let Some(z) = nearest(x.point, country.and_then(|c| cities_of.get(&c)), items)
            .and_then(|j| stated.get(j).copied().flatten())
        {
            (Some(z), "unit: the nearest city of its country")
        } else {
            (country_zone, "unit: its country's zone")
        };
        if let Some(slot) = zone.get_mut(i) {
            *slot = z;
        }
        *how.entry(why).or_default() += 1;
    }
    // Cities, and how often their own zone differs from their parent's.
    let (mut people, mut off_people, mut off) = (0_i64, 0_i64, 0_usize);
    for (i, x) in items.iter().enumerate() {
        if x.kind != PlaceKind::Settlement {
            continue;
        }
        let parent = x
            .parent
            .and_then(|p| at.get(p))
            .and_then(|&p| zone.get(p).copied().flatten());
        let own = stated.get(i).copied().flatten();
        if let (Some(own), Some(parent)) = (own, parent) {
            let w = x.population.unwrap_or(0).max(1);
            people += w;
            if own != parent {
                off_people += w;
                off += 1;
            }
        }
        if let Some(slot) = zone.get_mut(i) {
            *slot = own.or(parent);
        }
        *how.entry(if own.is_some() {
            "city: its own zone"
        } else {
            "city: its parent's zone"
        })
        .or_default() += 1;
    }
    let mut ids: Vec<&str> = zone.iter().flatten().copied().collect();
    ids.sort_unstable();
    ids.dedup();
    let zones: Vec<(String, (i32, i32))> = ids
        .iter()
        .map(|z| {
            tz.point
                .get(*z)
                .map(|&p| ((*z).to_string(), p))
                .ok_or_else(|| format!("{z}: not in zone.tab"))
        })
        .collect::<Result<_, _>>()?;
    let of = zone
        .iter()
        .map(|z| z.and_then(|z| ids.binary_search(&z).ok()))
        .collect();
    #[allow(clippy::cast_precision_loss, reason = "a share of people")]
    let off_share = off_people as f64 / people.max(1) as f64;
    let mut report = format!(
        "- {} timezones (tzdb `zone.tab` IDs). {off} cities ({:.3}% of their people) state a zone \
         other than their unit's or country's, where `tz_at` differs from their own zone.\n",
        zones.len(),
        off_share * 100.0
    );
    for (why, n) in &how {
        let _ = writeln!(report, "  - {why}: {n}");
    }
    Ok(Assigned {
        zones,
        of,
        report,
        off_share,
    })
}

/// The item of `among` nearest `p` (longitudes scaled by the cosine of `p`'s latitude); ties
/// to the first.
fn nearest(p: (i32, i32), among: Option<&Vec<usize>>, items: &[Item<'_>]) -> Option<usize> {
    let k = crate::num::cos_deg(f64::from(p.1) * 1e-7);
    among?
        .iter()
        .filter_map(|&j| {
            let q = items.get(j)?.point;
            let dx = f64::from(q.0.wrapping_sub(p.0)) * k;
            let dy = f64::from(q.1 - p.1);
            Some((dx.hypot(dy), j))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, j)| j)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test code")]

    use super::*;

    const ZONE_TAB: &str = "# comment\n\
        AA\t+2743+08519\tArea/One\n\
        BB\t+1000+01000\tArea/West\n\
        BB\t+1000+02000\tArea/East\n\
        CC\t+052030-0103015\tArea/Cee\tsecond-precision point\n";
    const BACKWARD: &str = "Link\tArea/East\tArea/Old_East\n";

    fn item<'a>(
        key: &'a str,
        kind: PlaceKind,
        parent: Option<&'a str>,
        iso2: Option<&'a str>,
        stated: Option<&'a str>,
        population: i64,
    ) -> Item<'a> {
        Item {
            key,
            kind,
            parent,
            iso2,
            point: (0, 0),
            population: Some(population),
            stated,
        }
    }

    #[test]
    fn iso6709_points() {
        assert_eq!(iso6709("+2743+08519"), Some((853_166_666, 277_166_666)));
        assert_eq!(iso6709("+052030-0103015"), Some((-105_041_666, 53_416_666)));
        assert_eq!(iso6709("+27+085"), None);
    }

    /// One-row country, several-row country with units voting, a unit without a city, a
    /// link resolved, an unknown ID refused.
    #[test]
    fn zones_by_row_vote_and_nearest() {
        use PlaceKind::{Admin, Country, Settlement};
        let tz = Tzdb::parse(ZONE_TAB, BACKWARD).unwrap();
        let mut items = vec![
            item("a", Country, None, Some("aa"), None, 0),
            item("a1", Admin, Some("a"), None, None, 0),
            item("b", Country, None, Some("bb"), None, 0),
            item("b1", Admin, Some("b"), None, None, 0),
            item("b2", Admin, Some("b"), None, None, 0),
            item("b3", Admin, Some("b"), None, None, 0),
            item("c1", Settlement, Some("b1"), None, Some("Area/West"), 10),
            item(
                "c2",
                Settlement,
                Some("b1"),
                None,
                Some("Area/Old_East"),
                30,
            ),
            item("c3", Settlement, Some("b2"), None, Some("Area/West"), 5),
            item("c4", Settlement, Some("a1"), None, None, 5),
        ];
        items[5].point = (1_000, 0); // b3 nearest c3
        items[8].point = (1_100, 0);
        items[6].point = (9_000, 0);
        items[7].point = (9_000, 0);
        let a = assign(&items, &tz).unwrap();
        let z = |k: &str| {
            let i = items.iter().position(|x| x.key == k).unwrap();
            a.of[i].map(|z| a.zones[z].0.as_str())
        };
        assert_eq!(z("a"), Some("Area/One"));
        assert_eq!(z("a1"), Some("Area/One"), "a one-row country's unit");
        assert_eq!(
            z("c4"),
            Some("Area/One"),
            "a city without a zone takes its parent's"
        );
        assert_eq!(
            z("b1"),
            Some("Area/East"),
            "30 people beat 10, through the link"
        );
        assert_eq!(z("b"), Some("Area/East"), "35 against 15 over the country");
        assert_eq!(z("b2"), Some("Area/West"));
        assert_eq!(
            z("b3"),
            Some("Area/West"),
            "the nearest city of its country"
        );
        assert_eq!(z("c1"), Some("Area/West"), "a city keeps its own");
        // c1 (10 people) differs from b1 (East): 10 of the 45 people with a stated zone.
        assert!((a.off_share - 10.0 / 45.0).abs() < 1e-9, "{}", a.off_share);
        items.push(item(
            "x",
            Settlement,
            Some("b1"),
            None,
            Some("Mars/Base"),
            1,
        ));
        assert!(assign(&items, &tz).is_err_and(|e| e.contains("Mars/Base")));
    }
}
