//! The tour: every query of the guide, run on a released pack, with the typed engine
//! (`teigeodata-engine`).
//!
//! `cargo run --release -p teigeodata-api --example tour -- world-np-lite.tgd`
#![allow(
    clippy::print_stdout,
    clippy::too_many_lines,
    reason = "a program that prints the tour, section by section"
)]

use std::error::Error;

// #region import
use teigeodata_core::{E6, E7, Point};
use teigeodata_engine::{
    Engine, Focus, Layer, LayerOptions, Outline, Place, Scratch, SearchOptions, bearing_deg,
    meters_between,
};
// #endregion import

// #region describe
/// What the tour prints of a place: its name, kind and the rest read on demand.
struct Described {
    id: String,
    name: String,
    kind: &'static str,
    level: u8,
    lat: f64,
    lon: f64,
    iso: String,
}

fn describe(engine: &Engine, place: &Place) -> Result<Described, Box<dyn Error>> {
    let mut buf = [0u8; 1024];
    Ok(Described {
        id: place.id.to_string(),
        name: engine.name(place, &mut buf)?.unwrap_or_default().to_owned(),
        kind: place.kind.map_or("", |k| k.name()),
        level: place.admin_level,
        lat: place.point.lat_degrees(),
        lon: place.point.lon_degrees(),
        iso: engine
            .iso_code(place)?
            .map(|c| c.to_string())
            .unwrap_or_default(),
    })
}
// #endregion describe

fn main() -> Result<(), Box<dyn Error>> {
    let pack = std::env::args().nth(1).ok_or("usage: tour <pack.tgd>")?;

    println!("=== open");
    // #region open
    let mut engine = Engine::new();
    engine.add(Layer::open(&pack, LayerOptions::default())?);
    println!("layers: {}", engine.layers().len());
    // Buffers the queries write into: reuse them, and a warm query allocates nothing.
    let mut scratch = Scratch::default();
    let (mut hits, mut near, mut places) = (Vec::new(), Vec::new(), Vec::new());
    // #endregion open

    println!("=== search");
    // #region search
    engine.search_with(
        "Pokhara",
        3,
        &SearchOptions::default(),
        &mut scratch,
        &mut hits,
    )?;
    for hit in &hits {
        let p = describe(&engine, &hit.place)?;
        println!(
            "{}  {}  {}  {:.4}, {:.4}  score {}",
            p.id, p.name, p.kind, p.lat, p.lon, hit.score
        );
    }
    // #endregion search

    println!("=== search-parent");
    // #region search-parent
    engine.search_with(
        "Kathmandu, Bagmati",
        2,
        &SearchOptions::default(),
        &mut scratch,
        &mut hits,
    )?;
    for hit in &hits {
        let p = describe(&engine, &hit.place)?;
        println!("{}  {}  {} {}", p.id, p.name, p.kind, p.level);
    }
    // #endregion search-parent

    println!("=== search-focus");
    // #region search-focus
    // Many places share a name; a focus (the user's location) ranks the near ones first.
    let focus = Focus {
        at: Point::<E6>::from_degrees(28.7, 80.6)?,
        scale_m: 30_000.0,
    };
    let options = SearchOptions::default().with_focus(focus);
    engine.search_with("Shivapur", 3, &options, &mut scratch, &mut hits)?;
    for hit in &hits {
        let p = describe(&engine, &hit.place)?;
        println!("{}  {}  {:.4}, {:.4}", p.id, p.name, p.lat, p.lon);
    }
    // #endregion search-focus

    println!("=== get");
    // #region get
    let pokhara = engine.get("tgd:np/1xhb".parse()?)?.ok_or("not found")?;
    let p = describe(&engine, &pokhara)?;
    println!("{} {}", p.name, p.kind);
    let elevation = engine.elevation(&pokhara)?.unwrap_or_default();
    println!("at {:.6}, {:.6}, {elevation} m", p.lat, p.lon);
    let mut buf = [0u8; 256];
    let zone = match engine.tz_of(&pokhara)? {
        Some(area) => engine.name(&area, &mut buf)?.unwrap_or_default().to_owned(),
        None => String::new(),
    };
    let parent = engine
        .parent(&pokhara)?
        .map(|p| p.id.to_string())
        .unwrap_or_default();
    println!("timezone {zone} parent {parent}");
    // #endregion get

    println!("=== hierarchy");
    // #region hierarchy
    engine.ancestors(&pokhara, &mut places)?;
    for place in &places {
        let p = describe(&engine, place)?;
        println!(
            "{}",
            format!("{} {}  {}  {}", p.kind, p.level, p.name, p.iso).trim_end()
        );
    }
    let kaski = engine.get("tgd:np/295c".parse()?)?.ok_or("not found")?;
    engine.children(&kaski, &mut places)?;
    println!("Kaski has {} children", places.len());
    // #endregion hierarchy

    println!("=== reverse");
    // #region reverse
    let at = Point::<E6>::from_degrees(27.7172, 85.3240)?;
    engine.reverse(at, 3, &mut scratch, &mut near)?;
    for n in &near {
        let p = describe(&engine, &n.place)?;
        println!("{}  {:.0} m at {:.0}°", p.name, n.meters, n.bearing_deg);
    }
    // #endregion reverse

    println!("=== radius");
    // #region radius
    engine.within_radius(at, 500.0, 10, &mut scratch, &mut near)?;
    let farthest = near.last().map_or(0.0, |n| n.meters);
    println!(
        "{} places within 500 m; farthest {farthest:.0} m",
        near.len()
    );
    // #endregion radius

    println!("=== nearby");
    // #region nearby
    let kathmandu = engine.get("tgd:np/2cn".parse()?)?.ok_or("not found")?;
    engine.nearby(&kathmandu, 3, &mut scratch, &mut near)?;
    for n in &near {
        println!("{}  {:.0} m", describe(&engine, &n.place)?.name, n.meters);
    }
    // #endregion nearby

    println!("=== admin");
    // #region admin
    engine.admin_at(
        Point::<E7>::from_degrees(28.2096, 83.9856)?,
        &mut scratch,
        &mut places,
    )?;
    for place in &places {
        let p = describe(&engine, place)?;
        println!(
            "{}",
            format!("{}  {}  {}", p.level, p.name, p.iso).trim_end()
        );
    }
    let state = engine.by_iso("NP-P4".parse()?)?.ok_or("not found")?;
    let s = describe(&engine, &state)?;
    println!("NP-P4 is {} {}", s.name, s.id);
    // #endregion admin

    println!("=== timezone");
    // #region timezone
    let area = engine
        .tz_at(Point::<E7>::from_degrees(27.7172, 85.3240)?)?
        .ok_or("no zone")?;
    let zone = describe(&engine, &area)?.name;
    let offset = engine
        .tz_offset_at(&zone, 1_700_000_000)?
        .ok_or("no rules")?;
    println!(
        "{zone} {} s east of UTC, {} dst {}",
        offset.seconds,
        offset.abbr(),
        u8::from(offset.is_dst)
    );
    let civil = teigeodata_engine::Civil {
        year: 2026,
        month: 3,
        day: 8,
        hour: 2,
        minute: 30,
        second: 0,
    };
    let (kind, before, after) = match engine.tz_local_candidates("America/New_York", civil)? {
        Some(teigeodata_engine::Local::Unique(o)) => ("unique", o, o),
        Some(teigeodata_engine::Local::Gap { before, after }) => ("gap", before, after),
        Some(teigeodata_engine::Local::Overlap { earlier, later }) => ("overlap", earlier, later),
        None => return Err("no rules for New York".into()),
    };
    println!("02:30 on 2026-03-08 in New York: {kind} {before} -> {after}");
    println!("tzdb {}", engine.tz_version().unwrap_or_default());
    // #endregion timezone

    println!("=== elevation");
    // #region elevation
    let height = engine
        .elevation_at(Point::<E7>::from_degrees(27.7172, 85.3240)?, &mut scratch)?
        .ok_or("no terrain")?;
    println!(
        "{:.1} m above {} ({}, {:.0} m grid)",
        height.meters,
        height.datum.name(),
        height.source.map_or("", |s| s.key()),
        height.resolution_m
    );
    // #endregion elevation

    println!("=== distance");
    // #region distance
    let meters = meters_between(27.7172, 85.3240, 28.2096, 83.9856);
    let heading = bearing_deg(27.7172, 85.3240, 28.2096, 83.9856);
    println!(
        "Kathmandu to Pokhara: {:.1} km, heading {heading:.0}°",
        meters / 1000.0
    );
    let pokhara = engine.get("tgd:np/1xhb".parse()?)?.ok_or("not found")?;
    let (a, b) = (kathmandu.point, pokhara.point);
    let (lat1, lon1, lat2, lon2) = (
        a.lat_degrees(),
        a.lon_degrees(),
        b.lat_degrees(),
        b.lon_degrees(),
    );
    println!(
        "place to place: {:.1} km at {:.0}°",
        meters_between(lat1, lon1, lat2, lon2) / 1000.0,
        bearing_deg(lat1, lon1, lat2, lon2)
    );
    // #endregion distance

    println!("=== boundary");
    // #region boundary
    let metro = engine.get("tgd:np/2gm".parse()?)?.ok_or("not found")?;
    let mut shape = Outline::default();
    engine
        .boundary(&metro, 0, &mut shape)?
        .ok_or("no outline")?;
    // The geometry as GeoJSON (RFC 7946), the bytes every binding's `geojson` column holds.
    let mut geometry = Vec::new();
    shape.write_geojson(&mut geometry);
    let kind = if geometry.starts_with(br#"{"type":"Polygon""#) {
        "Polygon"
    } else {
        "MultiPolygon"
    };
    println!("{kind} of {} vertices at level 0", shape.points.len());
    let tolerance_m = f64::from(shape.tolerance_cm) / 100.0;
    let feature = format!(
        r#"{{"type":"Feature","id":"{}","geometry":{},"properties":{{"lod":0,"tolerance_m":{tolerance_m},"vertices":{}}}}}"#,
        metro.id,
        String::from_utf8_lossy(&geometry),
        shape.points.len()
    );
    println!(
        "tolerance {tolerance_m:.0} m, {} bytes of GeoJSON",
        feature.len()
    );
    // #endregion boundary

    println!("=== errors");
    // #region errors
    if let Err(e) = Point::<E6>::from_degrees(91.0, 0.0) {
        println!("invalid argument: (91, 0) is not a point: {e}");
    }
    // #endregion errors
    Ok(())
}
