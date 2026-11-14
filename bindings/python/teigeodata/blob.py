"""Reads a teigeodata result blob (format v1, D-275) in place.

Numbers are ``memoryview`` views of the blob, with no copy; strings are decoded when read, a
dictionary column's strings once each. The Rust library validated the blob when it made it.

>>> b = Blob(engine.search("Kathmandu", 5))  # doctest: +SKIP
>>> b.rows, b.strings("name")[0], b.numbers("lat")[0]  # doctest: +SKIP
"""

from __future__ import annotations

import struct

_FIXED = {1: "i", 2: "I", 3: "q", 4: "Q", 5: "f", 6: "d", 8: "B", 9: "h", 10: "H"}
_UTF8, _DICT16, _DICT32 = 7, 11, 12


class Blob:
    """A view of one blob: its rows and named columns."""

    def __init__(self, data: bytes) -> None:
        if data[:4] not in (b"TGB1", b"TGB0"):
            raise ValueError("not a teigeodata blob")
        self._data = data
        self._view = memoryview(data)
        self.rows, count, _ = struct.unpack_from("<III", data, 4)
        self._columns: dict[str, tuple[int, ...]] = {}
        for i in range(count):
            d = 16 + 40 * i
            name = bytes(data[d : d + 16]).rstrip(b"\0").decode()
            self._columns[name] = struct.unpack_from("<6I", data, d + 16)

    @property
    def columns(self) -> list[str]:
        """The column names, in order."""
        return list(self._columns)

    def numbers(self, name: str) -> memoryview:
        """A number column, viewed in place. A null row reads as zero: see ``nulls``."""
        ty, at, length, _, _, _ = self._columns[name]
        return self._view[at : at + length].cast(_FIXED[ty])

    def nulls(self, name: str) -> list[bool]:
        """Per row, whether it is null."""
        nulls_at = self._columns[name][5]
        if nulls_at == 0:
            return [False] * self.rows
        bits = self._data
        return [not (bits[nulls_at + r // 8] >> (r % 8)) & 1 for r in range(self.rows)]

    def strings(self, name: str) -> list[str | None]:
        """A string column (UTF-8 or a dictionary), with ``None`` for a null row."""
        ty, at, _, aux, _, _ = self._columns[name]
        data, rows = self._data, self.rows
        if ty == _UTF8:
            offsets = self._view[at : at + 4 * (rows + 1)].cast("I")
            values = [data[aux + offsets[r] : aux + offsets[r + 1]].decode() for r in range(rows)]
        elif ty in (_DICT16, _DICT32):
            (count,) = struct.unpack_from("<I", data, aux)
            offsets = self._view[aux + 4 : aux + 8 + 4 * count].cast("I")
            base = aux + 8 + 4 * count
            words = [data[base + offsets[k] : base + offsets[k + 1]].decode() for k in range(count)]
            width = 2 if ty == _DICT16 else 4
            codes = self._view[at : at + width * rows].cast("H" if width == 2 else "I")
            values = [words[c] if c < count else "" for c in codes]
        else:
            raise TypeError(f"{name} is not a string column")
        nulls = self.nulls(name)
        return [None if nulls[r] else values[r] for r in range(rows)]
