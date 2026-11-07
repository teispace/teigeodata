// The binding benchmark's Node rows (docs/02-research/benches/s4-bindings.md): best of five
// rounds, ns per call. Run from bindings/node after building teigeodata.node.
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

let sink = 0;
const row = (c, ns) => console.log(`${"node".padEnd(10)} ${c.padEnd(22)} ${ns.toFixed(1).padStart(10)} ns`);
row("noop", best(10_000_000, () => tg.noop()));
row("fold", best(1_000_000, () => { sink += tg.fold("Kathmandu").length; }));
row("sample_rows(1000)+sum", best(10_000, () => { sink += latSum(tg.sampleRows(1000)); }));
row("callback", best(1_000, () => { sink += tg.progress(1000, () => true); }) / 1000);
if (sink === 42) console.log("");
