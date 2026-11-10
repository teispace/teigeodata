"""Checks the catalog's JSON Schema with python-jsonschema, a second validator (D-160).

    python -I tools/tuf-oracle/check_schema.py crates/teigeodata-sync/schema/catalog-custom-v1.json

A valid entry passes; each invalid value and each missing required field is refused. The
cross-field rules (names against the path and hash, sizes against lengths) are the Rust
reader's, `teigeodata_sync::catalog::Entry::from_target`.
"""

import json, sys, copy
from jsonschema import Draft202012Validator
schema = json.load(open(sys.argv[1]))
Draft202012Validator.check_schema(schema)
v = Draft202012Validator(schema)
good = {"schema": 1, "address": "np", "component": "places", "release": "2026.11.0",
  "file": "np.places.2026.11.0.1a1a1a1a.tgd.zst", "pack": {"length": 11748636, "head": "ab"*16},
  "transport": {"encoding": "zstd-seekable", "seek_table": 293},
  "sizes": {"point": [522192, 601344], "names": [917602, 1302528]}, "deps": ["np.baseline.tgd"],
  "min_reader": 0, "bbox": [801000000, 263000000, 882000000, 304500000], "licence": "ODbL-1.0", "future": 1}
assert not list(v.iter_errors(good)), list(v.iter_errors(good))
bad = [("schema", 2), ("address", "NP"), ("release", "2026/11"), ("file", "x.tgd"), ("pack", {"length": 1, "head": "AB"*16}),
  ("transport", {"encoding": "raw", "seek_table": 1}), ("transport", {"encoding": "zstd-seekable", "seek_table": 0}),
  ("sizes", {}), ("sizes", {"point": [1]}), ("deps", ["np"]), ("deps", ["np.dem.tgd"]*2), ("min_reader", 70000),
  ("bbox", [80.1, 26.3, 88.2, 30.4]), ("bbox", [0, 0, 1, 900000001]), ("licence", "CC BY")]
for k, val in bad:
    b = copy.deepcopy(good); b[k] = val
    assert list(v.iter_errors(b)), (k, val)
for k in schema["required"]:
    b = copy.deepcopy(good); del b[k]
    assert list(v.iter_errors(b)), k
print(f"schema ok: 1 valid, {len(bad)} invalid values and {len(schema['required'])} missing fields refused")
