# Elevation (/docs/guides/elevation)

<Callout title="Every example ran">
Each snippet is cut from one program per language, [the tour](https://github.com/teispace/teigeodata/tree/main/site/examples),
which CI runs on the released pack `world-np-lite.tgd` (`data-2026.11.14.3`) and checks line by
line against the output shown. The output is the same in every language. Pick your language
once: every page remembers it.
</Callout>

`elevation_at` reads the terrain height at a point from the pack's elevation grid,
interpolated between the four samples around it: `meters` above EGM2008, the `source` product,
the grid's `resolution_m` and the `datum`. There is no row at sea or outside every grid.

<Tabs groupId="lang" persist items={["Rust","Python","Node.js","Browser","Swift","Kotlin","Java","Dart","Go","C","C++","C#"]}>
<Tab value="Rust">
```rust title="tour.rs"
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
```
</Tab>

<Tab value="Python">
```python title="tour.py"
(height,) = rows(engine.elevation_at(27.7172, 85.3240))
print(f"{height['meters']:.1f} m above {height['datum']} ({height['source']}, {height['resolution_m']:.0f} m grid)")
```
</Tab>

<Tab value="Node.js">
```js title="tour.mjs"
const [height] = new Reader(engine.elevationAt(27.7172, 85.324)).records();
console.log(
  `${fixed(height.meters, 1)} m above ${height.datum} (${height.source}, ${fixed(height.resolution_m, 0)} m grid)`,
);
```
</Tab>

<Tab value="Browser">
```js title="tour.mjs"
const [height] = new Reader(engine.elevationAt(27.7172, 85.324)).records();
console.log(
  `${fixed(height.meters, 1)} m above ${height.datum} (${height.source}, ${fixed(height.resolution_m, 0)} m grid)`,
);
```
</Tab>

<Tab value="Swift">
```swift title="main.swift"
let height = try Reader(engine.elevationAt(27.7172, lon: 85.3240)).records()[0]
print("\(fixed(height.double("meters"), 1)) m above \(height["datum"]!) (\(height["source"]!), \(fixed(height.double("resolution_m"), 0)) m grid)")
```
</Tab>

<Tab value="Kotlin">
```kotlin title="Tour.kt"
val height = Reader(engine.elevationAt(27.7172, 85.3240)).single()
println("%.1f m above %s (%s, %.0f m grid)".f(height["meters"], height["datum"], height["source"], height["resolution_m"]))
```
</Tab>

<Tab value="Java">
```java title="Tour.java"
Map<String, Object> height = new Reader(engine.elevationAt(27.7172, 85.3240)).records().get(0);
System.out.println(f("%.1f m above %s (%s, %.0f m grid)",
        height.get("meters"), height.get("datum"), height.get("source"), height.get("resolution_m")));
```
</Tab>

<Tab value="Dart">
```dart title="tour.dart"
final height = Reader(engine.elevationAt(27.7172, 85.3240)).single;
final (meters, grid) = (
  height['meters'] as double,
  height['resolution_m'] as double,
);
print(
  '${meters.toStringAsFixed(1)} m above ${height['datum']} '
  '(${height['source']}, ${grid.toStringAsFixed(0)} m grid)',
);
```
</Tab>

<Tab value="Go">
```go title="tour.go"
height := rows(engine.ElevationAt(27.7172, 85.3240))[0]
fmt.Printf("%.1f m above %s (%s, %.0f m grid)\n",
	height["meters"], height["datum"], height["source"], height["resolution_m"])
```
</Tab>

<Tab value="C">
```c title="tour.c"
r = reader(tg_engine_elevation_at(engine, 27.7172, 85.3240, &blob), &blob);
printf("%.1f m above %.*s (%.*s, %.0f m grid)\n", num(&r, "meters", 0), S(&r, "datum", 0), S(&r, "source", 0),
       num(&r, "resolution_m", 0));
tg_bytes_free(blob);
```
</Tab>

<Tab value="C++">
```cpp title="tour.cpp"
const tg::Blob ground = engine.elevation_at(27.7172, 85.3240);
const auto height = tg::Reader(ground)[0];
std::cout << fixed(height.number("meters"), 1) << " m above " << *height.string("datum") << " ("
          << *height.string("source") << ", " << fixed(height.number("resolution_m"), 0) << " m grid)\n";
```
</Tab>

<Tab value="C#">
```csharp title="Program.cs"
var height = new BlobReader(engine.ElevationAt(27.7172, 85.3240)).Single();
Console.WriteLine($"{height["meters"]:F1} m above {height["datum"]} ({height["source"]}, {height["resolution_m"]:F0} m grid)");
```
</Tab>
</Tabs>

```text title="Output"
1304.2 m above EGM2008 (nasadem, 464 m grid)
```

Every place also has its own `elevation` column. The world pack carries a coarse grid; a
country's terrain pack adds a finer one, and the engine answers from the finest grid that covers
the point.

## Next [#next]

- [Reference](/docs/reference): every query, its columns and its errors.
- [Errors](/docs/guides/errors): the eight codes and how each language raises them.
- [Concepts](/docs/concepts): places and IDs, packs and components, results, the store.