// Parity: the Node.js binding gives the results bindings/parity.json expects (checked there
// against Rust). Run with `node --test test/parity.test.js` after building teigeodata.node.
"use strict";
const assert = require("node:assert/strict");
const { readFileSync } = require("node:fs");
const { join } = require("node:path");
const { test } = require("node:test");
const tg = require("..");

const parity = JSON.parse(readFileSync(join(__dirname, "../../parity.json"), "utf8"));

/** A fixed-width column of a blob, viewed in place (no copy). */
function column(blob, name) {
  const view = new DataView(blob.buffer, blob.byteOffset, blob.byteLength);
  assert.equal(blob.toString("latin1", 0, 4), "TGB1");
  assert.equal(view.getUint32(12, true), blob.byteLength);
  const columns = view.getUint32(8, true);
  for (let i = 0; i < columns; i++) {
    const d = 16 + 40 * i;
    if (blob.toString("utf8", d, d + 16).replace(/\0+$/, "") !== name) continue;
    const [type, at, len] = [0, 4, 8].map((k) => view.getUint32(d + 16 + k, true));
    const Kind = { 1: Int32Array, 2: Uint32Array, 3: BigInt64Array, 4: BigUint64Array, 5: Float32Array, 6: Float64Array }[type];
    return new Kind(blob.buffer, blob.byteOffset + at, len / Kind.BYTES_PER_ELEMENT);
  }
  throw new Error(`no column ${name}`);
}

test("version and noop", () => {
  assert.equal(tg.version(), parity.version);
  assert.equal(tg.noop(), undefined);
});

test("fold", () => {
  for (const [text, folded] of parity.fold) assert.equal(tg.fold(text), folded);
});

test("sampleRows", () => {
  const s = parity.sample_rows;
  const blob = tg.sampleRows(s.rows);
  assert.ok(Buffer.isBuffer(blob));
  assert.equal(blob.byteLength, s.bytes);
  assert.equal(blob.byteOffset % 8, 0);
  let lat = 0;
  for (const v of column(blob, "lat")) lat += v;
  assert.equal(lat, s.lat_sum);
  let elevation = 0;
  for (const v of column(blob, "elevation")) elevation += v;
  assert.equal(elevation, s.elevation_sum);
  assert.equal(tg.SAMPLE_ROWS_MAX, parity.sample_rows_max);
});

test("distanceM", () => {
  const d = parity.distance_m;
  assert.ok(Math.abs(tg.distanceM(...d.args) - d.metres) < 1e-6);
  assert.throws(() => tg.distanceM(...d.out_of_range), { code: "InvalidArgument" });
});

test("errors carry their code", () => {
  assert.throws(() => tg.sampleRows(tg.SAMPLE_ROWS_MAX + 1), { code: "InvalidArgument", message: /at most/ });
  assert.throws(() => tg.sampleRows(-1));
  assert.throws(() => tg.fold(42));
});

test("progress calls back, cancels and rethrows", () => {
  const seen = [];
  assert.equal(tg.progress(3, (s) => (seen.push(s), true)), 3);
  assert.deepEqual(seen, [1, 2, 3]);
  const g = parity.progress;
  assert.throws(() => tg.progress(g.steps, (s) => s < g.cancel_after), { code: "Cancelled", message: g.cancelled_message });
  const boom = new RangeError("boom");
  assert.throws(() => tg.progress(5, () => { throw boom; }), (e) => e === boom);
});

/** A string column (UTF-8 or a dictionary) of a v1 blob, null rows as null. */
function strings(blob, name) {
  const view = new DataView(blob.buffer, blob.byteOffset, blob.byteLength);
  const u32 = (at) => view.getUint32(at, true);
  const rows = u32(4);
  for (let i = 0; i < u32(8); i++) {
    const d = 16 + 40 * i;
    if (blob.toString("utf8", d, d + 16).replace(/\0+$/, "") !== name) continue;
    const [type, at, , aux, , nulls] = [0, 4, 8, 12, 16, 20].map((k) => u32(d + 16 + k));
    const isNull = (r) => nulls !== 0 && ((blob[nulls + (r >> 3)] >> (r & 7)) & 1) === 0;
    const out = [];
    if (type === 7) {
      for (let r = 0; r < rows; r++) out.push(blob.toString("utf8", aux + u32(at + 4 * r), aux + u32(at + 4 * r + 4)));
    } else {
      const count = u32(aux);
      const base = aux + 8 + 4 * count;
      const words = [];
      for (let k = 0; k < count; k++) words.push(blob.toString("utf8", base + u32(aux + 4 + 4 * k), base + u32(aux + 8 + 4 * k)));
      for (let r = 0; r < rows; r++) out.push(words[type === 11 ? view.getUint16(at + 2 * r, true) : u32(at + 4 * r)] ?? "");
    }
    return out.map((s, r) => (isNull(r) ? null : s));
  }
  throw new Error(`no column ${name}`);
}

/** A number column of a v1 blob, viewed in place. */
function numbers(blob, name) {
  return column(blob, name);
}

test("engine handle on the fixture", () => {
  const p = parity.engine;
  const engine = new tg.Engine();
  assert.equal(engine.openFile(join(__dirname, "../..", p.fixture)), 0);
  const got = engine.get(p.get.id);
  assert.deepEqual(strings(got, "name"), [p.get.name]);
  assert.deepEqual(strings(got, "kind"), [p.get.kind]);
  assert.deepEqual([...numbers(got, "lat")], [p.get.lat]);
  assert.equal(strings(engine.search(p.search.query, p.search.limit), "id")[0], p.search.first_id);
  const near = engine.reverse(p.reverse.lat, p.reverse.lon, p.reverse.limit);
  assert.deepEqual(strings(near, "id"), p.reverse.ids);
  assert.ok(Math.abs(numbers(near, "meters")[0] - p.reverse.first_meters) < 0.1);
  assert.deepEqual(strings(engine.adminAt(p.admin_at.lat, p.admin_at.lon), "id"), p.admin_at.ids);
  for (const method of ["parent", "ancestors", "children"]) {
    assert.deepEqual(strings(engine[method](p[method].id), "id"), p[method].ids, method);
  }
  assert.deepEqual(strings(engine.nearby(p.nearby.id, p.nearby.limit), "id"), p.nearby.ids);
  const w = p.within_radius;
  assert.deepEqual(strings(engine.withinRadius(w.lat, w.lon, w.meters, w.limit), "id"), w.ids);
  assert.deepEqual(strings(engine.tzAt(p.tz_at.lat, p.tz_at.lon), "tz"), [p.tz_at.tz]);
  assert.deepEqual(strings(engine.byIso(p.by_iso.code), "id"), [p.by_iso.id]);
  assert.throws(() => engine.byIso(p.bad_iso), { code: "InvalidArgument" });
  assert.throws(() => engine.withinRadius(w.lat, w.lon, -1, 1), { code: "InvalidArgument" });
  const pl = p.postal_lookup;
  const hit = engine.postalLookup(pl.code, pl.scheme);
  assert.deepEqual([strings(hit, "postal"), strings(hit, "id")], [pl.postal, pl.ids]);
  const pp = p.postal_prefix;
  assert.deepEqual(strings(engine.postalPrefix(pp.input, pp.scheme, pp.limit), "postal"), pp.postal);
  const pa = engine.postalAt(p.postal_at.lat, p.postal_at.lon);
  assert.deepEqual([strings(pa, "postal"), strings(pa, "code_kind")], [p.postal_at.postal, p.postal_at.code_kind]);
  assert.deepEqual(strings(engine.postalCodesOf(p.postal_codes_of.id), "postal"), p.postal_codes_of.postal);
  assert.throws(() => engine.get(p.bad_id), { code: "InvalidArgument" });
  assert.throws(() => engine.openFile("/nonexistent.tgd"), { code: "Io" });
  assert.throws(() => engine.openBytes(Buffer.from("not a pack")), { code: "Corrupt" });
  assert.equal(engine.layers(), 1);
  const h = p.elevation_at;
  assert.equal(engine.openFile(join(__dirname, "../..", h.fixture)), 1);
  const height = engine.elevationAt(h.lat, h.lon);
  assert.deepEqual(strings(height, "source"), [h.source]);
  assert.equal(numbers(height, "meters")[0], h.meters);
  assert.equal(engine.remove(1), true);
  assert.equal(engine.remove(1), false);
  assert.equal(engine.layers(), 1);
});
