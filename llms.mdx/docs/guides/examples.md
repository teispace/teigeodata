# The example programs (/docs/guides/examples)

Every snippet in these docs is a region of one program per language. CI runs each on the
released pack and compares its output line by line. Run one from a checkout, which builds what
the language needs and fetches the pinned pack:

```sh
cargo xtask docs-examples --fetch --check --lang python   # or rust, node, wasm, swift, kotlin, dart, go, c, cpp, dotnet
```

<Tabs groupId="lang" persist items={["Rust","Python","Node.js","Browser","Swift","Kotlin","Java","Dart","Go","C","C++","C#"]}>
<Tab value="Rust">
```rust title="tour.rs"
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
```
</Tab>

<Tab value="Python">
```python title="tour.py"
"""The tour: every query of the guide, run on a released pack.

Usage: python tour.py world-np-lite.tgd
"""

import json
import sys

# region import
import teigeodata
from teigeodata.blob import Reader
# endregion import

# region rows
def rows(blob):
    """Each row of a result as a dict: column name to value, None where null."""
    return Reader(blob).records()
# endregion rows

def main(pack):
    print("=== open")
    # region open
    engine = teigeodata.Engine()
    engine.open_file(pack)
    print("layers:", engine.layers())
    # endregion open

    print("=== search")
    # region search
    for p in rows(engine.search("Pokhara", 3)):
        print(f"{p['id']}  {p['name']}  {p['kind']}  {p['lat']:.4f}, {p['lon']:.4f}  score {p['score']}")
    # endregion search

    print("=== search-parent")
    # region search-parent
    for p in rows(engine.search("Kathmandu, Bagmati", 2)):
        print(f"{p['id']}  {p['name']}  {p['kind']} {p['level']}")
    # endregion search-parent

    print("=== search-focus")
    # region search-focus
    # Many places share a name; a focus (the user's location) ranks the near ones first.
    options = teigeodata.SearchOptions(focus_lat=28.7, focus_lon=80.6, focus_scale_m=30_000)
    for p in rows(engine.search_with("Shivapur", 3, options)):
        print(f"{p['id']}  {p['name']}  {p['lat']:.4f}, {p['lon']:.4f}")
    # endregion search-focus

    print("=== get")
    # region get
    (place,) = rows(engine.get("tgd:np/1xhb"))
    print(place["name"], place["kind"])
    print(f"at {place['lat']:.6f}, {place['lon']:.6f}, {place['elevation']} m")
    print("timezone", place["tz"], "parent", place["parent"])
    # endregion get

    print("=== hierarchy")
    # region hierarchy
    for p in rows(engine.ancestors("tgd:np/1xhb")):
        print(f"{p['kind']} {p['level']}  {p['name']}  {p['iso'] or ''}".rstrip())
    children = rows(engine.children("tgd:np/295c"))
    print("Kaski has", len(children), "children")
    # endregion hierarchy

    print("=== reverse")
    # region reverse
    for p in rows(engine.reverse(27.7172, 85.3240, 3)):
        print(f"{p['name']}  {p['meters']:.0f} m at {p['bearing']:.0f}°")
    # endregion reverse

    print("=== radius")
    # region radius
    within = rows(engine.within_radius(27.7172, 85.3240, 500.0, 10))
    print(len(within), "places within 500 m; farthest", f"{within[-1]['meters']:.0f} m")
    # endregion radius

    print("=== nearby")
    # region nearby
    for p in rows(engine.nearby("tgd:np/2cn", 3)):
        print(f"{p['name']}  {p['meters']:.0f} m")
    # endregion nearby

    print("=== admin")
    # region admin
    for p in rows(engine.admin_at(28.2096, 83.9856)):
        print(f"{p['level']}  {p['name']}  {p['iso'] or ''}".rstrip())
    (state,) = rows(engine.by_iso("NP-P4"))
    print("NP-P4 is", state["name"], state["id"])
    # endregion admin

    print("=== timezone")
    # region timezone
    (area,) = rows(engine.tz_at(27.7172, 85.3240))
    zone = area["tz"]
    (offset,) = rows(engine.tz_offset_at(zone, 1_700_000_000))
    print(zone, offset["seconds"], "s east of UTC,", offset["abbr"], "dst", offset["dst"])
    (local,) = rows(engine.tz_local("America/New_York", 2026, 3, 8, 2, 30, 0))
    print("02:30 on 2026-03-08 in New York:", local["kind"], local["before"], "->", local["after"])
    print("tzdb", engine.tz_version())
    # endregion timezone

    print("=== elevation")
    # region elevation
    (height,) = rows(engine.elevation_at(27.7172, 85.3240))
    print(f"{height['meters']:.1f} m above {height['datum']} ({height['source']}, {height['resolution_m']:.0f} m grid)")
    # endregion elevation

    print("=== distance")
    # region distance
    meters = teigeodata.distance_m(27.7172, 85.3240, 28.2096, 83.9856)
    heading = teigeodata.bearing_deg(27.7172, 85.3240, 28.2096, 83.9856)
    print(f"Kathmandu to Pokhara: {meters / 1000:.1f} km, heading {heading:.0f}°")
    (apart,) = rows(engine.distance("tgd:np/2cn", "tgd:np/1xhb"))
    print(f"place to place: {apart['meters'] / 1000:.1f} km at {apart['bearing']:.0f}°")
    # endregion distance

    print("=== boundary")
    # region boundary
    (shape,) = rows(engine.boundary("tgd:np/2gm", 0))
    feature = json.loads(shape["geojson"])  # a GeoJSON Feature, for any map library
    print(f"{feature['geometry']['type']} of {shape['vertices']} vertices at level {shape['lod']}")
    print(f"tolerance {shape['tolerance_m']:.0f} m, {len(shape['geojson'])} bytes of GeoJSON")
    # endregion boundary

    print("=== errors")
    # region errors
    try:
        engine.reverse(91.0, 0.0, 1)
    except teigeodata.InvalidArgumentError as error:
        print("invalid argument:", error)
    # endregion errors

if __name__ == "__main__":
    main(sys.argv[1])
```
</Tab>

<Tab value="Node.js">
```js title="tour.mjs"
// The tour: every query of the guide, run on a released pack.
// Usage: node tour.mjs world-np-lite.tgd

// #region import
import { Engine, Reader, bearingDeg, distanceM } from "teigeodata";
// #endregion import

const pack = process.argv[2];
const fixed = (x, digits) => x.toFixed(digits);

console.log("=== open");
// #region open
const engine = new Engine();
engine.openFile(pack);
console.log("layers:", engine.layers());
// #endregion open

console.log("=== search");
// #region search
for (const p of new Reader(engine.search("Pokhara", 3))) {
  console.log(`${p.id}  ${p.name}  ${p.kind}  ${fixed(p.lat, 4)}, ${fixed(p.lon, 4)}  score ${p.score}`);
}
// #endregion search

console.log("=== search-parent");
// #region search-parent
for (const p of new Reader(engine.search("Kathmandu, Bagmati", 2))) {
  console.log(`${p.id}  ${p.name}  ${p.kind} ${p.level}`);
}
// #endregion search-parent

console.log("=== search-focus");
// #region search-focus
// Many places share a name; a focus (the user's location) ranks the near ones first.
const options = { focusLat: 28.7, focusLon: 80.6, focusScaleM: 30_000 };
for (const p of new Reader(engine.searchWith("Shivapur", 3, options))) {
  console.log(`${p.id}  ${p.name}  ${fixed(p.lat, 4)}, ${fixed(p.lon, 4)}`);
}
// #endregion search-focus

console.log("=== get");
// #region get
const [place] = new Reader(engine.get("tgd:np/1xhb")).records();
console.log(place.name, place.kind);
console.log(`at ${fixed(place.lat, 6)}, ${fixed(place.lon, 6)}, ${place.elevation} m`);
console.log("timezone", place.tz, "parent", place.parent);
// #endregion get

console.log("=== hierarchy");
// #region hierarchy
for (const p of new Reader(engine.ancestors("tgd:np/1xhb"))) {
  console.log(`${p.kind} ${p.level}  ${p.name}  ${p.iso ?? ""}`.trimEnd());
}
const children = new Reader(engine.children("tgd:np/295c"));
console.log("Kaski has", children.rows, "children");
// #endregion hierarchy

console.log("=== reverse");
// #region reverse
for (const p of new Reader(engine.reverse(27.7172, 85.324, 3))) {
  console.log(`${p.name}  ${fixed(p.meters, 0)} m at ${fixed(p.bearing, 0)}°`);
}
// #endregion reverse

console.log("=== radius");
// #region radius
const within = new Reader(engine.withinRadius(27.7172, 85.324, 500, 10));
const meters = within.numbers("meters");
console.log(within.rows, "places within 500 m; farthest", `${fixed(meters[within.rows - 1], 0)} m`);
// #endregion radius

console.log("=== nearby");
// #region nearby
for (const p of new Reader(engine.nearby("tgd:np/2cn", 3))) {
  console.log(`${p.name}  ${fixed(p.meters, 0)} m`);
}
// #endregion nearby

console.log("=== admin");
// #region admin
for (const p of new Reader(engine.adminAt(28.2096, 83.9856))) {
  console.log(`${p.level}  ${p.name}  ${p.iso ?? ""}`.trimEnd());
}
const [state] = new Reader(engine.byIso("NP-P4")).records();
console.log("NP-P4 is", state.name, state.id);
// #endregion admin

console.log("=== timezone");
// #region timezone
const [area] = new Reader(engine.tzAt(27.7172, 85.324)).records();
const zone = area.tz;
const [offset] = new Reader(engine.tzOffsetAt(zone, 1_700_000_000)).records();
console.log(zone, offset.seconds, "s east of UTC,", offset.abbr, "dst", offset.dst);
const [local] = new Reader(engine.tzLocal("America/New_York", 2026, 3, 8, 2, 30, 0)).records();
console.log("02:30 on 2026-03-08 in New York:", local.kind, local.before, "->", local.after);
console.log("tzdb", engine.tzVersion());
// #endregion timezone

console.log("=== elevation");
// #region elevation
const [height] = new Reader(engine.elevationAt(27.7172, 85.324)).records();
console.log(
  `${fixed(height.meters, 1)} m above ${height.datum} (${height.source}, ${fixed(height.resolution_m, 0)} m grid)`,
);
// #endregion elevation

console.log("=== distance");
// #region distance
const km = distanceM(27.7172, 85.324, 28.2096, 83.9856) / 1000;
const heading = bearingDeg(27.7172, 85.324, 28.2096, 83.9856);
console.log(`Kathmandu to Pokhara: ${fixed(km, 1)} km, heading ${fixed(heading, 0)}°`);
const [apart] = new Reader(engine.distance("tgd:np/2cn", "tgd:np/1xhb")).records();
console.log(`place to place: ${fixed(apart.meters / 1000, 1)} km at ${fixed(apart.bearing, 0)}°`);
// #endregion distance

console.log("=== boundary");
// #region boundary
const [shape] = new Reader(engine.boundary("tgd:np/2gm", 0)).records();
const feature = JSON.parse(shape.geojson); // a GeoJSON Feature: map.addSource("x", { type: "geojson", data: feature })
console.log(`${feature.geometry.type} of ${shape.vertices} vertices at level ${shape.lod}`);
console.log(`tolerance ${fixed(shape.tolerance_m, 0)} m, ${shape.geojson.length} bytes of GeoJSON`);
// #endregion boundary

console.log("=== errors");
// #region errors
try {
  engine.reverse(91, 0, 1);
} catch (error) {
  if (error.code !== "InvalidArgument") throw error;
  console.log("invalid argument:", error.message);
}
// #endregion errors
```
</Tab>

<Tab value="Browser">
```js title="tour.mjs"
// The tour: every query of the guide, run on a released pack.
// The WebAssembly build, here on Node.js; in a browser the pack's bytes come from `fetch`.
// Usage: node tour.mjs world-np-lite.tgd

import { readFileSync } from "node:fs";
// #region import
import { init, Engine, Reader, bearingDeg, distanceM } from "teigeodata-wasm";

await init(); // loads the module once; at once on Node.js, Bun and Workers
// #endregion import

const pack = process.argv[2];
const fixed = (x, digits) => x.toFixed(digits);

console.log("=== open");
// #region open
const engine = new Engine();
engine.openBytes(readFileSync(pack)); // in a browser: new Uint8Array(await (await fetch(url)).arrayBuffer())
console.log("layers:", engine.layers());
// #endregion open

console.log("=== search");
// #region search
for (const p of new Reader(engine.search("Pokhara", 3))) {
  console.log(`${p.id}  ${p.name}  ${p.kind}  ${fixed(p.lat, 4)}, ${fixed(p.lon, 4)}  score ${p.score}`);
}
// #endregion search

console.log("=== search-parent");
// #region search-parent
for (const p of new Reader(engine.search("Kathmandu, Bagmati", 2))) {
  console.log(`${p.id}  ${p.name}  ${p.kind} ${p.level}`);
}
// #endregion search-parent

console.log("=== search-focus");
// #region search-focus
// Many places share a name; a focus (the user's location) ranks the near ones first.
const options = { focusLat: 28.7, focusLon: 80.6, focusScaleM: 30_000 };
for (const p of new Reader(engine.searchWith("Shivapur", 3, options))) {
  console.log(`${p.id}  ${p.name}  ${fixed(p.lat, 4)}, ${fixed(p.lon, 4)}`);
}
// #endregion search-focus

console.log("=== get");
// #region get
const [place] = new Reader(engine.get("tgd:np/1xhb")).records();
console.log(place.name, place.kind);
console.log(`at ${fixed(place.lat, 6)}, ${fixed(place.lon, 6)}, ${place.elevation} m`);
console.log("timezone", place.tz, "parent", place.parent);
// #endregion get

console.log("=== hierarchy");
// #region hierarchy
for (const p of new Reader(engine.ancestors("tgd:np/1xhb"))) {
  console.log(`${p.kind} ${p.level}  ${p.name}  ${p.iso ?? ""}`.trimEnd());
}
const children = new Reader(engine.children("tgd:np/295c"));
console.log("Kaski has", children.rows, "children");
// #endregion hierarchy

console.log("=== reverse");
// #region reverse
for (const p of new Reader(engine.reverse(27.7172, 85.324, 3))) {
  console.log(`${p.name}  ${fixed(p.meters, 0)} m at ${fixed(p.bearing, 0)}°`);
}
// #endregion reverse

console.log("=== radius");
// #region radius
const within = new Reader(engine.withinRadius(27.7172, 85.324, 500, 10));
const meters = within.numbers("meters");
console.log(within.rows, "places within 500 m; farthest", `${fixed(meters[within.rows - 1], 0)} m`);
// #endregion radius

console.log("=== nearby");
// #region nearby
for (const p of new Reader(engine.nearby("tgd:np/2cn", 3))) {
  console.log(`${p.name}  ${fixed(p.meters, 0)} m`);
}
// #endregion nearby

console.log("=== admin");
// #region admin
for (const p of new Reader(engine.adminAt(28.2096, 83.9856))) {
  console.log(`${p.level}  ${p.name}  ${p.iso ?? ""}`.trimEnd());
}
const [state] = new Reader(engine.byIso("NP-P4")).records();
console.log("NP-P4 is", state.name, state.id);
// #endregion admin

console.log("=== timezone");
// #region timezone
const [area] = new Reader(engine.tzAt(27.7172, 85.324)).records();
const zone = area.tz;
const [offset] = new Reader(engine.tzOffsetAt(zone, 1_700_000_000)).records();
console.log(zone, offset.seconds, "s east of UTC,", offset.abbr, "dst", offset.dst);
const [local] = new Reader(engine.tzLocal("America/New_York", 2026, 3, 8, 2, 30, 0)).records();
console.log("02:30 on 2026-03-08 in New York:", local.kind, local.before, "->", local.after);
console.log("tzdb", engine.tzVersion());
// #endregion timezone

console.log("=== elevation");
// #region elevation
const [height] = new Reader(engine.elevationAt(27.7172, 85.324)).records();
console.log(
  `${fixed(height.meters, 1)} m above ${height.datum} (${height.source}, ${fixed(height.resolution_m, 0)} m grid)`,
);
// #endregion elevation

console.log("=== distance");
// #region distance
const km = distanceM(27.7172, 85.324, 28.2096, 83.9856) / 1000;
const heading = bearingDeg(27.7172, 85.324, 28.2096, 83.9856);
console.log(`Kathmandu to Pokhara: ${fixed(km, 1)} km, heading ${fixed(heading, 0)}°`);
const [apart] = new Reader(engine.distance("tgd:np/2cn", "tgd:np/1xhb")).records();
console.log(`place to place: ${fixed(apart.meters / 1000, 1)} km at ${fixed(apart.bearing, 0)}°`);
// #endregion distance

console.log("=== boundary");
// #region boundary
const [shape] = new Reader(engine.boundary("tgd:np/2gm", 0)).records();
const feature = JSON.parse(shape.geojson); // a GeoJSON Feature: map.addSource("x", { type: "geojson", data: feature })
console.log(`${feature.geometry.type} of ${shape.vertices} vertices at level ${shape.lod}`);
console.log(`tolerance ${fixed(shape.tolerance_m, 0)} m, ${shape.geojson.length} bytes of GeoJSON`);
// #endregion boundary

console.log("=== errors");
// #region errors
try {
  engine.reverse(91, 0, 1);
} catch (error) {
  if (error.code !== "InvalidArgument") throw error;
  console.log("invalid argument:", error.message);
}
// #endregion errors
```
</Tab>

<Tab value="Swift">
```swift title="main.swift"
// The tour: every query of the guide, run on a released pack.
//
// Usage: swift run -c release Tour world-np-lite.tgd (in site/examples/swift/Tour)

// #region import
import Foundation
import TeiGeodata
// #endregion import

/// `value` with `digits` decimals, as the other languages print it.
func fixed(_ value: Double?, _ digits: Int) -> String {
    String(format: "%.\(digits)f", value ?? .nan)
}

let pack = CommandLine.arguments[1]

do {
    print("=== open")
    // #region open
    let engine = Engine()
    _ = try engine.openFile(pack)  // the layer number
    print("layers:", engine.layers())
    // #endregion open

    print("=== search")
    // #region search
    for p in try Reader(engine.search("Pokhara", limit: 3)) {
        print("\(p["id"]!)  \(p["name"]!)  \(p["kind"]!)  \(fixed(p.double("lat"), 4)), \(fixed(p.double("lon"), 4))  score \(p["score"]!)")
    }
    // #endregion search

    print("=== search-parent")
    // #region search-parent
    for p in try Reader(engine.search("Kathmandu, Bagmati", limit: 2)) {
        print("\(p["id"]!)  \(p["name"]!)  \(p["kind"]!) \(p["level"]!)")
    }
    // #endregion search-parent

    print("=== search-focus")
    // #region search-focus
    // Many places share a name; a focus (the user's location) ranks the near ones first.
    let options = SearchOptions(focusLat: 28.7, focusLon: 80.6, focusScaleM: 30_000)
    for p in try Reader(engine.searchWith("Shivapur", limit: 3, options: options)) {
        print("\(p["id"]!)  \(p["name"]!)  \(fixed(p.double("lat"), 4)), \(fixed(p.double("lon"), 4))")
    }
    // #endregion search-focus

    print("=== get")
    // #region get
    let place = try Reader(engine.get("tgd:np/1xhb")).records()[0]
    print(place["name"]!, place["kind"]!)
    print("at \(fixed(place.double("lat"), 6)), \(fixed(place.double("lon"), 6)), \(place["elevation"]!) m")
    print("timezone", place["tz"]!, "parent", place["parent"]!)
    // #endregion get

    print("=== hierarchy")
    // #region hierarchy
    for p in try Reader(engine.ancestors("tgd:np/1xhb")) {
        let line = "\(p["kind"]!) \(p["level"]!)  \(p["name"]!)  \(p.string("iso") ?? "")"
        print(line.replacingOccurrences(of: "\\s+$", with: "", options: .regularExpression))
    }
    let children = try Reader(engine.children("tgd:np/295c"))
    print("Kaski has", children.rows, "children")
    // #endregion hierarchy

    print("=== reverse")
    // #region reverse
    for p in try Reader(engine.reverse(27.7172, lon: 85.3240, limit: 3)) {
        print("\(p["name"]!)  \(fixed(p.double("meters"), 0)) m at \(fixed(p.double("bearing"), 0))°")
    }
    // #endregion reverse

    print("=== radius")
    // #region radius
    let within = try Reader(engine.withinRadius(27.7172, lon: 85.3240, radiusM: 500, limit: 10)).records()
    print(within.count, "places within 500 m; farthest", fixed(within.last?.double("meters"), 0), "m")
    // #endregion radius

    print("=== nearby")
    // #region nearby
    for p in try Reader(engine.nearby("tgd:np/2cn", limit: 3)) {
        print("\(p["name"]!)  \(fixed(p.double("meters"), 0)) m")
    }
    // #endregion nearby

    print("=== admin")
    // #region admin
    for p in try Reader(engine.adminAt(28.2096, lon: 83.9856)) {
        let line = "\(p["level"]!)  \(p["name"]!)  \(p.string("iso") ?? "")"
        print(line.replacingOccurrences(of: "\\s+$", with: "", options: .regularExpression))
    }
    let state = try Reader(engine.byIso("NP-P4")).records()[0]
    print("NP-P4 is", state["name"]!, state["id"]!)
    // #endregion admin

    print("=== timezone")
    // #region timezone
    let zone = try Reader(engine.tzAt(27.7172, lon: 85.3240)).records()[0].string("tz")!
    let offset = try Reader(engine.tzOffsetAt(zone, unixSeconds: 1_700_000_000)).records()[0]
    print(zone, offset["seconds"]!, "s east of UTC,", offset["abbr"]!, "dst", offset["dst"]!)
    let local = try Reader(engine.tzLocal("America/New_York", year: 2026, month: 3, day: 8, hour: 2, minute: 30, second: 0)).records()[0]
    print("02:30 on 2026-03-08 in New York:", local["kind"]!, local["before"]!, "->", local["after"]!)
    print("tzdb", engine.tzVersion())
    // #endregion timezone

    print("=== elevation")
    // #region elevation
    let height = try Reader(engine.elevationAt(27.7172, lon: 85.3240)).records()[0]
    print("\(fixed(height.double("meters"), 1)) m above \(height["datum"]!) (\(height["source"]!), \(fixed(height.double("resolution_m"), 0)) m grid)")
    // #endregion elevation

    print("=== distance")
    // #region distance
    let meters = try TeiGeodata.distanceM(27.7172, lon1: 85.3240, lat2: 28.2096, lon2: 83.9856)
    let heading = try TeiGeodata.bearingDeg(27.7172, lon1: 85.3240, lat2: 28.2096, lon2: 83.9856)
    print("Kathmandu to Pokhara: \(fixed(meters / 1000, 1)) km, heading \(fixed(heading, 0))°")
    let apart = try Reader(engine.distance("tgd:np/2cn", to: "tgd:np/1xhb")).records()[0]
    print("place to place: \(fixed(apart.double("meters")! / 1000, 1)) km at \(fixed(apart.double("bearing"), 0))°")
    // #endregion distance

    print("=== boundary")
    // #region boundary
    let shape = try Reader(engine.boundary("tgd:np/2gm", lod: 0)).records()[0]
    let geojson = shape["geojson"]!.description // a GeoJSON Feature, for any map library
    let feature = try JSONSerialization.jsonObject(with: Data(geojson.utf8)) as! [String: Any]
    let type = (feature["geometry"] as! [String: Any])["type"] as! String
    print("\(type) of \(shape["vertices"]!) vertices at level \(shape["lod"]!)")
    print("tolerance \(fixed(shape.double("tolerance_m"), 0)) m, \(geojson.utf8.count) bytes of GeoJSON")
    // #endregion boundary

    print("=== errors")
    // #region errors
    do {
        _ = try engine.reverse(91, lon: 0, limit: 1)
    } catch where error.code == .invalidArgument {
        print("invalid argument:", error.message)
    }
    // #endregion errors
} catch {
    FileHandle.standardError.write(Data("\(error)\n".utf8))
    exit(1)
}
```
</Tab>

<Tab value="Kotlin">
```kotlin title="Tour.kt"
// The tour: every query of the guide, run on a released pack.
//
// Usage: kotlinc Tour.kt -cp <the package's classes> -include-runtime -d tour.jar
//        java --enable-native-access=ALL-UNNAMED -cp tour.jar:<the package's classes> TourKt world-np-lite.tgd

// #region import
import java.util.Locale
import org.teispace.teigeodata.Reader
import org.teispace.teigeodata.TeiGeodata
import org.teispace.teigeodata.TeiGeodata.Engine
import org.teispace.teigeodata.TeiGeodata.SearchOptions
import org.teispace.teigeodata.TeiGeodataException
// #endregion import

/** `format` in the root locale: a decimal point whatever the user's locale. */
fun String.f(vararg args: Any?): String = String.format(Locale.ROOT, this, *args)

fun main(args: Array<String>) {
    val pack = args[0]

    println("=== open")
    // #region open
    Engine().use { engine ->
        engine.openFile(pack)
        println("layers: ${engine.layers()}")
        // #endregion open
        tour(engine)
    }
}

fun tour(engine: Engine) {
    println("=== search")
    // #region search
    for (p in Reader(engine.search("Pokhara", 3))) {
        println("%s  %s  %s  %.4f, %.4f  score %d".f(p["id"], p["name"], p["kind"], p["lat"], p["lon"], p["score"]))
    }
    // #endregion search

    println("=== search-parent")
    // #region search-parent
    for (p in Reader(engine.search("Kathmandu, Bagmati", 2))) {
        println("${p["id"]}  ${p["name"]}  ${p["kind"]} ${p["level"]}")
    }
    // #endregion search-parent

    println("=== search-focus")
    // #region search-focus
    // Many places share a name; a focus (the user's location) ranks the near ones first.
    val options = SearchOptions(28.7, 80.6, 30_000.0, false)
    for (p in Reader(engine.searchWith("Shivapur", 3, options))) {
        println("%s  %s  %.4f, %.4f".f(p["id"], p["name"], p["lat"], p["lon"]))
    }
    // #endregion search-focus

    println("=== get")
    // #region get
    val place = Reader(engine.get("tgd:np/1xhb")).single()
    println("${place["name"]} ${place["kind"]}")
    println("at %.6f, %.6f, %d m".f(place["lat"], place["lon"], place["elevation"]))
    println("timezone ${place["tz"]} parent ${place["parent"]}")
    // #endregion get

    println("=== hierarchy")
    // #region hierarchy
    for (p in Reader(engine.ancestors("tgd:np/1xhb"))) {
        println("${p["kind"]} ${p["level"]}  ${p["name"]}  ${p["iso"] ?: ""}".trimEnd())
    }
    val children = Reader(engine.children("tgd:np/295c"))
    println("Kaski has ${children.rows()} children")
    // #endregion hierarchy

    println("=== reverse")
    // #region reverse
    for (p in Reader(engine.reverse(27.7172, 85.3240, 3))) {
        println("%s  %.0f m at %.0f°".f(p["name"], p["meters"], p["bearing"]))
    }
    // #endregion reverse

    println("=== radius")
    // #region radius
    val within = Reader(engine.withinRadius(27.7172, 85.3240, 500.0, 10)).records()
    println("%d places within 500 m; farthest %.0f m".f(within.size, within.last()["meters"]))
    // #endregion radius

    println("=== nearby")
    // #region nearby
    for (p in Reader(engine.nearby("tgd:np/2cn", 3))) {
        println("%s  %.0f m".f(p["name"], p["meters"]))
    }
    // #endregion nearby

    println("=== admin")
    // #region admin
    for (p in Reader(engine.adminAt(28.2096, 83.9856))) {
        println("${p["level"]}  ${p["name"]}  ${p["iso"] ?: ""}".trimEnd())
    }
    val state = Reader(engine.byIso("NP-P4")).single()
    println("NP-P4 is ${state["name"]} ${state["id"]}")
    // #endregion admin

    println("=== timezone")
    // #region timezone
    val zone = Reader(engine.tzAt(27.7172, 85.3240)).single()["tz"] as String
    val offset = Reader(engine.tzOffsetAt(zone, 1_700_000_000)).single()
    println("$zone ${offset["seconds"]} s east of UTC, ${offset["abbr"]} dst ${offset["dst"]}")
    val local = Reader(engine.tzLocal("America/New_York", 2026, 3, 8, 2, 30, 0)).single()
    println("02:30 on 2026-03-08 in New York: ${local["kind"]} ${local["before"]} -> ${local["after"]}")
    println("tzdb ${engine.tzVersion()}")
    // #endregion timezone

    println("=== elevation")
    // #region elevation
    val height = Reader(engine.elevationAt(27.7172, 85.3240)).single()
    println("%.1f m above %s (%s, %.0f m grid)".f(height["meters"], height["datum"], height["source"], height["resolution_m"]))
    // #endregion elevation

    println("=== distance")
    // #region distance
    val meters = TeiGeodata.distanceM(27.7172, 85.3240, 28.2096, 83.9856)
    val heading = TeiGeodata.bearingDeg(27.7172, 85.3240, 28.2096, 83.9856)
    println("Kathmandu to Pokhara: %.1f km, heading %.0f°".f(meters / 1000, heading))
    val apart = Reader(engine.distance("tgd:np/2cn", "tgd:np/1xhb")).single()
    println("place to place: %.1f km at %.0f°".f(apart["meters"] as Double / 1000, apart["bearing"]))
    // #endregion distance

    println("=== boundary")
    // #region boundary
    val shape = Reader(engine.boundary("tgd:np/2gm", 0)).single()
    val geojson = shape["geojson"] as String // a GeoJSON Feature, for any map library
    val type = geojson.substringAfter("\"geometry\":{\"type\":\"").substringBefore('"')
    println("%s of %s vertices at level %s".f(type, shape["vertices"], shape["lod"]))
    println("tolerance %.0f m, %d bytes of GeoJSON".f(shape["tolerance_m"], geojson.length))
    // #endregion boundary

    println("=== errors")
    // #region errors
    try {
        engine.reverse(91.0, 0.0, 1)
    } catch (error: TeiGeodataException.InvalidArgument) {
        println("invalid argument: ${error.message}")
    }
    // #endregion errors
}
```
</Tab>

<Tab value="Java">
```java title="Tour.java"
// The tour: every query of the guide, run on a released pack.
//
// Usage: java --enable-native-access=ALL-UNNAMED -cp <the package's classes> Tour.java world-np-lite.tgd

// #region import
import java.util.List;
import java.util.Locale;
import java.util.Map;
import org.teispace.teigeodata.Reader;
import org.teispace.teigeodata.TeiGeodata;
import org.teispace.teigeodata.TeiGeodata.Engine;
import org.teispace.teigeodata.TeiGeodata.SearchOptions;
import org.teispace.teigeodata.TeiGeodataException;
// #endregion import

public final class Tour {
    /** {@code String.format} in the root locale: a decimal point whatever the user's locale. */
    static String f(String format, Object... args) {
        return String.format(Locale.ROOT, format, args);
    }

    /** {@code value}, or "" where it is null. */
    static Object orEmpty(Object value) {
        return value == null ? "" : value;
    }

    public static void main(String[] args) {
        String pack = args[0];

        System.out.println("=== open");
        // #region open
        try (Engine engine = new Engine()) {
            engine.openFile(pack);
            System.out.println("layers: " + engine.layers());
            // #endregion open
            tour(engine);
        }
    }

    static void tour(Engine engine) {
        System.out.println("=== search");
        // #region search
        for (Map<String, Object> p : new Reader(engine.search("Pokhara", 3))) {
            System.out.println(f("%s  %s  %s  %.4f, %.4f  score %d",
                    p.get("id"), p.get("name"), p.get("kind"), p.get("lat"), p.get("lon"), p.get("score")));
        }
        // #endregion search

        System.out.println("=== search-parent");
        // #region search-parent
        for (Map<String, Object> p : new Reader(engine.search("Kathmandu, Bagmati", 2))) {
            System.out.println(f("%s  %s  %s %d", p.get("id"), p.get("name"), p.get("kind"), p.get("level")));
        }
        // #endregion search-parent

        System.out.println("=== search-focus");
        // #region search-focus
        // Many places share a name; a focus (the user's location) ranks the near ones first.
        SearchOptions options = SearchOptions.DEFAULTS.withFocusLat(28.7).withFocusLon(80.6).withFocusScaleM(30_000.0);
        for (Map<String, Object> p : new Reader(engine.searchWith("Shivapur", 3, options))) {
            System.out.println(f("%s  %s  %.4f, %.4f", p.get("id"), p.get("name"), p.get("lat"), p.get("lon")));
        }
        // #endregion search-focus

        System.out.println("=== get");
        // #region get
        Map<String, Object> place = new Reader(engine.get("tgd:np/1xhb")).records().get(0);
        System.out.println(place.get("name") + " " + place.get("kind"));
        System.out.println(f("at %.6f, %.6f, %d m", place.get("lat"), place.get("lon"), place.get("elevation")));
        System.out.println("timezone " + place.get("tz") + " parent " + place.get("parent"));
        // #endregion get

        System.out.println("=== hierarchy");
        // #region hierarchy
        for (Map<String, Object> p : new Reader(engine.ancestors("tgd:np/1xhb"))) {
            System.out.println(f("%s %d  %s  %s", p.get("kind"), p.get("level"), p.get("name"), orEmpty(p.get("iso"))).stripTrailing());
        }
        Reader children = new Reader(engine.children("tgd:np/295c"));
        System.out.println("Kaski has " + children.rows() + " children");
        // #endregion hierarchy

        System.out.println("=== reverse");
        // #region reverse
        for (Map<String, Object> p : new Reader(engine.reverse(27.7172, 85.3240, 3))) {
            System.out.println(f("%s  %.0f m at %.0f°", p.get("name"), p.get("meters"), p.get("bearing")));
        }
        // #endregion reverse

        System.out.println("=== radius");
        // #region radius
        List<Map<String, Object>> within = new Reader(engine.withinRadius(27.7172, 85.3240, 500, 10)).records();
        System.out.println(f("%d places within 500 m; farthest %.0f m",
                within.size(), within.get(within.size() - 1).get("meters")));
        // #endregion radius

        System.out.println("=== nearby");
        // #region nearby
        for (Map<String, Object> p : new Reader(engine.nearby("tgd:np/2cn", 3))) {
            System.out.println(f("%s  %.0f m", p.get("name"), p.get("meters")));
        }
        // #endregion nearby

        System.out.println("=== admin");
        // #region admin
        for (Map<String, Object> p : new Reader(engine.adminAt(28.2096, 83.9856))) {
            System.out.println(f("%d  %s  %s", p.get("level"), p.get("name"), orEmpty(p.get("iso"))).stripTrailing());
        }
        Map<String, Object> state = new Reader(engine.byIso("NP-P4")).records().get(0);
        System.out.println("NP-P4 is " + state.get("name") + " " + state.get("id"));
        // #endregion admin

        System.out.println("=== timezone");
        // #region timezone
        String zone = (String) new Reader(engine.tzAt(27.7172, 85.3240)).records().get(0).get("tz");
        Map<String, Object> offset = new Reader(engine.tzOffsetAt(zone, 1_700_000_000L)).records().get(0);
        System.out.println(zone + " " + offset.get("seconds") + " s east of UTC, " + offset.get("abbr") + " dst " + offset.get("dst"));
        Map<String, Object> local = new Reader(engine.tzLocal("America/New_York", 2026, 3, 8, 2, 30, 0)).records().get(0);
        System.out.println("02:30 on 2026-03-08 in New York: " + local.get("kind") + " " + local.get("before") + " -> " + local.get("after"));
        System.out.println("tzdb " + engine.tzVersion());
        // #endregion timezone

        System.out.println("=== elevation");
        // #region elevation
        Map<String, Object> height = new Reader(engine.elevationAt(27.7172, 85.3240)).records().get(0);
        System.out.println(f("%.1f m above %s (%s, %.0f m grid)",
                height.get("meters"), height.get("datum"), height.get("source"), height.get("resolution_m")));
        // #endregion elevation

        System.out.println("=== distance");
        // #region distance
        double meters = TeiGeodata.distanceM(27.7172, 85.3240, 28.2096, 83.9856);
        double heading = TeiGeodata.bearingDeg(27.7172, 85.3240, 28.2096, 83.9856);
        System.out.println(f("Kathmandu to Pokhara: %.1f km, heading %.0f°", meters / 1000, heading));
        Map<String, Object> apart = new Reader(engine.distance("tgd:np/2cn", "tgd:np/1xhb")).records().get(0);
        System.out.println(f("place to place: %.1f km at %.0f°", (Double) apart.get("meters") / 1000, apart.get("bearing")));
        // #endregion distance

        System.out.println("=== boundary");
        // #region boundary
        Map<String, Object> shape = new Reader(engine.boundary("tgd:np/2gm", 0)).records().get(0);
        String geojson = (String) shape.get("geojson"); // a GeoJSON Feature, for any map library
        int at = geojson.indexOf("\"type\":\"", geojson.indexOf("\"geometry\"")) + 8;
        String type = geojson.substring(at, geojson.indexOf('"', at));
        System.out.println(f("%s of %s vertices at level %s", type, shape.get("vertices"), shape.get("lod")));
        System.out.println(f("tolerance %.0f m, %d bytes of GeoJSON", shape.get("tolerance_m"), geojson.length()));
        // #endregion boundary

        System.out.println("=== errors");
        // #region errors
        try {
            engine.reverse(91, 0, 1);
        } catch (TeiGeodataException.InvalidArgument error) {
            System.out.println("invalid argument: " + error.getMessage());
        }
        // #endregion errors
    }
}
```
</Tab>

<Tab value="Dart">
```dart title="tour.dart"
// The tour: every query of the guide, run on a released pack.
//
// Usage: dart run bin/tour.dart world-np-lite.tgd

// #region import
import 'dart:convert';

import 'package:teigeodata/teigeodata.dart';

// #endregion import

void main(List<String> args) async {
  final pack = args[0];

  print('=== open');
  // #region open
  await TeiGeodata.init(); // loads the module on the web; returns at once natively
  final tg = TeiGeodata();
  final engine = Engine(tg);
  engine.openFile(pack);
  print('layers: ${engine.layers()}');
  // #endregion open

  print('=== search');
  // #region search
  for (final p in Reader(engine.search('Pokhara', 3))) {
    final (lat, lon) = (p['lat'] as double, p['lon'] as double);
    print(
      '${p['id']}  ${p['name']}  ${p['kind']}  ${lat.toStringAsFixed(4)}, '
      '${lon.toStringAsFixed(4)}  score ${p['score']}',
    );
  }
  // #endregion search

  print('=== search-parent');
  // #region search-parent
  for (final p in Reader(engine.search('Kathmandu, Bagmati', 2))) {
    print('${p['id']}  ${p['name']}  ${p['kind']} ${p['level']}');
  }
  // #endregion search-parent

  print('=== search-focus');
  // #region search-focus
  // Many places share a name; a focus (the user's location) ranks the near ones first.
  const options = SearchOptions(
    focusLat: 28.7,
    focusLon: 80.6,
    focusScaleM: 30000,
  );
  for (final p in Reader(engine.searchWith('Shivapur', 3, options))) {
    final (lat, lon) = (p['lat'] as double, p['lon'] as double);
    print(
      '${p['id']}  ${p['name']}  ${lat.toStringAsFixed(4)}, ${lon.toStringAsFixed(4)}',
    );
  }
  // #endregion search-focus

  print('=== get');
  // #region get
  final place = Reader(engine.get('tgd:np/1xhb')).single;
  print('${place['name']} ${place['kind']}');
  final (lat, lon) = (place['lat'] as double, place['lon'] as double);
  print(
    'at ${lat.toStringAsFixed(6)}, ${lon.toStringAsFixed(6)}, ${place['elevation']} m',
  );
  print('timezone ${place['tz']} parent ${place['parent']}');
  // #endregion get

  print('=== hierarchy');
  // #region hierarchy
  for (final p in Reader(engine.ancestors('tgd:np/1xhb'))) {
    print(
      '${p['kind']} ${p['level']}  ${p['name']}  ${p['iso'] ?? ''}'.trimRight(),
    );
  }
  final children = Reader(engine.children('tgd:np/295c'));
  print('Kaski has ${children.rows} children');
  // #endregion hierarchy

  print('=== reverse');
  // #region reverse
  for (final p in Reader(engine.reverse(27.7172, 85.3240, 3))) {
    final (meters, bearing) = (p['meters'] as double, p['bearing'] as double);
    print(
      '${p['name']}  ${meters.toStringAsFixed(0)} m at ${bearing.toStringAsFixed(0)}°',
    );
  }
  // #endregion reverse

  print('=== radius');
  // #region radius
  final within = Reader(
    engine.withinRadius(27.7172, 85.3240, 500, 10),
  ).records();
  final farthest = within.last['meters'] as double;
  print(
    '${within.length} places within 500 m; farthest ${farthest.toStringAsFixed(0)} m',
  );
  // #endregion radius

  print('=== nearby');
  // #region nearby
  for (final p in Reader(engine.nearby('tgd:np/2cn', 3))) {
    print('${p['name']}  ${(p['meters'] as double).toStringAsFixed(0)} m');
  }
  // #endregion nearby

  print('=== admin');
  // #region admin
  for (final p in Reader(engine.adminAt(28.2096, 83.9856))) {
    print('${p['level']}  ${p['name']}  ${p['iso'] ?? ''}'.trimRight());
  }
  final state = Reader(engine.byIso('NP-P4')).single;
  print('NP-P4 is ${state['name']} ${state['id']}');
  // #endregion admin

  print('=== timezone');
  // #region timezone
  final zone = Reader(engine.tzAt(27.7172, 85.3240)).single['tz'] as String;
  final offset = Reader(engine.tzOffsetAt(zone, 1700000000)).single;
  print(
    '$zone ${offset['seconds']} s east of UTC, ${offset['abbr']} dst ${offset['dst']}',
  );
  final local = Reader(
    engine.tzLocal('America/New_York', 2026, 3, 8, 2, 30, 0),
  ).single;
  print(
    '02:30 on 2026-03-08 in New York: ${local['kind']} ${local['before']} -> ${local['after']}',
  );
  print('tzdb ${engine.tzVersion()}');
  // #endregion timezone

  print('=== elevation');
  // #region elevation
  final height = Reader(engine.elevationAt(27.7172, 85.3240)).single;
  final (meters, grid) = (
    height['meters'] as double,
    height['resolution_m'] as double,
  );
  print(
    '${meters.toStringAsFixed(1)} m above ${height['datum']} '
    '(${height['source']}, ${grid.toStringAsFixed(0)} m grid)',
  );
  // #endregion elevation

  print('=== distance');
  // #region distance
  final km = tg.distanceM(27.7172, 85.3240, 28.2096, 83.9856) / 1000;
  final heading = tg.bearingDeg(27.7172, 85.3240, 28.2096, 83.9856);
  print('Kathmandu to Pokhara: ${km.toStringAsFixed(1)} km, heading ${heading.toStringAsFixed(0)}°');
  final apart = Reader(engine.distance('tgd:np/2cn', 'tgd:np/1xhb')).single;
  final (far, bearing) = (apart['meters'] as double, apart['bearing'] as double);
  print('place to place: ${(far / 1000).toStringAsFixed(1)} km at ${bearing.toStringAsFixed(0)}°');
  // #endregion distance

  print('=== boundary');
  // #region boundary
  final shape = Reader(engine.boundary('tgd:np/2gm', 0)).single;
  final geojson = shape['geojson'] as String; // a GeoJSON Feature, for any map library
  final feature = jsonDecode(geojson) as Map<String, dynamic>;
  print('${feature['geometry']['type']} of ${shape['vertices']} vertices at level ${shape['lod']}');
  final tolerance = shape['tolerance_m'] as double;
  print('tolerance ${tolerance.toStringAsFixed(0)} m, ${utf8.encode(geojson).length} bytes of GeoJSON');
  // #endregion boundary

  print('=== errors');
  // #region errors
  try {
    engine.reverse(91, 0, 1);
  } on InvalidArgumentException catch (error) {
    print('invalid argument: ${error.message}');
  }
  // #endregion errors

  engine.close();
  tg.close();
}
```
</Tab>

<Tab value="Go">
```go title="tour.go"
// The tour: every query of the guide, run on a released pack.
//
// Usage: go run . world-np-lite.tgd
package main

// #region import
import (
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"os"
	"strings"

	tg "github.com/teispace/teigeodata/bindings/go"
)

// #endregion import

// #region rows
// rows reads a result: every row as a Record (column name to value, nil where null).
func rows(blob []byte, err error) []tg.Record {
	if err != nil {
		log.Fatal(err)
	}
	r, err := tg.NewReader(blob)
	if err != nil {
		log.Fatal(err)
	}
	records, err := r.Records()
	if err != nil {
		log.Fatal(err)
	}
	return records
}

// #endregion rows

// orEmpty is a nullable string column's value, "" where null.
func orEmpty(v any) string {
	if s, ok := v.(string); ok {
		return s
	}
	return ""
}

func main() {
	pack := os.Args[1]

	fmt.Println("=== open")
	// #region open
	engine, err := tg.NewEngine()
	if err != nil {
		log.Fatal(err)
	}
	defer engine.Close()
	if _, err := engine.OpenFile(pack); err != nil {
		log.Fatal(err)
	}
	fmt.Println("layers:", engine.Layers())
	// #endregion open

	fmt.Println("=== search")
	// #region search
	for _, p := range rows(engine.Search("Pokhara", 3)) {
		fmt.Printf("%s  %s  %s  %.4f, %.4f  score %d\n",
			p["id"], p["name"], p["kind"], p["lat"], p["lon"], p["score"])
	}
	// #endregion search

	fmt.Println("=== search-parent")
	// #region search-parent
	for _, p := range rows(engine.Search("Kathmandu, Bagmati", 2)) {
		fmt.Printf("%s  %s  %s %d\n", p["id"], p["name"], p["kind"], p["level"])
	}
	// #endregion search-parent

	fmt.Println("=== search-focus")
	// #region search-focus
	// Many places share a name; a focus (the user's location) ranks the near ones first.
	lat, lon, scale := 28.7, 80.6, 30_000.0
	options := &tg.SearchOptions{FocusLat: &lat, FocusLon: &lon, FocusScaleM: &scale}
	for _, p := range rows(engine.SearchWith("Shivapur", 3, options)) {
		fmt.Printf("%s  %s  %.4f, %.4f\n", p["id"], p["name"], p["lat"], p["lon"])
	}
	// #endregion search-focus

	fmt.Println("=== get")
	// #region get
	place := rows(engine.Get("tgd:np/1xhb"))[0]
	fmt.Println(place["name"], place["kind"])
	fmt.Printf("at %.6f, %.6f, %d m\n", place["lat"], place["lon"], place["elevation"])
	fmt.Println("timezone", place["tz"], "parent", place["parent"])
	// #endregion get

	fmt.Println("=== hierarchy")
	// #region hierarchy
	for _, p := range rows(engine.Ancestors("tgd:np/1xhb")) {
		line := fmt.Sprintf("%s %d  %s  %s", p["kind"], p["level"], p["name"], orEmpty(p["iso"]))
		fmt.Println(strings.TrimRight(line, " "))
	}
	children := rows(engine.Children("tgd:np/295c"))
	fmt.Println("Kaski has", len(children), "children")
	// #endregion hierarchy

	fmt.Println("=== reverse")
	// #region reverse
	for _, p := range rows(engine.Reverse(27.7172, 85.3240, 3)) {
		fmt.Printf("%s  %.0f m at %.0f°\n", p["name"], p["meters"], p["bearing"])
	}
	// #endregion reverse

	fmt.Println("=== radius")
	// #region radius
	within := rows(engine.WithinRadius(27.7172, 85.3240, 500, 10))
	fmt.Printf("%d places within 500 m; farthest %.0f m\n", len(within), within[len(within)-1]["meters"])
	// #endregion radius

	fmt.Println("=== nearby")
	// #region nearby
	for _, p := range rows(engine.Nearby("tgd:np/2cn", 3)) {
		fmt.Printf("%s  %.0f m\n", p["name"], p["meters"])
	}
	// #endregion nearby

	fmt.Println("=== admin")
	// #region admin
	for _, p := range rows(engine.AdminAt(28.2096, 83.9856)) {
		fmt.Println(strings.TrimRight(fmt.Sprintf("%d  %s  %s", p["level"], p["name"], orEmpty(p["iso"])), " "))
	}
	state := rows(engine.ByIso("NP-P4"))[0]
	fmt.Println("NP-P4 is", state["name"], state["id"])
	// #endregion admin

	fmt.Println("=== timezone")
	// #region timezone
	zone := rows(engine.TzAt(27.7172, 85.3240))[0]["tz"].(string)
	offset := rows(engine.TzOffsetAt(zone, 1_700_000_000))[0]
	fmt.Println(zone, offset["seconds"], "s east of UTC,", offset["abbr"], "dst", offset["dst"])
	local := rows(engine.TzLocal("America/New_York", 2026, 3, 8, 2, 30, 0))[0]
	fmt.Println("02:30 on 2026-03-08 in New York:", local["kind"], local["before"], "->", local["after"])
	fmt.Println("tzdb", engine.TzVersion())
	// #endregion timezone

	fmt.Println("=== elevation")
	// #region elevation
	height := rows(engine.ElevationAt(27.7172, 85.3240))[0]
	fmt.Printf("%.1f m above %s (%s, %.0f m grid)\n",
		height["meters"], height["datum"], height["source"], height["resolution_m"])
	// #endregion elevation

	fmt.Println("=== distance")
	// #region distance
	meters, err := tg.DistanceM(27.7172, 85.3240, 28.2096, 83.9856)
	if err != nil {
		log.Fatal(err)
	}
	heading, err := tg.BearingDeg(27.7172, 85.3240, 28.2096, 83.9856)
	if err != nil {
		log.Fatal(err)
	}
	fmt.Printf("Kathmandu to Pokhara: %.1f km, heading %.0f°\n", meters/1000, heading)
	apart := rows(engine.Distance("tgd:np/2cn", "tgd:np/1xhb"))[0]
	fmt.Printf("place to place: %.1f km at %.0f°\n", apart["meters"].(float64)/1000, apart["bearing"])
	// #endregion distance

	fmt.Println("=== boundary")
	// #region boundary
	shape := rows(engine.Boundary("tgd:np/2gm", 0))[0]
	geojson := shape["geojson"].(string) // a GeoJSON Feature, for any map library
	var feature struct {
		Geometry struct{ Type string } `json:"geometry"`
	}
	if err := json.Unmarshal([]byte(geojson), &feature); err != nil {
		log.Fatal(err)
	}
	fmt.Printf("%s of %d vertices at level %d\n", feature.Geometry.Type, shape["vertices"], shape["lod"])
	fmt.Printf("tolerance %.0f m, %d bytes of GeoJSON\n", shape["tolerance_m"], len(geojson))
	// #endregion boundary

	fmt.Println("=== errors")
	// #region errors
	_, err = engine.Reverse(91, 0, 1)
	var failure *tg.Error
	if errors.As(err, &failure) && failure.Code == tg.CodeInvalidArgument {
		fmt.Println("invalid argument:", failure.Message)
	}
	// #endregion errors
}
```
</Tab>

<Tab value="C">
```c title="tour.c"
/* The tour: every query of the guide, run on a released pack.
 *
 * Usage: cc -std=c11 tour.c -I<include> -L<lib> -lteigeodata -o tour && ./tour world-np-lite.tgd
 */

/* #region import */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "teigeodata.h"
#include "teigeodata_reader.h"
/* #endregion import */

/* #region helpers */
#define TEXT(s) (const uint8_t *)(s), strlen(s)

/* Exits with the thread's last error when a call failed. */
static void check(int32_t status) {
    if (status == TG_OK) return;
    tg_bytes message = {0};
    tg_last_error(&message);
    fprintf(stderr, "error %d: %.*s\n", (int)status, (int)message.len, (const char *)message.ptr);
    tg_bytes_free(message);
    exit(1);
}

/* A reader over a result the call just wrote into `blob`. */
static tg_reader reader(int32_t status, const tg_bytes *blob) {
    check(status);
    tg_reader r;
    check(tg_reader_init(&r, blob->ptr, blob->len));
    return r;
}

/* Column `name` of row `row`: a string ("" where null), a number, an integer. */
static tg_str str(const tg_reader *r, const char *name, uint32_t row) {
    tg_str s = tg_reader_string(r, tg_reader_find(r, name), row);
    if (s.ptr == NULL) s.ptr = "";
    return s;
}
static double num(const tg_reader *r, const char *name, uint32_t row) {
    return tg_reader_number(r, tg_reader_find(r, name), row);
}
static long long integer(const tg_reader *r, const char *name, uint32_t row) {
    return (long long)tg_reader_int(r, tg_reader_find(r, name), row);
}
#define S(r, name, row) (int)str(r, name, row).len, str(r, name, row).ptr
/* #endregion helpers */

int main(int argc, char **argv) {
    if (argc < 2) return 2;
    const char *pack = argv[1];
    tg_bytes blob = {0};
    tg_reader r;

    puts("=== open");
    /* #region open */
    tg_engine *engine = NULL;
    check(tg_engine_new(&engine));
    uint32_t layer = 0, layers = 0;
    check(tg_engine_open_file(engine, TEXT(pack), &layer));
    check(tg_engine_layers(engine, &layers));
    printf("layers: %u\n", layers);
    /* #endregion open */

    puts("=== search");
    /* #region search */
    r = reader(tg_engine_search(engine, TEXT("Pokhara"), 3, &blob), &blob);
    for (uint32_t i = 0; i < r.rows; i++) {
        printf("%.*s  %.*s  %.*s  %.4f, %.4f  score %lld\n", S(&r, "id", i), S(&r, "name", i), S(&r, "kind", i),
               num(&r, "lat", i), num(&r, "lon", i), integer(&r, "score", i));
    }
    tg_bytes_free(blob);
    /* #endregion search */

    puts("=== search-parent");
    /* #region search-parent */
    r = reader(tg_engine_search(engine, TEXT("Kathmandu, Bagmati"), 2, &blob), &blob);
    for (uint32_t i = 0; i < r.rows; i++) {
        printf("%.*s  %.*s  %.*s %lld\n", S(&r, "id", i), S(&r, "name", i), S(&r, "kind", i), integer(&r, "level", i));
    }
    tg_bytes_free(blob);
    /* #endregion search-parent */

    puts("=== search-focus");
    /* #region search-focus */
    /* Many places share a name; a focus (the user's location) ranks the near ones first. */
    tg_search_options options = {0};
    options.has_focus_lat = true;
    options.focus_lat = 28.7;
    options.has_focus_lon = true;
    options.focus_lon = 80.6;
    options.has_focus_scale_m = true;
    options.focus_scale_m = 30000;
    r = reader(tg_engine_search_with(engine, TEXT("Shivapur"), 3, &options, &blob), &blob);
    for (uint32_t i = 0; i < r.rows; i++) {
        printf("%.*s  %.*s  %.4f, %.4f\n", S(&r, "id", i), S(&r, "name", i), num(&r, "lat", i), num(&r, "lon", i));
    }
    tg_bytes_free(blob);
    /* #endregion search-focus */

    puts("=== get");
    /* #region get */
    r = reader(tg_engine_get(engine, TEXT("tgd:np/1xhb"), &blob), &blob);
    printf("%.*s %.*s\n", S(&r, "name", 0), S(&r, "kind", 0));
    printf("at %.6f, %.6f, %lld m\n", num(&r, "lat", 0), num(&r, "lon", 0), integer(&r, "elevation", 0));
    printf("timezone %.*s parent %.*s\n", S(&r, "tz", 0), S(&r, "parent", 0));
    tg_bytes_free(blob);
    /* #endregion get */

    puts("=== hierarchy");
    /* #region hierarchy */
    r = reader(tg_engine_ancestors(engine, TEXT("tgd:np/1xhb"), &blob), &blob);
    for (uint32_t i = 0; i < r.rows; i++) {
        printf("%.*s %lld  %.*s", S(&r, "kind", i), integer(&r, "level", i), S(&r, "name", i));
        if (!tg_reader_is_null(&r, tg_reader_find(&r, "iso"), i)) printf("  %.*s", S(&r, "iso", i));
        putchar('\n');
    }
    tg_bytes_free(blob);
    r = reader(tg_engine_children(engine, TEXT("tgd:np/295c"), &blob), &blob);
    printf("Kaski has %u children\n", r.rows);
    tg_bytes_free(blob);
    /* #endregion hierarchy */

    puts("=== reverse");
    /* #region reverse */
    r = reader(tg_engine_reverse(engine, 27.7172, 85.3240, 3, &blob), &blob);
    for (uint32_t i = 0; i < r.rows; i++) {
        printf("%.*s  %.0f m at %.0f°\n", S(&r, "name", i), num(&r, "meters", i), num(&r, "bearing", i));
    }
    tg_bytes_free(blob);
    /* #endregion reverse */

    puts("=== radius");
    /* #region radius */
    r = reader(tg_engine_within_radius(engine, 27.7172, 85.3240, 500, 10, &blob), &blob);
    printf("%u places within 500 m; farthest %.0f m\n", r.rows, num(&r, "meters", r.rows - 1));
    tg_bytes_free(blob);
    /* #endregion radius */

    puts("=== nearby");
    /* #region nearby */
    r = reader(tg_engine_nearby(engine, TEXT("tgd:np/2cn"), 3, &blob), &blob);
    for (uint32_t i = 0; i < r.rows; i++) printf("%.*s  %.0f m\n", S(&r, "name", i), num(&r, "meters", i));
    tg_bytes_free(blob);
    /* #endregion nearby */

    puts("=== admin");
    /* #region admin */
    r = reader(tg_engine_admin_at(engine, 28.2096, 83.9856, &blob), &blob);
    for (uint32_t i = 0; i < r.rows; i++) {
        printf("%lld  %.*s", integer(&r, "level", i), S(&r, "name", i));
        if (!tg_reader_is_null(&r, tg_reader_find(&r, "iso"), i)) printf("  %.*s", S(&r, "iso", i));
        putchar('\n');
    }
    tg_bytes_free(blob);
    r = reader(tg_engine_by_iso(engine, TEXT("NP-P4"), &blob), &blob);
    printf("NP-P4 is %.*s %.*s\n", S(&r, "name", 0), S(&r, "id", 0));
    tg_bytes_free(blob);
    /* #endregion admin */

    puts("=== timezone");
    /* #region timezone */
    tg_bytes area = {0}, at = {0}, local = {0}, version = {0};
    tg_reader zr = reader(tg_engine_tz_at(engine, 27.7172, 85.3240, &area), &area);
    tg_str zone = str(&zr, "tz", 0);
    r = reader(tg_engine_tz_offset_at(engine, (const uint8_t *)zone.ptr, zone.len, 1700000000, &at), &at);
    printf("%.*s %lld s east of UTC, %.*s dst %lld\n", (int)zone.len, zone.ptr, integer(&r, "seconds", 0),
           S(&r, "abbr", 0), integer(&r, "dst", 0));
    r = reader(tg_engine_tz_local(engine, TEXT("America/New_York"), 2026, 3, 8, 2, 30, 0, &local), &local);
    printf("02:30 on 2026-03-08 in New York: %.*s %lld -> %lld\n", S(&r, "kind", 0), integer(&r, "before", 0),
           integer(&r, "after", 0));
    check(tg_engine_tz_version(engine, &version));
    printf("tzdb %.*s\n", (int)version.len, (const char *)version.ptr);
    tg_bytes_free(area);
    tg_bytes_free(at);
    tg_bytes_free(local);
    tg_bytes_free(version);
    /* #endregion timezone */

    puts("=== elevation");
    /* #region elevation */
    r = reader(tg_engine_elevation_at(engine, 27.7172, 85.3240, &blob), &blob);
    printf("%.1f m above %.*s (%.*s, %.0f m grid)\n", num(&r, "meters", 0), S(&r, "datum", 0), S(&r, "source", 0),
           num(&r, "resolution_m", 0));
    tg_bytes_free(blob);
    /* #endregion elevation */

    puts("=== distance");
    /* #region distance */
    double meters = 0;
    check(tg_distance_m(27.7172, 85.3240, 28.2096, 83.9856, &meters));
    double heading = 0;
    check(tg_bearing_deg(27.7172, 85.3240, 28.2096, 83.9856, &heading));
    printf("Kathmandu to Pokhara: %.1f km, heading %.0f°\n", meters / 1000, heading);
    r = reader(tg_engine_distance(engine, TEXT("tgd:np/2cn"), TEXT("tgd:np/1xhb"), &blob), &blob);
    printf("place to place: %.1f km at %.0f°\n", num(&r, "meters", 0) / 1000, num(&r, "bearing", 0));
    tg_bytes_free(blob);
    /* #endregion distance */

    puts("=== boundary");
    /* #region boundary */
    r = reader(tg_engine_boundary(engine, TEXT("tgd:np/2gm"), 0, &blob), &blob);
    tg_str geojson = str(&r, "geojson", 0); /* a GeoJSON Feature, for any map library */
    const char *type = strstr(geojson.ptr, "\"geometry\":{\"type\":\"") + 20;
    printf("%.*s of %lld vertices at level %lld\n", (int)strcspn(type, "\""), type, integer(&r, "vertices", 0),
           integer(&r, "lod", 0));
    printf("tolerance %.0f m, %zu bytes of GeoJSON\n", num(&r, "tolerance_m", 0), geojson.len);
    tg_bytes_free(blob);
    /* #endregion boundary */

    puts("=== errors");
    /* #region errors */
    if (tg_engine_reverse(engine, 91, 0, 1, &blob) == TG_ERROR_INVALID_ARGUMENT) {
        tg_bytes message = {0};
        tg_last_error(&message);
        printf("invalid argument: %.*s\n", (int)message.len, (const char *)message.ptr);
        tg_bytes_free(message);
    }
    /* #endregion errors */

    tg_engine_free(engine);
    return 0;
}
```
</Tab>

<Tab value="C++">
```cpp title="tour.cpp"
// The tour: every query of the guide, run on a released pack.
//
// Usage: c++ -std=c++17 tour.cpp -I<include> -L<lib> -lteigeodata -o tour && ./tour world-np-lite.tgd

// #region import
#include <cstdio>
#include <iostream>
#include <string>

#include "teigeodata.hpp"
#include "teigeodata_reader.hpp"

namespace tg = teigeodata;
// #endregion import

/// `value` with `digits` decimals, as printf("%.*f") writes it.
static std::string fixed(std::optional<double> value, int digits) {
    char text[64];
    std::snprintf(text, sizeof text, "%.*f", digits, value.value_or(0.0));
    return text;
}

int main(int argc, char **argv) try {
    if (argc < 2) return 2;
    const std::string pack = argv[1];

    std::cout << "=== open\n";
    // #region open
    tg::Engine engine;
    engine.open_file(pack);
    std::cout << "layers: " << engine.layers() << '\n';
    // #endregion open

    std::cout << "=== search\n";
    // #region search
    const tg::Blob hits = engine.search("Pokhara", 3);
    for (auto p : tg::Reader(hits)) {
        std::cout << *p.string("id") << "  " << *p.string("name") << "  " << *p.string("kind") << "  "
                  << fixed(p.number("lat"), 4) << ", " << fixed(p.number("lon"), 4) << "  score "
                  << *p.integer("score") << '\n';
    }
    // #endregion search

    std::cout << "=== search-parent\n";
    // #region search-parent
    const tg::Blob parents = engine.search("Kathmandu, Bagmati", 2);
    for (auto p : tg::Reader(parents)) {
        std::cout << *p.string("id") << "  " << *p.string("name") << "  " << *p.string("kind") << ' '
                  << *p.integer("level") << '\n';
    }
    // #endregion search-parent

    std::cout << "=== search-focus\n";
    // #region search-focus
    // Many places share a name; a focus (the user's location) ranks the near ones first.
    tg::SearchOptions options;
    options.focus_lat = 28.7;
    options.focus_lon = 80.6;
    options.focus_scale_m = 30000;
    const tg::Blob near_focus = engine.search_with("Shivapur", 3, options);
    for (auto p : tg::Reader(near_focus)) {
        std::cout << *p.string("id") << "  " << *p.string("name") << "  " << fixed(p.number("lat"), 4) << ", "
                  << fixed(p.number("lon"), 4) << '\n';
    }
    // #endregion search-focus

    std::cout << "=== get\n";
    // #region get
    const tg::Blob got = engine.get("tgd:np/1xhb");
    const tg::Reader one(got);
    const auto place = one[0];
    std::cout << *place.string("name") << ' ' << *place.string("kind") << '\n';
    std::cout << "at " << fixed(place.number("lat"), 6) << ", " << fixed(place.number("lon"), 6) << ", "
              << *place.integer("elevation") << " m\n";
    std::cout << "timezone " << *place.string("tz") << " parent " << *place.string("parent") << '\n';
    // #endregion get

    std::cout << "=== hierarchy\n";
    // #region hierarchy
    const tg::Blob ancestors = engine.ancestors("tgd:np/1xhb");
    for (auto p : tg::Reader(ancestors)) {
        std::cout << *p.string("kind") << ' ' << *p.integer("level") << "  " << *p.string("name");
        if (auto iso = p.string("iso")) std::cout << "  " << *iso;
        std::cout << '\n';
    }
    const tg::Blob children = engine.children("tgd:np/295c");
    std::cout << "Kaski has " << tg::Reader(children).rows() << " children\n";
    // #endregion hierarchy

    std::cout << "=== reverse\n";
    // #region reverse
    const tg::Blob around = engine.reverse(27.7172, 85.3240, 3);
    for (auto p : tg::Reader(around)) {
        std::cout << *p.string("name") << "  " << fixed(p.number("meters"), 0) << " m at "
                  << fixed(p.number("bearing"), 0) << "°\n";
    }
    // #endregion reverse

    std::cout << "=== radius\n";
    // #region radius
    const tg::Blob within = engine.within_radius(27.7172, 85.3240, 500, 10);
    const tg::Reader in_radius(within);
    std::cout << in_radius.rows() << " places within 500 m; farthest "
              << fixed(in_radius[in_radius.rows() - 1].number("meters"), 0) << " m\n";
    // #endregion radius

    std::cout << "=== nearby\n";
    // #region nearby
    const tg::Blob nearby = engine.nearby("tgd:np/2cn", 3);
    for (auto p : tg::Reader(nearby)) std::cout << *p.string("name") << "  " << fixed(p.number("meters"), 0) << " m\n";
    // #endregion nearby

    std::cout << "=== admin\n";
    // #region admin
    const tg::Blob units = engine.admin_at(28.2096, 83.9856);
    for (auto p : tg::Reader(units)) {
        std::cout << *p.integer("level") << "  " << *p.string("name");
        if (auto iso = p.string("iso")) std::cout << "  " << *iso;
        std::cout << '\n';
    }
    const tg::Blob state = engine.by_iso("NP-P4");
    const auto province = tg::Reader(state)[0];
    std::cout << "NP-P4 is " << *province.string("name") << ' ' << *province.string("id") << '\n';
    // #endregion admin

    std::cout << "=== timezone\n";
    // #region timezone
    const tg::Blob area = engine.tz_at(27.7172, 85.3240);
    const std::string zone(*tg::Reader(area)[0].string("tz"));
    const tg::Blob at = engine.tz_offset_at(zone, 1700000000);
    const auto offset = tg::Reader(at)[0];
    std::cout << zone << ' ' << *offset.integer("seconds") << " s east of UTC, " << *offset.string("abbr") << " dst "
              << *offset.integer("dst") << '\n';
    const tg::Blob civil = engine.tz_local("America/New_York", 2026, 3, 8, 2, 30, 0);
    const auto local = tg::Reader(civil)[0];
    std::cout << "02:30 on 2026-03-08 in New York: " << *local.string("kind") << ' ' << *local.integer("before")
              << " -> " << *local.integer("after") << '\n';
    std::cout << "tzdb " << engine.tz_version() << '\n';
    // #endregion timezone

    std::cout << "=== elevation\n";
    // #region elevation
    const tg::Blob ground = engine.elevation_at(27.7172, 85.3240);
    const auto height = tg::Reader(ground)[0];
    std::cout << fixed(height.number("meters"), 1) << " m above " << *height.string("datum") << " ("
              << *height.string("source") << ", " << fixed(height.number("resolution_m"), 0) << " m grid)\n";
    // #endregion elevation

    std::cout << "=== distance\n";
    // #region distance
    const double meters = tg::distance_m(27.7172, 85.3240, 28.2096, 83.9856);
    const double heading = tg::bearing_deg(27.7172, 85.3240, 28.2096, 83.9856);
    std::cout << "Kathmandu to Pokhara: " << fixed(meters / 1000, 1) << " km, heading " << fixed(heading, 0) << "°\n";
    const tg::Blob between = engine.distance("tgd:np/2cn", "tgd:np/1xhb");
    const auto apart = tg::Reader(between)[0];
    std::cout << "place to place: " << fixed(*apart.number("meters") / 1000, 1) << " km at "
              << fixed(apart.number("bearing"), 0) << "°\n";
    // #endregion distance

    std::cout << "=== boundary\n";
    // #region boundary
    const tg::Blob outline = engine.boundary("tgd:np/2gm", 0);
    const auto shape = tg::Reader(outline)[0];
    const std::string_view geojson = *shape.string("geojson"); // a GeoJSON Feature, for any map library
    const std::string_view key = "\"geometry\":{\"type\":\"";
    const std::string_view type = geojson.substr(geojson.find(key) + key.size());
    std::cout << type.substr(0, type.find('"')) << " of " << *shape.integer("vertices") << " vertices at level "
              << *shape.integer("lod") << '\n';
    std::cout << "tolerance " << fixed(shape.number("tolerance_m"), 0) << " m, " << geojson.size() << " bytes of GeoJSON\n";
    // #endregion boundary

    std::cout << "=== errors\n";
    // #region errors
    try {
        engine.reverse(91, 0, 1);
    } catch (const tg::Error &error) {
        if (error.code() == tg::ErrorCode::InvalidArgument) std::cout << "invalid argument: " << error.what() << '\n';
    }
    // #endregion errors
    return 0;
} catch (const tg::Error &error) {
    std::cerr << "error " << static_cast<int>(error.code()) << ": " << error.what() << '\n';
    return 1;
}
```
</Tab>

<Tab value="C#">
```csharp title="Program.cs"
// The tour: every query of the guide, run on a released pack.
//
// Usage: dotnet run -c Release -- world-np-lite.tgd

// #region import
using System.Globalization;
using Teispace.TeiGeodata;
// #endregion import

string pack = args[0];
// Numbers print with a decimal point whatever the user's culture.
CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;

Console.WriteLine("=== open");
// #region open
using var engine = new Engine();
engine.OpenFile(pack);
Console.WriteLine($"layers: {engine.Layers()}");
// #endregion open

Console.WriteLine("=== search");
// #region search
foreach (var p in new BlobReader(engine.Search("Pokhara", 3)))
{
    Console.WriteLine($"{p["id"]}  {p["name"]}  {p["kind"]}  {p["lat"]:F4}, {p["lon"]:F4}  score {p["score"]}");
}
// #endregion search

Console.WriteLine("=== search-parent");
// #region search-parent
foreach (var p in new BlobReader(engine.Search("Kathmandu, Bagmati", 2)))
{
    Console.WriteLine($"{p["id"]}  {p["name"]}  {p["kind"]} {p["level"]}");
}
// #endregion search-parent

Console.WriteLine("=== search-focus");
// #region search-focus
// Many places share a name; a focus (the user's location) ranks the near ones first.
var options = new SearchOptions { FocusLat = 28.7, FocusLon = 80.6, FocusScaleM = 30_000 };
foreach (var p in new BlobReader(engine.SearchWith("Shivapur", 3, options)))
{
    Console.WriteLine($"{p["id"]}  {p["name"]}  {p["lat"]:F4}, {p["lon"]:F4}");
}
// #endregion search-focus

Console.WriteLine("=== get");
// #region get
var place = new BlobReader(engine.Get("tgd:np/1xhb")).Single();
Console.WriteLine($"{place["name"]} {place["kind"]}");
Console.WriteLine($"at {place["lat"]:F6}, {place["lon"]:F6}, {place["elevation"]} m");
Console.WriteLine($"timezone {place["tz"]} parent {place["parent"]}");
// #endregion get

Console.WriteLine("=== hierarchy");
// #region hierarchy
foreach (var p in new BlobReader(engine.Ancestors("tgd:np/1xhb")))
{
    Console.WriteLine($"{p["kind"]} {p["level"]}  {p["name"]}  {p["iso"]}".TrimEnd());
}
var children = new BlobReader(engine.Children("tgd:np/295c"));
Console.WriteLine($"Kaski has {children.Rows} children");
// #endregion hierarchy

Console.WriteLine("=== reverse");
// #region reverse
foreach (var p in new BlobReader(engine.Reverse(27.7172, 85.3240, 3)))
{
    Console.WriteLine($"{p["name"]}  {p["meters"]:F0} m at {p["bearing"]:F0}°");
}
// #endregion reverse

Console.WriteLine("=== radius");
// #region radius
var within = new BlobReader(engine.WithinRadius(27.7172, 85.3240, 500, 10)).Records();
Console.WriteLine($"{within.Count} places within 500 m; farthest {within[^1]["meters"]:F0} m");
// #endregion radius

Console.WriteLine("=== nearby");
// #region nearby
foreach (var p in new BlobReader(engine.Nearby("tgd:np/2cn", 3)))
{
    Console.WriteLine($"{p["name"]}  {p["meters"]:F0} m");
}
// #endregion nearby

Console.WriteLine("=== admin");
// #region admin
foreach (var p in new BlobReader(engine.AdminAt(28.2096, 83.9856)))
{
    Console.WriteLine($"{p["level"]}  {p["name"]}  {p["iso"]}".TrimEnd());
}
var state = new BlobReader(engine.ByIso("NP-P4")).Single();
Console.WriteLine($"NP-P4 is {state["name"]} {state["id"]}");
// #endregion admin

Console.WriteLine("=== timezone");
// #region timezone
var zone = (string)new BlobReader(engine.TzAt(27.7172, 85.3240)).Single()["tz"]!;
var offset = new BlobReader(engine.TzOffsetAt(zone, 1_700_000_000)).Single();
Console.WriteLine($"{zone} {offset["seconds"]} s east of UTC, {offset["abbr"]} dst {offset["dst"]}");
var local = new BlobReader(engine.TzLocal("America/New_York", 2026, 3, 8, 2, 30, 0)).Single();
Console.WriteLine($"02:30 on 2026-03-08 in New York: {local["kind"]} {local["before"]} -> {local["after"]}");
Console.WriteLine($"tzdb {engine.TzVersion()}");
// #endregion timezone

Console.WriteLine("=== elevation");
// #region elevation
var height = new BlobReader(engine.ElevationAt(27.7172, 85.3240)).Single();
Console.WriteLine($"{height["meters"]:F1} m above {height["datum"]} ({height["source"]}, {height["resolution_m"]:F0} m grid)");
// #endregion elevation

Console.WriteLine("=== distance");
// #region distance
double meters = Geodata.DistanceM(27.7172, 85.3240, 28.2096, 83.9856);
double heading = Geodata.BearingDeg(27.7172, 85.3240, 28.2096, 83.9856);
Console.WriteLine($"Kathmandu to Pokhara: {meters / 1000:F1} km, heading {heading:F0}°");
var apart = new BlobReader(engine.Distance("tgd:np/2cn", "tgd:np/1xhb")).Single();
Console.WriteLine($"place to place: {(double)apart["meters"]! / 1000:F1} km at {apart["bearing"]:F0}°");
// #endregion distance

Console.WriteLine("=== boundary");
// #region boundary
var shape = new BlobReader(engine.Boundary("tgd:np/2gm", 0)).Single();
string geojson = (string)shape["geojson"]!; // a GeoJSON Feature, for any map library
using var feature = System.Text.Json.JsonDocument.Parse(geojson);
string type = feature.RootElement.GetProperty("geometry").GetProperty("type").GetString()!;
Console.WriteLine($"{type} of {shape["vertices"]} vertices at level {shape["lod"]}");
Console.WriteLine($"tolerance {shape["tolerance_m"]:F0} m, {geojson.Length} bytes of GeoJSON");
// #endregion boundary

Console.WriteLine("=== errors");
// #region errors
try
{
    engine.Reverse(91, 0, 1);
}
catch (TeiGeodataException error) when (error.Code == ErrorCode.InvalidArgument)
{
    Console.WriteLine($"invalid argument: {error.Message}");
}
// #endregion errors
```
</Tab>
</Tabs>

The output, the same for every language:

```text title="tour.out"
=== open
layers: 1
=== search
tgd:np/1xhb  Pokhara  settlement  28.2669, 83.9680  score 132
tgd:np/1xnr  Pokhara  admin  28.2094, 83.9818  score 105
tgd:np/15t1  Pokhara  settlement  28.0636, 82.3786  score 90
=== search-parent
tgd:np/2cn  Kathmandu  settlement 0
tgd:np/2gm  Kathmandu  admin 3
=== search-focus
tgd:np/2v99  Shivapur  28.5310, 80.8213
tgd:np/2raj  Shivapur  28.7141, 80.9869
tgd:np/2rkx  Shivapur  28.5061, 81.3032
=== get
Pokhara settlement
at 28.266890, 83.968000, 1006 m
timezone Asia/Kathmandu parent tgd:np/1xnr
=== hierarchy
admin 3  Pokhara
admin 2  Kaski
admin 1  Gandaki  NP-P4
country 0  Nepal  NP
Kaski has 5 children
=== reverse
Narayanhiti Royal Palace  331 m at 116°
Gairidhara  375 m at 220°
Naxal  384 m at 329°
=== radius
3 places within 500 m; farthest 384 m
=== nearby
Bhrikuti Mandap  168 m
Bagbazar  354 m
Shahidgate  393 m
=== admin
0  Nepal  NP
1  Gandaki  NP-P4
2  Kaski
3  Pokhara
NP-P4 is Gandaki tgd:np/2922
=== timezone
Asia/Kathmandu 20700 s east of UTC, +0545 dst 0
02:30 on 2026-03-08 in New York: gap -18000 -> -14400
tzdb 2026f
=== elevation
1304.2 m above EGM2008 (nasadem, 464 m grid)
=== distance
Kathmandu to Pokhara: 142.4 km, heading 293°
place to place: 146.9 km at 296°
=== boundary
Polygon of 176 vertices at level 0
tolerance 0 m, 4362 bytes of GeoJSON
=== errors
invalid argument: (91, 0) is not a point: the latitude is outside -90 to 90 degrees
```