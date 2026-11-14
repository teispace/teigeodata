# teigeodata-server

The request core the teigeodata servers share (phase 6,
[D-310 and D-311](https://github.com/teispace/teigeodata/blob/main/docs/03-decisions/log.md)),
with no runtime of its own:

- `packs`: the pack set, files and the store's installed packs, behind a shared engine that
  follows the store's journal while it answers.
- `query`: every query a client may ask, the bounds it is held to and the stable error codes.
- `output`: one mapping from a place to each output (native JSON, GeoJSON,
  Nominatim-compatible JSON), with the attribution of the packs that answered.
- `seams`: who is asking, may they, how much they used; open by default, for a hosted tier to
  plug its own in.

The `tgd` command line runs its queries and prints through the same code.

Licence: Apache-2.0.
