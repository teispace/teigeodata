//! The embedded baseline (feature `baseline`): the file `forge embed` builds, compiled in.
//!
//! ```text
//! TEIGEODATA_BASELINE=$PWD/data/build/baseline/baseline-main.tgd \
//!   cargo test --release -p teigeodata-engine --features baseline --test baseline
//! ```

#![allow(
    clippy::unwrap_used,
    reason = "tests: failures panic with their message"
)]

use teigeodata_core::{E7, PlaceKind, Point};
use teigeodata_engine::{BASELINE, Engine, Place, Scratch};

fn name(e: &Engine, p: &Place) -> String {
    e.name(p, &mut [0; 512])
        .unwrap()
        .unwrap_or_default()
        .to_owned()
}

fn at(lat: f64, lon: f64) -> Point<E7> {
    Point::<E7>::from_degrees(lat, lon).unwrap()
}

/// Z1 (D-144): the embedded file fits its 8 MB budget.
#[test]
fn the_baseline_fits_its_budget() {
    assert!(BASELINE.len() <= 8_000_000, "{} B", BASELINE.len());
}

/// It answers the world: a country, its unit and zone at a point, and a city by name, in a
/// country with a pack's slice (Nepal) and one without (France).
#[test]
fn the_baseline_answers_the_world() {
    let e = Engine::with_baseline().unwrap();
    let (mut s, mut units, mut hits) = (Scratch::default(), Vec::new(), Vec::new());
    for (lat, lon, country, zone) in [
        (27.7172, 85.3240, "Nepal", "Asia/Kathmandu"),
        (48.8566, 2.3522, "France", "Europe/Paris"),
    ] {
        e.admin_at(at(lat, lon), &mut s, &mut units).unwrap();
        let found: Vec<_> = units.iter().map(|u| (u.kind, name(&e, u))).collect();
        assert!(
            found.contains(&(Some(PlaceKind::Country), country.to_owned())),
            "{found:?}"
        );
        assert!(
            units.iter().any(|u| u.kind == Some(PlaceKind::Admin)),
            "a unit at {lat}, {lon}: {found:?}"
        );
        let tz = e.tz_at(at(lat, lon)).unwrap().unwrap();
        assert_eq!(name(&e, &tz), zone);
    }
    for city in ["Kathmandu", "Paris", "Pokhara", "Tokyo"] {
        e.search(city, 5, &mut s, &mut hits).unwrap();
        let top = hits.first().map(|h| name(&e, &h.place));
        assert_eq!(top.as_deref(), Some(city), "{city}");
    }
}

/// TG-19: `reverse` gives each answer's bearing from the place to the point, and `mark_inside`
/// says, for every country and unit among the answers, whether it holds the point, as
/// `admin_at` does, and for settlements and localities by radius (D-238); other places stay
/// `None`.
#[test]
fn reverse_says_bearing_and_inside() {
    let e = Engine::with_baseline().unwrap();
    let mut s = Scratch::default();
    let (mut near, mut units) = (Vec::new(), Vec::new());
    let at = Point::<teigeodata_core::E6>::from_degrees(28.2669, 84.10).unwrap();
    e.reverse(at, 300, &mut s, &mut near).unwrap();
    e.mark_inside(at, &mut s, &mut near).unwrap();
    e.admin_at(at.to_e7(), &mut s, &mut units).unwrap();
    let pokhara = near
        .iter()
        .find(|n| name(&e, &n.place) == "Pokhara")
        .unwrap();
    assert!(
        (pokhara.bearing_deg - 90.0).abs() < 1.0,
        "{}",
        pokhara.bearing_deg
    );
    for n in &near {
        let held = units.iter().any(|u| u.id == n.place.id);
        match n.place.kind {
            Some(PlaceKind::Country | PlaceKind::Admin) => assert_eq!(n.inside, Some(held)),
            Some(k @ (PlaceKind::Settlement | PlaceKind::Locality)) => {
                // D-238: inside only the nearest of its kind, within the kind's radius.
                let nearest = near
                    .iter()
                    .filter(|m| m.place.kind == Some(k))
                    .all(|m| m.meters >= n.meters);
                let radius = if k == PlaceKind::Settlement {
                    teigeodata_engine::SETTLEMENT_RADIUS_M
                } else {
                    teigeodata_engine::LOCALITY_RADIUS_M
                };
                assert_eq!(n.inside, Some(nearest && n.meters <= radius));
            }
            _ => assert_eq!(n.inside, None),
        }
    }
    assert!(near.iter().any(|n| n.inside == Some(true)));
    assert!(near.iter().any(|n| n.inside == Some(false)));
}

/// TG-19: by Birgunj, inside Nepal, `reverse` names India's Raxaul, the nearest place; kept to
/// the point's country it names places in Nepal only, nearest first, and a point inside India
/// still gets Raxaul.
#[test]
fn reverse_in_country_stays_on_the_point_s_side_of_the_border() {
    let e = Engine::with_baseline().unwrap();
    let mut s = Scratch::default();
    let (mut near, mut kept) = (Vec::new(), Vec::new());
    let country = |n: &teigeodata_engine::Near| n.place.id.namespace().country().unwrap();
    let by_birgunj = Point::<teigeodata_core::E6>::from_degrees(26.985, 84.880).unwrap();
    e.reverse(by_birgunj, 1, &mut s, &mut near).unwrap();
    assert_eq!(name(&e, &near[0].place), "Raxaul");
    e.reverse_in_country(by_birgunj, 5, &mut s, &mut kept)
        .unwrap();
    assert_eq!(kept.len(), 5);
    assert!(
        kept.iter().all(|n| country(n) == *b"np"),
        "{:?}",
        kept.iter().map(|n| name(&e, &n.place)).collect::<Vec<_>>()
    );
    assert!(kept.windows(2).all(|w| w[0].meters <= w[1].meters));
    let in_india = Point::<teigeodata_core::E6>::from_degrees(26.960, 84.850).unwrap();
    e.reverse_in_country(in_india, 1, &mut s, &mut kept)
        .unwrap();
    assert_eq!(name(&e, &kept[0].place), "Raxaul");
}

/// D-226: a focus point lifts the namesake near it (Hyderabad, Pakistan over Hyderabad,
/// India), and lifts it less the farther it is; strict order puts an exact match (Rome) above a
/// more important prefix match (Romania); neither changes which places match.
#[test]
fn focus_and_strict_reorder_search() {
    use teigeodata_engine::{Focus, SearchOptions};
    let e = Engine::with_baseline().unwrap();
    let (mut s, mut hits) = (Scratch::default(), Vec::new());
    let country = |h: &teigeodata_engine::Hit| h.place.id.namespace().country().unwrap();
    let ids = |hits: &[teigeodata_engine::Hit]| {
        let mut v: Vec<_> = hits.iter().map(|h| h.place.id).collect();
        v.sort_unstable();
        v
    };
    e.search("Hyderabad", 2, &mut s, &mut hits).unwrap();
    assert_eq!(country(&hits[0]), *b"in");
    let plain = ids(&hits);
    let sindh = Point::<teigeodata_core::E6>::from_degrees(25.3969, 68.3772).unwrap();
    let opts = SearchOptions::default().with_focus(Focus::new(sindh));
    e.search_with("Hyderabad", 2, &opts, &mut s, &mut hits)
        .unwrap();
    assert_eq!(country(&hits[0]), *b"pk");
    assert_eq!(ids(&hits), plain);
    let near_score = hits[0].score;
    let mut far = Focus::new(Point::from_degrees(24.86, 67.0).unwrap());
    e.search_with(
        "Hyderabad",
        2,
        &SearchOptions::default().with_focus(far),
        &mut s,
        &mut hits,
    )
    .unwrap();
    let pk = hits.iter().find(|h| country(h) == *b"pk").unwrap();
    assert!(pk.score < near_score, "{} vs {near_score}", pk.score);
    far.scale_m = 0.0;
    e.search_with(
        "Hyderabad",
        2,
        &SearchOptions::default().with_focus(far),
        &mut s,
        &mut hits,
    )
    .unwrap();
    assert_eq!(country(&hits[0]), *b"in", "a scale of 0 adds nothing");

    e.search("rome", 4, &mut s, &mut hits).unwrap();
    assert_eq!(name(&e, &hits[0].place), "Romania");
    let strict = SearchOptions::default().with_strict(true);
    e.search_with("rome", 4, &strict, &mut s, &mut hits)
        .unwrap();
    assert_eq!(name(&e, &hits[0].place), "Rome");
}
