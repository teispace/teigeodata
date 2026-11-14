"""The binding benchmark's Python rows (docs/02-research/benches/s4-bindings.md): best of five
rounds, ns per call. Run from bindings/python after building the module. With
TEIGEODATA_BENCH_PACK naming a pack, the fifth case runs the 1,000-row query on it and decodes
it with the package's `Reader`."""

import os
import struct
import time

import teigeodata as tg
from teigeodata.blob import Reader


def best(n: int, f) -> float:
    out = float("inf")
    for _ in range(5):
        t = time.perf_counter_ns()
        for _ in range(n):
            f()
        out = min(out, (time.perf_counter_ns() - t) / n)
    return out


def lat_sum(blob) -> float:
    # Descriptor 1 is `lat` (the order sample_rows writes); offset and length at +20 and +24.
    view = memoryview(blob)
    at, length = struct.unpack_from("<II", view, 16 + 40 + 20)
    return sum(view[at : at + length].cast("d"))


def row(case: str, ns: float) -> None:
    print(f"{'python':<10} {case:<22} {ns:>10.1f} ns")


row("noop", best(1_000_000, tg.noop))
row("fold", best(1_000_000, lambda: tg.fold("Kathmandu")))
row("sample_rows(1000)+sum", best(10_000, lambda: lat_sum(tg.sample_rows(1000))))
row("callback", best(100, lambda: tg.progress(1000, lambda s: True)) / 1000)

pack = os.environ.get("TEIGEODATA_BENCH_PACK")
if pack:
    engine = tg.Engine()
    engine.open_file(pack)

    def radius() -> tuple[int, float]:
        r = Reader(engine.within_radius(27.7, 85.3, 50_000.0, 1000))
        return sum(len(n or "") for n in r.strings("name")), sum(r.numbers("lat"))

    if Reader(engine.within_radius(27.7, 85.3, 50_000.0, 1000)).rows == 1000:
        row("radius(1000)+decode", best(50, radius))
    else:
        print(f"python: {pack}: the query does not return 1,000 rows")
