// Parity: the Node.js binding gives the results bindings/parity.json expects (checked there
// against Rust). Run with `node --test test/` after building teigeodata.node.
"use strict";
const assert = require("node:assert/strict");
const { readFileSync } = require("node:fs");
const { join } = require("node:path");
const { test } = require("node:test");
const tg = require("..");

const parity = JSON.parse(readFileSync(join(__dirname, "../../parity.json"), "utf8"));

/** A fixed-width column of a v0 blob, viewed in place (no copy). */
function column(blob, name) {
  const view = new DataView(blob.buffer, blob.byteOffset, blob.byteLength);
  assert.equal(blob.toString("latin1", 0, 4), "TGB0");
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
