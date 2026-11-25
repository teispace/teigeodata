# Places and the hierarchy (/docs/guides/places)

<Callout title="Every example ran">
Each snippet is cut from one program per language, [the tour](https://github.com/teispace/teigeodata/tree/main/site/examples),
which CI runs on the released pack `world-np-lite.tgd` (`data-2026.11.14.3`) and checks line by
line against the output shown. The output is the same in every language. Pick your language
once: every page remembers it.
</Callout>

## Get a place by its ID [#get-a-place-by-its-id]

Every place has a stable ID such as `tgd:np/1xhb`: the country, then a short code. IDs
survive rebuilds of the data; when two places are merged, `get` on the retired ID answers
with the survivor, so IDs you stored keep working.

<Tabs groupId="lang" persist items={["Rust","Python","Node.js","Browser","Swift","Kotlin","Java","Dart","Go","C","C++","C#"]}>
<Tab value="Rust">
IDs parse into a `PlaceId`. A `Place` holds its ID, kind, level and point; read the rest on
demand: `Engine::elevation`, `Engine::tz_of`, `Engine::parent`, each an `Option`.

```rust title="tour.rs"
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
```
</Tab>

<Tab value="Python">
```python title="tour.py"
(place,) = rows(engine.get("tgd:np/1xhb"))
print(place["name"], place["kind"])
print(f"at {place['lat']:.6f}, {place['lon']:.6f}, {place['elevation']} m")
print("timezone", place["tz"], "parent", place["parent"])
```
</Tab>

<Tab value="Node.js">
```js title="tour.mjs"
const [place] = new Reader(engine.get("tgd:np/1xhb")).records();
console.log(place.name, place.kind);
console.log(`at ${fixed(place.lat, 6)}, ${fixed(place.lon, 6)}, ${place.elevation} m`);
console.log("timezone", place.tz, "parent", place.parent);
```
</Tab>

<Tab value="Browser">
```js title="tour.mjs"
const [place] = new Reader(engine.get("tgd:np/1xhb")).records();
console.log(place.name, place.kind);
console.log(`at ${fixed(place.lat, 6)}, ${fixed(place.lon, 6)}, ${place.elevation} m`);
console.log("timezone", place.tz, "parent", place.parent);
```
</Tab>

<Tab value="Swift">
```swift title="main.swift"
let place = try Reader(engine.get("tgd:np/1xhb")).records()[0]
print(place["name"]!, place["kind"]!)
print("at \(fixed(place.double("lat"), 6)), \(fixed(place.double("lon"), 6)), \(place["elevation"]!) m")
print("timezone", place["tz"]!, "parent", place["parent"]!)
```
</Tab>

<Tab value="Kotlin">
```kotlin title="Tour.kt"
val place = Reader(engine.get("tgd:np/1xhb")).single()
println("${place["name"]} ${place["kind"]}")
println("at %.6f, %.6f, %d m".f(place["lat"], place["lon"], place["elevation"]))
println("timezone ${place["tz"]} parent ${place["parent"]}")
```
</Tab>

<Tab value="Java">
```java title="Tour.java"
Map<String, Object> place = new Reader(engine.get("tgd:np/1xhb")).records().get(0);
System.out.println(place.get("name") + " " + place.get("kind"));
System.out.println(f("at %.6f, %.6f, %d m", place.get("lat"), place.get("lon"), place.get("elevation")));
System.out.println("timezone " + place.get("tz") + " parent " + place.get("parent"));
```
</Tab>

<Tab value="Dart">
```dart title="tour.dart"
final place = Reader(engine.get('tgd:np/1xhb')).single;
print('${place['name']} ${place['kind']}');
final (lat, lon) = (place['lat'] as double, place['lon'] as double);
print(
  'at ${lat.toStringAsFixed(6)}, ${lon.toStringAsFixed(6)}, ${place['elevation']} m',
);
print('timezone ${place['tz']} parent ${place['parent']}');
```
</Tab>

<Tab value="Go">
```go title="tour.go"
place := rows(engine.Get("tgd:np/1xhb"))[0]
fmt.Println(place["name"], place["kind"])
fmt.Printf("at %.6f, %.6f, %d m\n", place["lat"], place["lon"], place["elevation"])
fmt.Println("timezone", place["tz"], "parent", place["parent"])
```
</Tab>

<Tab value="C">
```c title="tour.c"
r = reader(tg_engine_get(engine, TEXT("tgd:np/1xhb"), &blob), &blob);
printf("%.*s %.*s\n", S(&r, "name", 0), S(&r, "kind", 0));
printf("at %.6f, %.6f, %lld m\n", num(&r, "lat", 0), num(&r, "lon", 0), integer(&r, "elevation", 0));
printf("timezone %.*s parent %.*s\n", S(&r, "tz", 0), S(&r, "parent", 0));
tg_bytes_free(blob);
```
</Tab>

<Tab value="C++">
```cpp title="tour.cpp"
const tg::Blob got = engine.get("tgd:np/1xhb");
const tg::Reader one(got);
const auto place = one[0];
std::cout << *place.string("name") << ' ' << *place.string("kind") << '\n';
std::cout << "at " << fixed(place.number("lat"), 6) << ", " << fixed(place.number("lon"), 6) << ", "
          << *place.integer("elevation") << " m\n";
std::cout << "timezone " << *place.string("tz") << " parent " << *place.string("parent") << '\n';
```
</Tab>

<Tab value="C#">
```csharp title="Program.cs"
var place = new BlobReader(engine.Get("tgd:np/1xhb")).Single();
Console.WriteLine($"{place["name"]} {place["kind"]}");
Console.WriteLine($"at {place["lat"]:F6}, {place["lon"]:F6}, {place["elevation"]} m");
Console.WriteLine($"timezone {place["tz"]} parent {place["parent"]}");
```
</Tab>
</Tabs>

```text title="Output"
Pokhara settlement
at 28.266890, 83.968000, 1006 m
timezone Asia/Kathmandu parent tgd:np/1xnr
```

`elevation` is metres above mean sea level (EGM2008), `tz` the IANA zone and `parent` the
ID of the unit that holds the place. Any column may be null where the data has no value: read
each through the reader, which gives your language's null.

## Walk the admin hierarchy [#walk-the-admin-hierarchy]

`ancestors` lists the units that hold a place, parent first, up to its country; `parent`
gives the first of them alone and `children` the units and places one level down.

<Tabs groupId="lang" persist items={["Rust","Python","Node.js","Browser","Swift","Kotlin","Java","Dart","Go","C","C++","C#"]}>
<Tab value="Rust">
```rust title="tour.rs"
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
```
</Tab>

<Tab value="Python">
```python title="tour.py"
for p in rows(engine.ancestors("tgd:np/1xhb")):
    print(f"{p['kind']} {p['level']}  {p['name']}  {p['iso'] or ''}".rstrip())
children = rows(engine.children("tgd:np/295c"))
print("Kaski has", len(children), "children")
```
</Tab>

<Tab value="Node.js">
```js title="tour.mjs"
for (const p of new Reader(engine.ancestors("tgd:np/1xhb"))) {
  console.log(`${p.kind} ${p.level}  ${p.name}  ${p.iso ?? ""}`.trimEnd());
}
const children = new Reader(engine.children("tgd:np/295c"));
console.log("Kaski has", children.rows, "children");
```
</Tab>

<Tab value="Browser">
```js title="tour.mjs"
for (const p of new Reader(engine.ancestors("tgd:np/1xhb"))) {
  console.log(`${p.kind} ${p.level}  ${p.name}  ${p.iso ?? ""}`.trimEnd());
}
const children = new Reader(engine.children("tgd:np/295c"));
console.log("Kaski has", children.rows, "children");
```
</Tab>

<Tab value="Swift">
```swift title="main.swift"
for p in try Reader(engine.ancestors("tgd:np/1xhb")) {
    let line = "\(p["kind"]!) \(p["level"]!)  \(p["name"]!)  \(p.string("iso") ?? "")"
    print(line.replacingOccurrences(of: "\\s+$", with: "", options: .regularExpression))
}
let children = try Reader(engine.children("tgd:np/295c"))
print("Kaski has", children.rows, "children")
```
</Tab>

<Tab value="Kotlin">
```kotlin title="Tour.kt"
for (p in Reader(engine.ancestors("tgd:np/1xhb"))) {
    println("${p["kind"]} ${p["level"]}  ${p["name"]}  ${p["iso"] ?: ""}".trimEnd())
}
val children = Reader(engine.children("tgd:np/295c"))
println("Kaski has ${children.rows()} children")
```
</Tab>

<Tab value="Java">
```java title="Tour.java"
for (Map<String, Object> p : new Reader(engine.ancestors("tgd:np/1xhb"))) {
    System.out.println(f("%s %d  %s  %s", p.get("kind"), p.get("level"), p.get("name"), orEmpty(p.get("iso"))).stripTrailing());
}
Reader children = new Reader(engine.children("tgd:np/295c"));
System.out.println("Kaski has " + children.rows() + " children");
```
</Tab>

<Tab value="Dart">
```dart title="tour.dart"
for (final p in Reader(engine.ancestors('tgd:np/1xhb'))) {
  print(
    '${p['kind']} ${p['level']}  ${p['name']}  ${p['iso'] ?? ''}'.trimRight(),
  );
}
final children = Reader(engine.children('tgd:np/295c'));
print('Kaski has ${children.rows} children');
```
</Tab>

<Tab value="Go">
```go title="tour.go"
for _, p := range rows(engine.Ancestors("tgd:np/1xhb")) {
	line := fmt.Sprintf("%s %d  %s  %s", p["kind"], p["level"], p["name"], orEmpty(p["iso"]))
	fmt.Println(strings.TrimRight(line, " "))
}
children := rows(engine.Children("tgd:np/295c"))
fmt.Println("Kaski has", len(children), "children")
```
</Tab>

<Tab value="C">
```c title="tour.c"
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
```
</Tab>

<Tab value="C++">
```cpp title="tour.cpp"
const tg::Blob ancestors = engine.ancestors("tgd:np/1xhb");
for (auto p : tg::Reader(ancestors)) {
    std::cout << *p.string("kind") << ' ' << *p.integer("level") << "  " << *p.string("name");
    if (auto iso = p.string("iso")) std::cout << "  " << *iso;
    std::cout << '\n';
}
const tg::Blob children = engine.children("tgd:np/295c");
std::cout << "Kaski has " << tg::Reader(children).rows() << " children\n";
```
</Tab>

<Tab value="C#">
```csharp title="Program.cs"
foreach (var p in new BlobReader(engine.Ancestors("tgd:np/1xhb")))
{
    Console.WriteLine($"{p["kind"]} {p["level"]}  {p["name"]}  {p["iso"]}".TrimEnd());
}
var children = new BlobReader(engine.Children("tgd:np/295c"));
Console.WriteLine($"Kaski has {children.Rows} children");
```
</Tab>
</Tabs>

```text title="Output"
admin 3  Pokhara
admin 2  Kaski
admin 1  Gandaki  NP-P4
country 0  Nepal  NP
Kaski has 5 children
```

`level` is the admin level as each country defines it: 0 is a country (and a settlement,
which is not a unit), 1 a province or state, and so on down to the lowest level the data has.
Each country's ladder is in its profile on the [data pages](/docs/data).

## Next [#next]

- [Reference](/docs/reference): every query, its columns and its errors.
- [Errors](/docs/guides/errors): the eight codes and how each language raises them.
- [Concepts](/docs/concepts): places and IDs, packs and components, results, the store.