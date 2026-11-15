"""Writes site/content/docs/guides/<lang>.mdx: one guide per language, every query section
drawn from that language's tour (site/examples) and the output every tour prints.

The guides share their prose, so it lives here once: edit this file, not the pages, then run
`python3 site/scripts/guides.py`. `--check` fails when a page differs from what this writes."""

import json
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = f"{ROOT}/content/docs/guides"
EX = "../../../examples"  # from content/docs/guides/x.mdx to site/examples


def snake(name):
    return name


def camel(name):
    head, *rest = name.split("_")
    return head + "".join(w.capitalize() for w in rest)


def pascal(name):
    return "".join(w.capitalize() for w in name.split("_"))


# Each section: id (the tour's region and output file), title, prose before, prose after.
# {m:name} is a method of Engine in the language's spelling; {f:name} a free function;
# {c:name} a constant or type the language spells its own way (SPELL, or the entry's `spell`).
SECTIONS = [
    (
        "open",
        "Open a pack",
        """An `Engine` holds the packs it answers from. {m:open_file} memory-maps a pack file and
adds it as a **layer**; the engine answers from every layer at once, and a higher layer wins
where two disagree. Opening is cheap: the head of the file is read and checked and the block hashes of large
sections are proved; each section is verified against its hash the first time a query reads
it.""",
        """- {m:open_bytes} adds a pack from bytes already in memory (a download, an app asset, the
  browser). {m:open_file_range} opens a pack stored uncompressed inside another file, such as an
  Android APK.
- Each {c:with} form takes {c:open_options}: precedence over the other packs, when sections are
  verified (lazy, eager or trusted), whether the file is mapped or read, and which components,
  languages and outline level to read.
- {m:open_file} returns the layer's number; {m:remove} takes it out again while other threads
  keep querying. One engine answers from any number of threads at once.""",
    ),
    (
        "search",
        "Search by name",
        """{m:search} finds places by name, best first. It matches every name a place has, in every
script, and forgives spelling: काठमाडौं, Kathmandu and Kathamandu find the same city, and so do
typos, missing accents and abbreviations. A result row has the place columns (`id`, `name`,
`kind`, `level`, `lat`, `lon`, `elevation`, `parent`, `tz`, `iso`) and a `score`.""",
        """The `score` ranks the rows of one query and means nothing across queries. It adds how well the
name matched to how important the place is, so a city ranks above a village of the same name.
`limit` is at most 10,000 ({c:limit_max}).""",
    ),
    (
        "search-parent",
        "Narrow a search by its parent",
        """Write the place's parent after a comma, as people do: `"Kathmandu, Bagmati"`. Places
inside a unit of that name rank first; the parent can be any ancestor (a district, a province,
a country) and is matched with the same tolerance as the name.""",
        """The city (a settlement, level 0) and the metropolitan municipality (an admin unit at
level 3) share the name, and both are in Bagmati.""",
    ),
    (
        "search-focus",
        "Rank by distance from a point",
        """Many places share a name. {m:search_with} takes {c:search_options}: a **focus** point (the
user's location, or the middle of the map they are looking at) ranks near places first. A place
at the focus gains 30 points, one {c:focus_scale_m} away 15, and the gain keeps halving with
distance (the default scale is 50 km).""",
        """Set `strict` to rank by how well the name matched first (exact, loose, prefix, one edit,
substring, two edits, phonetic), then by importance and focus, instead of one combined score:
useful for an autocomplete that must show exact matches on top.""",
    ),
    (
        "get",
        "Get a place by its ID",
        """Every place has a stable ID such as `tgd:np/1xhb`: the country, then a short code. IDs
survive rebuilds of the data; when two places are merged, {m:get} on the retired ID answers
with the survivor, so IDs you stored keep working.""",
        """`elevation` is metres above mean sea level (EGM2008), `tz` the IANA zone and `parent` the
ID of the unit that holds the place. Any column may be null where the data has no value: read
each through the reader, which gives your language's null.""",
    ),
    (
        "hierarchy",
        "Walk the admin hierarchy",
        """{m:ancestors} lists the units that hold a place, parent first, up to its country; {m:parent}
gives the first of them alone and {m:children} the units and places one level down.""",
        """`level` is the admin level as each country defines it: 0 is a country (and a settlement,
which is not a unit), 1 a province or state, and so on down to the lowest level the data has.
Each country's ladder is in its profile on the [data pages](/docs/data).""",
    ),
    (
        "reverse",
        "Reverse geocode a point",
        """{m:reverse} gives the places nearest a point, nearest first, each with `meters` (the
great-circle distance) and `bearing` (degrees clockwise from north, from the place to the
point: "3.2 km at 45° from Pokhara").""",
        """For the admin units that *hold* a point (its district, province and country) use
{m:admin_at} below; reverse answers with what is *near* it.""",
    ),
    (
        "radius",
        "Find everything within a radius",
        """{m:within_radius} gives the places within a distance of a point, nearest first, with the
same `meters` and `bearing` columns, at most `limit` of them.""",
        "",
    ),
    (
        "nearby",
        "Find places near a place",
        """{m:nearby} is reverse geocoding from a place you already have: the places nearest it,
itself left out.""",
        "",
    ),
    (
        "admin",
        "Admin units at a point, and ISO 3166 codes",
        """{m:admin_at} tests the point against the admin boundaries and gives every unit that holds
it, country first. {m:by_iso} finds a country or a subdivision by its ISO 3166 code (`NP`,
`NP-P4`, in either case).""",
        """Boundaries are exact polygons, so a point near a border is answered by the side it is on,
not by the nearest centre. Disputed boundaries are drawn from one stated perspective; see
[accuracy](/docs/concepts/accuracy).""",
    ),
    (
        "timezone",
        "Timezones: the zone, its offset and local times",
        """{m:tz_at} gives the IANA zone at a point as the zone boundaries draw it. With the zone,
{m:tz_offset_at} gives the UTC offset at an instant (from the tzdb rules inside the pack, not
from the operating system), and {m:tz_local} tells whether a wall-clock time exists: `unique`,
`gap` (skipped by a daylight-saving change) or `overlap` (it happens twice), with the offsets
before and after.""",
        """{m:tz_version} names the tzdb release the rules come from, so you can tell when a pack is
older than a rule change; {m:tz_zones} lists every zone the packs hold.""",
    ),
    (
        "elevation",
        "Elevation at any point",
        """{m:elevation_at} reads the terrain height at a point from the pack's elevation grid,
interpolated between the four samples around it: `meters` above EGM2008, the `source` product,
the grid's `resolution_m` and the `datum`. There is no row at sea or outside every grid.""",
        """Every place also has its own `elevation` column. The world pack carries a coarse grid; a
country's terrain pack adds a finer one, and the engine answers from the finest grid that covers
the point.""",
    ),
    (
        "distance",
        "Distance and bearing",
        """{f:distance_m} is the great-circle distance in metres (haversine on the WGS84 mean
sphere, within about 0.5% of the ellipsoid), and {f:bearing_deg} the initial bearing from the
first point to the second, in degrees clockwise from north. Neither needs a pack. Between two
places you already have, {m:distance} takes their IDs and answers one row with `meters` and
`bearing`.""",
        """{m:distance} answers no row when either ID is unknown. From a point to a place, pass the
place's `lat` and `lon` to {f:distance_m}. The bearing is the direction you set off in: on a
great circle it changes along the way, so the bearing back is not the reverse of this one.""",
    ),
    (
        "boundary",
        "A place's outline",
        """{m:boundary} answers a unit's outline as one row: `geojson`, an RFC 7946 Feature (a
Polygon or MultiPolygon, outer rings counterclockwise and holes clockwise, seven decimals as
stored), with the `lod` read, its `tolerance_m` and the number of `vertices`. A place with no
outline answers its point as the geometry; an unknown ID answers no row.""",
        """The Feature goes to a map as it is: MapLibre's and Leaflet's GeoJSON sources, OpenLayers,
Shapely's `shape()`, Turf and JTS all read it.

- **Level of detail.** `lod` 0 is full resolution; 1 to 6 are coarser, each within its
  `tolerance_m`, with neighbouring units still sharing their borders at every level. A pack
  stored at full resolution only (the lite packs) answers every level as 0. A country at level 0
  can be megabytes: draw a coarser level, and keep 0 for measuring.
- `lod` above 6 is `InvalidArgument`; a pack installed without `boundaries` is
  `MissingComponent`.
- Whether a point is *inside* a unit is {m:admin_at}, which tests the same outlines without
  building them.""",
    ),
    (
        "errors",
        "Errors",
        """A call fails with one of eight stable codes, the same in every language:""",
        """| Code | Meaning |
|---|---|
| `InvalidArgument` | An argument is outside its range: a latitude of 91, a limit over 10,000, a malformed ID. |
| `MissingComponent` | The query needs a component the pack was installed without (search on a pack installed without `search`). |
| `Corrupt` | The bytes are not a valid pack, or a section does not match its hash. |
| `Io` | A file could not be read. |
| `Network` | No mirror answered, or the connection broke (the store only). |
| `Refused` | The store refused: the catalog did not verify, a pack is not installed or another needs it. |
| `Cancelled` | A callback or a job was asked to stop. |
| `Internal` | A bug inside teigeodata; a panic never crosses into your program. |

A query that finds nothing is not an error: it returns no rows.""",
    ),
]

NAMING = {
    "snake": snake,
    "camel": camel,
    "pascal": pascal,
}


# Defaults by naming; an entry's `spell` overrides any of them.
SPELL = {
    "snake": {"with": "`_with`", "open_options": "`OpenOptions`", "search_options": "`SearchOptions`",
              "limit_max": "`LIMIT_MAX`", "focus_scale_m": "`focus_scale_m`"},
    "camel": {"with": "`…With`", "open_options": "`OpenOptions`", "search_options": "`SearchOptions`",
              "limit_max": "`LIMIT_MAX`", "focus_scale_m": "`focusScaleM`"},
    "pascal": {"with": "`…With`", "open_options": "`OpenOptions`", "search_options": "`SearchOptions`",
               "limit_max": "`LimitMax`", "focus_scale_m": "`FocusScaleM`"},
    "c": {"with": "`_with`", "open_options": "`tg_open_options`", "search_options": "`tg_search_options`",
          "limit_max": "`TG_LIMIT_MAX`", "focus_scale_m": "`focus_scale_m`"},
}


def fill(text, lang):
    def method(m):
        kind, name = m.group(1), m.group(2)
        if kind == "c":
            return lang.get("spell", {}).get(name) or SPELL[lang["naming"]][name]
        if lang["naming"] == "c":
            return f"`tg_engine_{name}`" if kind == "m" else f"`tg_{name}`"
        spelled = NAMING[lang["naming"]](name)
        if kind == "f" and lang.get("free_prefix"):
            spelled = lang["free_prefix"] + spelled
        return f"`{spelled}`"

    return re.sub(r"\{([mfc]):([a-z_]+)\}", method, text)


def include(path, region, code_lang, title=None):
    meta = f" meta='title=\"{title}\"'" if title else ""
    target = f"{path}#{region}" if region else path
    return f'<include lang="{code_lang}"{meta}>{target}</include>'


def code_block(lang, region):
    files = lang["files"]
    if len(files) == 1:
        label, path, code_lang = files[0]
        return include(f"{EX}/{path}", region, code_lang, os.path.basename(path))
    parts = [f"<Tabs groupId=\"lang\" persist items={{{[f[0] for f in files]!r}}}>".replace("'", '"')]
    for label, path, code_lang in files:
        parts.append(f'<Tab value="{label}">\n')
        parts.append(include(f"{EX}/{path}", region, code_lang, os.path.basename(path)))
        parts.append("\n</Tab>")
    parts.append("</Tabs>")
    return "\n".join(parts)


def page(lang):
    out = []
    out.append("---")
    out.append(f"title: {lang['title']}")
    out.append("description: " + json.dumps(lang["description"], ensure_ascii=False))
    out.append("---")
    out.append("")
    out.append(lang["intro"].strip())
    out.append("")
    out.append(
        """<Callout title="Every example on this page ran">
The code below is cut from one program, [the tour]({tour}), which CI runs on the released
public pack `world-np-lite.tgd` (`data-2026.11.14.3`) and checks line by line against the
output shown here. The output is the same in every language.
</Callout>""".replace("{tour}", lang["tour_url"])
    )
    out.append("")
    out.append("## Install")
    out.append("")
    out.append(lang["install"].strip())
    out.append("")
    out.append("## Get a pack")
    out.append("")
    out.append(
        """Queries read **packs**: signed, memory-mapped files of places. This guide uses the world
lite pack: the world's countries, first-level units and cities of 5,000 or more, India's
districts and statutory towns, and Nepal in full, 172,339 places named in Nepali, Hindi and
English (15.6 MB, CC BY 4.0):

```sh
curl -LO https://github.com/teispace/teigeodata/releases/download/data-2026.11.14.3/world-np-lite.tgd
shasum -a 256 world-np-lite.tgd   # 243ae55509a538b1f61da50994a791cc78e7deb70bb4bf743b9f59d458cbe3f3
```

An app ships a pack inside its bundle, or downloads one when a user needs it. Once the signed
catalog is live, the [store](/docs/concepts/store) does that download for you: it verifies every
byte and keeps the packs updated."""
    )
    out.append("")
    out.append("## Read a result")
    out.append("")
    out.append(lang["read"].strip())
    out.append("")
    for region, title, before, after in SECTIONS:
        if region == "errors":
            continue
        before, after = lang.get("prose", {}).get(region, (before, after))
        out.append(f"## {title}")
        out.append("")
        out.append(fill(before, lang).strip())
        out.append("")
        out.append(code_block(lang, region))
        out.append("")
        out.append(include(f"{EX}/out/{region}.txt", None, "text", "Output"))
        out.append("")
        if after:
            out.append(fill(after, lang).strip())
            out.append("")
    region, title, before, after = SECTIONS[-1]
    before, after = lang.get("prose", {}).get(region, (before, after))
    out.append(f"## {title}")
    out.append("")
    out.append(fill(before, lang).strip())
    out.append("")
    out.append(fill(after, lang).strip())
    out.append("")
    out.append(lang["errors"].strip())
    out.append("")
    out.append(code_block(lang, "errors"))
    out.append("")
    out.append(include(f"{EX}/out/errors.txt", None, "text", "Output"))
    out.append("")
    if lang.get("extras"):
        out.append(lang["extras"].strip())
        out.append("")
    out.append("## The whole program")
    out.append("")
    out.append(
        """Run it from a checkout of the repository, which builds what the language needs, fetches
the pinned pack and compares the output:

```sh
cargo xtask docs-examples --fetch --check --lang {key}
```

<Accordions>
<Accordion title="{file}">
{code}
</Accordion>
</Accordions>""".replace("{key}", lang["key"])
        .replace("{file}", ", ".join(os.path.basename(f[1]) for f in lang["files"]))
        .replace("{code}", "\n\n".join(include(f"{EX}/{f[1]}", None, f[2], os.path.basename(f[1])) for f in lang["files"]))
    )
    out.append("")
    out.append("## Next")
    out.append("")
    out.append(
        """- [Concepts](/docs/concepts): places and IDs, packs and components, results, the store.
- [Reference](/docs/reference): every query, its columns and its errors.
- [Data](/docs/data): what each country's packs hold, with their sources and measured accuracy."""
    )
    out.append("")
    return "\n".join(out)


TOUR = "https://github.com/teispace/teigeodata/blob/main/site/examples/"

FROM_CHECKOUT = """Until the registries are live, build from a checkout (a Rust toolchain is all it
needs; `rust-toolchain.toml` pins the version):

```sh
git clone https://github.com/teispace/teigeodata && cd teigeodata
git checkout v0.3.1
"""

LANGS = []


def lang(**k):
    LANGS.append(k)


lang(
    key="rust",
    slug="rust",
    title="Rust",
    description="The typed engine from Rust: search, reverse, admin, timezones and elevation on an offline pack, with no allocation in a warm query.",
    naming="snake",
    files=[("Rust", "rust/tour.rs", "rust")],
    tour_url=TOUR + "rust/tour.rs",
    # The typed engine's own calls where they differ from the bindings'.
    prose={
        "open": (
            """An `Engine` holds the packs it answers from, each as a **layer**. `Layer::open` maps a pack
file and checks its head; `Engine::add` adds the layer and returns its `LayerId`. The engine
answers from every layer at once, and a higher layer wins where two disagree. Opening is cheap:
the head is read and checked, and the block hashes of large sections are proved; each section
is verified against its hash the first time a query reads it.""",
            """- `Layer::from_bytes` opens a pack from bytes already in memory (a download, an app asset).
  `Layer::open_range` opens a pack stored uncompressed inside another file, such as an Android
  APK.
- `LayerOptions` sets the layer's `priority`, when sections are verified (`Verify::Lazy`,
  `Eager` or `Trusted`), whether the file is mapped or read (`Load::Map`, `Load::Read`), and
  which components, languages and outline level to read (`installed`,
  `installed_parts`, `outline_level`).
- `Engine::remove` takes a layer out again by its `LayerId`. For an engine that other threads
  keep querying while layers come and go, see `Shared` below.""",
        ),
        "search": (
            """`Engine::search` finds places by name, best first, into a `Vec<Hit>` you reuse; each `Hit`
is a `Place` and its `score`. It matches every name a place has, in every script, and forgives
spelling: काठमाडौं, Kathmandu and Kathamandu find the same city, and so do typos, missing accents
and abbreviations. `Engine::search_with` takes `SearchOptions` too; the tour uses it throughout.""",
            """The `score` ranks the hits of one query and means nothing across queries. It adds how well the
name matched to how important the place is, so a city ranks above a village of the same name.
The engine takes any `k`; the bindings cap `limit` at 10,000 (`LIMIT_MAX` in
`teigeodata-api`).""",
        ),
        "search-focus": (
            """Many places share a name. `SearchOptions::with_focus` takes a `Focus { at, scale_m }`: a
point (the user's location, or the middle of the map they are looking at) that ranks near
places first. A place at the focus gains 30 points, one `scale_m` away 15, and the gain keeps
halving with distance.""",
            """Set `SearchOptions::strict` to rank by how well the name matched first (exact, loose,
prefix, one edit, substring, two edits, phonetic), then by importance and focus, instead of one
combined score: useful for an autocomplete that must show exact matches on top.""",
        ),
        "get": (
            """Every place has a stable ID such as `tgd:np/1xhb`: the country, then a short code; it parses
into a `PlaceId`. IDs survive rebuilds of the data; when two places are merged, `Engine::get`
on the retired ID answers with the survivor, so IDs you stored keep working.""",
            """`Engine::elevation` is metres above mean sea level (EGM2008), `Engine::tz_of` the place's
timezone area and `Engine::parent` the unit that holds it. Each returns an `Option`: `None`
where the data has no value.""",
        ),
        "reverse": (
            """`Engine::reverse` gives the places nearest a point, nearest first, as `Near` values: the
`place`, `meters` (the great-circle distance) and `bearing_deg` (degrees clockwise from north,
from the place to the point: "3.2 km at 45° from Pokhara").""",
            """For the admin units that *hold* a point (its district, province and country) use
`Engine::admin_at` below; reverse answers with what is *near* it.""",
        ),
        "timezone": (
            """`Engine::tz_at` gives the timezone area at a point as the zone boundaries draw it, a `Place`
whose name is the IANA zone (`Engine::zone_name` reads it from the zone rules, when the pack
has them). With the zone, `Engine::tz_offset_at` gives the UTC offset at an instant (from the
tzdb rules inside the pack, not from the operating system), and
`Engine::tz_local_candidates` tells whether a wall-clock time exists: `Local::Unique`,
`Local::Gap` (skipped by a daylight-saving change) or `Local::Overlap` (it happens twice),
with the offsets before and after.""",
            """`Engine::tz_version` names the tzdb release the rules come from, so you can tell when a pack
is older than a rule change; `Engine::tz_zones` lists every zone the packs hold.""",
        ),
        "errors": (
            """A query fails with the engine's `Error`; the bindings map it to their eight stable codes:""",
            """| `Error` | Meaning | Bindings' code |
|---|---|---|
| `Io` | A file could not be read. | `Io` |
| `Format` | The bytes are not a valid pack, or a section's structure is broken. | `Corrupt` |
| `Integrity { address, kind }` | A section's bytes do not match its hash. | `Corrupt` |
| `Range` | A section is missing or a byte range is outside it. | `Corrupt` |
| `MissingComponent { address, component }` | The query needs a component the layer was installed without. | `MissingComponent` |

Arguments are checked by their types before a query runs: a `Point` is in range when it is made,
and a `PlaceId` or an `IsoCode` when it is parsed. The store (`teigeodata-sync`) has its own
error type for the network and the catalog. A query that finds nothing is not an error: it
returns `None` or no hits.""",
        ),
        "distance": (
            """`meters_between` is the great-circle distance in metres (haversine on the WGS84 mean
sphere, within about 0.5% of the ellipsoid), and `bearing_deg` the initial bearing from the
first point to the second, in degrees clockwise from north. Both take degrees and need no
pack; between two places, pass their points.""",
            """The bindings' `distance` takes two place IDs and answers both in one row. The bearing
is the direction you set off in: on a great circle it changes along the way.""",
        ),
        "boundary": (
            """`Engine::boundary` decodes a unit's outline at a level of detail into an `Outline` you
reuse: its rings, its points and the level's `tolerance_cm`. It returns the layer that held it,
or `None` (and an empty outline) for a place without one. `Outline::write_geojson` writes the
geometry as RFC 7946 GeoJSON (outer rings counterclockwise, holes clockwise, seven decimals as
stored): the same bytes the servers and every binding's `boundary` answer.""",
            """- **Level of detail.** Level 0 is full resolution; 1 to 6 are coarser, each within its
  tolerance, with neighbouring units still sharing their borders at every level. A pack stored
  at full resolution only (the lite packs) reads every level as 0. Draw a coarser level; keep 0
  for measuring.
- `Engine::boundary_vertices` counts an outline's vertices without decoding it, and
  `admin_at` tests a point against the same outlines without building them.""",
        ),
    },
    intro="""teigeodata is written in Rust, and Rust gets the engine itself: typed places, typed IDs and
points, and queries that write into buffers you reuse, so a warm query allocates nothing. Every
other language reaches the same engine through a generated binding.""",
    install="""<Tabs items={["Git tag (today)", "crates.io"]}>
<Tab value="Git tag (today)">

```toml title="Cargo.toml"
[dependencies]
teigeodata-core = { git = "https://github.com/teispace/teigeodata", tag = "v0.3.1" }
teigeodata-engine = { git = "https://github.com/teispace/teigeodata", tag = "v0.3.1" }
```

</Tab>
<Tab value="crates.io">

The crates (`teigeodata-core`, `-engine`, `-sync`, `-api`, `-server`, `-cli` and what they
need) are published to crates.io from the first registry release:

```sh
cargo add teigeodata-core teigeodata-engine
```

</Tab>
</Tabs>

The minimum Rust version is the one `rust-toolchain.toml` pins. `teigeodata-engine`'s default
features read packs from files (`mmap`); `teigeodata-sync` adds the store (download, verify,
update); the core crates build for `no_std` targets.""",
    read="""The Rust engine returns typed values, not result blobs: a `Place` holds its ID, kind, admin
level and point, and the rest (its name, ISO code, elevation, timezone) is read on demand from
the engine, into a buffer you pass. The tour gathers what it prints in one helper:

<include lang="rust" meta='title="tour.rs"'>../../../examples/rust/tour.rs#describe</include>

Queries that return several places write into a `Vec` you own and reuse (`hits`, `near`,
`places` in the tour), with a `Scratch` for the search's working memory. Points are fixed-point
(`Point<E6>`, `Point<E7>`): `Point::from_degrees` checks the range once, so a query never sees
an invalid coordinate.

If you want the same blobs every binding gets, `teigeodata-api` wraps this engine with the
binding API (`Engine::search` returning a result blob).""",
    errors="""Rust returns `Result`s: the tour's `main` returns `Result<(), Box<dyn Error>>` and uses `?`
throughout. A point is checked when it is made, so the latitude error comes from
`Point::from_degrees`:""",
    extras="""## Threads and updates

`Engine` is `Send + Sync`: share it in an `Arc` and query from any number of threads. To add or
remove layers while other threads query, hold a `Shared`: `Shared::load` gives the snapshot a
query runs on, and `Shared::update` changes a copy and publishes it atomically, so queries
already running finish on the snapshot they started with:

```rust
use teigeodata_engine::{Engine, Layer, LayerOptions, Shared};

let shared = Shared::new(Engine::new());
let id = shared.update(|e| e.add(Layer::open("world-np-lite.tgd", LayerOptions::default()).unwrap()));
let snapshot = shared.load(); // derefs to &Engine
```""",
)

lang(
    key="python",
    slug="python",
    title="Python",
    description="teigeodata from Python 3.11 and later: offline search, reverse geocoding, admin units, timezones and elevation, with rows as dicts.",
    naming="snake",
    files=[("Python", "python/tour.py", "python")],
    tour_url=TOUR + "python/tour.py",
    intro="""The Python package is a native extension (PyO3) over the Rust engine, with type stubs. One
`abi3` wheel per platform serves CPython 3.11 and later; free-threaded builds (3.14t, 3.15t) get
their own wheels and run queries in parallel without the GIL.""",
    install="""<Tabs items={["From a checkout (today)", "PyPI"]}>
<Tab value="From a checkout (today)">

"""
    + FROM_CHECKOUT
    + """PYO3_BUILD_EXTENSION_MODULE=1 cargo build --release -p teigeodata-python --manifest-path bindings/Cargo.toml
cp bindings/target/release/libteigeodata_python.so bindings/python/teigeodata/_native.abi3.so   # .dylib on macOS
export PYTHONPATH=$PWD/bindings/python
```

Or build a wheel with [maturin](https://www.maturin.rs): `pip install ./bindings/python`.

</Tab>
<Tab value="PyPI">

From the first registry release:

```sh
pip install teigeodata
```

</Tab>
</Tabs>""",
    read="""Every query returns a **result blob**: the rows as compact columns in one `Blob`, a buffer the
library lends without a copy (`memoryview(blob)`; `bytes(blob)` copies it), read in place. `teigeodata.blob.Reader` reads it; iterating gives each row as a `dict` from column
name to value, with `None` where a value is null:

<include lang="python" meta='title="tour.py"'>../../../examples/python/tour.py#rows</include>

For large results read whole columns instead: `reader.numbers("lat")` is a `memoryview` of the
blob (no copy),
`reader.strings("name")` a list, `reader.nulls("elevation")` which rows are null, and
`reader.column("level")` one value per row. `reader.rows` and `reader.columns` describe the
shape. The import:

<include lang="python" meta='title="tour.py"'>../../../examples/python/tour.py#import</include>""",
    errors="""Each code is an exception class under `teigeodata.TeiGeodataError`:
`InvalidArgumentError`, `MissingComponentError`, `CorruptError` and so on.""",
    extras="""## Threads and asyncio

An `Engine` is safe to share between threads, and queries release the GIL: a thread pool runs
them in parallel, and on a free-threaded build so does every thread. The store's long calls
(install, update) run as a `Job`; `teigeodata.aio` awaits one (`await aio.result(job)`) and
streams its progress (`async for done, total in aio.progress(job)`).""",
)

lang(
    key="node",
    slug="node",
    title="Node.js",
    description="teigeodata from Node.js, Bun and Deno through a native addon: offline geocoding with rows as plain objects.",
    naming="camel",
    free_prefix="",
    files=[("JavaScript", "node/tour.mjs", "js")],
    tour_url=TOUR + "node/tour.mjs",
    intro="""`@teispace/teigeodata` is a native addon (N-API, napi-rs layout) with TypeScript types: one
prebuilt addon per platform, installed as an optional dependency, so `npm install` downloads only
yours. It runs on Node.js, and on Bun and Deno through their N-API support. For browsers and
edge workers use the [WebAssembly package](/docs/guides/wasm), which has the same API.""",
    install="""<Tabs items={["From a checkout (today)", "npm"]}>
<Tab value="From a checkout (today)">

"""
    + FROM_CHECKOUT
    + """cargo build --release -p teigeodata-node --manifest-path bindings/Cargo.toml
cp bindings/target/release/libteigeodata_node.so bindings/node/teigeodata.node   # .dylib on macOS
npm install ./bindings/node
```

`cargo xtask package-node` makes the packages as they are published, under `target/node/`.

</Tab>
<Tab value="npm">

From the first registry release:

```sh
npm install @teispace/teigeodata
```

</Tab>
</Tabs>

Node.js 22 or later. The package is ESM and CommonJS (`require` works too).""",
    read="""Every query returns a **result blob** (a `Buffer`): the rows as compact columns, read in
place. `Reader` reads it, and iterating gives each row as a plain object, with `null` where a
value is null:

<include lang="js" meta='title="tour.mjs"'>../../../examples/node/tour.mjs#import</include>

```js
for (const place of new Reader(engine.search("Pokhara", 3))) {
  console.log(place.name, place.lat, place.lon);
}
```

For large results read whole columns: `reader.numbers("lat")` is a typed array of the column's
own type (a `Float64Array` for `lat`) viewing the blob, `reader.strings("name")` an array,
`reader.nulls("elevation")` which rows are null; `records()` gives every row at once. Numbers are
JavaScript numbers, except 64-bit integer columns, which read as `bigint` (and as
`BigInt64Array` or `BigUint64Array` columns).""",
    errors="""Errors are thrown as `Error`s with a `code` property, one of the names above:""",
    extras="""## Threads and the event loop

Queries are synchronous and fast (microseconds), so they run on the calling thread. The store's
long calls have `…Async` forms that return promises and run off the event loop
(`await store.catalogAsync()`), and a `Job` reports its progress while it downloads. Worker
threads can each open their own `Engine` on the same file: the pages are mapped once and shared.""",
)

lang(
    key="wasm",
    slug="wasm",
    title="Browser and WebAssembly",
    description="teigeodata in the browser, Deno, Bun, Node.js and edge workers: the same engine compiled to WebAssembly, offline after one download.",
    naming="camel",
    files=[("JavaScript", "wasm/tour.mjs", "js")],
    tour_url=TOUR + "wasm/tour.mjs",
    prose={
        "open": (
            """An `Engine` holds the packs it answers from. A page has no file system, so
`engine.openBytes(bytes)` adds a pack from a `Uint8Array` as a **layer**: a download, an asset,
or a file the user picked. The engine answers from every layer at once, and a higher layer wins
where two disagree. Opening copies the bytes into the module's memory once and checks the head;
each section is verified against its hash the first time a query reads it.""",
            """- `openBytesWith` takes `OpenOptions`: precedence over the other packs, when sections are
  verified (lazy, eager or trusted), and which components, languages and outline level to read.
- `openBytes` returns the layer's number; `remove` takes it out again.
- What needs a file system or threads is left out of this package: `openFile`, the store (its
  downloads and updates) and the `…Async` forms. Fetch and cache packs yourself (below).""",
        ),
    },
    intro="""`@teispace/teigeodata-wasm` is the engine compiled to WebAssembly, for browsers, Web Workers,
Deno, Bun, Node.js and Cloudflare Workers. The API is the [Node.js package's](/docs/guides/node);
the difference is where packs come from: a page has no file system, so it opens a pack from its
bytes. After that one download, every query runs locally. The [playground](/playground) is this
package in a page: every query on the pack below, with the code each answer came from.""",
    install="""<Tabs items={["From a checkout (today)", "npm"]}>
<Tab value="From a checkout (today)">

"""
    + FROM_CHECKOUT
    + """rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130 --locked
cargo xtask package-wasm          # needs binaryen's wasm-opt; writes target/wasm/full/
npm install ./target/wasm/full
```

</Tab>
<Tab value="npm">

From the first registry release:

```sh
npm install @teispace/teigeodata-wasm          # every component
npm install @teispace/teigeodata-wasm-lookup   # no name search: a smaller module
```

</Tab>
</Tabs>

Two packages, the same API. The full module is at most 256 KiB gzipped (a CI gate); the
`-lookup` one leaves out name search and is at most 150 KiB, for apps that only reverse geocode,
read timezones or look places up by ID.""",
    read="""Every query returns a **result blob** (a `Uint8Array`), read in place by `Reader`; iterating
gives each row as a plain object, with `null` where a value is null. `init()` loads and compiles
the module once; call it before the first `Engine`:

<include lang="js" meta='title="tour.mjs"'>../../../examples/wasm/tour.mjs#import</include>

Whole columns read faster for large results: `reader.numbers("lat")` is a typed array viewing
the blob, `reader.strings("name")` an array. 64-bit integer columns read as `bigint`.""",
    errors="""Errors are thrown as `Error`s with a `code` property, one of the names above:""",
    extras="""## In a page

```js
import { init, Engine, Reader } from "@teispace/teigeodata-wasm";

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
  `workerd` (Cloudflare Workers); Vite, webpack and esbuild handle the `.wasm` file as an asset.""",
)

lang(
    key="swift",
    slug="swift",
    title="Swift",
    description="teigeodata in iOS and macOS apps: offline search, reverse geocoding, timezones and elevation from Swift, with typed errors and async store calls.",
    naming="camel",
    spell={"limit_max": "`TeiGeodata.limitMax`"},
    files=[("Swift", "swift/Tour/Sources/Tour/main.swift", "swift")],
    tour_url=TOUR + "swift/Tour/Sources/Tour/main.swift",
    intro="""The Swift package wraps the C library: on Apple platforms it links an XCFramework (iOS,
the iOS simulators, macOS), on Linux the library you build. Swift 6 with typed throws; every
handle is `Sendable`.""",
    install="""<Tabs items={["Swift Package Manager", "From a checkout"]}>
<Tab value="Swift Package Manager">

The release tags are the Swift releases. From the first release that carries the XCFramework:

```swift title="Package.swift"
dependencies: [
    .package(url: "https://github.com/teispace/teigeodata", from: "1.0.0"),
],
targets: [
    .target(name: "MyApp", dependencies: [.product(name: "TeiGeodata", package: "teigeodata")]),
]
```

In Xcode: File › Add Package Dependencies, and the repository URL.

</Tab>
<Tab value="From a checkout">

"""
    + FROM_CHECKOUT
    + """cargo build --release -p teigeodata-ffi        # the C library the package links
```

and depend on `bindings/swift` by path, linking `target/release` as the tour's
[Package.swift](https://github.com/teispace/teigeodata/blob/main/site/examples/swift/Tour/Package.swift)
does. `cargo xtask package-swift` makes the XCFramework the release ships.

</Tab>
</Tabs>

iOS 15, macOS 12, and Linux.""",
    read="""Every query returns a **result blob** (`Data`): the rows as compact columns, read in place.
`Reader` reads it, and iterating gives a `Record` per row, with typed accessors that return
`nil` where a value is null:

```swift
for place in try Reader(engine.search("Pokhara", 3)) {
    print(place.string("name") ?? "", place.double("lat") ?? 0)
}
```

`place["level"]` is a `Value` (`.int`, `.uint`, `.double`, `.string`) when you want the raw
kind. For large results read whole columns: `reader.numbers("lat")`, `reader.strings("name")`,
`reader.nulls("elevation")`.

<include lang="swift" meta='title="main.swift"'>../../../examples/swift/Tour/Sources/Tour/main.swift#import</include>""",
    errors="""Calls throw `TeiGeodataError` (typed throws), whose `code` is one of the names above
(`.invalidArgument`, `.missingComponent`, …) and whose `message` says what was wrong:""",
    extras="""## Concurrency

`Engine`, `Store` and `Job` are `Sendable`: share one engine between tasks and threads. Queries
are fast enough for the main actor; the store's blocking calls have `async` forms
(`try await store.catalogAsync()`), and a `Job` streams its progress as an `AsyncSequence`.

## Bundling a pack

Add the `.tgd` file to the app's resources and open it in place:
`try engine.openFile(Bundle.main.path(forResource: "world-np-lite", ofType: "tgd")!)`. The file is
memory-mapped, so it costs no heap.""",
)

lang(
    key="kotlin",
    slug="kotlin-java",
    title="Kotlin and Java",
    description="teigeodata on the JVM and Android: one library for Kotlin and Java, offline geocoding with rows as maps.",
    naming="camel",
    spell={"limit_max": "`TeiGeodata.LIMIT_MAX`"},
    files=[
        ("Kotlin", "kotlin/Tour.kt", "kotlin"),
        ("Java", "java/Tour.java", "java"),
    ],
    tour_url=TOUR + "kotlin/Tour.kt",
    intro="""One library serves Kotlin and Java, on desktop JVMs (Java 17 or later) and on Android (API 24
or later). It calls the native library through JNI; the JAR carries it for each desktop
platform, the AAR for each Android ABI (arm64-v8a, armeabi-v7a, x86_64, 16 KB page aligned).""",
    install="""<Tabs items={["Gradle", "From a checkout (today)"]}>
<Tab value="Gradle">

From the first registry release, on Maven Central:

```kotlin title="build.gradle.kts"
dependencies {
    implementation("com.teispace:teigeodata:1.0.0")            // desktop JVM
    // implementation("com.teispace:teigeodata-android:1.0.0") // Android
}
```

</Tab>
<Tab value="From a checkout (today)">

"""
    + FROM_CHECKOUT
    + """cargo xtask package-jvm --desktop         # target/jvm/teigeodata-<version>.jar
cargo xtask package-jvm --android         # target/jvm/teigeodata-<version>.aar (needs the NDK)
```

</Tab>
</Tabs>""",
    read="""Every query returns a **result blob** (`byte[]`): the rows as compact columns, read in place.
`Reader` reads it; iterating gives each row as a `Map<String, Object>` in column order, with
`null` where a value is null. Values are `String`, `Integer`, `Long`, `Float` or `Double`, by the
column's type:

```kotlin
for (place in Reader(engine.search("Pokhara", 3))) {
    println("${place["name"]} ${place["lat"]}")
}
```

For large results read whole columns: `numbers("lat")` is a `double[]`, `strings("name")` a
`String[]`, `nulls("elevation")` a `boolean[]`.""",
    errors="""Each code is a subclass of the unchecked `TeiGeodataException`:
`TeiGeodataException.InvalidArgument`, `.MissingComponent` and so on.""",
    extras="""## Threads, coroutines and Android

Handles are thread-safe and `AutoCloseable` (`use { }` in Kotlin, try-with-resources in Java);
a cleaner frees one you forget. The store's blocking calls have `…Async` forms returning
`CompletableFuture`, which Kotlin awaits with `kotlinx-coroutines-jdk8`'s `await()`.

On Android, keep a bundled pack uncompressed (`androidResources { noCompress += "tgd" }`) and
open it in place, without copying it out of the APK:

```kotlin
context.assets.openFd("world-np-lite.tgd").use { fd ->
    engine.openFileRange(context.applicationInfo.sourceDir, fd.startOffset, fd.length)
}
```""",
)

lang(
    key="dart",
    slug="dart-flutter",
    title="Dart and Flutter",
    description="teigeodata in Flutter apps and Dart programs on Android, iOS, desktop and the web: offline places with rows as maps.",
    naming="camel",
    spell={"limit_max": "`TeiGeodata.limitMax`"},
    files=[("Dart", "dart/bin/tour.dart", "dart")],
    tour_url=TOUR + "dart/bin/tour.dart",
    intro="""One package for Dart and Flutter on Android, iOS, macOS, Linux, Windows and the web. Natively
it calls the C library through FFI, supplied by the package's build hook; on the web (dart2js or
dart2wasm) the same classes run on the WebAssembly module.""",
    install="""<Tabs items={["pub.dev", "From a checkout (today)"]}>
<Tab value="pub.dev">

From the first registry release:

```sh
dart pub add teigeodata      # or: flutter pub add teigeodata
```

</Tab>
<Tab value="From a checkout (today)">

```yaml title="pubspec.yaml"
dependencies:
  teigeodata:
    git:
      url: https://github.com/teispace/teigeodata
      ref: v0.3.1
      path: bindings/dart
```

With no release library to download yet, point the build hook at a checkout to build it with
cargo:

```yaml title="pubspec.yaml"
hooks:
  user_defines:
    teigeodata:
      source: path/to/teigeodata
```

</Tab>
</Tabs>

Dart 3.10 or later (Flutter 3.38 or later). The build hook fetches the prebuilt library for the
target and checks it against the SHA-256 the package pins; `library:` names one you built,
`prebuilt_base:` a mirror.""",
    read="""Every query returns a **result blob** (`Uint8List`): the rows as compact columns, read in
place. `Reader` reads it; iterating gives each row as a `Map<String, Object?>`, with `null` where
a value is null. Dart records make the numbers easy to pull out:

```dart
for (final p in Reader(engine.search('Pokhara', 3))) {
  final (lat, lon) = (p['lat'] as double, p['lon'] as double);
  print('${p['name']} $lat, $lon');
}
```

`Reader(blob).single` takes the one row of a lookup. Whole columns: `numbers('lat')`,
`strings('name')`, `nulls('elevation')`.

<include lang="dart" meta='title="tour.dart"'>../../../examples/dart/bin/tour.dart#import</include>""",
    errors="""Each code is a subclass of `TeiGeodataException`: `InvalidArgumentException`,
`MissingComponentException` and so on:""",
    extras="""## Flutter

- **Await `TeiGeodata.init()` once** before the first engine. Natively it returns at once; on
  the web it loads the WebAssembly module, whose files go in `web/teigeodata_wasm/`.
- **Bundle a pack** as an asset and open it from its bytes
  (`engine.openBytes((await rootBundle.load('assets/world-np-lite.tgd')).buffer.asUint8List())`),
  or copy it to the app's documents directory once and `openFile` it there to map it instead.
- **Isolates.** Queries take microseconds and are fine on the UI isolate. The store's blocking
  calls have `…Async` forms that run on another thread, and a `Job` streams its progress.
- `cargo xtask flutter-check` runs the package's integration test in a Flutter app on the
  desktop, an iOS simulator or an Android device.""",
)

lang(
    key="go",
    slug="go",
    title="Go",
    description="teigeodata from Go, with cgo or without it (purego): offline geocoding with typed column reads and rows as maps.",
    naming="pascal",
    spell={"limit_max": "`tg.LimitMax`"},
    files=[("Go", "go/tour.go", "go")],
    tour_url=TOUR + "go/tour.go",
    intro="""The Go module calls the C library through cgo, or through
[purego](https://github.com/ebitengine/purego) when built with `CGO_ENABLED=0`, so it cross-compiles
without a C toolchain. The library is loaded at run time: the file passed to `LoadLibrary`, then
the one `TEIGEODATA_LIB` names, then the release's prebuilt library in the user cache (checked
against the SHA-256 the module pins), then the system's.""",
    install="""```sh
go get github.com/teispace/teigeodata/bindings/go
```

The module's versions are the tags `bindings/go/vX.Y.Z`, made with each release. Go 1.26 or
later. Fetch the prebuilt library once (from the first
release that carries it):

```sh
go run github.com/teispace/teigeodata/bindings/go/cmd/teigeodata-fetch
```

or build it from a checkout and name it:

```sh
cargo build --release -p teigeodata-ffi
export TEIGEODATA_LIB=$PWD/target/release/libteigeodata_ffi.so   # .dylib on macOS, .dll on Windows
```""",
    read="""Every query returns a **result blob** (`[]byte`), read in place by `NewReader`. `Records()`
gives each row as a `Record` (a `map[string]any`, `nil` where null); the tour reads every
result through one helper:

<include lang="go" meta='title="tour.go"'>../../../examples/go/tour.go#rows</include>

Typed column reads avoid the maps for large results: `r.Strings("name")`,
`tg.Values[float64](r, "lat")` (a slice viewing the blob, no copy), `r.Nulls("elevation")`.""",
    errors="""Every failure is a `*tg.Error` with a stable `Code`; match it with `errors.As`, or
`errors.Is(err, tg.CodeMissingComponent)`:""",
    extras="""## Goroutines

Handles may be used from any goroutine at once; `Close` frees one once no call holds it. The
store's long calls run as a `Job` whose progress you poll or wait for (`WaitProgress`) and which
`Cancel` stops.""",
)

lang(
    key="c",
    slug="c",
    title="C",
    description="teigeodata from C: a stable C ABI with pkg-config and CMake files, and a header-only reader for results.",
    naming="c",
    files=[("C", "c/tour.c", "c")],
    tour_url=TOUR + "c/tour.c",
    intro="""The C library is the ABI every other binding builds on: plain functions, opaque handles,
status codes, and no global state but each thread's last error. Its header is checked against the
last release's on every change, so a minor release never breaks a program built against it.""",
    install="""<Tabs items={["Release package", "From a checkout (today)"]}>
<Tab value="Release package">

From the first release: unpack `teigeodata-c-<version>-<target>.tar.gz`, which holds the headers,
the shared and static libraries, a pkg-config file and a CMake package.

```sh
cc tour.c $(pkg-config --cflags --libs teigeodata) -o tour
```

```cmake title="CMakeLists.txt"
find_package(teigeodata REQUIRED)
target_link_libraries(tour PRIVATE teigeodata::teigeodata)
```

</Tab>
<Tab value="From a checkout (today)">

"""
    + FROM_CHECKOUT
    + """cargo xtask package-c        # target/package/teigeodata-c-<version>-<target>/
```

</Tab>
</Tabs>""",
    read="""Every query writes a **result blob** into a `tg_bytes` you free with `tg_bytes_free`.
`teigeodata_reader.h` (header-only, C99) reads it in place: `tg_reader_init` checks the layout
once, then `tg_reader_find` names a column and `tg_reader_string`, `tg_reader_number`,
`tg_reader_int` and `tg_reader_is_null` read a row of it without allocating. The tour wraps them
in a few helpers:

<include lang="c" meta='title="tour.c"'>../../../examples/c/tour.c#helpers</include>

A `tg_str` points into the blob: copy it if you need it after `tg_bytes_free`.""",
    errors="""Every function returns `TG_OK` (0) or an error code (`TG_ERROR_INVALID_ARGUMENT`, …);
`tg_last_error` gives the calling thread's message:""",
    extras="""## Threads and memory

Handles are thread-safe: any number of threads may query one `tg_engine` at once. Every
`tg_bytes` the library returns is freed with `tg_bytes_free`, every handle with its `_free`; a
panic inside the library is caught at the boundary and returned as `TG_ERROR_INTERNAL`.""",
)

lang(
    key="cpp",
    slug="cpp",
    title="C++",
    description="teigeodata from C++17: RAII handles, exceptions with stable codes, and a range-based reader for results.",
    naming="snake",
    spell={"limit_max": "`teigeodata::LIMIT_MAX`"},
    files=[("C++", "cpp/tour.cpp", "cpp")],
    tour_url=TOUR + "cpp/tour.cpp",
    intro="""`teigeodata.hpp` is a header-only C++17 layer over the C library: RAII handles, `std::string_view`
arguments, exceptions with the stable error codes, and `teigeodata_reader.hpp` to read results
with a range-based `for`. Everything is in namespace `teigeodata`; the tour shortens it with
`namespace tg = teigeodata;`.""",
    install="""Install the [C package](/docs/guides/c#install): the C++ headers ship in it. Then
compile as C++17 or later:

```sh
c++ -std=c++17 tour.cpp $(pkg-config --cflags --libs teigeodata) -o tour
```""",
    read="""Every query returns a `tg::Blob` (the result's bytes, freed when it goes out of scope).
`tg::Reader` reads it in place; iterating gives a `Row` per row with `string`, `number` and
`integer` accessors that return `std::optional` (empty where null):

<include lang="cpp" meta='title="tour.cpp"'>../../../examples/cpp/tour.cpp#search</include>

Keep the `Blob` alive while you read it: the reader and every `string_view` borrow it, and a
`Reader` cannot be made from a temporary. `records()` gives each row as a `std::map` of
`std::variant` values; `numbers`, `strings` and `nulls` read whole columns.""",
    errors="""Calls throw `teigeodata::Error`, whose `code()` is a `teigeodata::ErrorCode`:""",
    extras="""## Threads

An `Engine` answers from any number of threads at once. Handles are move-only.""",
)

lang(
    key="dotnet",
    slug="dotnet",
    title=".NET",
    description="teigeodata from C# and every .NET language: source-generated P/Invoke, NativeAOT and trimming ready, spans over results.",
    naming="pascal",
    spell={"limit_max": "`Geodata.LimitMax`"},
    files=[("C#", "dotnet/Program.cs", "csharp")],
    tour_url=TOUR + "dotnet/Program.cs",
    intro="""`Teispace.TeiGeodata` is the .NET binding: source-generated P/Invoke (`LibraryImport`) with
no marshalling at run time, compatible with NativeAOT and trimming. The package carries the
native library for Windows, macOS and Linux (glibc and musl) on x64 and Arm64.""",
    install="""<Tabs items={["NuGet", "From a checkout (today)"]}>
<Tab value="NuGet">

From the first registry release:

```sh
dotnet add package Teispace.TeiGeodata
```

</Tab>
<Tab value="From a checkout (today)">

"""
    + FROM_CHECKOUT
    + """cargo xtask package-dotnet    # target/dotnet/Teispace.TeiGeodata.<version>.nupkg
```

and add `target/dotnet` as a package source, or reference
`bindings/dotnet/TeiGeodata/TeiGeodata.csproj` as the tour's project does.

</Tab>
</Tabs>

.NET 10 or later.""",
    read="""Every query returns a **result blob** (`byte[]`), read in place by `BlobReader`; iterating
gives each row as an `IReadOnlyDictionary<string, object?>`, `null` where a value is null:

```csharp
foreach (var place in new BlobReader(engine.Search("Pokhara", 3)))
    Console.WriteLine($"{place["name"]} {place["lat"]}");
```

For large results, `Values<double>("lat")` is a `ReadOnlySpan<double>` over the blob itself, no
copy; `Strings("name")` and `Nulls("elevation")` read the rest.""",
    errors="""Calls throw `TeiGeodataException`, whose `Code` is an `ErrorCode`:""",
    extras="""## Threads and async

An `Engine` answers from any number of threads; `Dispose` frees it once no call holds it. The
store's blocking calls have awaitable forms (`Store.OpenAsync`, `CatalogAsync`, `StatusAsync`,
`RemoveAsync`, …), and a `Job` streams its progress with `ProgressAsync`, gives its result with
`ResultAsync` and cancels through a `CancellationToken`.""",
)

def main():
    check = "--check" in sys.argv[1:]
    stale = []
    os.makedirs(OUT, exist_ok=True)
    for lang in LANGS:
        path = f"{OUT}/{lang['slug']}.mdx"
        text = page(lang)
        if check:
            try:
                with open(path, encoding="utf-8") as f:
                    if f.read() != text:
                        stale.append(path)
            except FileNotFoundError:
                stale.append(path)
            continue
        with open(path, "w", encoding="utf-8") as f:
            f.write(text)
    if stale:
        sys.exit("stale guides (run python3 site/scripts/guides.py):\n  " + "\n  ".join(stale))
    print(f"guides {'ok' if check else 'written'}: {len(LANGS)}")


if __name__ == "__main__":
    main()
