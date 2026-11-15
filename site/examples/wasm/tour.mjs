// The tour: every query of the guide, run on a released pack.
// The WebAssembly build, here on Node.js; in a browser the pack's bytes come from `fetch`.
// Usage: node tour.mjs world-np-lite.tgd

import { readFileSync } from "node:fs";
// #region import
import { init, Engine, Reader, distanceM } from "@teispace/teigeodata-wasm";

await init(); // loads the module once; at once on Node.js, Bun and Workers
// #endregion import

const pack = process.argv[2];
const fixed = (x, digits) => x.toFixed(digits);

console.log("=== open");
// #region open
const engine = new Engine();
engine.openBytes(readFileSync(pack)); // in a browser: new Uint8Array(await (await fetch(url)).arrayBuffer())
console.log("layers:", engine.layers());
// #endregion open

console.log("=== search");
// #region search
for (const p of new Reader(engine.search("Pokhara", 3))) {
  console.log(`${p.id}  ${p.name}  ${p.kind}  ${fixed(p.lat, 4)}, ${fixed(p.lon, 4)}  score ${p.score}`);
}
// #endregion search

console.log("=== search-parent");
// #region search-parent
for (const p of new Reader(engine.search("Kathmandu, Bagmati", 2))) {
  console.log(`${p.id}  ${p.name}  ${p.kind} ${p.level}`);
}
// #endregion search-parent

console.log("=== search-focus");
// #region search-focus
// Many places share a name; a focus (the user's location) ranks the near ones first.
const options = { focusLat: 28.7, focusLon: 80.6, focusScaleM: 30_000 };
for (const p of new Reader(engine.searchWith("Shivapur", 3, options))) {
  console.log(`${p.id}  ${p.name}  ${fixed(p.lat, 4)}, ${fixed(p.lon, 4)}`);
}
// #endregion search-focus

console.log("=== get");
// #region get
const [place] = new Reader(engine.get("tgd:np/1xhb")).records();
console.log(place.name, place.kind);
console.log(`at ${fixed(place.lat, 6)}, ${fixed(place.lon, 6)}, ${place.elevation} m`);
console.log("timezone", place.tz, "parent", place.parent);
// #endregion get

console.log("=== hierarchy");
// #region hierarchy
for (const p of new Reader(engine.ancestors("tgd:np/1xhb"))) {
  console.log(`${p.kind} ${p.level}  ${p.name}  ${p.iso ?? ""}`.trimEnd());
}
const children = new Reader(engine.children("tgd:np/295c"));
console.log("Kaski has", children.rows, "children");
// #endregion hierarchy

console.log("=== reverse");
// #region reverse
for (const p of new Reader(engine.reverse(27.7172, 85.324, 3))) {
  console.log(`${p.name}  ${fixed(p.meters, 0)} m at ${fixed(p.bearing, 0)}°`);
}
// #endregion reverse

console.log("=== radius");
// #region radius
const within = new Reader(engine.withinRadius(27.7172, 85.324, 500, 10));
const meters = within.numbers("meters");
console.log(within.rows, "places within 500 m; farthest", `${fixed(meters[within.rows - 1], 0)} m`);
// #endregion radius

console.log("=== nearby");
// #region nearby
for (const p of new Reader(engine.nearby("tgd:np/2cn", 3))) {
  console.log(`${p.name}  ${fixed(p.meters, 0)} m`);
}
// #endregion nearby

console.log("=== admin");
// #region admin
for (const p of new Reader(engine.adminAt(28.2096, 83.9856))) {
  console.log(`${p.level}  ${p.name}  ${p.iso ?? ""}`.trimEnd());
}
const [state] = new Reader(engine.byIso("NP-P4")).records();
console.log("NP-P4 is", state.name, state.id);
// #endregion admin

console.log("=== timezone");
// #region timezone
const [area] = new Reader(engine.tzAt(27.7172, 85.324)).records();
const zone = area.tz;
const [offset] = new Reader(engine.tzOffsetAt(zone, 1_700_000_000)).records();
console.log(zone, offset.seconds, "s east of UTC,", offset.abbr, "dst", offset.dst);
const [local] = new Reader(engine.tzLocal("America/New_York", 2026, 3, 8, 2, 30, 0)).records();
console.log("02:30 on 2026-03-08 in New York:", local.kind, local.before, "->", local.after);
console.log("tzdb", engine.tzVersion());
// #endregion timezone

console.log("=== elevation");
// #region elevation
const [height] = new Reader(engine.elevationAt(27.7172, 85.324)).records();
console.log(
  `${fixed(height.meters, 1)} m above ${height.datum} (${height.source}, ${fixed(height.resolution_m, 0)} m grid)`,
);
// #endregion elevation

console.log("=== distance");
// #region distance
const km = distanceM(27.7172, 85.324, 28.2096, 83.9856) / 1000;
console.log(`Kathmandu to Pokhara: ${fixed(km, 1)} km`);
// #endregion distance

console.log("=== errors");
// #region errors
try {
  engine.reverse(91, 0, 1);
} catch (error) {
  if (error.code !== "InvalidArgument") throw error;
  console.log("invalid argument:", error.message);
}
// #endregion errors
