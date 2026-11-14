# teigeodata-server

The request core the teigeodata servers share (phase 6,
[D-309](https://github.com/teispace/teigeodata/blob/main/docs/03-decisions/log.md)): one mapping
from a place to each output (native JSON, GeoJSON, Nominatim-compatible JSON), the attribution
every answer from main data carries, and the bounds every query is held to. The `tgd` command
line prints through the same mappings.

Licence: Apache-2.0.
