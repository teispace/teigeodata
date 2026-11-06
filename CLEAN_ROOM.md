# Clean-room rules for the lite build

The **lite** data build is published under CC-BY 4.0. To keep that licence valid, its pipeline
must never read data whose licence would make the result share-alike or non-commercial
([ADR-0002](docs/03-decisions/adr/ADR-0002-licences.md)).

## Rules

1. Every source adapter declares a **licence class**: `public-domain`, `cc0`, `cc-by`,
   `cc-by-igo`, `odc-by`, `godl` (and similar attribution-only), `odbl`, `share-alike`,
   `non-commercial`, `proprietary`.
2. The lite build may read only attribution-only classes (and public domain). `odbl`,
   `share-alike`, `non-commercial` and `proprietary` are refused **at ingest**, before any step,
   including steps that only *decide* things (matching, merging, validation).
3. Corrections and lexicon entries carry their evidence's licence class; a correction whose
   evidence is ODbL (for example "it is like this in OSM") is used by the main build only.
4. The **licence gate** fails a lite build if any field's provenance includes a refused class,
   and generates the lite `ATTRIBUTION.md` from provenance.
5. Lite and main share the schema and the ID space. Lite never reads main's packs.
6. Attribution-only sources that were themselves built from ODbL data (for example Kontur
   Population) are treated as ODbL. The full admission table is
   [ADR-0030](docs/03-decisions/adr/ADR-0030-source-admission.md); note that the main build also
   refuses CC BY-SA and non-commercial sources.

Contributors working on lite-specific code or data must not copy values from OpenStreetMap,
Overture's OSM-derived themes, or any share-alike source into lite inputs.
