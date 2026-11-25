# Timezones (/docs/guides/timezones)

<Callout title="Every example ran">
Each snippet is cut from one program per language, [the tour](https://github.com/teispace/teigeodata/tree/main/site/examples),
which CI runs on the released pack `world-np-lite.tgd` (`data-2026.11.14.3`) and checks line by
line against the output shown. The output is the same in every language. Pick your language
once: every page remembers it.
</Callout>

`tz_at` gives the IANA zone at a point as the zone boundaries draw it. With the zone,
`tz_offset_at` gives the UTC offset at an instant (from the tzdb rules inside the pack, not
from the operating system), and `tz_local` tells whether a wall-clock time exists: `unique`,
`gap` (skipped by a daylight-saving change) or `overlap` (it happens twice), with the offsets
before and after.

<Tabs groupId="lang" persist items={["Rust","Python","Node.js","Browser","Swift","Kotlin","Java","Dart","Go","C","C++","C#"]}>
<Tab value="Rust">
`tz_at` answers the timezone area as a `Place`; `Engine::zone_name` reads its IANA name.
Local times are `Engine::tz_local_candidates`: `Local::Unique`, `Local::Gap` or
`Local::Overlap`.

```rust title="tour.rs"
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
```
</Tab>

<Tab value="Python">
```python title="tour.py"
(area,) = rows(engine.tz_at(27.7172, 85.3240))
zone = area["tz"]
(offset,) = rows(engine.tz_offset_at(zone, 1_700_000_000))
print(zone, offset["seconds"], "s east of UTC,", offset["abbr"], "dst", offset["dst"])
(local,) = rows(engine.tz_local("America/New_York", 2026, 3, 8, 2, 30, 0))
print("02:30 on 2026-03-08 in New York:", local["kind"], local["before"], "->", local["after"])
print("tzdb", engine.tz_version())
```
</Tab>

<Tab value="Node.js">
```js title="tour.mjs"
const [area] = new Reader(engine.tzAt(27.7172, 85.324)).records();
const zone = area.tz;
const [offset] = new Reader(engine.tzOffsetAt(zone, 1_700_000_000)).records();
console.log(zone, offset.seconds, "s east of UTC,", offset.abbr, "dst", offset.dst);
const [local] = new Reader(engine.tzLocal("America/New_York", 2026, 3, 8, 2, 30, 0)).records();
console.log("02:30 on 2026-03-08 in New York:", local.kind, local.before, "->", local.after);
console.log("tzdb", engine.tzVersion());
```
</Tab>

<Tab value="Browser">
```js title="tour.mjs"
const [area] = new Reader(engine.tzAt(27.7172, 85.324)).records();
const zone = area.tz;
const [offset] = new Reader(engine.tzOffsetAt(zone, 1_700_000_000)).records();
console.log(zone, offset.seconds, "s east of UTC,", offset.abbr, "dst", offset.dst);
const [local] = new Reader(engine.tzLocal("America/New_York", 2026, 3, 8, 2, 30, 0)).records();
console.log("02:30 on 2026-03-08 in New York:", local.kind, local.before, "->", local.after);
console.log("tzdb", engine.tzVersion());
```
</Tab>

<Tab value="Swift">
```swift title="main.swift"
let zone = try Reader(engine.tzAt(27.7172, lon: 85.3240)).records()[0].string("tz")!
let offset = try Reader(engine.tzOffsetAt(zone, unixSeconds: 1_700_000_000)).records()[0]
print(zone, offset["seconds"]!, "s east of UTC,", offset["abbr"]!, "dst", offset["dst"]!)
let local = try Reader(engine.tzLocal("America/New_York", year: 2026, month: 3, day: 8, hour: 2, minute: 30, second: 0)).records()[0]
print("02:30 on 2026-03-08 in New York:", local["kind"]!, local["before"]!, "->", local["after"]!)
print("tzdb", engine.tzVersion())
```
</Tab>

<Tab value="Kotlin">
```kotlin title="Tour.kt"
val zone = Reader(engine.tzAt(27.7172, 85.3240)).single()["tz"] as String
val offset = Reader(engine.tzOffsetAt(zone, 1_700_000_000)).single()
println("$zone ${offset["seconds"]} s east of UTC, ${offset["abbr"]} dst ${offset["dst"]}")
val local = Reader(engine.tzLocal("America/New_York", 2026, 3, 8, 2, 30, 0)).single()
println("02:30 on 2026-03-08 in New York: ${local["kind"]} ${local["before"]} -> ${local["after"]}")
println("tzdb ${engine.tzVersion()}")
```
</Tab>

<Tab value="Java">
```java title="Tour.java"
String zone = (String) new Reader(engine.tzAt(27.7172, 85.3240)).records().get(0).get("tz");
Map<String, Object> offset = new Reader(engine.tzOffsetAt(zone, 1_700_000_000L)).records().get(0);
System.out.println(zone + " " + offset.get("seconds") + " s east of UTC, " + offset.get("abbr") + " dst " + offset.get("dst"));
Map<String, Object> local = new Reader(engine.tzLocal("America/New_York", 2026, 3, 8, 2, 30, 0)).records().get(0);
System.out.println("02:30 on 2026-03-08 in New York: " + local.get("kind") + " " + local.get("before") + " -> " + local.get("after"));
System.out.println("tzdb " + engine.tzVersion());
```
</Tab>

<Tab value="Dart">
```dart title="tour.dart"
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
```
</Tab>

<Tab value="Go">
```go title="tour.go"
zone := rows(engine.TzAt(27.7172, 85.3240))[0]["tz"].(string)
offset := rows(engine.TzOffsetAt(zone, 1_700_000_000))[0]
fmt.Println(zone, offset["seconds"], "s east of UTC,", offset["abbr"], "dst", offset["dst"])
local := rows(engine.TzLocal("America/New_York", 2026, 3, 8, 2, 30, 0))[0]
fmt.Println("02:30 on 2026-03-08 in New York:", local["kind"], local["before"], "->", local["after"])
fmt.Println("tzdb", engine.TzVersion())
```
</Tab>

<Tab value="C">
```c title="tour.c"
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
```
</Tab>

<Tab value="C++">
```cpp title="tour.cpp"
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
```
</Tab>

<Tab value="C#">
```csharp title="Program.cs"
var zone = (string)new BlobReader(engine.TzAt(27.7172, 85.3240)).Single()["tz"]!;
var offset = new BlobReader(engine.TzOffsetAt(zone, 1_700_000_000)).Single();
Console.WriteLine($"{zone} {offset["seconds"]} s east of UTC, {offset["abbr"]} dst {offset["dst"]}");
var local = new BlobReader(engine.TzLocal("America/New_York", 2026, 3, 8, 2, 30, 0)).Single();
Console.WriteLine($"02:30 on 2026-03-08 in New York: {local["kind"]} {local["before"]} -> {local["after"]}");
Console.WriteLine($"tzdb {engine.TzVersion()}");
```
</Tab>
</Tabs>

```text title="Output"
Asia/Kathmandu 20700 s east of UTC, +0545 dst 0
02:30 on 2026-03-08 in New York: gap -18000 -> -14400
tzdb 2026f
```

`tz_version` names the tzdb release the rules come from, so you can tell when a pack is
older than a rule change; `tz_zones` lists every zone the packs hold.

## Next [#next]

- [Reference](/docs/reference): every query, its columns and its errors.
- [Errors](/docs/guides/errors): the eight codes and how each language raises them.
- [Concepts](/docs/concepts): places and IDs, packs and components, results, the store.