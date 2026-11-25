# Search by name (/docs/guides/search)

Search finds places by any of their names, forgives spelling, and ranks the right place first. Three forms, from the simplest:

<Callout title="Every example ran">
Each snippet is cut from one program per language, [the tour](https://github.com/teispace/teigeodata/tree/main/site/examples),
which CI runs on the released pack `world-np-lite.tgd` (`data-2026.11.14.3`) and checks line by
line against the output shown. The output is the same in every language. Pick your language
once: every page remembers it.
</Callout>

## Search by name [#search-by-name]

`search` finds places by name, best first. It matches every name a place has, in every
script, and forgives spelling: काठमाडौं, Kathmandu and Kathamandu find the same city, and so do
typos, missing accents and abbreviations. A result row has the place columns (`id`, `name`,
`kind`, `level`, `lat`, `lon`, `elevation`, `parent`, `tz`, `iso`, `population`,
`population_kind`) and a `score`.

<Tabs groupId="lang" persist items={["Rust","Python","Node.js","Browser","Swift","Kotlin","Java","Dart","Go","C","C++","C#"]}>
<Tab value="Rust">
`Engine::search_with` writes `Hit`s (a `Place` and its `score`) into a `Vec` you reuse,
with a `Scratch` for its working memory, so a warm search allocates nothing. The engine takes
any `k`; the bindings cap `limit` at `LIMIT_MAX` in `teigeodata-api`.

```rust title="tour.rs"
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
```
</Tab>

<Tab value="Python">
```python title="tour.py"
for p in rows(engine.search("Pokhara", 3)):
    print(f"{p['id']}  {p['name']}  {p['kind']}  {p['lat']:.4f}, {p['lon']:.4f}  score {p['score']}")
```
</Tab>

<Tab value="Node.js">
```js title="tour.mjs"
for (const p of new Reader(engine.search("Pokhara", 3))) {
  console.log(`${p.id}  ${p.name}  ${p.kind}  ${fixed(p.lat, 4)}, ${fixed(p.lon, 4)}  score ${p.score}`);
}
```
</Tab>

<Tab value="Browser">
```js title="tour.mjs"
for (const p of new Reader(engine.search("Pokhara", 3))) {
  console.log(`${p.id}  ${p.name}  ${p.kind}  ${fixed(p.lat, 4)}, ${fixed(p.lon, 4)}  score ${p.score}`);
}
```
</Tab>

<Tab value="Swift">
```swift title="main.swift"
for p in try Reader(engine.search("Pokhara", limit: 3)) {
    print("\(p["id"]!)  \(p["name"]!)  \(p["kind"]!)  \(fixed(p.double("lat"), 4)), \(fixed(p.double("lon"), 4))  score \(p["score"]!)")
}
```
</Tab>

<Tab value="Kotlin">
```kotlin title="Tour.kt"
for (p in Reader(engine.search("Pokhara", 3))) {
    println("%s  %s  %s  %.4f, %.4f  score %d".f(p["id"], p["name"], p["kind"], p["lat"], p["lon"], p["score"]))
}
```
</Tab>

<Tab value="Java">
```java title="Tour.java"
for (Map<String, Object> p : new Reader(engine.search("Pokhara", 3))) {
    System.out.println(f("%s  %s  %s  %.4f, %.4f  score %d",
            p.get("id"), p.get("name"), p.get("kind"), p.get("lat"), p.get("lon"), p.get("score")));
}
```
</Tab>

<Tab value="Dart">
```dart title="tour.dart"
for (final p in Reader(engine.search('Pokhara', 3))) {
  final (lat, lon) = (p['lat'] as double, p['lon'] as double);
  print(
    '${p['id']}  ${p['name']}  ${p['kind']}  ${lat.toStringAsFixed(4)}, '
    '${lon.toStringAsFixed(4)}  score ${p['score']}',
  );
}
```
</Tab>

<Tab value="Go">
```go title="tour.go"
for _, p := range rows(engine.Search("Pokhara", 3)) {
	fmt.Printf("%s  %s  %s  %.4f, %.4f  score %d\n",
		p["id"], p["name"], p["kind"], p["lat"], p["lon"], p["score"])
}
```
</Tab>

<Tab value="C">
```c title="tour.c"
r = reader(tg_engine_search(engine, TEXT("Pokhara"), 3, &blob), &blob);
for (uint32_t i = 0; i < r.rows; i++) {
    printf("%.*s  %.*s  %.*s  %.4f, %.4f  score %lld\n", S(&r, "id", i), S(&r, "name", i), S(&r, "kind", i),
           num(&r, "lat", i), num(&r, "lon", i), integer(&r, "score", i));
}
tg_bytes_free(blob);
```
</Tab>

<Tab value="C++">
```cpp title="tour.cpp"
const tg::Blob hits = engine.search("Pokhara", 3);
for (auto p : tg::Reader(hits)) {
    std::cout << *p.string("id") << "  " << *p.string("name") << "  " << *p.string("kind") << "  "
              << fixed(p.number("lat"), 4) << ", " << fixed(p.number("lon"), 4) << "  score "
              << *p.integer("score") << '\n';
}
```
</Tab>

<Tab value="C#">
```csharp title="Program.cs"
foreach (var p in new BlobReader(engine.Search("Pokhara", 3)))
{
    Console.WriteLine($"{p["id"]}  {p["name"]}  {p["kind"]}  {p["lat"]:F4}, {p["lon"]:F4}  score {p["score"]}");
}
```
</Tab>
</Tabs>

```text title="Output"
tgd:np/1xhb  Pokhara  settlement  28.2669, 83.9680  score 132
tgd:np/1xnr  Pokhara  admin  28.2094, 83.9818  score 105
tgd:np/15t1  Pokhara  settlement  28.0636, 82.3786  score 90
```

The `score` ranks the rows of one query and means nothing across queries. It adds how well the
name matched to how important the place is, so a city ranks above a village of the same name.
`limit` is at most 10,000 (`LIMIT_MAX`).

## Narrow a search by its parent [#narrow-a-search-by-its-parent]

Write the place's parent after a comma, as people do: `"Kathmandu, Bagmati"`. Places
inside a unit of that name rank first; the parent can be any ancestor (a district, a province,
a country) and is matched with the same tolerance as the name.

<Tabs groupId="lang" persist items={["Rust","Python","Node.js","Browser","Swift","Kotlin","Java","Dart","Go","C","C++","C#"]}>
<Tab value="Rust">
```rust title="tour.rs"
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
```
</Tab>

<Tab value="Python">
```python title="tour.py"
for p in rows(engine.search("Kathmandu, Bagmati", 2)):
    print(f"{p['id']}  {p['name']}  {p['kind']} {p['level']}")
```
</Tab>

<Tab value="Node.js">
```js title="tour.mjs"
for (const p of new Reader(engine.search("Kathmandu, Bagmati", 2))) {
  console.log(`${p.id}  ${p.name}  ${p.kind} ${p.level}`);
}
```
</Tab>

<Tab value="Browser">
```js title="tour.mjs"
for (const p of new Reader(engine.search("Kathmandu, Bagmati", 2))) {
  console.log(`${p.id}  ${p.name}  ${p.kind} ${p.level}`);
}
```
</Tab>

<Tab value="Swift">
```swift title="main.swift"
for p in try Reader(engine.search("Kathmandu, Bagmati", limit: 2)) {
    print("\(p["id"]!)  \(p["name"]!)  \(p["kind"]!) \(p["level"]!)")
}
```
</Tab>

<Tab value="Kotlin">
```kotlin title="Tour.kt"
for (p in Reader(engine.search("Kathmandu, Bagmati", 2))) {
    println("${p["id"]}  ${p["name"]}  ${p["kind"]} ${p["level"]}")
}
```
</Tab>

<Tab value="Java">
```java title="Tour.java"
for (Map<String, Object> p : new Reader(engine.search("Kathmandu, Bagmati", 2))) {
    System.out.println(f("%s  %s  %s %d", p.get("id"), p.get("name"), p.get("kind"), p.get("level")));
}
```
</Tab>

<Tab value="Dart">
```dart title="tour.dart"
for (final p in Reader(engine.search('Kathmandu, Bagmati', 2))) {
  print('${p['id']}  ${p['name']}  ${p['kind']} ${p['level']}');
}
```
</Tab>

<Tab value="Go">
```go title="tour.go"
for _, p := range rows(engine.Search("Kathmandu, Bagmati", 2)) {
	fmt.Printf("%s  %s  %s %d\n", p["id"], p["name"], p["kind"], p["level"])
}
```
</Tab>

<Tab value="C">
```c title="tour.c"
r = reader(tg_engine_search(engine, TEXT("Kathmandu, Bagmati"), 2, &blob), &blob);
for (uint32_t i = 0; i < r.rows; i++) {
    printf("%.*s  %.*s  %.*s %lld\n", S(&r, "id", i), S(&r, "name", i), S(&r, "kind", i), integer(&r, "level", i));
}
tg_bytes_free(blob);
```
</Tab>

<Tab value="C++">
```cpp title="tour.cpp"
const tg::Blob parents = engine.search("Kathmandu, Bagmati", 2);
for (auto p : tg::Reader(parents)) {
    std::cout << *p.string("id") << "  " << *p.string("name") << "  " << *p.string("kind") << ' '
              << *p.integer("level") << '\n';
}
```
</Tab>

<Tab value="C#">
```csharp title="Program.cs"
foreach (var p in new BlobReader(engine.Search("Kathmandu, Bagmati", 2)))
{
    Console.WriteLine($"{p["id"]}  {p["name"]}  {p["kind"]} {p["level"]}");
}
```
</Tab>
</Tabs>

```text title="Output"
tgd:np/2cn  Kathmandu  settlement 0
tgd:np/2gm  Kathmandu  admin 3
```

The city (a settlement, level 0) and the metropolitan municipality (an admin unit at
level 3) share the name, and both are in Bagmati.

## Rank by distance from a point [#rank-by-distance-from-a-point]

Many places share a name. `search_with` takes `SearchOptions`: a **focus** point (the
user's location, or the middle of the map they are looking at) ranks near places first. A place
at the focus gains 30 points, one `focus_scale_m` away 15, and the gain keeps halving with
distance (the default scale is 50 km).

<Tabs groupId="lang" persist items={["Rust","Python","Node.js","Browser","Swift","Kotlin","Java","Dart","Go","C","C++","C#"]}>
<Tab value="Rust">
The focus is `SearchOptions::with_focus(Focus { at, scale_m })`; `SearchOptions::strict`
sets the strict order.

```rust title="tour.rs"
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
```
</Tab>

<Tab value="Python">
```python title="tour.py"
# Many places share a name; a focus (the user's location) ranks the near ones first.
options = teigeodata.SearchOptions(focus_lat=28.7, focus_lon=80.6, focus_scale_m=30_000)
for p in rows(engine.search_with("Shivapur", 3, options)):
    print(f"{p['id']}  {p['name']}  {p['lat']:.4f}, {p['lon']:.4f}")
```
</Tab>

<Tab value="Node.js">
```js title="tour.mjs"
// Many places share a name; a focus (the user's location) ranks the near ones first.
const options = { focusLat: 28.7, focusLon: 80.6, focusScaleM: 30_000 };
for (const p of new Reader(engine.searchWith("Shivapur", 3, options))) {
  console.log(`${p.id}  ${p.name}  ${fixed(p.lat, 4)}, ${fixed(p.lon, 4)}`);
}
```
</Tab>

<Tab value="Browser">
```js title="tour.mjs"
// Many places share a name; a focus (the user's location) ranks the near ones first.
const options = { focusLat: 28.7, focusLon: 80.6, focusScaleM: 30_000 };
for (const p of new Reader(engine.searchWith("Shivapur", 3, options))) {
  console.log(`${p.id}  ${p.name}  ${fixed(p.lat, 4)}, ${fixed(p.lon, 4)}`);
}
```
</Tab>

<Tab value="Swift">
```swift title="main.swift"
// Many places share a name; a focus (the user's location) ranks the near ones first.
let options = SearchOptions(focusLat: 28.7, focusLon: 80.6, focusScaleM: 30_000)
for p in try Reader(engine.searchWith("Shivapur", limit: 3, options: options)) {
    print("\(p["id"]!)  \(p["name"]!)  \(fixed(p.double("lat"), 4)), \(fixed(p.double("lon"), 4))")
}
```
</Tab>

<Tab value="Kotlin">
```kotlin title="Tour.kt"
// Many places share a name; a focus (the user's location) ranks the near ones first.
val options = SearchOptions(28.7, 80.6, 30_000.0, false)
for (p in Reader(engine.searchWith("Shivapur", 3, options))) {
    println("%s  %s  %.4f, %.4f".f(p["id"], p["name"], p["lat"], p["lon"]))
}
```
</Tab>

<Tab value="Java">
```java title="Tour.java"
// Many places share a name; a focus (the user's location) ranks the near ones first.
SearchOptions options = SearchOptions.DEFAULTS.withFocusLat(28.7).withFocusLon(80.6).withFocusScaleM(30_000.0);
for (Map<String, Object> p : new Reader(engine.searchWith("Shivapur", 3, options))) {
    System.out.println(f("%s  %s  %.4f, %.4f", p.get("id"), p.get("name"), p.get("lat"), p.get("lon")));
}
```
</Tab>

<Tab value="Dart">
```dart title="tour.dart"
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
```
</Tab>

<Tab value="Go">
```go title="tour.go"
// Many places share a name; a focus (the user's location) ranks the near ones first.
lat, lon, scale := 28.7, 80.6, 30_000.0
options := &tg.SearchOptions{FocusLat: &lat, FocusLon: &lon, FocusScaleM: &scale}
for _, p := range rows(engine.SearchWith("Shivapur", 3, options)) {
	fmt.Printf("%s  %s  %.4f, %.4f\n", p["id"], p["name"], p["lat"], p["lon"])
}
```
</Tab>

<Tab value="C">
```c title="tour.c"
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
```
</Tab>

<Tab value="C++">
```cpp title="tour.cpp"
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
```
</Tab>

<Tab value="C#">
```csharp title="Program.cs"
// Many places share a name; a focus (the user's location) ranks the near ones first.
var options = new SearchOptions { FocusLat = 28.7, FocusLon = 80.6, FocusScaleM = 30_000 };
foreach (var p in new BlobReader(engine.SearchWith("Shivapur", 3, options)))
{
    Console.WriteLine($"{p["id"]}  {p["name"]}  {p["lat"]:F4}, {p["lon"]:F4}");
}
```
</Tab>
</Tabs>

```text title="Output"
tgd:np/2v99  Shivapur  28.5310, 80.8213
tgd:np/2raj  Shivapur  28.7141, 80.9869
tgd:np/2rkx  Shivapur  28.5061, 81.3032
```

Set `strict` to rank by how well the name matched first (exact, loose, prefix, one edit,
substring, two edits, phonetic), then by importance and focus, instead of one combined score:
useful for an autocomplete that must show exact matches on top.

## Next [#next]

- [Reference](/docs/reference): every query, its columns and its errors.
- [Errors](/docs/guides/errors): the eight codes and how each language raises them.
- [Concepts](/docs/concepts): places and IDs, packs and components, results, the store.