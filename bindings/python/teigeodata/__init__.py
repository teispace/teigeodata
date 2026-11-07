"""teigeodata: accurate, offline places.

Every function is generated from the Rust API (``cargo xtask bindings``); see ``_native.pyi``
for signatures and documentation. A blob result is ``bytes`` in the teigeodata blob format;
``teigeodata.blob`` reads it.
"""

from teigeodata._native import *  # noqa: F403
from teigeodata._native import __all__ as _native_all

__all__ = list(_native_all)
