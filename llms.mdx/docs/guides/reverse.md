# Reverse geocoding (/docs/guides/reverse)

Three ways to ask what is near, each nearest first with its distance and bearing:

<Callout title="Every example ran">
Each snippet is cut from one program per language, [the tour](https://github.com/teispace/teigeodata/tree/main/site/examples),
which CI runs on the released pack `world-np-lite.tgd` (`data-2026.11.14.3`) and checks line by
line against the output shown. The output is the same in every language. Pick your language
once: every page remembers it.
</Callout>

## Reverse geocode a point [#reverse-geocode-a-point]

`reverse` gives the places nearest a point, nearest first, each with `meters` (the
great-circle distance) and `bearing` (degrees clockwise from north, from the place to the
point: "3.2 km at 45° from Pokhara").

<Tabs groupId="lang" persist items={["Rust","Python","Node.js","Browser","Swift","Kotlin","Java","Dart","Go","C","C++","C#"]}>
<Tab value="Rust">
Results are `Near` values: the `place`, `meters` and `bearing_deg`, written into a `Vec`
you reuse.

```rust title="tour.rs"
let at = Point::<E6>::from_degrees(27.7172, 85.3240)?;
engine.reverse(at, 3, &mut scratch, &mut near)?;
for n in &near {
    let p = describe(&engine, &n.place)?;
    println!("{}  {:.0} m at {:.0}°", p.name, n.meters, n.bearing_deg);
}
```
</Tab>

<Tab value="Python">
```python title="tour.py"
for p in rows(engine.reverse(27.7172, 85.3240, 3)):
    print(f"{p['name']}  {p['meters']:.0f} m at {p['bearing']:.0f}°")
```
</Tab>

<Tab value="Node.js">
```js title="tour.mjs"
for (const p of new Reader(engine.reverse(27.7172, 85.324, 3))) {
  console.log(`${p.name}  ${fixed(p.meters, 0)} m at ${fixed(p.bearing, 0)}°`);
}
```
</Tab>

<Tab value="Browser">
```js title="tour.mjs"
for (const p of new Reader(engine.reverse(27.7172, 85.324, 3))) {
  console.log(`${p.name}  ${fixed(p.meters, 0)} m at ${fixed(p.bearing, 0)}°`);
}
```
</Tab>

<Tab value="Swift">
```swift title="main.swift"
for p in try Reader(engine.reverse(27.7172, lon: 85.3240, limit: 3)) {
    print("\(p["name"]!)  \(fixed(p.double("meters"), 0)) m at \(fixed(p.double("bearing"), 0))°")
}
```
</Tab>

<Tab value="Kotlin">
```kotlin title="Tour.kt"
for (p in Reader(engine.reverse(27.7172, 85.3240, 3))) {
    println("%s  %.0f m at %.0f°".f(p["name"], p["meters"], p["bearing"]))
}
```
</Tab>

<Tab value="Java">
```java title="Tour.java"
for (Map<String, Object> p : new Reader(engine.reverse(27.7172, 85.3240, 3))) {
    System.out.println(f("%s  %.0f m at %.0f°", p.get("name"), p.get("meters"), p.get("bearing")));
}
```
</Tab>

<Tab value="Dart">
```dart title="tour.dart"
for (final p in Reader(engine.reverse(27.7172, 85.3240, 3))) {
  final (meters, bearing) = (p['meters'] as double, p['bearing'] as double);
  print(
    '${p['name']}  ${meters.toStringAsFixed(0)} m at ${bearing.toStringAsFixed(0)}°',
  );
}
```
</Tab>

<Tab value="Go">
```go title="tour.go"
for _, p := range rows(engine.Reverse(27.7172, 85.3240, 3)) {
	fmt.Printf("%s  %.0f m at %.0f°\n", p["name"], p["meters"], p["bearing"])
}
```
</Tab>

<Tab value="C">
```c title="tour.c"
r = reader(tg_engine_reverse(engine, 27.7172, 85.3240, 3, &blob), &blob);
for (uint32_t i = 0; i < r.rows; i++) {
    printf("%.*s  %.0f m at %.0f°\n", S(&r, "name", i), num(&r, "meters", i), num(&r, "bearing", i));
}
tg_bytes_free(blob);
```
</Tab>

<Tab value="C++">
```cpp title="tour.cpp"
const tg::Blob around = engine.reverse(27.7172, 85.3240, 3);
for (auto p : tg::Reader(around)) {
    std::cout << *p.string("name") << "  " << fixed(p.number("meters"), 0) << " m at "
              << fixed(p.number("bearing"), 0) << "°\n";
}
```
</Tab>

<Tab value="C#">
```csharp title="Program.cs"
foreach (var p in new BlobReader(engine.Reverse(27.7172, 85.3240, 3)))
{
    Console.WriteLine($"{p["name"]}  {p["meters"]:F0} m at {p["bearing"]:F0}°");
}
```
</Tab>
</Tabs>

```text title="Output"
Narayanhiti Royal Palace  331 m at 116°
Gairidhara  375 m at 220°
Naxal  384 m at 329°
```

For the admin units that *hold* a point (its district, province and country) use
`admin_at` below; reverse answers with what is *near* it.

## Find everything within a radius [#find-everything-within-a-radius]

`within_radius` gives the places within a distance of a point, nearest first, with the
same `meters` and `bearing` columns, at most `limit` of them.

<Tabs groupId="lang" persist items={["Rust","Python","Node.js","Browser","Swift","Kotlin","Java","Dart","Go","C","C++","C#"]}>
<Tab value="Rust">
```rust title="tour.rs"
engine.within_radius(at, 500.0, 10, &mut scratch, &mut near)?;
let farthest = near.last().map_or(0.0, |n| n.meters);
println!(
    "{} places within 500 m; farthest {farthest:.0} m",
    near.len()
);
```
</Tab>

<Tab value="Python">
```python title="tour.py"
within = rows(engine.within_radius(27.7172, 85.3240, 500.0, 10))
print(len(within), "places within 500 m; farthest", f"{within[-1]['meters']:.0f} m")
```
</Tab>

<Tab value="Node.js">
```js title="tour.mjs"
const within = new Reader(engine.withinRadius(27.7172, 85.324, 500, 10));
const meters = within.numbers("meters");
console.log(within.rows, "places within 500 m; farthest", `${fixed(meters[within.rows - 1], 0)} m`);
```
</Tab>

<Tab value="Browser">
```js title="tour.mjs"
const within = new Reader(engine.withinRadius(27.7172, 85.324, 500, 10));
const meters = within.numbers("meters");
console.log(within.rows, "places within 500 m; farthest", `${fixed(meters[within.rows - 1], 0)} m`);
```
</Tab>

<Tab value="Swift">
```swift title="main.swift"
let within = try Reader(engine.withinRadius(27.7172, lon: 85.3240, radiusM: 500, limit: 10)).records()
print(within.count, "places within 500 m; farthest", fixed(within.last?.double("meters"), 0), "m")
```
</Tab>

<Tab value="Kotlin">
```kotlin title="Tour.kt"
val within = Reader(engine.withinRadius(27.7172, 85.3240, 500.0, 10)).records()
println("%d places within 500 m; farthest %.0f m".f(within.size, within.last()["meters"]))
```
</Tab>

<Tab value="Java">
```java title="Tour.java"
List<Map<String, Object>> within = new Reader(engine.withinRadius(27.7172, 85.3240, 500, 10)).records();
System.out.println(f("%d places within 500 m; farthest %.0f m",
        within.size(), within.get(within.size() - 1).get("meters")));
```
</Tab>

<Tab value="Dart">
```dart title="tour.dart"
final within = Reader(
  engine.withinRadius(27.7172, 85.3240, 500, 10),
).records();
final farthest = within.last['meters'] as double;
print(
  '${within.length} places within 500 m; farthest ${farthest.toStringAsFixed(0)} m',
);
```
</Tab>

<Tab value="Go">
```go title="tour.go"
within := rows(engine.WithinRadius(27.7172, 85.3240, 500, 10))
fmt.Printf("%d places within 500 m; farthest %.0f m\n", len(within), within[len(within)-1]["meters"])
```
</Tab>

<Tab value="C">
```c title="tour.c"
r = reader(tg_engine_within_radius(engine, 27.7172, 85.3240, 500, 10, &blob), &blob);
printf("%u places within 500 m; farthest %.0f m\n", r.rows, num(&r, "meters", r.rows - 1));
tg_bytes_free(blob);
```
</Tab>

<Tab value="C++">
```cpp title="tour.cpp"
const tg::Blob within = engine.within_radius(27.7172, 85.3240, 500, 10);
const tg::Reader in_radius(within);
std::cout << in_radius.rows() << " places within 500 m; farthest "
          << fixed(in_radius[in_radius.rows() - 1].number("meters"), 0) << " m\n";
```
</Tab>

<Tab value="C#">
```csharp title="Program.cs"
var within = new BlobReader(engine.WithinRadius(27.7172, 85.3240, 500, 10)).Records();
Console.WriteLine($"{within.Count} places within 500 m; farthest {within[^1]["meters"]:F0} m");
```
</Tab>
</Tabs>

```text title="Output"
3 places within 500 m; farthest 384 m
```

## Find places near a place [#find-places-near-a-place]

`nearby` is reverse geocoding from a place you already have: the places nearest it,
itself left out.

<Tabs groupId="lang" persist items={["Rust","Python","Node.js","Browser","Swift","Kotlin","Java","Dart","Go","C","C++","C#"]}>
<Tab value="Rust">
```rust title="tour.rs"
let kathmandu = engine.get("tgd:np/2cn".parse()?)?.ok_or("not found")?;
engine.nearby(&kathmandu, 3, &mut scratch, &mut near)?;
for n in &near {
    println!("{}  {:.0} m", describe(&engine, &n.place)?.name, n.meters);
}
```
</Tab>

<Tab value="Python">
```python title="tour.py"
for p in rows(engine.nearby("tgd:np/2cn", 3)):
    print(f"{p['name']}  {p['meters']:.0f} m")
```
</Tab>

<Tab value="Node.js">
```js title="tour.mjs"
for (const p of new Reader(engine.nearby("tgd:np/2cn", 3))) {
  console.log(`${p.name}  ${fixed(p.meters, 0)} m`);
}
```
</Tab>

<Tab value="Browser">
```js title="tour.mjs"
for (const p of new Reader(engine.nearby("tgd:np/2cn", 3))) {
  console.log(`${p.name}  ${fixed(p.meters, 0)} m`);
}
```
</Tab>

<Tab value="Swift">
```swift title="main.swift"
for p in try Reader(engine.nearby("tgd:np/2cn", limit: 3)) {
    print("\(p["name"]!)  \(fixed(p.double("meters"), 0)) m")
}
```
</Tab>

<Tab value="Kotlin">
```kotlin title="Tour.kt"
for (p in Reader(engine.nearby("tgd:np/2cn", 3))) {
    println("%s  %.0f m".f(p["name"], p["meters"]))
}
```
</Tab>

<Tab value="Java">
```java title="Tour.java"
for (Map<String, Object> p : new Reader(engine.nearby("tgd:np/2cn", 3))) {
    System.out.println(f("%s  %.0f m", p.get("name"), p.get("meters")));
}
```
</Tab>

<Tab value="Dart">
```dart title="tour.dart"
for (final p in Reader(engine.nearby('tgd:np/2cn', 3))) {
  print('${p['name']}  ${(p['meters'] as double).toStringAsFixed(0)} m');
}
```
</Tab>

<Tab value="Go">
```go title="tour.go"
for _, p := range rows(engine.Nearby("tgd:np/2cn", 3)) {
	fmt.Printf("%s  %.0f m\n", p["name"], p["meters"])
}
```
</Tab>

<Tab value="C">
```c title="tour.c"
r = reader(tg_engine_nearby(engine, TEXT("tgd:np/2cn"), 3, &blob), &blob);
for (uint32_t i = 0; i < r.rows; i++) printf("%.*s  %.0f m\n", S(&r, "name", i), num(&r, "meters", i));
tg_bytes_free(blob);
```
</Tab>

<Tab value="C++">
```cpp title="tour.cpp"
const tg::Blob nearby = engine.nearby("tgd:np/2cn", 3);
for (auto p : tg::Reader(nearby)) std::cout << *p.string("name") << "  " << fixed(p.number("meters"), 0) << " m\n";
```
</Tab>

<Tab value="C#">
```csharp title="Program.cs"
foreach (var p in new BlobReader(engine.Nearby("tgd:np/2cn", 3)))
{
    Console.WriteLine($"{p["name"]}  {p["meters"]:F0} m");
}
```
</Tab>
</Tabs>

```text title="Output"
Bhrikuti Mandap  168 m
Bagbazar  354 m
Shahidgate  393 m
```

## Next [#next]

- [Reference](/docs/reference): every query, its columns and its errors.
- [Errors](/docs/guides/errors): the eight codes and how each language raises them.
- [Concepts](/docs/concepts): places and IDs, packs and components, results, the store.