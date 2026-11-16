# Threads and platforms (/docs/guides/threads)

One engine answers from any number of threads at once, and opening a pack maps it once
for all of them. What changes by language is how you share it and how the store's long calls
(download, install, update) run without blocking:

<Tabs groupId="lang" persist items={["Rust","Python","Node.js","Browser","Swift","Kotlin","Java","Dart","Go","C","C++","C#"]}>
<Tab value="Rust">
**Threads and updates**

`Engine` is `Send + Sync`: share it in an `Arc` and query from any number of threads. To add or
remove layers while other threads query, hold a `Shared`: `Shared::load` gives the snapshot a
query runs on, and `Shared::update` changes a copy and publishes it atomically, so queries
already running finish on the snapshot they started with:

```rust
use teigeodata_engine::{Engine, Layer, LayerOptions, Shared};

let shared = Shared::new(Engine::new());
let id = shared.update(|e| e.add(Layer::open("world-np-lite.tgd", LayerOptions::default()).unwrap()));
let snapshot = shared.load(); // derefs to &Engine
```
</Tab>

<Tab value="Python">
**Threads and asyncio**

An `Engine` is safe to share between threads, and queries release the GIL: a thread pool runs
them in parallel, and on a free-threaded build so does every thread. The store's long calls
(install, update) run as a `Job`; `teigeodata.aio` awaits one (`await aio.result(job)`) and
streams its progress (`async for done, total in aio.progress(job)`).
</Tab>

<Tab value="Node.js">
**Threads and the event loop**

Queries are synchronous and fast (microseconds), so they run on the calling thread. The store's
long calls have `…Async` forms that return promises and run off the event loop
(`await store.catalogAsync()`), and a `Job` reports its progress while it downloads. Worker
threads can each open their own `Engine` on the same file: the pages are mapped once and shared.
</Tab>

<Tab value="Browser">
**In a page**

```js
import { init, Engine, Reader } from "teigeodata-wasm";

await init();
const engine = new Engine();
const bytes = new Uint8Array(await (await fetch("/packs/world-np-lite.tgd")).arrayBuffer());
engine.openBytes(bytes);
const [here] = new Reader(engine.reverse(27.7172, 85.324, 1));
```

- **Cache the pack.** Serve it with a long `Cache-Control`, or keep it in the Cache API or
  OPFS so later visits work offline.
- **Run it in a Worker** when the pack is large: opening copies the bytes into the module's
  memory once; queries then take microseconds and never block on the network.
- **Bundlers:** the package's `exports` pick the right entry for browsers, Node.js, Deno and
  `workerd` (Cloudflare Workers); Vite, webpack and esbuild handle the `.wasm` file as an asset.
</Tab>

<Tab value="Swift">
**Concurrency**

`Engine`, `Store` and `Job` are `Sendable`: share one engine between tasks and threads. Queries
are fast enough for the main actor; the store's blocking calls have `async` forms
(`try await store.catalogAsync()`), and a `Job` streams its progress as an `AsyncSequence`.

**Bundling a pack**

Add the `.tgd` file to the app's resources and open it in place:
`try engine.openFile(Bundle.main.path(forResource: "world-np-lite", ofType: "tgd")!)`. The file is
memory-mapped, so it costs no heap.
</Tab>

<Tab value="Kotlin">
**Threads, coroutines and Android**

Handles are thread-safe and `AutoCloseable` (`use { }` in Kotlin, try-with-resources in Java);
a cleaner frees one you forget. The store's blocking calls have `…Async` forms returning
`CompletableFuture`, which Kotlin awaits with `kotlinx-coroutines-jdk8`'s `await()`.

On Android, keep a bundled pack uncompressed (`androidResources { noCompress += "tgd" }`) and
open it in place, without copying it out of the APK:

```kotlin
context.assets.openFd("world-np-lite.tgd").use { fd ->
    engine.openFileRange(context.applicationInfo.sourceDir, fd.startOffset, fd.length)
}
```
</Tab>

<Tab value="Java">
**Threads, coroutines and Android**

Handles are thread-safe and `AutoCloseable` (`use { }` in Kotlin, try-with-resources in Java);
a cleaner frees one you forget. The store's blocking calls have `…Async` forms returning
`CompletableFuture`, which Kotlin awaits with `kotlinx-coroutines-jdk8`'s `await()`.

On Android, keep a bundled pack uncompressed (`androidResources { noCompress += "tgd" }`) and
open it in place, without copying it out of the APK:

```kotlin
context.assets.openFd("world-np-lite.tgd").use { fd ->
    engine.openFileRange(context.applicationInfo.sourceDir, fd.startOffset, fd.length)
}
```
</Tab>

<Tab value="Dart">
**Flutter**

- **Await `TeiGeodata.init()` once** before the first engine. Natively it returns at once; on
  the web it loads the WebAssembly module, whose files go in `web/teigeodata_wasm/`.
- **Bundle a pack** as an asset and open it from its bytes
  (`engine.openBytes((await rootBundle.load('assets/world-np-lite.tgd')).buffer.asUint8List())`),
  or copy it to the app's documents directory once and `openFile` it there to map it instead.
- **Isolates.** Queries take microseconds and are fine on the UI isolate. The store's blocking
  calls have `…Async` forms that run on another thread, and a `Job` streams its progress.
- `cargo xtask flutter-check` runs the package's integration test in a Flutter app on the
  desktop, an iOS simulator or an Android device.
</Tab>

<Tab value="Go">
**Goroutines**

Handles may be used from any goroutine at once; `Close` frees one once no call holds it. The
store's long calls run as a `Job` whose progress you poll or wait for (`WaitProgress`) and which
`Cancel` stops.
</Tab>

<Tab value="C">
**Threads and memory**

Handles are thread-safe: any number of threads may query one `tg_engine` at once. Every
`tg_bytes` the library returns is freed with `tg_bytes_free`, every handle with its `_free`; a
panic inside the library is caught at the boundary and returned as `TG_ERROR_INTERNAL`.
</Tab>

<Tab value="C++">
**Threads**

An `Engine` answers from any number of threads at once. Handles are move-only.
</Tab>

<Tab value="C#">
**Threads and async**

An `Engine` answers from any number of threads; `Dispose` frees it once no call holds it. The
store's blocking calls have awaitable forms (`Store.OpenAsync`, `CatalogAsync`, `StatusAsync`,
`RemoveAsync`, …), and a `Job` streams its progress with `ProgressAsync`, gives its result with
`ResultAsync` and cancels through a `CancellationToken`.
</Tab>
</Tabs>