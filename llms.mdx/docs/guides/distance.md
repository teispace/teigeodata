# Distance and bearing (/docs/guides/distance)

<Callout title="Every example ran">
Each snippet is cut from one program per language, [the tour](https://github.com/teispace/teigeodata/tree/main/site/examples),
which CI runs on the released pack `world-np-lite.tgd` (`data-2026.11.14.3`) and checks line by
line against the output shown. The output is the same in every language. Pick your language
once: every page remembers it.
</Callout>

`distance_m` is the great-circle distance in metres (haversine on the WGS84 mean
sphere, within about 0.5% of the ellipsoid), and `bearing_deg` the initial bearing from the
first point to the second, in degrees clockwise from north. Neither needs a pack. Between two
places you already have, `distance` takes their IDs and answers one row with `meters` and
`bearing`.

<Tabs groupId="lang" persist items={["Rust","Python","Node.js","Browser","Swift","Kotlin","Java","Dart","Go","C","C++","C#"]}>
<Tab value="Rust">
The free functions are `meters_between` and `bearing_deg`, taking degrees; between two
places, pass their points.

```rust title="tour.rs"
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
```
</Tab>

<Tab value="Python">
```python title="tour.py"
meters = teigeodata.distance_m(27.7172, 85.3240, 28.2096, 83.9856)
heading = teigeodata.bearing_deg(27.7172, 85.3240, 28.2096, 83.9856)
print(f"Kathmandu to Pokhara: {meters / 1000:.1f} km, heading {heading:.0f}°")
(apart,) = rows(engine.distance("tgd:np/2cn", "tgd:np/1xhb"))
print(f"place to place: {apart['meters'] / 1000:.1f} km at {apart['bearing']:.0f}°")
```
</Tab>

<Tab value="Node.js">
```js title="tour.mjs"
const km = distanceM(27.7172, 85.324, 28.2096, 83.9856) / 1000;
const heading = bearingDeg(27.7172, 85.324, 28.2096, 83.9856);
console.log(`Kathmandu to Pokhara: ${fixed(km, 1)} km, heading ${fixed(heading, 0)}°`);
const [apart] = new Reader(engine.distance("tgd:np/2cn", "tgd:np/1xhb")).records();
console.log(`place to place: ${fixed(apart.meters / 1000, 1)} km at ${fixed(apart.bearing, 0)}°`);
```
</Tab>

<Tab value="Browser">
```js title="tour.mjs"
const km = distanceM(27.7172, 85.324, 28.2096, 83.9856) / 1000;
const heading = bearingDeg(27.7172, 85.324, 28.2096, 83.9856);
console.log(`Kathmandu to Pokhara: ${fixed(km, 1)} km, heading ${fixed(heading, 0)}°`);
const [apart] = new Reader(engine.distance("tgd:np/2cn", "tgd:np/1xhb")).records();
console.log(`place to place: ${fixed(apart.meters / 1000, 1)} km at ${fixed(apart.bearing, 0)}°`);
```
</Tab>

<Tab value="Swift">
```swift title="main.swift"
let meters = try TeiGeodata.distanceM(27.7172, lon1: 85.3240, lat2: 28.2096, lon2: 83.9856)
let heading = try TeiGeodata.bearingDeg(27.7172, lon1: 85.3240, lat2: 28.2096, lon2: 83.9856)
print("Kathmandu to Pokhara: \(fixed(meters / 1000, 1)) km, heading \(fixed(heading, 0))°")
let apart = try Reader(engine.distance("tgd:np/2cn", to: "tgd:np/1xhb")).records()[0]
print("place to place: \(fixed(apart.double("meters")! / 1000, 1)) km at \(fixed(apart.double("bearing"), 0))°")
```
</Tab>

<Tab value="Kotlin">
```kotlin title="Tour.kt"
val meters = TeiGeodata.distanceM(27.7172, 85.3240, 28.2096, 83.9856)
val heading = TeiGeodata.bearingDeg(27.7172, 85.3240, 28.2096, 83.9856)
println("Kathmandu to Pokhara: %.1f km, heading %.0f°".f(meters / 1000, heading))
val apart = Reader(engine.distance("tgd:np/2cn", "tgd:np/1xhb")).single()
println("place to place: %.1f km at %.0f°".f(apart["meters"] as Double / 1000, apart["bearing"]))
```
</Tab>

<Tab value="Java">
```java title="Tour.java"
double meters = TeiGeodata.distanceM(27.7172, 85.3240, 28.2096, 83.9856);
double heading = TeiGeodata.bearingDeg(27.7172, 85.3240, 28.2096, 83.9856);
System.out.println(f("Kathmandu to Pokhara: %.1f km, heading %.0f°", meters / 1000, heading));
Map<String, Object> apart = new Reader(engine.distance("tgd:np/2cn", "tgd:np/1xhb")).records().get(0);
System.out.println(f("place to place: %.1f km at %.0f°", (Double) apart.get("meters") / 1000, apart.get("bearing")));
```
</Tab>

<Tab value="Dart">
```dart title="tour.dart"
final km = tg.distanceM(27.7172, 85.3240, 28.2096, 83.9856) / 1000;
final heading = tg.bearingDeg(27.7172, 85.3240, 28.2096, 83.9856);
print('Kathmandu to Pokhara: ${km.toStringAsFixed(1)} km, heading ${heading.toStringAsFixed(0)}°');
final apart = Reader(engine.distance('tgd:np/2cn', 'tgd:np/1xhb')).single;
final (far, bearing) = (apart['meters'] as double, apart['bearing'] as double);
print('place to place: ${(far / 1000).toStringAsFixed(1)} km at ${bearing.toStringAsFixed(0)}°');
```
</Tab>

<Tab value="Go">
```go title="tour.go"
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
```
</Tab>

<Tab value="C">
```c title="tour.c"
double meters = 0;
check(tg_distance_m(27.7172, 85.3240, 28.2096, 83.9856, &meters));
double heading = 0;
check(tg_bearing_deg(27.7172, 85.3240, 28.2096, 83.9856, &heading));
printf("Kathmandu to Pokhara: %.1f km, heading %.0f°\n", meters / 1000, heading);
r = reader(tg_engine_distance(engine, TEXT("tgd:np/2cn"), TEXT("tgd:np/1xhb"), &blob), &blob);
printf("place to place: %.1f km at %.0f°\n", num(&r, "meters", 0) / 1000, num(&r, "bearing", 0));
tg_bytes_free(blob);
```
</Tab>

<Tab value="C++">
```cpp title="tour.cpp"
const double meters = tg::distance_m(27.7172, 85.3240, 28.2096, 83.9856);
const double heading = tg::bearing_deg(27.7172, 85.3240, 28.2096, 83.9856);
std::cout << "Kathmandu to Pokhara: " << fixed(meters / 1000, 1) << " km, heading " << fixed(heading, 0) << "°\n";
const tg::Blob between = engine.distance("tgd:np/2cn", "tgd:np/1xhb");
const auto apart = tg::Reader(between)[0];
std::cout << "place to place: " << fixed(*apart.number("meters") / 1000, 1) << " km at "
          << fixed(apart.number("bearing"), 0) << "°\n";
```
</Tab>

<Tab value="C#">
```csharp title="Program.cs"
double meters = Geodata.DistanceM(27.7172, 85.3240, 28.2096, 83.9856);
double heading = Geodata.BearingDeg(27.7172, 85.3240, 28.2096, 83.9856);
Console.WriteLine($"Kathmandu to Pokhara: {meters / 1000:F1} km, heading {heading:F0}°");
var apart = new BlobReader(engine.Distance("tgd:np/2cn", "tgd:np/1xhb")).Single();
Console.WriteLine($"place to place: {(double)apart["meters"]! / 1000:F1} km at {apart["bearing"]:F0}°");
```
</Tab>
</Tabs>

```text title="Output"
Kathmandu to Pokhara: 142.4 km, heading 293°
place to place: 146.9 km at 296°
```

`distance` answers no row when either ID is unknown. From a point to a place, pass the
place's `lat` and `lon` to `distance_m`. The bearing is the direction you set off in: on a
great circle it changes along the way, so the bearing back is not the reverse of this one.

## Next [#next]

- [Reference](/docs/reference): every query, its columns and its errors.
- [Errors](/docs/guides/errors): the eight codes and how each language raises them.
- [Concepts](/docs/concepts): places and IDs, packs and components, results, the store.