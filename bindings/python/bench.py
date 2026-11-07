"""The binding benchmark's Python rows (docs/02-research/benches/s4-bindings.md): best of five
rounds, ns per call. Run from bindings/python after building the module."""

import struct
import time

import teigeodata as tg


def best(n: int, f) -> float:
    out = float("inf")
    for _ in range(5):
        t = time.perf_counter_ns()
        for _ in range(n):
            f()
        out = min(out, (time.perf_counter_ns() - t) / n)
    return out


def lat_sum(blob: bytes) -> float:
    # Descriptor 1 is `lat` (the order sample_rows writes); offset and length at +20 and +24.
    at, length = struct.unpack_from("<II", blob, 16 + 40 + 20)
    return sum(memoryview(blob)[at : at + length].cast("d"))


def row(case: str, ns: float) -> None:
    print(f"{'python':<10} {case:<22} {ns:>10.1f} ns")


row("noop", best(1_000_000, tg.noop))
row("fold", best(1_000_000, lambda: tg.fold("Kathmandu")))
row("sample_rows(1000)+sum", best(10_000, lambda: lat_sum(tg.sample_rows(1000))))
row("callback", best(100, lambda: tg.progress(1000, lambda s: True)) / 1000)
