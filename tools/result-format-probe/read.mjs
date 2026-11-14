import { readFileSync } from "node:fs";
const blob = readFileSync(process.argv[2]); const json = readFileSync(process.argv[3], "utf8");
const td = new TextDecoder();
function cols(b) {
  const v = new DataView(b.buffer, b.byteOffset, b.byteLength); const n = v.getUint32(8, true), rows = v.getUint32(4, true); const out = {};
  for (let i = 0; i < n; i++) { const d = 16 + 40 * i; const name = b.toString("latin1", d, d + 16).replace(/\0+$/, "");
    const [t, at, len, sat] = [0, 4, 8, 12].map((k) => v.getUint32(d + 16 + k, true));
    if (t === 7) out[name] = { off: new Uint32Array(b.buffer, b.byteOffset + at, rows + 1), bytes: b.subarray(sat) };
    else { const K = { 1: Int32Array, 2: Uint32Array, 6: Float64Array }[t]; out[name] = new K(b.buffer, b.byteOffset + at, len / K.BYTES_PER_ELEMENT); } }
  return [rows, out];
}
function readBlob() { const [rows, c] = cols(blob); let co = 0, el = 0, nu = 0, by = 0, ints = 0;
  for (let i = 0; i < rows; i++) { co += c.lat[i] + c.lon[i]; const e = c.elevation[i]; if (e === -2147483648) nu++; else el += e; ints += c.kind[i] + c.level[i]; }
  for (const k of ["id", "name", "parent", "tz", "iso"]) { const { off, bytes } = c[k];
    for (let i = 0; i < rows; i++) { const s = td.decode(bytes.subarray(off[i] - off[0], off[i + 1] - off[0])); by += s.length; if (!s && k !== "id" && k !== "name") nu++; } }
  return [co, el, nu, by, ints]; }
function readJson() { const rows = JSON.parse(json); let co = 0, el = 0, nu = 0, by = 0;
  for (const r of rows) { co += r.lat + r.lon; if (r.elevation === null) nu++; else el += r.elevation; for (const k of ["id","name","parent","timezone","iso_code"]) { const s = r[k]; if (s == null) { if (k !== "id" && k !== "name") nu++; } else by += s.length; } }
  return [co, el, nu, by]; }
function med(f) { const t = []; for (let i = 0; i < 61; i++) { const s = process.hrtime.bigint(); f(); t.push(Number(process.hrtime.bigint() - s) / 1e3); } t.sort((a, b) => a - b); return t[30]; }
console.log("blob", readBlob(), "json", readJson());
console.log(`node blob read-all ${med(readBlob).toFixed(0)} µs; strings left as views (numbers only) ${med(() => { const [r, c] = cols(blob); let x = 0; for (let i = 0; i < r; i++) x += c.lat[i] + c.lon[i] + c.elevation[i]; return x; }).toFixed(0)} µs; JSON.parse+read ${med(readJson).toFixed(0)} µs (${json.length} B)`);
function readBlob2(mode) { const [rows, c] = cols(blob); let by = 0;
  for (const k of ["id", "name", "parent", "tz", "iso"]) { const { off, bytes } = c[k]; const o0 = off[0];
    if (mode === 0) for (let i = 0; i < rows; i++) by += bytes.utf8Slice(off[i] - o0, off[i + 1] - o0).length;
    else { const all = bytes.utf8Slice(0, off[rows] - o0); let ascii = all.length === off[rows] - o0;
      for (let i = 0; i < rows; i++) by += ascii ? all.slice(off[i] - o0, off[i + 1] - o0).length : bytes.utf8Slice(off[i] - o0, off[i + 1] - o0).length; } }
  return by; }
console.log(`utf8Slice per string ${med(() => readBlob2(0)).toFixed(0)} µs; one decode per column + slice when ASCII ${med(() => readBlob2(1)).toFixed(0)} µs`, readBlob2(0), readBlob2(1));
