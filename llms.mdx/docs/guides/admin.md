# Admin units at a point (/docs/guides/admin)

<Callout title="Every example ran">
Each snippet is cut from one program per language, [the tour](https://github.com/teispace/teigeodata/tree/main/site/examples),
which CI runs on the released pack `world-np-lite.tgd` (`data-2026.11.14.3`) and checks line by
line against the output shown. The output is the same in every language. Pick your language
once: every page remembers it.
</Callout>

`admin_at` tests the point against the admin boundaries and gives every unit that holds
it, country first. `by_iso` finds a country or a subdivision by its ISO 3166 code (`NP`,
`NP-P4`, in either case).

<Tabs groupId="lang" persist items={["Rust","Python","Node.js","Browser","Swift","Kotlin","Java","Dart","Go","C","C++","C#"]}>
<Tab value="Rust">
```rust title="tour.rs"
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
```
</Tab>

<Tab value="Python">
```python title="tour.py"
for p in rows(engine.admin_at(28.2096, 83.9856)):
    print(f"{p['level']}  {p['name']}  {p['iso'] or ''}".rstrip())
(state,) = rows(engine.by_iso("NP-P4"))
print("NP-P4 is", state["name"], state["id"])
```
</Tab>

<Tab value="Node.js">
```js title="tour.mjs"
for (const p of new Reader(engine.adminAt(28.2096, 83.9856))) {
  console.log(`${p.level}  ${p.name}  ${p.iso ?? ""}`.trimEnd());
}
const [state] = new Reader(engine.byIso("NP-P4")).records();
console.log("NP-P4 is", state.name, state.id);
```
</Tab>

<Tab value="Browser">
```js title="tour.mjs"
for (const p of new Reader(engine.adminAt(28.2096, 83.9856))) {
  console.log(`${p.level}  ${p.name}  ${p.iso ?? ""}`.trimEnd());
}
const [state] = new Reader(engine.byIso("NP-P4")).records();
console.log("NP-P4 is", state.name, state.id);
```
</Tab>

<Tab value="Swift">
```swift title="main.swift"
for p in try Reader(engine.adminAt(28.2096, lon: 83.9856)) {
    let line = "\(p["level"]!)  \(p["name"]!)  \(p.string("iso") ?? "")"
    print(line.replacingOccurrences(of: "\\s+$", with: "", options: .regularExpression))
}
let state = try Reader(engine.byIso("NP-P4")).records()[0]
print("NP-P4 is", state["name"]!, state["id"]!)
```
</Tab>

<Tab value="Kotlin">
```kotlin title="Tour.kt"
for (p in Reader(engine.adminAt(28.2096, 83.9856))) {
    println("${p["level"]}  ${p["name"]}  ${p["iso"] ?: ""}".trimEnd())
}
val state = Reader(engine.byIso("NP-P4")).single()
println("NP-P4 is ${state["name"]} ${state["id"]}")
```
</Tab>

<Tab value="Java">
```java title="Tour.java"
for (Map<String, Object> p : new Reader(engine.adminAt(28.2096, 83.9856))) {
    System.out.println(f("%d  %s  %s", p.get("level"), p.get("name"), orEmpty(p.get("iso"))).stripTrailing());
}
Map<String, Object> state = new Reader(engine.byIso("NP-P4")).records().get(0);
System.out.println("NP-P4 is " + state.get("name") + " " + state.get("id"));
```
</Tab>

<Tab value="Dart">
```dart title="tour.dart"
for (final p in Reader(engine.adminAt(28.2096, 83.9856))) {
  print('${p['level']}  ${p['name']}  ${p['iso'] ?? ''}'.trimRight());
}
final state = Reader(engine.byIso('NP-P4')).single;
print('NP-P4 is ${state['name']} ${state['id']}');
```
</Tab>

<Tab value="Go">
```go title="tour.go"
for _, p := range rows(engine.AdminAt(28.2096, 83.9856)) {
	fmt.Println(strings.TrimRight(fmt.Sprintf("%d  %s  %s", p["level"], p["name"], orEmpty(p["iso"])), " "))
}
state := rows(engine.ByIso("NP-P4"))[0]
fmt.Println("NP-P4 is", state["name"], state["id"])
```
</Tab>

<Tab value="C">
```c title="tour.c"
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
```
</Tab>

<Tab value="C++">
```cpp title="tour.cpp"
const tg::Blob units = engine.admin_at(28.2096, 83.9856);
for (auto p : tg::Reader(units)) {
    std::cout << *p.integer("level") << "  " << *p.string("name");
    if (auto iso = p.string("iso")) std::cout << "  " << *iso;
    std::cout << '\n';
}
const tg::Blob state = engine.by_iso("NP-P4");
const auto province = tg::Reader(state)[0];
std::cout << "NP-P4 is " << *province.string("name") << ' ' << *province.string("id") << '\n';
```
</Tab>

<Tab value="C#">
```csharp title="Program.cs"
foreach (var p in new BlobReader(engine.AdminAt(28.2096, 83.9856)))
{
    Console.WriteLine($"{p["level"]}  {p["name"]}  {p["iso"]}".TrimEnd());
}
var state = new BlobReader(engine.ByIso("NP-P4")).Single();
Console.WriteLine($"NP-P4 is {state["name"]} {state["id"]}");
```
</Tab>
</Tabs>

```text title="Output"
0  Nepal  NP
1  Gandaki  NP-P4
2  Kaski
3  Pokhara
NP-P4 is Gandaki tgd:np/2922
```

Boundaries are exact polygons, so a point near a border is answered by the side it is on,
not by the nearest centre. Disputed boundaries are drawn from one stated perspective; see
[accuracy](/docs/concepts/accuracy).

## Next [#next]

- [Reference](/docs/reference): every query, its columns and its errors.
- [Errors](/docs/guides/errors): the eight codes and how each language raises them.
- [Concepts](/docs/concepts): places and IDs, packs and components, results, the store.