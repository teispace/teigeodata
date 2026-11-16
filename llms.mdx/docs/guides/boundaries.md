# Outlines (/docs/guides/boundaries)

<Callout title="Every example ran">
Each snippet is cut from one program per language, [the tour](https://github.com/teispace/teigeodata/tree/main/site/examples),
which CI runs on the released pack `world-np-lite.tgd` (`data-2026.11.14.3`) and checks line by
line against the output shown. The output is the same in every language. Pick your language
once: every page remembers it.
</Callout>

`boundary` answers a unit's outline as one row: `geojson`, an RFC 7946 Feature (a
Polygon or MultiPolygon, outer rings counterclockwise and holes clockwise, seven decimals as
stored), with the `lod` read, its `tolerance_m` and the number of `vertices`. A place with no
outline answers its point as the geometry; an unknown ID answers no row.

<Tabs groupId="lang" persist items={["Rust","Python","Node.js","Browser","Swift","Kotlin","Java","Dart","Go","C","C++","C#"]}>
<Tab value="Rust">
`Engine::boundary` decodes the outline into an `Outline` you reuse (its rings, points and
`tolerance_cm`) and returns the layer that held it; `Outline::write_geojson` writes the same
GeoJSON every binding answers. `Engine::boundary_vertices` counts vertices without decoding.

```rust title="tour.rs"
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
```
</Tab>

<Tab value="Python">
```python title="tour.py"
(shape,) = rows(engine.boundary("tgd:np/2gm", 0))
feature = json.loads(shape["geojson"])  # a GeoJSON Feature, for any map library
print(f"{feature['geometry']['type']} of {shape['vertices']} vertices at level {shape['lod']}")
print(f"tolerance {shape['tolerance_m']:.0f} m, {len(shape['geojson'])} bytes of GeoJSON")
```
</Tab>

<Tab value="Node.js">
```js title="tour.mjs"
const [shape] = new Reader(engine.boundary("tgd:np/2gm", 0)).records();
const feature = JSON.parse(shape.geojson); // a GeoJSON Feature: map.addSource("x", { type: "geojson", data: feature })
console.log(`${feature.geometry.type} of ${shape.vertices} vertices at level ${shape.lod}`);
console.log(`tolerance ${fixed(shape.tolerance_m, 0)} m, ${shape.geojson.length} bytes of GeoJSON`);
```
</Tab>

<Tab value="Browser">
```js title="tour.mjs"
const [shape] = new Reader(engine.boundary("tgd:np/2gm", 0)).records();
const feature = JSON.parse(shape.geojson); // a GeoJSON Feature: map.addSource("x", { type: "geojson", data: feature })
console.log(`${feature.geometry.type} of ${shape.vertices} vertices at level ${shape.lod}`);
console.log(`tolerance ${fixed(shape.tolerance_m, 0)} m, ${shape.geojson.length} bytes of GeoJSON`);
```
</Tab>

<Tab value="Swift">
```swift title="main.swift"
let shape = try Reader(engine.boundary("tgd:np/2gm", lod: 0)).records()[0]
let geojson = shape["geojson"]!.description // a GeoJSON Feature, for any map library
let feature = try JSONSerialization.jsonObject(with: Data(geojson.utf8)) as! [String: Any]
let type = (feature["geometry"] as! [String: Any])["type"] as! String
print("\(type) of \(shape["vertices"]!) vertices at level \(shape["lod"]!)")
print("tolerance \(fixed(shape.double("tolerance_m"), 0)) m, \(geojson.utf8.count) bytes of GeoJSON")
```
</Tab>

<Tab value="Kotlin">
```kotlin title="Tour.kt"
val shape = Reader(engine.boundary("tgd:np/2gm", 0)).single()
val geojson = shape["geojson"] as String // a GeoJSON Feature, for any map library
val type = geojson.substringAfter("\"geometry\":{\"type\":\"").substringBefore('"')
println("%s of %s vertices at level %s".f(type, shape["vertices"], shape["lod"]))
println("tolerance %.0f m, %d bytes of GeoJSON".f(shape["tolerance_m"], geojson.length))
```
</Tab>

<Tab value="Java">
```java title="Tour.java"
Map<String, Object> shape = new Reader(engine.boundary("tgd:np/2gm", 0)).records().get(0);
String geojson = (String) shape.get("geojson"); // a GeoJSON Feature, for any map library
int at = geojson.indexOf("\"type\":\"", geojson.indexOf("\"geometry\"")) + 8;
String type = geojson.substring(at, geojson.indexOf('"', at));
System.out.println(f("%s of %s vertices at level %s", type, shape.get("vertices"), shape.get("lod")));
System.out.println(f("tolerance %.0f m, %d bytes of GeoJSON", shape.get("tolerance_m"), geojson.length()));
```
</Tab>

<Tab value="Dart">
```dart title="tour.dart"
final shape = Reader(engine.boundary('tgd:np/2gm', 0)).single;
final geojson = shape['geojson'] as String; // a GeoJSON Feature, for any map library
final feature = jsonDecode(geojson) as Map<String, dynamic>;
print('${feature['geometry']['type']} of ${shape['vertices']} vertices at level ${shape['lod']}');
final tolerance = shape['tolerance_m'] as double;
print('tolerance ${tolerance.toStringAsFixed(0)} m, ${utf8.encode(geojson).length} bytes of GeoJSON');
```
</Tab>

<Tab value="Go">
```go title="tour.go"
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
```
</Tab>

<Tab value="C">
```c title="tour.c"
r = reader(tg_engine_boundary(engine, TEXT("tgd:np/2gm"), 0, &blob), &blob);
tg_str geojson = str(&r, "geojson", 0); /* a GeoJSON Feature, for any map library */
const char *type = strstr(geojson.ptr, "\"geometry\":{\"type\":\"") + 20;
printf("%.*s of %lld vertices at level %lld\n", (int)strcspn(type, "\""), type, integer(&r, "vertices", 0),
       integer(&r, "lod", 0));
printf("tolerance %.0f m, %zu bytes of GeoJSON\n", num(&r, "tolerance_m", 0), geojson.len);
tg_bytes_free(blob);
```
</Tab>

<Tab value="C++">
```cpp title="tour.cpp"
const tg::Blob outline = engine.boundary("tgd:np/2gm", 0);
const auto shape = tg::Reader(outline)[0];
const std::string_view geojson = *shape.string("geojson"); // a GeoJSON Feature, for any map library
const std::string_view key = "\"geometry\":{\"type\":\"";
const std::string_view type = geojson.substr(geojson.find(key) + key.size());
std::cout << type.substr(0, type.find('"')) << " of " << *shape.integer("vertices") << " vertices at level "
          << *shape.integer("lod") << '\n';
std::cout << "tolerance " << fixed(shape.number("tolerance_m"), 0) << " m, " << geojson.size() << " bytes of GeoJSON\n";
```
</Tab>

<Tab value="C#">
```csharp title="Program.cs"
var shape = new BlobReader(engine.Boundary("tgd:np/2gm", 0)).Single();
string geojson = (string)shape["geojson"]!; // a GeoJSON Feature, for any map library
using var feature = System.Text.Json.JsonDocument.Parse(geojson);
string type = feature.RootElement.GetProperty("geometry").GetProperty("type").GetString()!;
Console.WriteLine($"{type} of {shape["vertices"]} vertices at level {shape["lod"]}");
Console.WriteLine($"tolerance {shape["tolerance_m"]:F0} m, {geojson.Length} bytes of GeoJSON");
```
</Tab>
</Tabs>

```text title="Output"
Polygon of 176 vertices at level 0
tolerance 0 m, 4362 bytes of GeoJSON
```

The Feature goes to a map as it is: MapLibre's and Leaflet's GeoJSON sources, OpenLayers,
Shapely's `shape()`, Turf and JTS all read it.

- **Level of detail.** `lod` 0 is full resolution; 1 to 6 are coarser, each within its
  `tolerance_m`, with neighbouring units still sharing their borders at every level. A pack
  stored at full resolution only (the lite packs) answers every level as 0. A country at level 0
  can be megabytes: draw a coarser level, and keep 0 for measuring.
- `lod` above 6 is `InvalidArgument`; a pack installed without `boundaries` is
  `MissingComponent`.
- Whether a point is *inside* a unit is `admin_at`, which tests the same outlines without
  building them.

## Next [#next]

- [Reference](/docs/reference): every query, its columns and its errors.
- [Errors](/docs/guides/errors): the eight codes and how each language raises them.
- [Concepts](/docs/concepts): places and IDs, packs and components, results, the store.