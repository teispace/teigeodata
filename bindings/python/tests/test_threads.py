"""Many threads on one engine (D-274, D-290): every thread gets the answers one thread gets.

On a free-threaded Python (3.14t, or 3.15t with the abi3t module) the threads truly run at
once, and importing teigeodata must leave the GIL off: the module declares that it does not
need it. On a GIL build the calls still overlap, as each one releases the GIL in Rust.
"""

import json
import pathlib
import sys
import sysconfig
import threading
import unittest

import teigeodata as tg
from teigeodata.blob import Reader

ROOT = pathlib.Path(__file__).parents[2]
PARITY = json.loads((ROOT / "parity.json").read_text("utf-8"))
FIXTURE = str(ROOT / PARITY["engine"]["fixture"])
THREADS, ROUNDS = 8, 2000


def answers(engine: tg.Engine) -> tuple:
    """One round of reads, as plain values (each blob read and released here)."""
    e = PARITY["engine"]
    hits = Reader(engine.search(e["search"]["query"], e["search"]["limit"]))
    got = Reader(engine.get(e["get"]["id"]))
    near = Reader(engine.reverse(e["reverse"]["lat"], e["reverse"]["lon"], e["reverse"]["limit"]))
    rows = memoryview(tg.sample_rows(100))
    return (
        tuple(hits.strings("id")),
        tuple(got.strings("name")),
        tuple(near.strings("id")),
        rows.tobytes(),
        tg.fold("Lahachowk"),
    )


class Threads(unittest.TestCase):
    def test_gil_stays_off_on_free_threaded_builds(self) -> None:
        if not sysconfig.get_config_var("Py_GIL_DISABLED"):
            self.skipTest("a GIL build")
        self.assertFalse(sys._is_gil_enabled(), "importing teigeodata turned the GIL on")

    def test_one_engine_many_threads(self) -> None:
        engine = tg.Engine()
        engine.open_file(FIXTURE)
        want = answers(engine)
        start = threading.Barrier(THREADS)
        wrong: list[str] = []

        def work() -> None:
            start.wait()
            for _ in range(ROUNDS):
                got = answers(engine)
                if got != want:
                    wrong.append(repr(got)[:200])
                    return

        threads = [threading.Thread(target=work) for _ in range(THREADS)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        self.assertEqual(wrong, [])

    def test_a_blob_outlives_its_engine_and_is_shared(self) -> None:
        engine = tg.Engine()
        engine.open_file(FIXTURE)
        blob = engine.get(PARITY["engine"]["get"]["id"])
        del engine
        want = bytes(blob)
        seen: list[bytes] = []

        def read() -> None:
            for _ in range(ROUNDS):
                seen.append(memoryview(blob).tobytes())

        threads = [threading.Thread(target=read) for _ in range(THREADS)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        self.assertEqual(len(seen), THREADS * ROUNDS)
        self.assertTrue(all(s == want for s in seen))


if __name__ == "__main__":
    unittest.main()
