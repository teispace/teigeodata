# Functions (/docs/reference/functions)

These need no engine and no pack. `noop`, `sample_rows` and `progress` exist to measure and test
each binding's call, result and callback paths; an application does not need them.

## `version` [#version]

The library version, from its package metadata.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn version() -> String
```
</Tab>

<Tab value="Python">
```python
def version() -> str: ...
```
</Tab>

<Tab value="JavaScript">
```ts
export function version(): string;
```
</Tab>

<Tab value="Swift">
```swift
public static func version() -> String
```
</Tab>

<Tab value="Java and Kotlin">
```java
public static native String version();
```
</Tab>

<Tab value="Dart">
```dart
String version();
```
</Tab>

<Tab value="Go">
```go
func Version() (string, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_version(tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
inline std::string version();
```
</Tab>

<Tab value=".NET">
```csharp
public static string Version();
```
</Tab>
</Tabs>

## `components` [#components]

The components this build answers, in table order, comma-separated (`point, elevation, admin`). A build made without one answers its queries as a layer installed without it does: no rows, or `ErrorCode::MissingComponent`.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn components() -> String
```
</Tab>

<Tab value="Python">
```python
def components() -> str: ...
```
</Tab>

<Tab value="JavaScript">
```ts
export function components(): string;
```
</Tab>

<Tab value="Swift">
```swift
public static func components() -> String
```
</Tab>

<Tab value="Java and Kotlin">
```java
public static native String components();
```
</Tab>

<Tab value="Dart">
```dart
String components();
```
</Tab>

<Tab value="Go">
```go
func Components() (string, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_components(tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
inline std::string components();
```
</Tab>

<Tab value=".NET">
```csharp
public static string Components();
```
</Tab>
</Tabs>

## `noop` [#noop]

Does nothing. Measures the bare cost of a call across each boundary.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn noop()
```
</Tab>

<Tab value="Python">
```python
def noop() -> None: ...
```
</Tab>

<Tab value="JavaScript">
```ts
export function noop(): void;
```
</Tab>

<Tab value="Swift">
```swift
public static func noop()
```
</Tab>

<Tab value="Java and Kotlin">
```java
public static native void noop();
```
</Tab>

<Tab value="Dart">
```dart
void noop();
```
</Tab>

<Tab value="Go">
```go
func Noop() error
```
</Tab>

<Tab value="C">
```c
int32_t tg_noop(void);
```
</Tab>

<Tab value="C++">
```cpp
inline void noop();
```
</Tab>

<Tab value=".NET">
```csharp
public static void Noop();
```
</Tab>
</Tabs>

## `fold` [#fold]

The search fold of a name: NFKD, lower case, optional accents dropped by script, spaces collapsed, recomposed to NFC. The same fold the forge and the search index use.

**Errors.** `ErrorCode::MissingComponent` in a build without `search`, whose Unicode tables it uses.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn fold(text: &str) -> Result<String>
```
</Tab>

<Tab value="Python">
```python
def fold(text: str) -> str: ...
```
</Tab>

<Tab value="JavaScript">
```ts
export function fold(text: string): string;
```
</Tab>

<Tab value="Swift">
```swift
public static func fold(_ text: String) throws(TeiGeodataError) -> String
```
</Tab>

<Tab value="Java and Kotlin">
```java
public static native String fold(String text);
```
</Tab>

<Tab value="Dart">
```dart
String fold(String text);
```
</Tab>

<Tab value="Go">
```go
func Fold(text string) (string, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_fold(const uint8_t *text, size_t text_len, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
inline std::string fold(std::string_view text);
```
</Tab>

<Tab value=".NET">
```csharp
public static string Fold(string text);
```
</Tab>
</Tabs>

## `sample_rows` [#sample_rows]

Builds `rows` deterministic sample places as a blob with the columns `id` (`u32`), `lat` and `lon` (`f64`, degrees), `elevation` (`f32`, metres) and `name` (UTF-8). Exercises the result path of every binding.

**Errors.** `ErrorCode::InvalidArgument` if `rows` exceeds `SAMPLE_ROWS_MAX`.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn sample_rows(rows: u32) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def sample_rows(rows: int) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
export function sampleRows(rows: number): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public static func sampleRows(_ rows: UInt32) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public static native byte[] sampleRows(long rows);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List sampleRows(int rows);
```
</Tab>

<Tab value="Go">
```go
func SampleRows(rows uint32) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_sample_rows(uint32_t rows, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
inline Blob sample_rows(std::uint32_t rows);
```
</Tab>

<Tab value=".NET">
```csharp
public static byte[] SampleRows(uint rows);
```
</Tab>
</Tabs>

## `progress` [#progress]

Calls `on_step` once per step with the step number, from 1 to `steps`, on the calling thread, and returns the number of steps run. Exercises callbacks in every binding.

**Errors.** `ErrorCode::Cancelled` if `on_step` returns `false`.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn progress(steps: u32, on_step: &mut dyn FnMut(u32) -> bool) -> Result<u32>
```
</Tab>

<Tab value="Python">
```python
def progress(steps: int, on_step: Callable[[int], bool]) -> int: ...
```
</Tab>

<Tab value="JavaScript">
```ts
export function progress(steps: number, onStep: (a0: number) => boolean): number;
```
</Tab>

<Tab value="Swift">
```swift
public static func progress(_ steps: UInt32, onStep: (UInt32) -> Bool) throws(TeiGeodataError) -> UInt32
```
</Tab>

<Tab value="Java and Kotlin">
```java
public static native long progress(long steps, CallbackU32 onStep);
```
</Tab>

<Tab value="Dart">
```dart
int progress(int steps, bool Function(int) onStep);
```
</Tab>

<Tab value="Go">
```go
func Progress(steps uint32, onStep func(uint32) bool) (uint32, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_progress(uint32_t steps, tg_callback_u32 on_step, void *on_step_ctx, uint32_t *out);
```
</Tab>

<Tab value="C++">
```cpp
inline std::uint32_t progress(std::uint32_t steps, const std::function<bool(std::uint32_t)> &on_step);
```
</Tab>

<Tab value=".NET">
```csharp
public static uint Progress(uint steps, Func<uint, bool>? onStep);
```
</Tab>
</Tabs>

## `distance_m` [#distance_m]

Great-circle distance in metres between two points in degrees, by the haversine formula on the mean Earth sphere (error up to about 0.5% against the ellipsoid), as every query measures it, with the same digits on every platform.

**Errors.** `ErrorCode::InvalidArgument` if a latitude is outside -90..=90, a longitude outside -180..=180, or any value is not finite.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn distance_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> Result<f64>
```
</Tab>

<Tab value="Python">
```python
def distance_m(lat1: float, lon1: float, lat2: float, lon2: float) -> float: ...
```
</Tab>

<Tab value="JavaScript">
```ts
export function distanceM(lat1: number, lon1: number, lat2: number, lon2: number): number;
```
</Tab>

<Tab value="Swift">
```swift
public static func distanceM(_ lat1: Double, lon1: Double, lat2: Double, lon2: Double) throws(TeiGeodataError) -> Double
```
</Tab>

<Tab value="Java and Kotlin">
```java
public static native double distanceM(double lat1, double lon1, double lat2, double lon2);
```
</Tab>

<Tab value="Dart">
```dart
double distanceM(double lat1, double lon1, double lat2, double lon2);
```
</Tab>

<Tab value="Go">
```go
func DistanceM(lat1 float64, lon1 float64, lat2 float64, lon2 float64) (float64, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_distance_m(double lat1, double lon1, double lat2, double lon2, double *out);
```
</Tab>

<Tab value="C++">
```cpp
inline double distance_m(double lat1, double lon1, double lat2, double lon2);
```
</Tab>

<Tab value=".NET">
```csharp
public static double DistanceM(double lat1, double lon1, double lat2, double lon2);
```
</Tab>
</Tabs>

## `bearing_deg` [#bearing_deg]

The initial great-circle bearing from the first point to the second, in degrees clockwise from true north in `[0, 360)`; 0 for one point twice or from a pole. The same digits on every platform.

**Errors.** `ErrorCode::InvalidArgument` if a latitude is outside -90..=90, a longitude outside -180..=180, or any value is not finite.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn bearing_deg(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> Result<f64>
```
</Tab>

<Tab value="Python">
```python
def bearing_deg(lat1: float, lon1: float, lat2: float, lon2: float) -> float: ...
```
</Tab>

<Tab value="JavaScript">
```ts
export function bearingDeg(lat1: number, lon1: number, lat2: number, lon2: number): number;
```
</Tab>

<Tab value="Swift">
```swift
public static func bearingDeg(_ lat1: Double, lon1: Double, lat2: Double, lon2: Double) throws(TeiGeodataError) -> Double
```
</Tab>

<Tab value="Java and Kotlin">
```java
public static native double bearingDeg(double lat1, double lon1, double lat2, double lon2);
```
</Tab>

<Tab value="Dart">
```dart
double bearingDeg(double lat1, double lon1, double lat2, double lon2);
```
</Tab>

<Tab value="Go">
```go
func BearingDeg(lat1 float64, lon1 float64, lat2 float64, lon2 float64) (float64, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_bearing_deg(double lat1, double lon1, double lat2, double lon2, double *out);
```
</Tab>

<Tab value="C++">
```cpp
inline double bearing_deg(double lat1, double lon1, double lat2, double lon2);
```
</Tab>

<Tab value=".NET">
```csharp
public static double BearingDeg(double lat1, double lon1, double lat2, double lon2);
```
</Tab>
</Tabs>