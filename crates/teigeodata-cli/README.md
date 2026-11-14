# tgd

`tgd` is the teigeodata command line. It inspects, verifies, compares, cuts and exports pack
files; it answers queries over them; and it keeps a store of packs in sync with a signed
catalog, offline-first.

## Install

From a clone of the repository:

```sh
cargo install --path crates/teigeodata-cli
```

Shell completions, for bash, zsh, fish, PowerShell or Elvish:

```sh
tgd completions zsh > ~/.zfunc/_tgd        # zsh, with ~/.zfunc on $fpath
tgd completions bash > ~/.local/share/bash-completion/completions/tgd
tgd completions fish > ~/.config/fish/completions/tgd.fish
```

## A store, from start to finish

A store is one directory: its trusted root, its mirrors, the verified catalog and the installed
packs. `--store DIR` chooses it; so does the `TEIGEODATA_STORE` environment variable. The default
is the platform's data directory.

```sh
tgd init --repo https://example.org/meta --assets https://example.org/assets/ --root root.json
tgd refresh --countries np       # fetch the signed catalog: Nepal's packs, and what is installed
tgd catalog                      # the packs on offer, from the store alone
tgd install np                   # the whole pack, its dependencies first
tgd install np --components search   # or only some components, with what they need
tgd install np --names en --outlines 2@20m   # English names, outlines to admin level 2 within 20 m
tgd search Pokhara               # queries without --pack answer over the installed packs
tgd update                       # new releases, fetching only the frames that changed
tgd status --deep                # every installed section hashed
tgd repair np                    # fetch only what is damaged
tgd pin np                       # never evicted
tgd remove np                    # and the dependencies nothing else needs
```

`init` refuses a root that does not verify by itself. Every file `install` and `update` write
is checked against the signed catalog before it is used. With the network down, `refresh`
exits 3. Everything else still answers from the store.

`install` adds components to what is installed. When a component is not installed, a query
prints its field as null; a query that needs it, such as `tz` without the timezone component,
is refused.

A pack may split names by language and outlines by admin level and level of detail (parts).
`--names en,ne` installs the base names and those languages; a language without a part of its
own comes with the rest (`*`). `--outlines LEVEL[@Mm]` installs the smallest outline part that
holds the admin units of LEVEL and coarser, simplified by at most M metres (full resolution
without `@`). Each replaces what is installed of its kind and keeps the rest; `tgd catalog
--json` lists each part's size, and `tgd status` shows what is installed.

An app whose platform downloads in the background (iOS `URLSession`, Android UIDT jobs, a
browser worker) installs in passes: `tgd install np --downloads DIR` installs from the files
under `DIR`, or exits 4 and prints the byte ranges still needed, with each file's name. The
platform downloads them, and install runs again. See `tools/platform-probe`.

## Packs

```sh
tgd inspect np.tgd                    # manifest, sections, bytes per component
tgd verify np.tgd --deep --key HEX    # every hash, every value, the signature
tgd diff old.tgd new.tgd              # sections, then places added, removed, moved, renamed
tgd extract np.tgd --components point,names,search --out small.tgd
tgd extract np.tgd --bbox 85.2,27.6,85.5,27.8 --out kathmandu.tgd
tgd export np.tgd --format geojson --out np.geojson
```

## Queries

```sh
tgd get tgd:np/da65m6
tgd reverse 27.7172 85.3240 -k 3
tgd search "Lahachowk, Kaski"
tgd near tgd:np/da65m6 --radius 5000
tgd admin 27.7172 85.3240
tgd tz 27.7172 85.3240
tgd elevation 27.7172 85.3240   # terrain height (m above EGM2008); --json
```

Each query reads `--pack FILE` (repeat for several) or, without it, the store. Output is text
by default, or:

| Flag | Output |
|---|---|
| `--json` | the native records: `id`, `name`, `kind`, `admin_level`, `lat`, `lon`, `elevation`, `parent`, `timezone` |
| `--geojson` | a GeoJSON FeatureCollection (RFC 7946), the attribution of the packs that answered as a foreign member |
| `--nominatim` | Nominatim's `json` format, for tools that read it; OSM-only fields are left out |

## Serving

```sh
tgd serve                          # the store's packs on http://127.0.0.1:8080
tgd serve --pack np.tgd --listen 127.0.0.1:0
curl 'http://127.0.0.1:8080/v1/search?q=Pokhara&limit=3'
```

Every query is a `GET` route under `/v1/` (`get`, `lookup`, `reverse`, `near`, `search`,
`hierarchy`, `children`, `admin`, `tz`, `elevation`, `postal`, `boundary`, `distance`,
`coverage`), with
`/packs` and `/health`. A place query answers
`{"results": [...], "attribution": ...}`, or GeoJSON with `format=geojson`; a failure answers
`{"error": {"code": ..., "message": ...}}` with its HTTP status. `/v1/boundary?id=` answers
the place's outline as GeoJSON, at the finest level of detail within 20,000 vertices unless
`lod` (0 to 6) asks for one; `/v1/distance?from=&to=` the metres and bearing between two
places or points (`from=tgd:np/2cn&to=28.21,83.99`); `/v1/coverage?lat=&lon=` what
`tgd locate` says, when the server runs over the store. `/search`, `/reverse` and
`/lookup` answer as Nominatim does (`format=json`, `jsonv2` or `geojson`), for clients written
for it; OSM-only fields are left out. The server listens on
127.0.0.1 unless `--listen` says otherwise, sends no CORS headers unless `--cors-origin` names
an origin, holds every query to its bounds (256 bytes of text, 50 results, a radius up to
1,000 km), follows the store as packs are installed or removed (`--follow`, every 5 s), and
stops on Ctrl-C. A build without the `serve` feature leaves it out.

`tgd mcp` serves the same queries to an AI agent over MCP (revisions 2026-07-28 and
2025-11-25), on standard input and output: an MCP client starts it, for example with
`{"command": "tgd", "args": ["mcp"]}` in its server list. It has fourteen read-only tools
(`search`, `reverse`, `nearby`, `get_place`, `lookup`, `admin_at`, `hierarchy`, `children`,
`timezone`, `elevation`, `postal`, `boundary`, `distance`, `packs`, and over the store
`coverage`), each answering structured
content with the data's attribution; long answers come in pages through a signed
`next_cursor`. Packs, places and outlines are resources: `teigeodata://dataset/np/places`,
`teigeodata://place/tgd:np/1xhb`, `teigeodata://boundary/tgd:np/1wmc?lod=2` (the `boundary`
tool answers what an outline is and links it, rather than filling the context with it). A build without
the `mcp` feature leaves it out. `tgd serve` answers the same MCP server at `/mcp` over HTTP,
for clients that connect by URL (`http://127.0.0.1:8080/mcp`).

## Exit codes

| Code | Meaning |
|---|---|
| 0 | done (also when the output is closed early, as by `head`) |
| 1 | refused: not verified, not found, nothing matched, a file not readable or writable |
| 2 | a usage error |
| 3 | the network: try again later |
| 4 | `install --downloads DIR`: the byte ranges to download first, printed as JSON |

## Licence

The code is under the workspace's licence. Packs carry their own data licence and attribution;
`tgd inspect` shows them, and `--geojson` and `--nominatim` output repeat the attribution.
