"""Parity: the Python binding gives the results bindings/parity.json expects (checked there
against Rust)."""

import json
import pathlib
import struct
import unittest

import teigeodata as tg

PARITY = json.loads((pathlib.Path(__file__).parents[2] / "parity.json").read_text("utf-8"))


def column(blob: bytes, name: str) -> memoryview:
    """A fixed-width column of a v0 blob, viewed in place (no copy)."""
    assert blob[:4] == b"TGB1"
    rows, cols, total = struct.unpack_from("<III", blob, 4)
    assert total == len(blob)
    for i in range(cols):
        d = 16 + 40 * i
        if blob[d : d + 16].rstrip(b"\0").decode() == name:
            ty, at, length = struct.unpack_from("<III", blob, d + 16)
            fmt = {1: "i", 2: "I", 3: "q", 4: "Q", 5: "f", 6: "d"}[ty]
            return memoryview(blob)[at : at + length].cast(fmt)
    raise KeyError(name)


class Parity(unittest.TestCase):
    def test_version(self) -> None:
        self.assertEqual(tg.version(), PARITY["version"])
        tg.noop()

    def test_fold(self) -> None:
        for text, folded in PARITY["fold"]:
            self.assertEqual(tg.fold(text), folded)

    def test_sample_rows(self) -> None:
        s = PARITY["sample_rows"]
        blob = tg.sample_rows(s["rows"])
        self.assertIsInstance(blob, bytes)
        self.assertEqual(len(blob), s["bytes"])
        lat = column(blob, "lat")
        self.assertEqual(len(lat), s["rows"])
        total = 0.0
        for v in lat:  # in row order, as Rust sums
            total += v
        self.assertEqual(total, s["lat_sum"])
        self.assertEqual(sum(float(v) for v in column(blob, "elevation")), s["elevation_sum"])
        self.assertEqual(tg.SAMPLE_ROWS_MAX, PARITY["sample_rows_max"])

    def test_distance(self) -> None:
        d = PARITY["distance_m"]
        self.assertAlmostEqual(tg.distance_m(*d["args"]), d["metres"], delta=1e-6)
        with self.assertRaises(tg.InvalidArgumentError):
            tg.distance_m(*d["out_of_range"])

    def test_errors(self) -> None:
        with self.assertRaises(tg.InvalidArgumentError) as e:
            tg.sample_rows(tg.SAMPLE_ROWS_MAX + 1)
        self.assertIsInstance(e.exception, tg.TeiGeodataError)
        self.assertIn("at most", str(e.exception))
        with self.assertRaises(OverflowError):
            tg.sample_rows(-1)
        with self.assertRaises(TypeError):
            tg.fold(None)  # type: ignore[arg-type]

    def test_progress(self) -> None:
        g = PARITY["progress"]
        seen: list[int] = []
        self.assertEqual(tg.progress(3, lambda s: seen.append(s) is None), 3)
        self.assertEqual(seen, [1, 2, 3])
        with self.assertRaises(tg.CancelledError) as e:
            tg.progress(g["steps"], lambda s: s < g["cancel_after"])
        self.assertEqual(str(e.exception), g["cancelled_message"])

    def test_callback_exception_propagates(self) -> None:
        def boom(step: int) -> bool:
            raise ValueError(f"boom at {step}")

        with self.assertRaisesRegex(ValueError, "boom at 1"):
            tg.progress(5, boom)


if __name__ == "__main__":
    unittest.main()
