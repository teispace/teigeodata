# Open a pack (/docs/guides/open)

<Callout title="Every example ran">
Each snippet is cut from one program per language, [the tour](https://github.com/teispace/teigeodata/tree/main/site/examples),
which CI runs on the released pack `world-np-lite.tgd` (`data-2026.11.14.3`) and checks line by
line against the output shown. The output is the same in every language. Pick your language
once: every page remembers it.
</Callout>

An `Engine` holds the packs it answers from. `open_file` memory-maps a pack file and
adds it as a **layer**; the engine answers from every layer at once, and a higher layer wins
where two disagree. Opening is cheap: the head of the file is read and checked and the block hashes of large
sections are proved; each section is verified against its hash the first time a query reads
it.

<Tabs groupId="lang" persist items={["Rust","Python","Node.js","Browser","Swift","Kotlin","Java","Dart","Go","C","C++","C#"]}>
<Tab value="Rust">
`Layer::open` maps the file and checks its head; `Engine::add` adds the layer and returns
its `LayerId`. `Layer::from_bytes` and `Layer::open_range` open bytes in memory and a pack inside
another file; `LayerOptions` holds the priority, `Verify`, `Load` and the parts to read.

```rust title="tour.rs"
let mut engine = Engine::new();
engine.add(Layer::open(&pack, LayerOptions::default())?);
println!("layers: {}", engine.layers().len());
// Buffers the queries write into: reuse them, and a warm query allocates nothing.
let mut scratch = Scratch::default();
let (mut hits, mut near, mut places) = (Vec::new(), Vec::new(), Vec::new());
```

`Engine::remove` takes a layer out by its `LayerId`. For layers that come and go while
other threads query, see `Shared` in [threads](/docs/guides/threads).
</Tab>

<Tab value="Python">
```python title="tour.py"
engine = teigeodata.Engine()
engine.open_file(pack)
print("layers:", engine.layers())
```
</Tab>

<Tab value="Node.js">
```js title="tour.mjs"
const engine = new Engine();
engine.openFile(pack);
console.log("layers:", engine.layers());
```
</Tab>

<Tab value="Browser">
A page has no file system: `engine.openBytes(bytes)` opens a pack from a `Uint8Array` (a
download, an asset, a file the user picked), copying it into the module's memory once.
`openFile`, the store and the `…Async` forms are left out of this package.

```js title="tour.mjs"
const engine = new Engine();
engine.openBytes(readFileSync(pack)); // in a browser: new Uint8Array(await (await fetch(url)).arrayBuffer())
console.log("layers:", engine.layers());
```
</Tab>

<Tab value="Swift">
```swift title="main.swift"
let engine = Engine()
_ = try engine.openFile(pack)  // the layer number
print("layers:", engine.layers())
```
</Tab>

<Tab value="Kotlin">
```kotlin title="Tour.kt"
Engine().use { engine ->
    engine.openFile(pack)
    println("layers: ${engine.layers()}")
```
</Tab>

<Tab value="Java">
```java title="Tour.java"
try (Engine engine = new Engine()) {
    engine.openFile(pack);
    System.out.println("layers: " + engine.layers());
```
</Tab>

<Tab value="Dart">
```dart title="tour.dart"
await TeiGeodata.init(); // loads the module on the web; returns at once natively
final tg = TeiGeodata();
final engine = Engine(tg);
engine.openFile(pack);
print('layers: ${engine.layers()}');
```
</Tab>

<Tab value="Go">
```go title="tour.go"
engine, err := tg.NewEngine()
if err != nil {
	log.Fatal(err)
}
defer engine.Close()
if _, err := engine.OpenFile(pack); err != nil {
	log.Fatal(err)
}
fmt.Println("layers:", engine.Layers())
```
</Tab>

<Tab value="C">
```c title="tour.c"
tg_engine *engine = NULL;
check(tg_engine_new(&engine));
uint32_t layer = 0, layers = 0;
check(tg_engine_open_file(engine, TEXT(pack), &layer));
check(tg_engine_layers(engine, &layers));
printf("layers: %u\n", layers);
```
</Tab>

<Tab value="C++">
```cpp title="tour.cpp"
tg::Engine engine;
engine.open_file(pack);
std::cout << "layers: " << engine.layers() << '\n';
```
</Tab>

<Tab value="C#">
```csharp title="Program.cs"
using var engine = new Engine();
engine.OpenFile(pack);
Console.WriteLine($"layers: {engine.Layers()}");
```
</Tab>
</Tabs>

```text title="Output"
layers: 1
```

- `open_bytes` adds a pack from bytes already in memory (a download, an app asset, the
  browser). `open_file_range` opens a pack stored uncompressed inside another file, such as an
  Android APK.
- Each `_with` form takes `OpenOptions`: precedence over the other packs, when sections are
  verified (lazy, eager or trusted), whether the file is mapped or read, and which components,
  languages and outline level to read.
- `open_file` returns the layer's number; `remove` takes it out again while other threads
  keep querying. One engine answers from any number of threads at once.

## Next [#next]

- [Reference](/docs/reference): every query, its columns and its errors.
- [Errors](/docs/guides/errors): the eight codes and how each language raises them.
- [Concepts](/docs/concepts): places and IDs, packs and components, results, the store.