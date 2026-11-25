# Errors (/docs/guides/errors)

A call fails with one of eight stable codes, the same in every language:

| Code | Meaning |
|---|---|
| `InvalidArgument` | An argument is outside its range: a latitude of 91, a limit over 10,000, a malformed ID. |
| `MissingComponent` | The query needs a component the pack was installed without (search on a pack installed without `search`). |
| `Corrupt` | The bytes are not a valid pack, or a section does not match its hash. |
| `Io` | A file could not be read. |
| `Network` | No mirror answered, or the connection broke (the store only). |
| `Refused` | The store refused: the catalog did not verify, a pack is not installed or another needs it. |
| `Cancelled` | A callback or a job was asked to stop. |
| `Internal` | A bug inside teigeodata; a panic never crosses into your program. |

A query that finds nothing is not an error: it returns no rows.

## In your language [#in-your-language]

<Tabs groupId="lang" persist items={["Rust","Python","Node.js","Browser","Swift","Kotlin","Java","Dart","Go","C","C++","C#"]}>
<Tab value="Rust">
Rust returns `Result`s: the tour's `main` returns `Result<(), Box<dyn Error>>` and uses `?`
throughout. A point is checked when it is made, so the latitude error comes from
`Point::from_degrees`:

A query fails with the engine's `Error`; the bindings map it to their eight codes:

| `Error` | Meaning | Bindings' code |
|---|---|---|
| `Io` | A file could not be read. | `Io` |
| `Format` | The bytes are not a valid pack, or a section's structure is broken. | `Corrupt` |
| `Integrity { address, kind }` | A section's bytes do not match its hash. | `Corrupt` |
| `Range` | A section is missing or a byte range is outside it. | `Corrupt` |
| `MissingComponent { address, component }` | The query needs a component the layer was installed without. | `MissingComponent` |

Arguments are checked by their types before a query runs: a `Point` is in range when it is made,
and a `PlaceId` or an `IsoCode` when it is parsed. The store (`teigeodata-sync`) has its own
error type for the network and the catalog.

```rust title="tour.rs"
if let Err(e) = Point::<E6>::from_degrees(91.0, 0.0) {
    println!("invalid argument: (91, 0) is not a point: {e}");
}
```
</Tab>

<Tab value="Python">
Each code is an exception class under `teigeodata.TeiGeodataError`:
`InvalidArgumentError`, `MissingComponentError`, `CorruptError` and so on.

```python title="tour.py"
try:
    engine.reverse(91.0, 0.0, 1)
except teigeodata.InvalidArgumentError as error:
    print("invalid argument:", error)
```
</Tab>

<Tab value="Node.js">
Errors are thrown as `Error`s with a `code` property, one of the names above:

```js title="tour.mjs"
try {
  engine.reverse(91, 0, 1);
} catch (error) {
  if (error.code !== "InvalidArgument") throw error;
  console.log("invalid argument:", error.message);
}
```
</Tab>

<Tab value="Browser">
Errors are thrown as `Error`s with a `code` property, one of the names above:

```js title="tour.mjs"
try {
  engine.reverse(91, 0, 1);
} catch (error) {
  if (error.code !== "InvalidArgument") throw error;
  console.log("invalid argument:", error.message);
}
```
</Tab>

<Tab value="Swift">
Calls throw `TeiGeodataError` (typed throws), whose `code` is one of the names above
(`.invalidArgument`, `.missingComponent`, …) and whose `message` says what was wrong:

```swift title="main.swift"
do {
    _ = try engine.reverse(91, lon: 0, limit: 1)
} catch where error.code == .invalidArgument {
    print("invalid argument:", error.message)
}
```
</Tab>

<Tab value="Kotlin">
Each code is a subclass of the unchecked `TeiGeodataException`:
`TeiGeodataException.InvalidArgument`, `.MissingComponent` and so on.

```kotlin title="Tour.kt"
try {
    engine.reverse(91.0, 0.0, 1)
} catch (error: TeiGeodataException.InvalidArgument) {
    println("invalid argument: ${error.message}")
}
```
</Tab>

<Tab value="Java">
Each code is a subclass of the unchecked `TeiGeodataException`:
`TeiGeodataException.InvalidArgument`, `.MissingComponent` and so on.

```java title="Tour.java"
try {
    engine.reverse(91, 0, 1);
} catch (TeiGeodataException.InvalidArgument error) {
    System.out.println("invalid argument: " + error.getMessage());
}
```
</Tab>

<Tab value="Dart">
Each code is a subclass of `TeiGeodataException`: `InvalidArgumentException`,
`MissingComponentException` and so on:

```dart title="tour.dart"
try {
  engine.reverse(91, 0, 1);
} on InvalidArgumentException catch (error) {
  print('invalid argument: ${error.message}');
}
```
</Tab>

<Tab value="Go">
Every failure is a `*tg.Error` with a stable `Code`; match it with `errors.As`, or
`errors.Is(err, tg.CodeMissingComponent)`:

```go title="tour.go"
_, err = engine.Reverse(91, 0, 1)
var failure *tg.Error
if errors.As(err, &failure) && failure.Code == tg.CodeInvalidArgument {
	fmt.Println("invalid argument:", failure.Message)
}
```
</Tab>

<Tab value="C">
Every function returns `TG_OK` (0) or an error code (`TG_ERROR_INVALID_ARGUMENT`, …);
`tg_last_error` gives the calling thread's message:

```c title="tour.c"
if (tg_engine_reverse(engine, 91, 0, 1, &blob) == TG_ERROR_INVALID_ARGUMENT) {
    tg_bytes message = {0};
    tg_last_error(&message);
    printf("invalid argument: %.*s\n", (int)message.len, (const char *)message.ptr);
    tg_bytes_free(message);
}
```
</Tab>

<Tab value="C++">
Calls throw `teigeodata::Error`, whose `code()` is a `teigeodata::ErrorCode`:

```cpp title="tour.cpp"
try {
    engine.reverse(91, 0, 1);
} catch (const tg::Error &error) {
    if (error.code() == tg::ErrorCode::InvalidArgument) std::cout << "invalid argument: " << error.what() << '\n';
}
```
</Tab>

<Tab value="C#">
Calls throw `TeiGeodataException`, whose `Code` is an `ErrorCode`:

```csharp title="Program.cs"
try
{
    engine.Reverse(91, 0, 1);
}
catch (TeiGeodataException error) when (error.Code == ErrorCode.InvalidArgument)
{
    Console.WriteLine($"invalid argument: {error.Message}");
}
```
</Tab>
</Tabs>

```text title="Output"
invalid argument: (91, 0) is not a point: the latitude is outside -90 to 90 degrees
```