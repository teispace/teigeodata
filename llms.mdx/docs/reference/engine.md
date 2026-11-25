# Engine (/docs/reference/engine)

Open packs answering place queries. Every method takes `&self`: one engine answers from any number of threads at once, and adding a pack publishes a new snapshot while queries go on.

Place results are blobs with the columns `id`, `name`, `kind`, `level`, `lat`, `lon`, `elevation`, `parent`, `tz`, `iso`, `population`, `population_kind` (`stated` or `modelled`) and `class` (a landmark's class, such as `temple`; D-381); absent values are null. A search adds `score`, and a reverse lookup adds `meters` and `bearing`.

Every query returns a [result blob](/docs/concepts/results); read it with your language's
`Reader`. Pick a language in any tab and every tab on the site follows.

## `new` [#new]

An engine with no packs.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn new() -> Self
```
</Tab>

<Tab value="Python">
```python
def __init__(self) -> None: ...
```
</Tab>

<Tab value="JavaScript">
```ts
constructor();
```
</Tab>

<Tab value="Swift">
```swift
public init()
```
</Tab>

<Tab value="Java and Kotlin">
```java
public Engine();
```
</Tab>

<Tab value="Dart">
```dart
factory Engine(TeiGeodata lib);
```
</Tab>

<Tab value="Go">
```go
func NewEngine() (*Engine, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_new(tg_engine **out);
```
</Tab>

<Tab value="C++">
```cpp
Engine();
```
</Tab>

<Tab value=".NET">
```csharp
public Engine();
```
</Tab>
</Tabs>

## `open_file` [#open_file]

Opens the pack at `path` (memory-mapped) and adds it. Returns its layer ID, for `remove`: unique in this engine, counting up from 0.

**Errors.** `ErrorCode::Io` if the file cannot be read, `ErrorCode::Corrupt` if it is not a valid pack.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn open_file(&self, path: &str) -> Result<u32>
```
</Tab>

<Tab value="Python">
```python
def open_file(self, path: str) -> int: ...
```
</Tab>

<Tab value="JavaScript">
```ts
openFile(path: string): number;
```
</Tab>

<Tab value="Swift">
```swift
public func openFile(_ path: String) throws(TeiGeodataError) -> UInt32
```
</Tab>

<Tab value="Java and Kotlin">
```java
public long openFile(String path);
```
</Tab>

<Tab value="Dart">
```dart
int openFile(String path);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) OpenFile(path string) (uint32, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_open_file(const tg_engine *handle, const uint8_t *path, size_t path_len, uint32_t *out);
```
</Tab>

<Tab value="C++">
```cpp
std::uint32_t open_file(std::string_view path) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe uint OpenFile(string path);
```
</Tab>
</Tabs>

## `open_file_with` [#open_file_with]

Opens the pack at `path` with these options and adds it. Returns its layer ID, as `open_file`.

**Errors.** As `open_file`, and `ErrorCode::InvalidArgument` for an outline level above 255.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn open_file_with(&self, path: &str, options: &OpenOptions) -> Result<u32>
```
</Tab>

<Tab value="Python">
```python
def open_file_with(self, path: str, options: OpenOptions) -> int: ...
```
</Tab>

<Tab value="JavaScript">
```ts
openFileWith(path: string, options?: OpenOptions): number;
```
</Tab>

<Tab value="Swift">
```swift
public func openFileWith(_ path: String, options: OpenOptions = .init()) throws(TeiGeodataError) -> UInt32
```
</Tab>

<Tab value="Java and Kotlin">
```java
public long openFileWith(String path, OpenOptions options);
```
</Tab>

<Tab value="Dart">
```dart
int openFileWith(String path, OpenOptions options);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) OpenFileWith(path string, options *OpenOptions) (uint32, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_open_file_with(const tg_engine *handle, const uint8_t *path, size_t path_len, const tg_open_options *options, uint32_t *out);
```
</Tab>

<Tab value="C++">
```cpp
std::uint32_t open_file_with(std::string_view path, const OpenOptions &options = {}) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe uint OpenFileWith(string path, OpenOptions? options = null);
```
</Tab>
</Tabs>

## `open_file_range` [#open_file_range]

Opens the pack stored whole in `length` bytes at `offset` of the file at `path` (memory-mapped) and adds it: a pack bundled as an uncompressed asset of an Android app (`noCompress`; the path is the APK, the offset and length its `AssetFileDescriptor`'s), or one inside any container that stores it uncompressed. The offset need not be aligned. Returns its layer ID, as `open_file`.

**Errors.** `ErrorCode::Io` if the file cannot be read or the range is not inside it, `ErrorCode::Corrupt` if the range is not a valid pack.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn open_file_range(&self, path: &str, offset: u64, length: u64) -> Result<u32>
```
</Tab>

<Tab value="Python">
```python
def open_file_range(self, path: str, offset: int, length: int) -> int: ...
```
</Tab>

<Tab value="JavaScript">
```ts
openFileRange(path: string, offset: number, length: number): number;
```
</Tab>

<Tab value="Swift">
```swift
public func openFileRange(_ path: String, offset: UInt64, length: UInt64) throws(TeiGeodataError) -> UInt32
```
</Tab>

<Tab value="Java and Kotlin">
```java
public long openFileRange(String path, long offset, long length);
```
</Tab>

<Tab value="Dart">
```dart
int openFileRange(String path, int offset, int length);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) OpenFileRange(path string, offset uint64, length uint64) (uint32, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_open_file_range(const tg_engine *handle, const uint8_t *path, size_t path_len, uint64_t offset, uint64_t length, uint32_t *out);
```
</Tab>

<Tab value="C++">
```cpp
std::uint32_t open_file_range(std::string_view path, std::uint64_t offset, std::uint64_t length) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe uint OpenFileRange(string path, ulong offset, ulong length);
```
</Tab>
</Tabs>

## `open_file_range_with` [#open_file_range_with]

As `open_file_range`, with these options.

**Errors.** As `open_file_range`, and `ErrorCode::InvalidArgument` for an outline level above 255.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn open_file_range_with(&self, path: &str, offset: u64, length: u64, options: &OpenOptions) -> Result<u32>
```
</Tab>

<Tab value="Python">
```python
def open_file_range_with(self, path: str, offset: int, length: int, options: OpenOptions) -> int: ...
```
</Tab>

<Tab value="JavaScript">
```ts
openFileRangeWith(path: string, offset: number, length: number, options?: OpenOptions): number;
```
</Tab>

<Tab value="Swift">
```swift
public func openFileRangeWith(_ path: String, offset: UInt64, length: UInt64, options: OpenOptions = .init()) throws(TeiGeodataError) -> UInt32
```
</Tab>

<Tab value="Java and Kotlin">
```java
public long openFileRangeWith(String path, long offset, long length, OpenOptions options);
```
</Tab>

<Tab value="Dart">
```dart
int openFileRangeWith(String path, int offset, int length, OpenOptions options);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) OpenFileRangeWith(path string, offset uint64, length uint64, options *OpenOptions) (uint32, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_open_file_range_with(const tg_engine *handle, const uint8_t *path, size_t path_len, uint64_t offset, uint64_t length, const tg_open_options *options, uint32_t *out);
```
</Tab>

<Tab value="C++">
```cpp
std::uint32_t open_file_range_with(std::string_view path, std::uint64_t offset, std::uint64_t length, const OpenOptions &options = {}) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe uint OpenFileRangeWith(string path, ulong offset, ulong length, OpenOptions? options = null);
```
</Tab>
</Tabs>

## `open_bytes` [#open_bytes]

Adds a pack from its bytes (copied). Returns its layer ID, as `open_file`.

**Errors.** `ErrorCode::Corrupt` if the bytes are not a valid pack.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn open_bytes(&self, bytes: &[u8]) -> Result<u32>
```
</Tab>

<Tab value="Python">
```python
def open_bytes(self, bytes: bytes) -> int: ...
```
</Tab>

<Tab value="JavaScript">
```ts
openBytes(bytes: Buffer): number;
```
</Tab>

<Tab value="Swift">
```swift
public func openBytes(_ bytes: Data) throws(TeiGeodataError) -> UInt32
```
</Tab>

<Tab value="Java and Kotlin">
```java
public long openBytes(byte[] bytes);
```
</Tab>

<Tab value="Dart">
```dart
int openBytes(Uint8List bytes);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) OpenBytes(bytes []byte) (uint32, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_open_bytes(const tg_engine *handle, const uint8_t *bytes, size_t bytes_len, uint32_t *out);
```
</Tab>

<Tab value="C++">
```cpp
std::uint32_t open_bytes(ByteView bytes) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe uint OpenBytes(ReadOnlySpan<byte> bytes);
```
</Tab>
</Tabs>

## `open_bytes_with` [#open_bytes_with]

Adds a pack from its bytes (copied) with these options. Returns its layer ID, as `open_file`.

**Errors.** As `open_bytes`, and `ErrorCode::InvalidArgument` for an outline level above 255.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn open_bytes_with(&self, bytes: &[u8], options: &OpenOptions) -> Result<u32>
```
</Tab>

<Tab value="Python">
```python
def open_bytes_with(self, bytes: bytes, options: OpenOptions) -> int: ...
```
</Tab>

<Tab value="JavaScript">
```ts
openBytesWith(bytes: Buffer, options?: OpenOptions): number;
```
</Tab>

<Tab value="Swift">
```swift
public func openBytesWith(_ bytes: Data, options: OpenOptions = .init()) throws(TeiGeodataError) -> UInt32
```
</Tab>

<Tab value="Java and Kotlin">
```java
public long openBytesWith(byte[] bytes, OpenOptions options);
```
</Tab>

<Tab value="Dart">
```dart
int openBytesWith(Uint8List bytes, OpenOptions options);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) OpenBytesWith(bytes []byte, options *OpenOptions) (uint32, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_open_bytes_with(const tg_engine *handle, const uint8_t *bytes, size_t bytes_len, const tg_open_options *options, uint32_t *out);
```
</Tab>

<Tab value="C++">
```cpp
std::uint32_t open_bytes_with(ByteView bytes, const OpenOptions &options = {}) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe uint OpenBytesWith(ReadOnlySpan<byte> bytes, OpenOptions? options = null);
```
</Tab>
</Tabs>

## `remove` [#remove]

Removes the pack with this layer ID; false if no pack has it. Queries already running finish on the snapshot they started with; the pack's memory is released after them.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn remove(&self, layer: u32) -> bool
```
</Tab>

<Tab value="Python">
```python
def remove(self, layer: int) -> bool: ...
```
</Tab>

<Tab value="JavaScript">
```ts
remove(layer: number): boolean;
```
</Tab>

<Tab value="Swift">
```swift
public func remove(_ layer: UInt32) -> Bool
```
</Tab>

<Tab value="Java and Kotlin">
```java
public boolean remove(long layer);
```
</Tab>

<Tab value="Dart">
```dart
bool remove(int layer);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) Remove(layer uint32) bool
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_remove(const tg_engine *handle, uint32_t layer, bool *out);
```
</Tab>

<Tab value="C++">
```cpp
bool remove(std::uint32_t layer) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe bool Remove(uint layer);
```
</Tab>
</Tabs>

## `layers` [#layers]

The number of packs.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn layers(&self) -> u32
```
</Tab>

<Tab value="Python">
```python
def layers(self) -> int: ...
```
</Tab>

<Tab value="JavaScript">
```ts
layers(): number;
```
</Tab>

<Tab value="Swift">
```swift
public func layers() -> UInt32
```
</Tab>

<Tab value="Java and Kotlin">
```java
public long layers();
```
</Tab>

<Tab value="Dart">
```dart
int layers();
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) Layers() uint32
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_layers(const tg_engine *handle, uint32_t *out);
```
</Tab>

<Tab value="C++">
```cpp
std::uint32_t layers() const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe uint Layers();
```
</Tab>
</Tabs>

## `get` [#get]

The place with this ID (`tgd:np/1`), or no row; a retired ID gives its survivor.

**Errors.** `ErrorCode::InvalidArgument` if `id` is not a place ID; `ErrorCode::Corrupt` if a section read is corrupt.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn get(&self, id: &str) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def get(self, id: str) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
get(id: string): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func get(_ id: String) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] get(String id);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List get(String id);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) Get(id string) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_get(const tg_engine *handle, const uint8_t *id, size_t id_len, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob get(std::string_view id) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] Get(string id);
```
</Tab>
</Tabs>

## `parent` [#parent]

The place's parent as the packs answer for it (one row), or no row for a root or an unknown ID.

**Errors.** `ErrorCode::InvalidArgument` if `id` is not a place ID; `ErrorCode::Corrupt` if a section read is corrupt.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn parent(&self, id: &str) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def parent(self, id: str) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
parent(id: string): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func parent(_ id: String) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] parent(String id);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List parent(String id);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) Parent(id string) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_parent(const tg_engine *handle, const uint8_t *id, size_t id_len, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob parent(std::string_view id) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] Parent(string id);
```
</Tab>
</Tabs>

## `ancestors` [#ancestors]

The place's ancestors, parent first up to the country; no rows for a root or an unknown ID.

**Errors.** As `parent`.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn ancestors(&self, id: &str) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def ancestors(self, id: str) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
ancestors(id: string): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func ancestors(_ id: String) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] ancestors(String id);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List ancestors(String id);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) Ancestors(id string) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_ancestors(const tg_engine *handle, const uint8_t *id, size_t id_len, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob ancestors(std::string_view id) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] Ancestors(string id);
```
</Tab>
</Tabs>

## `children` [#children]

The place's children, by ID; no rows for a leaf or an unknown ID.

**Errors.** As `parent`.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn children(&self, id: &str) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def children(self, id: str) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
children(id: string): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func children(_ id: String) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] children(String id);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List children(String id);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) Children(id string) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_children(const tg_engine *handle, const uint8_t *id, size_t id_len, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob children(std::string_view id) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] Children(string id);
```
</Tab>
</Tabs>

## `nearby` [#nearby]

The places nearest the place with this ID, not counting it, nearest first, at most `limit`, with `meters` and `bearing` (from each place to it); no rows for an unknown ID.

**Errors.** `ErrorCode::InvalidArgument` if `id` is not a place ID or `limit` exceeds `LIMIT_MAX`; `ErrorCode::Corrupt` if a section read is corrupt.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn nearby(&self, id: &str, limit: u32) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def nearby(self, id: str, limit: int) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
nearby(id: string, limit: number): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func nearby(_ id: String, limit: UInt32) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] nearby(String id, long limit);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List nearby(String id, int limit);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) Nearby(id string, limit uint32) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_nearby(const tg_engine *handle, const uint8_t *id, size_t id_len, uint32_t limit, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob nearby(std::string_view id, std::uint32_t limit) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] Nearby(string id, uint limit);
```
</Tab>
</Tabs>

## `search` [#search]

Places matching `query`, best first, at most `limit`; "Name, Parent" ranks a named parent first. The `score` column compares only within one query.

**Errors.** `ErrorCode::InvalidArgument` if `limit` exceeds `LIMIT_MAX`; `ErrorCode::Corrupt` if a section read is corrupt.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn search(&self, query: &str, limit: u32) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def search(self, query: str, limit: int) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
search(query: string, limit: number): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func search(_ query: String, limit: UInt32) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] search(String query, long limit);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List search(String query, int limit);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) Search(query string, limit uint32) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_search(const tg_engine *handle, const uint8_t *query, size_t query_len, uint32_t limit, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob search(std::string_view query, std::uint32_t limit) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] Search(string query, uint limit);
```
</Tab>
</Tabs>

## `search_with` [#search_with]

`search` ranked by `options`: near a focus point, or in strict match order.

**Errors.** `ErrorCode::InvalidArgument` if `limit` exceeds `LIMIT_MAX`, the focus is off the globe or has one coordinate only, or its scale is not positive; `ErrorCode::Corrupt` if a section read is corrupt.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn search_with(&self, query: &str, limit: u32, options: &SearchOptions) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def search_with(self, query: str, limit: int, options: SearchOptions) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
searchWith(query: string, limit: number, options?: SearchOptions): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func searchWith(_ query: String, limit: UInt32, options: SearchOptions = .init()) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] searchWith(String query, long limit, SearchOptions options);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List searchWith(String query, int limit, SearchOptions options);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) SearchWith(query string, limit uint32, options *SearchOptions) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_search_with(const tg_engine *handle, const uint8_t *query, size_t query_len, uint32_t limit, const tg_search_options *options, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob search_with(std::string_view query, std::uint32_t limit, const SearchOptions &options = {}) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] SearchWith(string query, uint limit, SearchOptions? options = null);
```
</Tab>
</Tabs>

## `reverse` [#reverse]

The places nearest the point, nearest first, at most `limit`, with `meters` and `bearing` (degrees clockwise from north, from each place to the point).

**Errors.** `ErrorCode::InvalidArgument` for a point off the globe or a `limit` above `LIMIT_MAX`; `ErrorCode::Corrupt` if a section read is corrupt.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn reverse(&self, lat: f64, lon: f64, limit: u32) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def reverse(self, lat: float, lon: float, limit: int) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
reverse(lat: number, lon: number, limit: number): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func reverse(_ lat: Double, lon: Double, limit: UInt32) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] reverse(double lat, double lon, long limit);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List reverse(double lat, double lon, int limit);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) Reverse(lat float64, lon float64, limit uint32) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_reverse(const tg_engine *handle, double lat, double lon, uint32_t limit, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob reverse(double lat, double lon, std::uint32_t limit) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] Reverse(double lat, double lon, uint limit);
```
</Tab>
</Tabs>

## `within_radius` [#within_radius]

The places within `radius_m` metres of the point (great-circle), nearest first, at most `limit`, with `meters` and `bearing` (from each place to the point).

**Errors.** `ErrorCode::InvalidArgument` for a point off the globe, a negative or non-finite radius, or a `limit` above `LIMIT_MAX`; `ErrorCode::Corrupt` if a section read is corrupt.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn within_radius(&self, lat: f64, lon: f64, radius_m: f64, limit: u32) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def within_radius(self, lat: float, lon: float, radius_m: float, limit: int) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
withinRadius(lat: number, lon: number, radiusM: number, limit: number): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func withinRadius(_ lat: Double, lon: Double, radiusM: Double, limit: UInt32) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] withinRadius(double lat, double lon, double radiusM, long limit);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List withinRadius(double lat, double lon, double radiusM, int limit);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) WithinRadius(lat float64, lon float64, radiusM float64, limit uint32) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_within_radius(const tg_engine *handle, double lat, double lon, double radius_m, uint32_t limit, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob within_radius(double lat, double lon, double radius_m, std::uint32_t limit) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] WithinRadius(double lat, double lon, double radiusM, uint limit);
```
</Tab>
</Tabs>

## `admin_at` [#admin_at]

The admin units holding the point, country first.

**Errors.** `ErrorCode::InvalidArgument` for a point off the globe; `ErrorCode::Corrupt` if a section read is corrupt.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn admin_at(&self, lat: f64, lon: f64) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def admin_at(self, lat: float, lon: float) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
adminAt(lat: number, lon: number): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func adminAt(_ lat: Double, lon: Double) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] adminAt(double lat, double lon);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List adminAt(double lat, double lon);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) AdminAt(lat float64, lon float64) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_admin_at(const tg_engine *handle, double lat, double lon, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob admin_at(double lat, double lon) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] AdminAt(double lat, double lon);
```
</Tab>
</Tabs>

## `elevation_at` [#elevation_at]

The terrain height at the point, as one row: `meters` above the datum (bilinear between the DEM samples around it), `source` (the DEM product's key, null where the pack does not state it), `resolution_m` (the sample spacing north to south) and `datum` (`EGM2008`). No row where no pack has terrain there (the sea, outside every grid).

**Errors.** `ErrorCode::InvalidArgument` for a point off the globe; `ErrorCode::Corrupt` if a grid read is corrupt.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn elevation_at(&self, lat: f64, lon: f64) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def elevation_at(self, lat: float, lon: float) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
elevationAt(lat: number, lon: number): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func elevationAt(_ lat: Double, lon: Double) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] elevationAt(double lat, double lon);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List elevationAt(double lat, double lon);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) ElevationAt(lat float64, lon float64) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_elevation_at(const tg_engine *handle, double lat, double lon, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob elevation_at(double lat, double lon) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] ElevationAt(double lat, double lon);
```
</Tab>
</Tabs>

## `postal_lookup` [#postal_lookup]

The places postal code `code` names, one row per code and place, with the code's `postal`, `scheme`, `code_kind`, `valid_from` and `valid_to` columns. `scheme` names one scheme (`np`, `gb`); empty means every scheme whose rule accepts the input. The code may be typed with or without separators, in any case, or in another script's digits. Retired codes are returned too, with their validity.

**Errors.** `ErrorCode::Corrupt` if a section read is corrupt.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn postal_lookup(&self, code: &str, scheme: &str) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def postal_lookup(self, code: str, scheme: str) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
postalLookup(code: string, scheme: string): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func postalLookup(_ code: String, scheme: String) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] postalLookup(String code, String scheme);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List postalLookup(String code, String scheme);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) PostalLookup(code string, scheme string) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_postal_lookup(const tg_engine *handle, const uint8_t *code, size_t code_len, const uint8_t *scheme, size_t scheme_len, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob postal_lookup(std::string_view code, std::string_view scheme) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] PostalLookup(string code, string scheme);
```
</Tab>
</Tabs>

## `postal_prefix` [#postal_prefix]

Up to `limit` postal codes starting with `prefix`, with their places (autocomplete), in code order per scheme; `scheme` as in `postal_lookup`.

**Errors.** `ErrorCode::InvalidArgument` if `limit` exceeds `LIMIT_MAX`; `ErrorCode::Corrupt` if a section read is corrupt.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn postal_prefix(&self, prefix: &str, scheme: &str, limit: u32) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def postal_prefix(self, prefix: str, scheme: str, limit: int) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
postalPrefix(prefix: string, scheme: string, limit: number): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func postalPrefix(_ prefix: String, scheme: String, limit: UInt32) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] postalPrefix(String prefix, String scheme, long limit);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List postalPrefix(String prefix, String scheme, int limit);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) PostalPrefix(prefix string, scheme string, limit uint32) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_postal_prefix(const tg_engine *handle, const uint8_t *prefix, size_t prefix_len, const uint8_t *scheme, size_t scheme_len, uint32_t limit, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob postal_prefix(std::string_view prefix, std::string_view scheme, std::uint32_t limit) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] PostalPrefix(string prefix, string scheme, uint limit);
```
</Tab>
</Tabs>

## `postal_codes_of` [#postal_codes_of]

The postal codes of the place with this ID; no rows for an unknown ID.

**Errors.** `ErrorCode::InvalidArgument` if `id` is not a place ID; `ErrorCode::Corrupt` if a section read is corrupt.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn postal_codes_of(&self, id: &str) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def postal_codes_of(self, id: str) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
postalCodesOf(id: string): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func postalCodesOf(_ id: String) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] postalCodesOf(String id);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List postalCodesOf(String id);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) PostalCodesOf(id string) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_postal_codes_of(const tg_engine *handle, const uint8_t *id, size_t id_len, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob postal_codes_of(std::string_view id) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] PostalCodesOf(string id);
```
</Tab>
</Tabs>

## `postal_at` [#postal_at]

The postal codes of the point: those of the deepest admin unit holding it that an admin-aligned scheme gives codes to, with its parts (a ward's code; the point is in one of them). No rows where no unit holding the point has such codes.

**Errors.** `ErrorCode::InvalidArgument` for a point off the globe; `ErrorCode::Corrupt` if a section read is corrupt.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn postal_at(&self, lat: f64, lon: f64) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def postal_at(self, lat: float, lon: float) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
postalAt(lat: number, lon: number): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func postalAt(_ lat: Double, lon: Double) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] postalAt(double lat, double lon);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List postalAt(double lat, double lon);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) PostalAt(lat float64, lon float64) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_postal_at(const tg_engine *handle, double lat, double lon, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob postal_at(double lat, double lon) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] PostalAt(double lat, double lon);
```
</Tab>
</Tabs>

## `tz_at` [#tz_at]

The time-zone area holding the point (one row; its `tz` column is the IANA zone), or no row where no pack answers.

**Errors.** `ErrorCode::InvalidArgument` for a point off the globe; `ErrorCode::Corrupt` if a section read is corrupt.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn tz_at(&self, lat: f64, lon: f64) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def tz_at(self, lat: float, lon: float) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
tzAt(lat: number, lon: number): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func tzAt(_ lat: Double, lon: Double) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] tzAt(double lat, double lon);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List tzAt(double lat, double lon);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) TzAt(lat float64, lon float64) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_tz_at(const tg_engine *handle, double lat, double lon, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob tz_at(double lat, double lon) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] TzAt(double lat, double lon);
```
</Tab>
</Tabs>

## `tz_offset_at` [#tz_offset_at]

The UTC offset in force in `zone` (a tzdb name or link, `Asia/Kathmandu`) at `unix_seconds`, from the first layer whose zone rules hold the zone: one row with `seconds` east of UTC, `dst` (1 when tzdb counts it as daylight saving time) and `abbr` (`+0545`, `EDT`); no row where no layer holds the zone.

**Errors.** `ErrorCode::InvalidArgument` for an instant outside the years ±9,999; `ErrorCode::MissingComponent` if the rules that would hold the zone were not installed; `ErrorCode::Corrupt` if they are corrupt.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn tz_offset_at(&self, zone: &str, unix_seconds: i64) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def tz_offset_at(self, zone: str, unix_seconds: int) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
tzOffsetAt(zone: string, unixSeconds: number): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func tzOffsetAt(_ zone: String, unixSeconds: Int64) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] tzOffsetAt(String zone, long unixSeconds);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List tzOffsetAt(String zone, int unixSeconds);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) TzOffsetAt(zone string, unixSeconds int64) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_tz_offset_at(const tg_engine *handle, const uint8_t *zone, size_t zone_len, int64_t unix_seconds, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob tz_offset_at(std::string_view zone, std::int64_t unix_seconds) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] TzOffsetAt(string zone, long unixSeconds);
```
</Tab>
</Tabs>

## `tz_local` [#tz_local]

The offsets a civil time may have in `zone`: one row with `kind` (`unique`, `gap` for a time a transition skips, `overlap` for one that happens twice), `before` and `after` in seconds east of UTC (the offsets before and after the transition: for an overlap the earlier and the later instant's; for a unique time both the same); no row where no layer holds the zone.

**Errors.** `ErrorCode::InvalidArgument` for a date that is not one of the years ±9,999 or a time out of range; otherwise as `Engine::tz_offset_at`.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn tz_local(&self, zone: &str, year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def tz_local(self, zone: str, year: int, month: int, day: int, hour: int, minute: int, second: int) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
tzLocal(zone: string, year: number, month: number, day: number, hour: number, minute: number, second: number): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func tzLocal(_ zone: String, year: Int32, month: UInt32, day: UInt32, hour: UInt32, minute: UInt32, second: UInt32) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] tzLocal(String zone, int year, long month, long day, long hour, long minute, long second);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List tzLocal(String zone, int year, int month, int day, int hour, int minute, int second);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) TzLocal(zone string, year int32, month uint32, day uint32, hour uint32, minute uint32, second uint32) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_tz_local(const tg_engine *handle, const uint8_t *zone, size_t zone_len, int32_t year, uint32_t month, uint32_t day, uint32_t hour, uint32_t minute, uint32_t second, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob tz_local(std::string_view zone, std::int32_t year, std::uint32_t month, std::uint32_t day, std::uint32_t hour, std::uint32_t minute, std::uint32_t second) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] TzLocal(string zone, int year, uint month, uint day, uint hour, uint minute, uint second);
```
</Tab>
</Tabs>

## `tz_zones` [#tz_zones]

Every zone name (tzdb names and links) the layers' zone rules hold, in byte order: one `zone` column.

**Errors.** `ErrorCode::MissingComponent` if no installed layer has rules and one has rules that were not installed.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn tz_zones(&self) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def tz_zones(self) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
tzZones(): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func tzZones() throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] tzZones();
```
</Tab>

<Tab value="Dart">
```dart
Uint8List tzZones();
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) TzZones() ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_tz_zones(const tg_engine *handle, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob tz_zones() const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] TzZones();
```
</Tab>
</Tabs>

## `tz_rules_start` [#tz_rules_start]

The earliest instant `zone`'s rules cover, in Unix seconds: one `unix` row (its first transition, before which tzdb knows only local mean time; the year −9,999 for a zone that never changed); no row where no layer holds the zone.

**Errors.** As `Engine::tz_offset_at`.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn tz_rules_start(&self, zone: &str) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def tz_rules_start(self, zone: str) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
tzRulesStart(zone: string): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func tzRulesStart(_ zone: String) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] tzRulesStart(String zone);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List tzRulesStart(String zone);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) TzRulesStart(zone string) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_tz_rules_start(const tg_engine *handle, const uint8_t *zone, size_t zone_len, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob tz_rules_start(std::string_view zone) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] TzRulesStart(string zone);
```
</Tab>
</Tabs>

## `tz_version` [#tz_version]

The tzdb release (`2026f`) of the first layer with zone rules; empty without rules.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn tz_version(&self) -> String
```
</Tab>

<Tab value="Python">
```python
def tz_version(self) -> str: ...
```
</Tab>

<Tab value="JavaScript">
```ts
tzVersion(): string;
```
</Tab>

<Tab value="Swift">
```swift
public func tzVersion() -> String
```
</Tab>

<Tab value="Java and Kotlin">
```java
public String tzVersion();
```
</Tab>

<Tab value="Dart">
```dart
String tzVersion();
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) TzVersion() string
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_tz_version(const tg_engine *handle, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
std::string tz_version() const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe string TzVersion();
```
</Tab>
</Tabs>

## `by_iso` [#by_iso]

The place whose ISO 3166 code is `code` (`NP`, `NP-P3`, either case), or no row.

**Errors.** `ErrorCode::InvalidArgument` if `code` is not an ISO 3166 code; `ErrorCode::Corrupt` if a section read is corrupt.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn by_iso(&self, code: &str) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def by_iso(self, code: str) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
byIso(code: string): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func byIso(_ code: String) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] byIso(String code);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List byIso(String code);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) ByIso(code string) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_by_iso(const tg_engine *handle, const uint8_t *code, size_t code_len, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob by_iso(std::string_view code) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] ByIso(string code);
```
</Tab>
</Tabs>

## `boundary` [#boundary]

The outline of the place with this ID at level of detail `lod`, as one row: `id`, `lod` (the level read: 0 from a pack stored at full resolution only), `tolerance_m` (how far it may stray from the full-resolution outline), `vertices` and `geojson`, an RFC 7946 Feature ready for a map: a `Polygon` or `MultiPolygon` (outer rings counterclockwise, holes clockwise), or the place's `Point` when it has no outline. Level 0 is full resolution and 1 to `COARSEST_LOD` coarser; neighbours share their borders at every level. A whole country at level 0 can be megabytes: ask a coarser level to draw it small. No row for an unknown ID.

**Errors.** `ErrorCode::InvalidArgument` if `id` is not a place ID or `lod` exceeds `COARSEST_LOD`; `ErrorCode::MissingComponent` without `boundaries`; `ErrorCode::Corrupt` if a section read is corrupt.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn boundary(&self, id: &str, lod: u32) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def boundary(self, id: str, lod: int) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
boundary(id: string, lod: number): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func boundary(_ id: String, lod: UInt32) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] boundary(String id, long lod);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List boundary(String id, int lod);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) Boundary(id string, lod uint32) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_boundary(const tg_engine *handle, const uint8_t *id, size_t id_len, uint32_t lod, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob boundary(std::string_view id, std::uint32_t lod) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] Boundary(string id, uint lod);
```
</Tab>
</Tabs>

## `distance` [#distance]

The great-circle distance between the places with these IDs, as one row: `meters` on the mean Earth sphere and `bearing`, the initial bearing from `from` to `to` in degrees clockwise from north. No row when either ID is unknown. From a point, use `distance_m` and `bearing_deg` with the place's `lat` and `lon`.

**Errors.** `ErrorCode::InvalidArgument` if an ID is not a place ID; `ErrorCode::Corrupt` if a section read is corrupt.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn distance(&self, from: &str, to: &str) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def distance(self, from: str, to: str) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
distance(from: string, to: string): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func distance(_ from: String, to: String) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] distance(String from, String to);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List distance(String from, String to);
```
</Tab>

<Tab value="Go">
```go
func (h *Engine) Distance(from string, to string) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_engine_distance(const tg_engine *handle, const uint8_t *from, size_t from_len, const uint8_t *to, size_t to_len, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob distance(std::string_view from, std::string_view to) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] Distance(string from, string to);
```
</Tab>
</Tabs>