"""Reads a teigeodata result blob (format v1, D-275) in place.

It reads any buffer: a ``teigeodata.Blob`` result (lent by the library without a copy),
``bytes`` or a ``memoryview``. Numbers are ``memoryview`` views of it, with no copy; strings
are decoded when read, a dictionary column's strings once each. The Rust library validated the
blob when it made it.

>>> b = Reader(engine.search("Kathmandu", 5))  # doctest: +SKIP
>>> b.rows, b.strings("name")[0], b.numbers("lat")[0]  # doctest: +SKIP
>>> for place in b:  # each row as a dict, None where null  # doctest: +SKIP
...     print(place["name"], place["lat"])
"""

from __future__ import annotations

import struct
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from collections.abc import Buffer, Iterator  # Buffer: 3.12; annotations only

_FIXED = {1: "i", 2: "I", 3: "q", 4: "Q", 5: "f", 6: "d", 8: "B", 9: "h", 10: "H"}
_UTF8, _DICT16, _DICT32 = 7, 11, 12


class Reader:
    """A view of one blob: its rows and named columns."""

    def __init__(self, data: Buffer) -> None:
        view = memoryview(data).cast("B")
        if bytes(view[:4]) not in (b"TGB1", b"TGB0"):
            raise ValueError("not a teigeodata blob")
        self._data = self._view = view
        self.rows, count, _ = struct.unpack_from("<III", view, 4)
        self._columns: dict[str, tuple[int, ...]] = {}
        for i in range(count):
            d = 16 + 40 * i
            name = bytes(view[d : d + 16]).rstrip(b"\0").decode()
            self._columns[name] = struct.unpack_from("<6I", view, d + 16)

    @property
    def columns(self) -> list[str]:
        """The column names, in order."""
        return list(self._columns)

    def has(self, name: str) -> bool:
        """Whether there is a column called ``name``."""
        return name in self._columns

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
            values = [str(data[aux + offsets[r] : aux + offsets[r + 1]], "utf-8") for r in range(rows)]
        elif ty in (_DICT16, _DICT32):
            (count,) = struct.unpack_from("<I", data, aux)
            offsets = self._view[aux + 4 : aux + 8 + 4 * count].cast("I")
            base = aux + 8 + 4 * count
            words = [str(data[base + offsets[k] : base + offsets[k + 1]], "utf-8") for k in range(count)]
            width = 2 if ty == _DICT16 else 4
            codes = self._view[at : at + width * rows].cast("H" if width == 2 else "I")
            values = [words[c] if c < count else "" for c in codes]
        else:
            raise TypeError(f"{name} is not a string column")
        nulls = self.nulls(name)
        return [None if nulls[r] else values[r] for r in range(rows)]

    def column(self, name: str) -> list[Any]:
        """A column with one value per row: ``str`` or a number, ``None`` where null."""
        if self._columns[name][0] in (_UTF8, _DICT16, _DICT32):
            return self.strings(name)
        nulls = self.nulls(name)
        return [None if nulls[r] else v for r, v in enumerate(self.numbers(name).tolist())]

    def records(self) -> list[dict[str, Any]]:
        """Every row as a dict, column name to value as ``column`` reads it."""
        columns = {name: self.column(name) for name in self._columns}
        return [{name: values[r] for name, values in columns.items()} for r in range(self.rows)]

    def __iter__(self) -> Iterator[dict[str, Any]]:
        """The rows as ``records`` gives them."""
        return iter(self.records())

    def __len__(self) -> int:
        """The number of rows."""
        return self.rows
