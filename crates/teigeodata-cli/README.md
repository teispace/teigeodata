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
```

Each query reads `--pack FILE` (repeat for several) or, without it, the store. Output is text
by default, or:

| Flag | Output |
|---|---|
| `--json` | the native records: `id`, `name`, `kind`, `admin_level`, `lat`, `lon`, `elevation`, `parent`, `timezone` |
| `--geojson` | a GeoJSON FeatureCollection (RFC 7946), the attribution as a foreign member |
| `--nominatim` | Nominatim's `json` format, for tools that read it; OSM-only fields are left out |

## Exit codes

| Code | Meaning |
|---|---|
| 0 | done (also when the output is closed early, as by `head`) |
| 1 | refused: not verified, not found, nothing matched, a file not readable or writable |
| 2 | a usage error |
| 3 | the network: try again later |

## Licence

The code is under the workspace's licence. Packs carry their own data licence and attribution;
`tgd inspect` shows them, and `--geojson` and `--nominatim` output repeat the attribution.
