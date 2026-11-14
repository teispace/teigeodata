# Teispace.TeiGeodata

Accurate, offline places for .NET: the .NET binding of
[teigeodata](https://github.com/teispace/teigeodata). Places with coordinates, elevation, the
admin hierarchy, local names, IANA time zones and postal codes, from signed, compact packs read
on the device.

```csharp
using Teispace.TeiGeodata;

using var engine = new Engine();
engine.OpenFile("np.tgd");
var places = new BlobReader(engine.Search("Kathmandu", 5));
string[] names = places.Strings("name");
ReadOnlySpan<double> lats = places.Values<double>("lat");
```

- **Generated from the Rust API**: every call, record and constant comes from the same
  description as the C, Swift, Kotlin, Python, Node.js, Dart and Go bindings.
- **No marshalling at run time**: source-generated, blittable P/Invoke (`LibraryImport`),
  NativeAOT and trimming compatible.
- **Results are blobs** (format v1): `BlobReader` views number columns in place as
  `ReadOnlySpan<T>` and decodes strings on demand.
- **Thread-safe handles**: an `Engine` answers from any number of threads; `Dispose` frees it
  once no call holds it.
- **Blocking store calls have awaitable forms** (`CatalogAsync`, `InstallPassAsync`, …) that
  run on threads of their own; a `Job` streams its progress with `ProgressAsync` and cancels
  with a `CancellationToken` passed to `ResultAsync`.
- **Errors** are `TeiGeodataException` with a stable `ErrorCode`, the same in every language.

The native library ships in the package under `runtimes/<rid>/native` for `osx-arm64`,
`osx-x64`, `linux-x64`, `linux-arm64`, `linux-musl-x64`, `linux-musl-arm64`, `win-x64` and
`win-arm64`. Set `TEIGEODATA_LIB` to a file, or call `Library.Load(path)`, to use another build.

Licence: Apache-2.0. Data packs carry their own licences (ODbL or CC-BY 4.0).
