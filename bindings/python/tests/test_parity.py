"""Parity: the Python binding gives the results bindings/parity.json expects (checked there
against Rust)."""

import json
import pathlib
import struct
import unittest

import teigeodata as tg
from teigeodata.blob import Blob

PARITY = json.loads((pathlib.Path(__file__).parents[2] / "parity.json").read_text("utf-8"))


def column(blob: bytes, name: str) -> memoryview:
    """A fixed-width column of a blob, viewed in place (no copy)."""
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



FIXTURE = str(pathlib.Path(__file__).parents[2] / PARITY["engine"]["fixture"])


class Engine(unittest.TestCase):
    """The engine handle on the fixture pack (parity.json, "engine")."""

    def setUp(self) -> None:
        self.engine = tg.Engine()
        self.assertEqual(self.engine.open_file(FIXTURE), 0)
        self.p = PARITY["engine"]

    def test_get(self) -> None:
        g = self.p["get"]
        b = Blob(self.engine.get(g["id"]))
        self.assertEqual(b.columns, self.p["columns"])
        self.assertEqual(b.rows, 1)
        self.assertEqual(b.strings("name"), [g["name"]])
        self.assertEqual(b.strings("kind"), [g["kind"]])
        self.assertEqual(list(b.numbers("lat")), [g["lat"]])
        self.assertEqual(list(b.numbers("lon")), [g["lon"]])

    def test_queries(self) -> None:
        q = self.p["search"]
        hits = Blob(self.engine.search(q["query"], q["limit"]))
        self.assertEqual(hits.strings("id")[0], q["first_id"])
        r = self.p["reverse"]
        near = Blob(self.engine.reverse(r["lat"], r["lon"], r["limit"]))
        self.assertEqual(near.strings("id"), r["ids"])
        self.assertAlmostEqual(near.numbers("meters")[0], r["first_meters"], delta=0.1)
        a = self.p["admin_at"]
        units = Blob(self.engine.admin_at(a["lat"], a["lon"]))
        self.assertEqual(units.strings("id"), a["ids"])

    def test_search_options(self) -> None:
        w = self.p["search_with"]
        options = tg.SearchOptions(focus_lat=w["focus_lat"], focus_lon=w["focus_lon"], focus_scale_m=w["focus_scale_m"])
        self.assertEqual((options.focus_lat, options.strict), (w["focus_lat"], False))
        got = Blob(self.engine.search_with(w["query"], w["limit"], options))
        self.assertEqual(got.strings("id"), w["ids"])
        plain = Blob(self.engine.search_with(w["query"], w["limit"], tg.SearchOptions()))
        self.assertNotEqual(plain.strings("id"), w["ids"])
        with self.assertRaises(tg.InvalidArgumentError):
            self.engine.search_with(w["query"], 1, tg.SearchOptions(focus_lat=1.0))
        with self.assertRaises(TypeError):
            tg.SearchOptions(1.0)  # keyword arguments only

    def test_open_options(self) -> None:
        o = self.p["open_with"]
        bad = bytearray(pathlib.Path(FIXTURE).read_bytes())
        bad[o["corrupt_at"]] ^= 0xFF
        engine = tg.Engine()
        with self.assertRaises(tg.CorruptError):
            engine.open_bytes_with(bytes(bad), tg.OpenOptions(verify=tg.Verify.EAGER))
        self.assertEqual(engine.open_bytes_with(bytes(bad), tg.OpenOptions()), 0)
        every = tg.OpenOptions(verify=tg.Verify.EAGER, load=tg.Load.READ, priority=2, outline_level=1)
        self.assertEqual((every.verify, every.load, int(tg.Verify.TRUSTED)), (tg.Verify.EAGER, tg.Load.READ, 2))
        self.assertEqual(engine.open_file_with(FIXTURE, every), 1)
        with self.assertRaises(tg.InvalidArgumentError):
            engine.open_file_with(FIXTURE, tg.OpenOptions(outline_level=o["bad_outline_level"]))
        with self.assertRaises(TypeError):
            tg.OpenOptions(verify=1)  # an enum, not its value
        points = tg.Engine()
        points.open_file_with(FIXTURE, tg.OpenOptions(components=o["components"], languages=o["languages"]))
        self.assertEqual(Blob(points.get(self.p["get"]["id"])).rows, 1)
        with self.assertRaises(tg.MissingComponentError):
            points.search("Synthland", 1)
        with self.assertRaises(tg.InvalidArgumentError):
            points.open_file_with(FIXTURE, tg.OpenOptions(components=o["bad_components"]))

    def test_postal(self) -> None:
        e, p = self.engine, self.p
        c = p["postal_lookup"]
        got = Blob(e.postal_lookup(c["code"], c["scheme"]))
        self.assertEqual((got.strings("postal"), got.strings("id")), (c["postal"], c["ids"]))
        c = p["postal_prefix"]
        got = Blob(e.postal_prefix(c["input"], c["scheme"], c["limit"]))
        self.assertEqual(got.strings("postal"), c["postal"])
        c = p["postal_at"]
        got = Blob(e.postal_at(c["lat"], c["lon"]))
        self.assertEqual((got.strings("postal"), got.strings("code_kind")), (c["postal"], c["code_kind"]))
        c = p["postal_codes_of"]
        self.assertEqual(Blob(e.postal_codes_of(c["id"])).strings("postal"), c["postal"])

    def test_elevation(self) -> None:
        h = self.p["elevation_at"]
        self.assertEqual(self.engine.open_file(str(pathlib.Path(FIXTURE).parents[1] / h["fixture"])), 1)
        got = Blob(self.engine.elevation_at(h["lat"], h["lon"]))
        self.assertEqual(list(got.numbers("meters")), [h["meters"]])
        self.assertEqual(got.strings("source"), [h["source"]])
        self.assertTrue(self.engine.remove(1))
        self.assertFalse(self.engine.remove(1))
        self.assertEqual(Blob(self.engine.elevation_at(h["lat"], h["lon"])).rows, 0)

    def test_graph_radius_zone_and_codes(self) -> None:
        e, p = self.engine, self.p
        for method in ("parent", "ancestors", "children"):
            got = Blob(getattr(e, method)(p[method]["id"]))
            self.assertEqual(got.strings("id"), p[method]["ids"], method)
        n = p["nearby"]
        self.assertEqual(Blob(e.nearby(n["id"], n["limit"])).strings("id"), n["ids"])
        w = p["within_radius"]
        got = Blob(e.within_radius(w["lat"], w["lon"], w["meters"], w["limit"]))
        self.assertEqual(got.strings("id"), w["ids"])
        t = p["tz_at"]
        self.assertEqual(Blob(e.tz_at(t["lat"], t["lon"])).strings("tz"), [t["tz"]])
        i = p["by_iso"]
        self.assertEqual(Blob(e.by_iso(i["code"])).strings("id"), [i["id"]])
        with self.assertRaises(tg.InvalidArgumentError):
            e.by_iso(p["bad_iso"])
        with self.assertRaises(tg.InvalidArgumentError):
            e.within_radius(w["lat"], w["lon"], -1.0, 1)

    def test_errors(self) -> None:
        with self.assertRaises(tg.InvalidArgumentError):
            self.engine.get(self.p["bad_id"])
        with self.assertRaises(tg.IoError):
            self.engine.open_file("/nonexistent.tgd")
        with self.assertRaises(tg.CorruptError):
            self.engine.open_bytes(b"not a pack")
        self.assertEqual(self.engine.layers(), 1)

    def test_threads(self) -> None:
        """Queries from threads at once: the GIL is released while Rust runs (D-274)."""
        import threading

        errors: list[BaseException] = []

        def work() -> None:
            try:
                for _ in range(50):
                    b = Blob(self.engine.get(self.p["get"]["id"]))
                    assert b.strings("name") == [self.p["get"]["name"]]
            except BaseException as e:  # noqa: BLE001
                errors.append(e)

        threads = [threading.Thread(target=work) for _ in range(4)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        self.assertEqual(errors, [])

if __name__ == "__main__":
    unittest.main()
