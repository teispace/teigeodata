// The binding benchmark's Node rows (docs/02-research/benches/s4-bindings.md): best of five
// rounds, ns per call. Run from bindings/node after building teigeodata.node. With
// TEIGEODATA_BENCH_PACK naming a pack, the fifth case runs the 1,000-row query on it.
"use strict";
const tg = require(".");

function best(n, f) {
  let out = Infinity;
  for (let r = 0; r < 5; r++) {
    const t = process.hrtime.bigint();
    for (let i = 0; i < n; i++) f();
    out = Math.min(out, Number(process.hrtime.bigint() - t) / n);
  }
  return out;
}

function latSum(blob) {
  // Descriptor 1 is `lat`; offset and length at +20 and +24.
  const view = new DataView(blob.buffer, blob.byteOffset, blob.byteLength);
  const at = view.getUint32(76, true), len = view.getUint32(80, true);
  const lat = new Float64Array(blob.buffer, blob.byteOffset + at, len / 8);
  let s = 0;
  for (let i = 0; i < lat.length; i++) s += lat[i];
  return s;
}

/** The descriptor of column `name`: [type, data, data length, aux, aux length, nulls]. */
function descriptor(blob, name) {
  for (let i = 0; i < blob.readUInt32LE(8); i++) {
    const d = 16 + 40 * i;
    if (blob.toString("latin1", d, d + 16).replace(/\0+$/, "") === name) {
      return [0, 4, 8, 12, 16, 20].map((k) => blob.readUInt32LE(d + 16 + k));
    }
  }
  throw new Error(`no column ${name}`);
}

/** The 1,000-row query's decoding: every `name` as a JS string, and the sum of `lat`. */
function decode(blob) {
  const rows = blob.readUInt32LE(4);
  const [, offsets, , text] = descriptor(blob, "name");
  let total = 0;
  for (let r = 0; r < rows; r++) {
    const at = text + blob.readUInt32LE(offsets + 4 * r), end = text + blob.readUInt32LE(offsets + 4 * r + 4);
    total += blob.toString("utf8", at, end).length;
  }
  const [, lat] = descriptor(blob, "lat");
  const values = new Float64Array(blob.buffer, blob.byteOffset + lat, rows);
  for (let r = 0; r < rows; r++) total += values[r];
  return total;
}

let sink = 0;
const row = (c, ns) => console.log(`${"node".padEnd(10)} ${c.padEnd(22)} ${ns.toFixed(1).padStart(10)} ns`);
row("noop", best(10_000_000, () => tg.noop()));
row("fold", best(1_000_000, () => { sink += tg.fold("Kathmandu").length; }));
row("sample_rows(1000)+sum", best(10_000, () => { sink += latSum(tg.sampleRows(1000)); }));
row("callback", best(1_000, () => { sink += tg.progress(1000, () => true); }) / 1000);
const pack = process.env.TEIGEODATA_BENCH_PACK;
if (pack) {
  const engine = new tg.Engine();
  engine.openFile(pack);
  const query = () => engine.withinRadius(27.7, 85.3, 50_000, 1000);
  if (query().readUInt32LE(4) === 1000) row("radius(1000)+decode", best(200, () => { sink += decode(query()); }));
  else console.log(`node: ${pack}: the query does not return 1,000 rows`);
}
if (sink === 42) console.log("");
