# Store and Job (/docs/reference/store)

<Callout title="Native builds">
This reference is the `store` feature of every native library. The browser has the
same store as `teigeodata-wasm/store`, over the origin private file system. Both install from the
live signed catalog; its addresses are on [the store](/docs/concepts/store).
</Callout>

## Store [#store]

A store of installed packs in one directory: its signed catalog, its packs and their journal, shared by every language and the command line (research note store-api). Long work runs as a `Job` on a thread of its own; one store runs its jobs one at a time, and its other calls wait for the job running.

Packs are named as catalog paths (`np.places.tgd`) or short (`np`, `np.dem`), several comma-separated. Results are blobs:
- `catalog`: `path`, `release`, `download`, `disk`, `components`, `licence`, `state` (`available`, `installed` or `update`);
- `status`: `path`, `release`, `bytes`, `pinned`, `asked`, `health` (`good`, `length`, `head`, `footer` or `sections`), `components`;
- an install or update: `path`, `release`, `fetched`, `reused`, `kept`, `changed`;
- a refresh: `timestamp`, `files`, `bytes`; a repair: `path`, `health`, `fetched`; `remove`: `path`, `release`;
- `find` and `locate`: one row per place and pack that covers it: `id`, `name`, `lat`, `lon`, `countries`, then the pack's `pack`, `release`, `download`, `disk` and `state`, null for a place no pack in the catalog covers.

### `create` [#create]

Sets up a store at `root` and opens it: the metadata mirrors `repo` and asset mirrors `assets` (comma-separated URLs), and `trusted_root`, the catalog's root metadata, which becomes its trust anchor once it verifies by itself. A store set up before is set up again with these.

**Errors.** `ErrorCode::Refused` if the root does not verify or the store cannot be written.

**Blocks.** While a job of this store runs, and on the disk: call it off a UI thread, or use the language's asynchronous form.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn create(root: &str, repo: &str, assets: &str, trusted_root: &[u8]) -> Result<Self>
```
</Tab>

<Tab value="Python">
```python
def create(root: str, repo: str, assets: str, trusted_root: bytes) -> Store: ...
```
</Tab>

<Tab value="JavaScript">
```ts
static create(root: string, repo: string, assets: string, trustedRoot: Buffer): Store;
```
</Tab>

<Tab value="Swift">
```swift
public static func create(_ root: String, repo: String, assets: String, trustedRoot: Data) throws(TeiGeodataError) -> Store
```
</Tab>

<Tab value="Java and Kotlin">
```java
public static Store create(String root, String repo, String assets, byte[] trustedRoot);
```
</Tab>

<Tab value="Dart">
```dart
static Store create(TeiGeodata lib, String root, String repo, String assets, Uint8List trustedRoot);
```
</Tab>

<Tab value="Go">
```go
func CreateStore(root string, repo string, assets string, trustedRoot []byte) (*Store, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_store_create(const uint8_t *root, size_t root_len, const uint8_t *repo, size_t repo_len, const uint8_t *assets, size_t assets_len, const uint8_t *trusted_root, size_t trusted_root_len, tg_store **out);
```
</Tab>

<Tab value="C++">
```cpp
static Store create(std::string_view root, std::string_view repo, std::string_view assets, ByteView trusted_root);
```
</Tab>

<Tab value=".NET">
```csharp
public static unsafe Store Create(string root, string repo, string assets, ReadOnlySpan<byte> trustedRoot);
```
</Tab>
</Tabs>

### `open` [#open]

Opens the store at `root`, finishing or undoing an install a crash cut short.

**Errors.** `ErrorCode::Refused` if it is not a store or its journal cannot be read.

**Blocks.** While a job of this store runs, and on the disk: call it off a UI thread, or use the language's asynchronous form.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn open(root: &str) -> Result<Self>
```
</Tab>

<Tab value="Python">
```python
def open(root: str) -> Store: ...
```
</Tab>

<Tab value="JavaScript">
```ts
static open(root: string): Store;
```
</Tab>

<Tab value="Swift">
```swift
public static func open(_ root: String) throws(TeiGeodataError) -> Store
```
</Tab>

<Tab value="Java and Kotlin">
```java
public static Store open(String root);
```
</Tab>

<Tab value="Dart">
```dart
static Store open(TeiGeodata lib, String root);
```
</Tab>

<Tab value="Go">
```go
func OpenStore(root string) (*Store, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_store_open(const uint8_t *root, size_t root_len, tg_store **out);
```
</Tab>

<Tab value="C++">
```cpp
static Store open(std::string_view root);
```
</Tab>

<Tab value=".NET">
```csharp
public static unsafe Store Open(string root);
```
</Tab>
</Tabs>

### `damaged` [#damaged]

The packs found damaged when the store was opened, comma-separated; empty when none. `repair` fetches what they lack.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn damaged(&self) -> String
```
</Tab>

<Tab value="Python">
```python
def damaged(self) -> str: ...
```
</Tab>

<Tab value="JavaScript">
```ts
damaged(): string;
```
</Tab>

<Tab value="Swift">
```swift
public func damaged() -> String
```
</Tab>

<Tab value="Java and Kotlin">
```java
public String damaged();
```
</Tab>

<Tab value="Dart">
```dart
String damaged();
```
</Tab>

<Tab value="Go">
```go
func (h *Store) Damaged() string
```
</Tab>

<Tab value="C">
```c
int32_t tg_store_damaged(const tg_store *handle, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
std::string damaged() const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe string Damaged();
```
</Tab>
</Tabs>

### `catalog` [#catalog]

The verified catalog as stored, one row per pack, with its state against the store.

**Errors.** `ErrorCode::Refused` before a refresh, or if the stored catalog no longer verifies.

**Blocks.** While a job of this store runs, and on the disk: call it off a UI thread, or use the language's asynchronous form.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn catalog(&self) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def catalog(self) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
catalog(): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func catalog() throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] catalog();
```
</Tab>

<Tab value="Dart">
```dart
Uint8List catalog();
```
</Tab>

<Tab value="Go">
```go
func (h *Store) Catalog() ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_store_catalog(const tg_store *handle, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob catalog() const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] Catalog();
```
</Tab>
</Tabs>

### `status` [#status]

Each installed pack and its health, from a full check of every byte when `deep`.

**Errors.** `ErrorCode::Refused` if a pack cannot be read.

**Blocks.** While a job of this store runs, and on the disk: call it off a UI thread, or use the language's asynchronous form.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn status(&self, deep: bool) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def status(self, deep: bool) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
status(deep: boolean): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func status(_ deep: Bool) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] status(boolean deep);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List status(bool deep);
```
</Tab>

<Tab value="Go">
```go
func (h *Store) Status(deep bool) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_store_status(const tg_store *handle, bool deep, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob status(bool deep) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] Status(bool deep);
```
</Tab>
</Tabs>

### `pin` [#pin]

Pins the pack (kept when room is made), or unpins it.

**Errors.** `ErrorCode::Refused` if it is not installed.

**Blocks.** While a job of this store runs, and on the disk: call it off a UI thread, or use the language's asynchronous form.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn pin(&self, pack: &str, on: bool) -> Result<()>
```
</Tab>

<Tab value="Python">
```python
def pin(self, pack: str, on: bool) -> None: ...
```
</Tab>

<Tab value="JavaScript">
```ts
pin(pack: string, on: boolean): void;
```
</Tab>

<Tab value="Swift">
```swift
public func pin(_ pack: String, on: Bool) throws(TeiGeodataError)
```
</Tab>

<Tab value="Java and Kotlin">
```java
public void pin(String pack, boolean on);
```
</Tab>

<Tab value="Dart">
```dart
void pin(String pack, bool on);
```
</Tab>

<Tab value="Go">
```go
func (h *Store) Pin(pack string, on bool) error
```
</Tab>

<Tab value="C">
```c
int32_t tg_store_pin(const tg_store *handle, const uint8_t *pack, size_t pack_len, bool on);
```
</Tab>

<Tab value="C++">
```cpp
void pin(std::string_view pack, bool on) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe void Pin(string pack, bool on);
```
</Tab>
</Tabs>

### `remove` [#remove]

Removes the packs and the dependencies nothing else needs; returns what was removed.

**Errors.** `ErrorCode::Refused` if one is not installed or another pack needs it.

**Blocks.** While a job of this store runs, and on the disk: call it off a UI thread, or use the language's asynchronous form.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn remove(&self, pack: &str) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def remove(self, pack: str) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
remove(pack: string): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func remove(_ pack: String) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] remove(String pack);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List remove(String pack);
```
</Tab>

<Tab value="Go">
```go
func (h *Store) Remove(pack string) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_store_remove(const tg_store *handle, const uint8_t *pack, size_t pack_len, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob remove(std::string_view pack) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] Remove(string pack);
```
</Tab>
</Tabs>

### `find` [#find]

The places named `query` in `engine` (one over a world pack the app ships, the installed packs, or both), best first, each with the catalog's packs that cover it: what to install for "Dubai". Runs offline on the stored catalog.

**Errors.** `ErrorCode::InvalidArgument` for a limit above `LIMIT_MAX`; `ErrorCode::Refused` if the stored catalog no longer verifies.

**Blocks.** While a job of this store runs, and on the disk: call it off a UI thread, or use the language's asynchronous form.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn find(&self, engine: &Engine, query: &str, limit: u32) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def find(self, engine: Engine, query: str, limit: int) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
find(engine: Engine, query: string, limit: number): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func find(_ engine: Engine, query: String, limit: UInt32) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] find(Engine engine, String query, long limit);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List find(Engine engine, String query, int limit);
```
</Tab>

<Tab value="Go">
```go
func (h *Store) Find(engine *Engine, query string, limit uint32) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_store_find(const tg_store *handle, const tg_engine *engine, const uint8_t *query, size_t query_len, uint32_t limit, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob find(const Engine &engine, std::string_view query, std::uint32_t limit) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] Find(Engine engine, string query, uint limit);
```
</Tab>
</Tabs>

### `locate` [#locate]

The countries within `radius_m` metres of a point in `engine`, each with the catalog's packs that cover the point: what to install for where the user stands.

**Errors.** `ErrorCode::InvalidArgument` for a point off the globe; `ErrorCode::Refused` if the stored catalog no longer verifies.

**Blocks.** While a job of this store runs, and on the disk: call it off a UI thread, or use the language's asynchronous form.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn locate(&self, engine: &Engine, lat: f64, lon: f64, radius_m: u32) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def locate(self, engine: Engine, lat: float, lon: float, radius_m: int) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
locate(engine: Engine, lat: number, lon: number, radiusM: number): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func locate(_ engine: Engine, lat: Double, lon: Double, radiusM: UInt32) throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] locate(Engine engine, double lat, double lon, long radiusM);
```
</Tab>

<Tab value="Dart">
```dart
Uint8List locate(Engine engine, double lat, double lon, int radiusM);
```
</Tab>

<Tab value="Go">
```go
func (h *Store) Locate(engine *Engine, lat float64, lon float64, radiusM uint32) ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_store_locate(const tg_store *handle, const tg_engine *engine, double lat, double lon, uint32_t radius_m, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob locate(const Engine &engine, double lat, double lon, std::uint32_t radius_m) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] Locate(Engine engine, double lat, double lon, uint radiusM);
```
</Tab>
</Tabs>

### `engine` [#engine]

An engine over the installed packs, each opened with the components and languages installed.

**Errors.** `ErrorCode::Io` or `ErrorCode::Corrupt` if a pack cannot be opened.

**Blocks.** While a job of this store runs, and on the disk: call it off a UI thread, or use the language's asynchronous form.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn engine(&self) -> Result<Engine>
```
</Tab>

<Tab value="Python">
```python
def engine(self) -> Engine: ...
```
</Tab>

<Tab value="JavaScript">
```ts
engine(): Engine;
```
</Tab>

<Tab value="Swift">
```swift
public func engine() throws(TeiGeodataError) -> Engine
```
</Tab>

<Tab value="Java and Kotlin">
```java
public Engine engine();
```
</Tab>

<Tab value="Dart">
```dart
Engine engine();
```
</Tab>

<Tab value="Go">
```go
func (h *Store) Engine() (*Engine, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_store_engine(const tg_store *handle, tg_engine **out);
```
</Tab>

<Tab value="C++">
```cpp
Engine engine() const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe Engine Engine();
```
</Tab>
</Tabs>

### `refresh` [#refresh]

Refreshes the catalog for the countries (comma-separated ISO codes) and every country installed. Fails with `ErrorCode::Network` or `ErrorCode::Refused`.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn refresh(&self, countries: &str) -> Job
```
</Tab>

<Tab value="Python">
```python
def refresh(self, countries: str) -> Job: ...
```
</Tab>

<Tab value="JavaScript">
```ts
refresh(countries: string): Job;
```
</Tab>

<Tab value="Swift">
```swift
public func refresh(_ countries: String) -> Job
```
</Tab>

<Tab value="Java and Kotlin">
```java
public Job refresh(String countries);
```
</Tab>

<Tab value="Dart">
```dart
Job refresh(String countries);
```
</Tab>

<Tab value="Go">
```go
func (h *Store) Refresh(countries string) *Job
```
</Tab>

<Tab value="C">
```c
int32_t tg_store_refresh(const tg_store *handle, const uint8_t *countries, size_t countries_len, tg_job **out);
```
</Tab>

<Tab value="C++">
```cpp
Job refresh(std::string_view countries) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe Job Refresh(string countries);
```
</Tab>
</Tabs>

### `install` [#install]

Installs the packs with `options`, dependencies first, refreshing the catalog for their countries. Cancelled, it leaves the store as it was; the packs installed before the one cut stay installed.

**Errors.** `ErrorCode::InvalidArgument` for an unknown component or language tag, more than 8 languages, or an outline level above 255; the job's own errors are the job's.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn install(&self, packs: &str, options: &InstallOptions) -> Result<Job>
```
</Tab>

<Tab value="Python">
```python
def install(self, packs: str, options: InstallOptions) -> Job: ...
```
</Tab>

<Tab value="JavaScript">
```ts
install(packs: string, options?: InstallOptions): Job;
```
</Tab>

<Tab value="Swift">
```swift
public func install(_ packs: String, options: InstallOptions = .init()) throws(TeiGeodataError) -> Job
```
</Tab>

<Tab value="Java and Kotlin">
```java
public Job install(String packs, InstallOptions options);
```
</Tab>

<Tab value="Dart">
```dart
Job install(String packs, InstallOptions options);
```
</Tab>

<Tab value="Go">
```go
func (h *Store) Install(packs string, options *InstallOptions) (*Job, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_store_install(const tg_store *handle, const uint8_t *packs, size_t packs_len, const tg_install_options *options, tg_job **out);
```
</Tab>

<Tab value="C++">
```cpp
Job install(std::string_view packs, const InstallOptions &options = {}) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe Job Install(string packs, InstallOptions? options = null);
```
</Tab>
</Tabs>

### `install_pass` [#install_pass]

One pass of an install whose bytes a platform's own download service fetches, for downloads that must go on while the app is suspended. The pass reads each asset's ranges from files under `dir`, checks every byte as from any host, and its result lists what is still missing, one row per range: `pack`, `urls` (the asset on each mirror, one per line), `length` (the asset's), `start` and `end` (the range, end exclusive) and `file`, where to save those bytes. Fetch every listed range (an HTTP `Range` request for `bytes=start-(end-1)`), then run the pass again; a result with no rows means the packs are installed and their files under `dir` removed. A first install takes three passes.

**Errors.** `ErrorCode::InvalidArgument` as for `Store::install`, or for an empty `dir`; the job's own errors are the job's.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn install_pass(&self, packs: &str, dir: &str, options: &InstallOptions) -> Result<Job>
```
</Tab>

<Tab value="Python">
```python
def install_pass(self, packs: str, dir: str, options: InstallOptions) -> Job: ...
```
</Tab>

<Tab value="JavaScript">
```ts
installPass(packs: string, dir: string, options?: InstallOptions): Job;
```
</Tab>

<Tab value="Swift">
```swift
public func installPass(_ packs: String, dir: String, options: InstallOptions = .init()) throws(TeiGeodataError) -> Job
```
</Tab>

<Tab value="Java and Kotlin">
```java
public Job installPass(String packs, String dir, InstallOptions options);
```
</Tab>

<Tab value="Dart">
```dart
Job installPass(String packs, String dir, InstallOptions options);
```
</Tab>

<Tab value="Go">
```go
func (h *Store) InstallPass(packs string, dir string, options *InstallOptions) (*Job, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_store_install_pass(const tg_store *handle, const uint8_t *packs, size_t packs_len, const uint8_t *dir, size_t dir_len, const tg_install_options *options, tg_job **out);
```
</Tab>

<Tab value="C++">
```cpp
Job install_pass(std::string_view packs, std::string_view dir, const InstallOptions &options = {}) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe Job InstallPass(string packs, string dir, InstallOptions? options = null);
```
</Tab>
</Tabs>

### `update` [#update]

Updates the packs (every installed one when empty) to the catalog's releases, keeping the components and languages installed.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn update(&self, packs: &str) -> Job
```
</Tab>

<Tab value="Python">
```python
def update(self, packs: str) -> Job: ...
```
</Tab>

<Tab value="JavaScript">
```ts
update(packs: string): Job;
```
</Tab>

<Tab value="Swift">
```swift
public func update(_ packs: String) -> Job
```
</Tab>

<Tab value="Java and Kotlin">
```java
public Job update(String packs);
```
</Tab>

<Tab value="Dart">
```dart
Job update(String packs);
```
</Tab>

<Tab value="Go">
```go
func (h *Store) Update(packs string) *Job
```
</Tab>

<Tab value="C">
```c
int32_t tg_store_update(const tg_store *handle, const uint8_t *packs, size_t packs_len, tg_job **out);
```
</Tab>

<Tab value="C++">
```cpp
Job update(std::string_view packs) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe Job Update(string packs);
```
</Tab>
</Tabs>

### `repair` [#repair]

Checks the pack in full and, unless it is whole, fetches what it lacks.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn repair(&self, pack: &str) -> Job
```
</Tab>

<Tab value="Python">
```python
def repair(self, pack: str) -> Job: ...
```
</Tab>

<Tab value="JavaScript">
```ts
repair(pack: string): Job;
```
</Tab>

<Tab value="Swift">
```swift
public func repair(_ pack: String) -> Job
```
</Tab>

<Tab value="Java and Kotlin">
```java
public Job repair(String pack);
```
</Tab>

<Tab value="Dart">
```dart
Job repair(String pack);
```
</Tab>

<Tab value="Go">
```go
func (h *Store) Repair(pack string) *Job
```
</Tab>

<Tab value="C">
```c
int32_t tg_store_repair(const tg_store *handle, const uint8_t *pack, size_t pack_len, tg_job **out);
```
</Tab>

<Tab value="C++">
```cpp
Job repair(std::string_view pack) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe Job Repair(string pack);
```
</Tab>
</Tabs>

## Job [#job]

A store's long work on a thread of its own: its progress in bytes, a wait for the next change, a cancel, and its result.

**Job.** Each language builds its own idioms on these methods: a stream of progress, and the awaited result cancelling the job when the caller's task is cancelled.

### `wait_progress` [#wait_progress]

Waits until the progress moves past `seq` (from `seq`), the job ends, or `timeout_ms` passes; returns the sequence then. Polling with the last one returned sees every change.

**Blocks.** Until the job moves or ends: call it off a UI thread, or use the language's asynchronous form.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn wait_progress(&self, seq: u64, timeout_ms: u32) -> u64
```
</Tab>

<Tab value="Python">
```python
def wait_progress(self, seq: int, timeout_ms: int) -> int: ...
```
</Tab>

<Tab value="JavaScript">
```ts
waitProgress(seq: number, timeoutMs: number): number;
```
</Tab>

<Tab value="Swift">
```swift
public func waitProgress(_ seq: UInt64, timeoutMs: UInt32) -> UInt64
```
</Tab>

<Tab value="Java and Kotlin">
```java
public long waitProgress(long seq, long timeoutMs);
```
</Tab>

<Tab value="Dart">
```dart
int waitProgress(int seq, int timeoutMs);
```
</Tab>

<Tab value="Go">
```go
func (h *Job) WaitProgress(seq uint64, timeoutMs uint32) uint64
```
</Tab>

<Tab value="C">
```c
int32_t tg_job_wait_progress(const tg_job *handle, uint64_t seq, uint32_t timeout_ms, uint64_t *out);
```
</Tab>

<Tab value="C++">
```cpp
std::uint64_t wait_progress(std::uint64_t seq, std::uint32_t timeout_ms) const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe ulong WaitProgress(ulong seq, uint timeoutMs);
```
</Tab>
</Tabs>

### `seq` [#seq]

The progress's sequence number: it moves on with every change.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn seq(&self) -> u64
```
</Tab>

<Tab value="Python">
```python
def seq(self) -> int: ...
```
</Tab>

<Tab value="JavaScript">
```ts
seq(): number;
```
</Tab>

<Tab value="Swift">
```swift
public func seq() -> UInt64
```
</Tab>

<Tab value="Java and Kotlin">
```java
public long seq();
```
</Tab>

<Tab value="Dart">
```dart
int seq();
```
</Tab>

<Tab value="Go">
```go
func (h *Job) Seq() uint64
```
</Tab>

<Tab value="C">
```c
int32_t tg_job_seq(const tg_job *handle, uint64_t *out);
```
</Tab>

<Tab value="C++">
```cpp
std::uint64_t seq() const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe ulong Seq();
```
</Tab>
</Tabs>

### `done_bytes` [#done_bytes]

Bytes received so far.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn done_bytes(&self) -> u64
```
</Tab>

<Tab value="Python">
```python
def done_bytes(self) -> int: ...
```
</Tab>

<Tab value="JavaScript">
```ts
doneBytes(): number;
```
</Tab>

<Tab value="Swift">
```swift
public func doneBytes() -> UInt64
```
</Tab>

<Tab value="Java and Kotlin">
```java
public long doneBytes();
```
</Tab>

<Tab value="Dart">
```dart
int doneBytes();
```
</Tab>

<Tab value="Go">
```go
func (h *Job) DoneBytes() uint64
```
</Tab>

<Tab value="C">
```c
int32_t tg_job_done_bytes(const tg_job *handle, uint64_t *out);
```
</Tab>

<Tab value="C++">
```cpp
std::uint64_t done_bytes() const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe ulong DoneBytes();
```
</Tab>
</Tabs>

### `total_bytes` [#total_bytes]

Bytes expected, from the catalog's sizes: never below the bytes received, and equal to them once the job ends.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn total_bytes(&self) -> u64
```
</Tab>

<Tab value="Python">
```python
def total_bytes(self) -> int: ...
```
</Tab>

<Tab value="JavaScript">
```ts
totalBytes(): number;
```
</Tab>

<Tab value="Swift">
```swift
public func totalBytes() -> UInt64
```
</Tab>

<Tab value="Java and Kotlin">
```java
public long totalBytes();
```
</Tab>

<Tab value="Dart">
```dart
int totalBytes();
```
</Tab>

<Tab value="Go">
```go
func (h *Job) TotalBytes() uint64
```
</Tab>

<Tab value="C">
```c
int32_t tg_job_total_bytes(const tg_job *handle, uint64_t *out);
```
</Tab>

<Tab value="C++">
```cpp
std::uint64_t total_bytes() const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe ulong TotalBytes();
```
</Tab>
</Tabs>

### `is_ended` [#is_ended]

Whether the job has ended (done, failed or cancelled); its result is then ready.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn is_ended(&self) -> bool
```
</Tab>

<Tab value="Python">
```python
def is_ended(self) -> bool: ...
```
</Tab>

<Tab value="JavaScript">
```ts
isEnded(): boolean;
```
</Tab>

<Tab value="Swift">
```swift
public func isEnded() -> Bool
```
</Tab>

<Tab value="Java and Kotlin">
```java
public boolean isEnded();
```
</Tab>

<Tab value="Dart">
```dart
bool isEnded();
```
</Tab>

<Tab value="Go">
```go
func (h *Job) IsEnded() bool
```
</Tab>

<Tab value="C">
```c
int32_t tg_job_is_ended(const tg_job *handle, bool *out);
```
</Tab>

<Tab value="C++">
```cpp
bool is_ended() const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe bool IsEnded();
```
</Tab>
</Tabs>

### `cancel` [#cancel]

Asks the job to stop at its next request or chunk; it then fails with `ErrorCode::Cancelled` and leaves the store as it was.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn cancel(&self)
```
</Tab>

<Tab value="Python">
```python
def cancel(self) -> None: ...
```
</Tab>

<Tab value="JavaScript">
```ts
cancel(): void;
```
</Tab>

<Tab value="Swift">
```swift
public func cancel()
```
</Tab>

<Tab value="Java and Kotlin">
```java
public void cancel();
```
</Tab>

<Tab value="Dart">
```dart
void cancel();
```
</Tab>

<Tab value="Go">
```go
func (h *Job) Cancel()
```
</Tab>

<Tab value="C">
```c
int32_t tg_job_cancel(const tg_job *handle);
```
</Tab>

<Tab value="C++">
```cpp
void cancel() const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe void Cancel();
```
</Tab>
</Tabs>

### `result` [#result]

The job's result, waiting for it to end.

**Errors.** The job's: `ErrorCode::Network`, `ErrorCode::Refused`, `ErrorCode::Cancelled`, or `ErrorCode::Io` and `ErrorCode::Corrupt` for an engine's packs.

**Blocks.** Until the job moves or ends: call it off a UI thread, or use the language's asynchronous form.

<Tabs groupId="lang" persist items={["Rust","Python","JavaScript","Swift","Java and Kotlin","Dart","Go","C","C++",".NET"]}>
<Tab value="Rust">
```rust
pub fn result(&self) -> Result<Blob>
```
</Tab>

<Tab value="Python">
```python
def result(self) -> Blob: ...
```
</Tab>

<Tab value="JavaScript">
```ts
result(): Buffer;
```
</Tab>

<Tab value="Swift">
```swift
public func result() throws(TeiGeodataError) -> Data
```
</Tab>

<Tab value="Java and Kotlin">
```java
public byte[] result();
```
</Tab>

<Tab value="Dart">
```dart
Uint8List result();
```
</Tab>

<Tab value="Go">
```go
func (h *Job) Result() ([]byte, error)
```
</Tab>

<Tab value="C">
```c
int32_t tg_job_result(const tg_job *handle, tg_bytes *out);
```
</Tab>

<Tab value="C++">
```cpp
Blob result() const;
```
</Tab>

<Tab value=".NET">
```csharp
public unsafe byte[] Result();
```
</Tab>
</Tabs>