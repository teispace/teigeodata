# Quickstart (/docs/getting-started)

This page takes you from nothing to answers in a few minutes: a pack, the `tgd` command line to
look inside it, and the same queries from code.

<Steps>
<Step>
### Get a pack [#get-a-pack]

A pack is one file. The world lite pack holds the world's countries, first-level units and
cities of 5,000 or more, India's districts and statutory towns, and Nepal in full: 172,339
places named in Nepali, Hindi and English, with heights, admin units, timezones and boundaries.

```sh
curl -LO https://github.com/teispace/teigeodata/releases/download/data-2026.11.14.3/world-np-lite.tgd
shasum -a 256 world-np-lite.tgd
# 243ae55509a538b1f61da50994a791cc78e7deb70bb4bf743b9f59d458cbe3f3  world-np-lite.tgd
```

It is 15.6 MB on disk and 9.8 MB compressed on the wire, under CC BY 4.0: you may ship it in an
app, with the credits it carries.
</Step>

<Step>
### Install the command line [#install-the-command-line]

`tgd` inspects, verifies and queries packs, and serves them over HTTP and MCP. Install it from
crates.io (a Rust toolchain is all it needs):

```sh
cargo install --locked teigeodata-cli
```
</Step>

<Step>
### Ask it something [#ask-it-something]

```sh title="Search"
tgd search Pokhara -k 3 --pack "$PACK"
```

```text title="Output"
Pokhara  settlement  28.266890,83.968000  tgd:np/1xhb  1006 m (model)  Asia/Kathmandu  score 132
Pokhara  admin 3  28.209407,83.981817  tgd:np/1xnr  842 m (model)  Asia/Kathmandu  score 105
Pokhara  settlement  28.063631,82.378569  tgd:np/15t1  612 m (model)  Asia/Kathmandu  score 90
```

The first is the city (a settlement), the second the metropolitan municipality that holds it
(an admin unit at level 3), the third a smaller settlement of the same name farther west. `tgd:np/1xhb` is
the city's stable ID. From a point:

```sh title="Admin units at a point"
tgd admin 28.2096 83.9856 --pack "$PACK"
```

```text title="Output"
Nepal  country NP  28.000000,84.000000  tgd:np/1wmc  1046 m (model)  Asia/Kathmandu
Gandaki  admin 1 NP-P4  28.384514,84.035774  tgd:np/2922  2689 m (model)  Asia/Kathmandu
Kaski  admin 2  28.345513,83.970886  tgd:np/295c  1231 m (model)  Asia/Kathmandu
Pokhara  admin 3  28.209407,83.981817  tgd:np/1xnr  842 m (model)  Asia/Kathmandu
```

The [command-line guide](/docs/cli) shows every query.
</Step>

<Step>
### Use it from code [#use-it-from-code]

[Install the package for your language](/docs/getting-started/install), then follow the
guides, which open this same pack and run every query with its output in each language:

<Cards>
<Card title="Open a pack" href="/docs/guides/open" />

<Card title="Search by name" href="/docs/guides/search" />

<Card title="Reverse geocoding" href="/docs/guides/reverse" />

<Card title="Timezones" href="/docs/guides/timezones" />
</Cards>

The search above, from code:

<Tabs groupId="lang" persist items={["Rust","Python","Node.js","Browser","Swift","Kotlin","Java","Dart","Go","C","C++","C#"]}>
<Tab value="Rust">
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
</Step>
</Steps>

## What to read next [#what-to-read-next]

- [Places and IDs](/docs/concepts/places): what a place is, its kinds and levels, and why IDs
  stay stable.
- [Packs and components](/docs/concepts/packs): what is inside a pack and how to ship only the
  parts you use.
- [Results](/docs/concepts/results): the columns every query returns and how to read them fast.
- [Licences](/docs/concepts/licences): what you must credit, and where.