// Parity for the WebAssembly module: the results bindings/parity.json expects (checked there
// against Rust), on any JavaScript host. `run` uses no host API: the runners for Node.js, Bun
// and Deno (parity.mjs), a browser and Workers load the module and the files and call it.
// A variant built without a component (D-291) must answer its checks as a layer installed
// without it: `MissingComponent`, or no rows for postal codes.

/** Runs every check; returns the number run, or throws the first failure. */
export function run(tg, parity, read, { full }) {
  let n = 0;
  const fail = (what, got, want) => {
    throw new Error(`${what}: got ${JSON.stringify(got)}, want ${JSON.stringify(want)}`);
  };
  const eq = (what, got, want) => {
    n++;
    if (JSON.stringify(got) !== JSON.stringify(want)) fail(what, got, want);
  };
  const ok = (what, cond) => eq(what, Boolean(cond), true);
  const throws = (what, f, code) => {
    n++;
    try {
      f();
    } catch (e) {
      if (code !== undefined && e.code !== code) fail(`${what} code`, e.code, code);
      return e;
    }
    fail(what, "no error", code ?? "an error");
  };
  const built = new Set(tg.components().split(", "));
  /** `component` built: run `check`; otherwise `query` must fail with MissingComponent. */
  const needs = (component, what, query, check) => {
    if (built.has(component)) check(query());
    else throws(`${what} without ${component}`, query, "MissingComponent");
  };

  eq("version", tg.version(), parity.version);
  if (full) eq("components", tg.components(), parity.components);
  ok("components start with point", tg.components().startsWith("point"));
  ok("every component is known", [...built].every((c) => parity.components.split(", ").includes(c)));
  eq("noop", tg.noop(), undefined);
  for (const [text, folded] of parity.fold) {
    needs("search", `fold ${text}`, () => tg.fold(text), (got) => eq(`fold ${text}`, got, folded));
  }

  const s = parity.sample_rows;
  const blob = tg.sampleRows(s.rows);
  ok("sampleRows is a Uint8Array", blob instanceof Uint8Array);
  eq("sampleRows bytes", blob.byteLength, s.bytes);
  eq("sampleRows aligned", blob.byteOffset % 8, 0);
  eq("lat sum", sum(column(blob, "lat")), s.lat_sum);
  eq("elevation sum", sum(column(blob, "elevation")), s.elevation_sum);
  eq("SAMPLE_ROWS_MAX", tg.SAMPLE_ROWS_MAX, parity.sample_rows_max);
  const d = parity.distance_m;
  ok("distanceM", Math.abs(tg.distanceM(...d.args) - d.metres) < 1e-6);
  throws("distanceM out of range", () => tg.distanceM(...d.out_of_range), "InvalidArgument");
  const tooMany = throws("sampleRows too many", () => tg.sampleRows(tg.SAMPLE_ROWS_MAX + 1), "InvalidArgument");
  ok("message", /at most/.test(tooMany.message));
  ok("a thrown error is an Error", tooMany instanceof Error);

  const seen = [];
  eq("progress", tg.progress(3, (step) => (seen.push(step), true)), 3);
  eq("progress steps", seen, [1, 2, 3]);
  const g = parity.progress;
  const cancelled = throws("progress cancelled", () => tg.progress(g.steps, (step) => step < g.cancel_after), "Cancelled");
  eq("cancel message", cancelled.message, g.cancelled_message);
  const boom = new RangeError("boom");
  const rethrown = throws("progress rethrows", () => tg.progress(5, () => { throw boom; }));
  ok("the callback's exception, unchanged", rethrown === boom);

  const p = parity.engine;
  const fixture = read(p.fixture);
  const engine = new tg.Engine();
  eq("openBytes", engine.openBytes(fixture), 0);
  const got = engine.get(p.get.id);
  eq("get id", strings(got, "id"), [p.get.id]);
  eq("get name", strings(got, "name"), [built.has("names") ? p.get.name : null]);
  eq("get kind", strings(got, "kind"), [p.get.kind]);
  eq("get lat", [...column(got, "lat")], [p.get.lat]);
  needs("search", "search", () => engine.search(p.search.query, p.search.limit),
    (r) => eq("search", strings(r, "id")[0], p.search.first_id));
  const near = engine.reverse(p.reverse.lat, p.reverse.lon, p.reverse.limit);
  eq("reverse", strings(near, "id"), p.reverse.ids);
  ok("reverse meters", Math.abs(column(near, "meters")[0] - p.reverse.first_meters) < 0.1);
  needs("admin-index", "adminAt", () => engine.adminAt(p.admin_at.lat, p.admin_at.lon),
    (r) => eq("adminAt", strings(r, "id"), p.admin_at.ids));
  for (const m of ["parent", "ancestors", "children"]) {
    needs("admin", m, () => engine[m](p[m].id), (r) => eq(m, strings(r, "id"), p[m].ids));
  }
  eq("nearby", strings(engine.nearby(p.nearby.id, p.nearby.limit), "id"), p.nearby.ids);
  const w = p.within_radius;
  eq("withinRadius", strings(engine.withinRadius(w.lat, w.lon, w.meters, w.limit), "id"), w.ids);
  throws("withinRadius negative", () => engine.withinRadius(w.lat, w.lon, -1, 1), "InvalidArgument");
  needs("timezone-index", "tzAt", () => engine.tzAt(p.tz_at.lat, p.tz_at.lon),
    (r) => eq("tzAt", strings(r, "tz"), [p.tz_at.tz]));
  needs("admin", "byIso", () => engine.byIso(p.by_iso.code), (r) => eq("byIso", strings(r, "id"), [p.by_iso.id]));
  throws("byIso bad", () => engine.byIso(p.bad_iso), "InvalidArgument");
  const sw = p.search_with;
  const focus = { focusLat: sw.focus_lat, focusLon: sw.focus_lon, focusScaleM: sw.focus_scale_m };
  needs("search", "searchWith", () => engine.searchWith(sw.query, sw.limit, focus), (r) => {
    eq("searchWith focus", strings(r, "id"), sw.ids);
    ok("searchWith without focus differs",
      JSON.stringify(strings(engine.searchWith(sw.query, sw.limit), "id")) !== JSON.stringify(sw.ids));
  });
  throws("searchWith half a focus", () => engine.searchWith(sw.query, 1, { focusLat: 1 }), "InvalidArgument");
  throws("searchWith a bad option type", () => engine.searchWith(sw.query, 1, { strict: "yes" }), "InvalidArgument");

  const ow = p.open_with;
  const bad = fixture.slice();
  bad[ow.corrupt_at] ^= 0xff;
  const scratch = new tg.Engine();
  throws("openBytesWith eager on a corrupt pack", () => scratch.openBytesWith(bad, { verify: "eager" }), "Corrupt");
  eq("openBytesWith lazy", scratch.openBytesWith(bad), 0);
  eq("openBytesWith options", scratch.openBytesWith(fixture, { verify: "eager", priority: 2, outlineLevel: 1 }), 1);
  throws("outline level", () => scratch.openBytesWith(fixture, { outlineLevel: ow.bad_outline_level }), "InvalidArgument");
  throws("verify", () => scratch.openBytesWith(fixture, { verify: "sometimes" }), "InvalidArgument");
  throws("a fraction", () => scratch.openBytesWith(fixture, { priority: 1.5 }), "InvalidArgument");
  const points = new tg.Engine();
  points.openBytesWith(fixture, { components: ow.components, languages: ow.languages });
  eq("points get", strings(points.get(p.get.id), "id").length, 1);
  throws("points search", () => points.search("Synthland", 1), "MissingComponent");
  throws("bad components", () => points.openBytesWith(fixture, { components: ow.bad_components }), "InvalidArgument");
  points.free();

  const postal = (what, query, check) => {
    if (built.has("postal")) check(query());
    else eq(`${what} without postal`, strings(query(), "postal").length, 0);
  };
  const pl = p.postal_lookup;
  postal("postalLookup", () => engine.postalLookup(pl.code, pl.scheme),
    (r) => eq("postalLookup", [strings(r, "postal"), strings(r, "id")], [pl.postal, pl.ids]));
  const pp = p.postal_prefix;
  postal("postalPrefix", () => engine.postalPrefix(pp.input, pp.scheme, pp.limit),
    (r) => eq("postalPrefix", strings(r, "postal"), pp.postal));
  postal("postalAt", () => engine.postalAt(p.postal_at.lat, p.postal_at.lon),
    (r) => eq("postalAt", [strings(r, "postal"), strings(r, "code_kind")], [p.postal_at.postal, p.postal_at.code_kind]));
  postal("postalCodesOf", () => engine.postalCodesOf(p.postal_codes_of.id),
    (r) => eq("postalCodesOf", strings(r, "postal"), p.postal_codes_of.postal));
  throws("get bad id", () => engine.get(p.bad_id), "InvalidArgument");
  throws("openBytes not a pack", () => engine.openBytes(new TextEncoder().encode("not a pack")), "Corrupt");
  eq("layers", engine.layers(), 1);
  const h = p.elevation_at;
  eq("open the DEM fixture", engine.openBytes(read(h.fixture)), 1);
  needs("dem", "elevationAt", () => engine.elevationAt(h.lat, h.lon), (r) => {
    eq("elevationAt source", strings(r, "source"), [h.source]);
    eq("elevationAt meters", column(r, "meters")[0], h.meters);
  });
  eq("remove", engine.remove(1), true);
  eq("remove again", engine.remove(1), false);
  eq("layers after remove", engine.layers(), 1);
  engine.free();

  const t = parity.tz;
  const zones = new tg.Engine();
  zones.openBytes(read(t.fixture));
  if (built.has("tz-rules")) {
    eq("tzVersion", zones.tzVersion(), t.version);
    eq("tzZones", strings(zones.tzZones(), "zone"), t.zones);
    const o = t.offset;
    const off = zones.tzOffsetAt(o.zone, o.unix);
    eq("tzOffsetAt seconds", [...column(off, "seconds")], [o.seconds]);
    eq("tzOffsetAt abbr", strings(off, "abbr"), [o.abbr]);
    for (const kind of ["gap", "overlap"]) {
      const c = t[kind];
      const l = zones.tzLocal(c.zone, ...c.civil);
      eq(`tzLocal ${kind}`, strings(l, "kind"), [kind]);
      eq(`tzLocal ${kind} offsets`, [...column(l, "before"), ...column(l, "after")], [c.before, c.after]);
    }
    eq("tzRulesStart", [...column(zones.tzRulesStart(t.start.zone), "unix")].map(String), [String(t.start.unix)]);
    eq("tzOffsetAt unknown", strings(zones.tzOffsetAt(t.unknown, 0), "abbr").length, 0);
    throws("tzLocal month 13", () => zones.tzLocal(t.offset.zone, 2024, 13, 1, 0, 0, 0), "InvalidArgument");
  } else {
    eq("tzVersion without tz-rules", zones.tzVersion(), "");
    throws("tzZones without tz-rules", () => zones.tzZones(), "MissingComponent");
  }
  zones.free();
  return n;
}

function sum(xs) {
  let s = 0;
  for (const x of xs) s += x;
  return s;
}

const utf8 = new TextDecoder();
const text = (blob, from, to) => utf8.decode(blob.subarray(from, to));

/** A fixed-width column of a v1 blob, viewed in place (no copy). */
function column(blob, name) {
  const view = new DataView(blob.buffer, blob.byteOffset, blob.byteLength);
  if (text(blob, 0, 4) !== "TGB1") throw new Error("not a v1 blob");
  if (view.getUint32(12, true) !== blob.byteLength) throw new Error("blob length");
  for (let i = 0; i < view.getUint32(8, true); i++) {
    const d = 16 + 40 * i;
    if (text(blob, d, d + 16).replace(/\0+$/, "") !== name) continue;
    const [type, at, len] = [0, 4, 8].map((k) => view.getUint32(d + 16 + k, true));
    const Kind = { 1: Int32Array, 2: Uint32Array, 3: BigInt64Array, 4: BigUint64Array, 5: Float32Array, 6: Float64Array, 8: Uint8Array, 9: Int16Array, 10: Uint16Array }[type];
    return new Kind(blob.buffer, blob.byteOffset + at, len / Kind.BYTES_PER_ELEMENT);
  }
  throw new Error(`no column ${name}`);
}

/** A string column (UTF-8 or a dictionary) of a v1 blob, null rows as null. */
function strings(blob, name) {
  const view = new DataView(blob.buffer, blob.byteOffset, blob.byteLength);
  const u32 = (at) => view.getUint32(at, true);
  const rows = u32(4);
  for (let i = 0; i < u32(8); i++) {
    const d = 16 + 40 * i;
    if (text(blob, d, d + 16).replace(/\0+$/, "") !== name) continue;
    const [type, at, , aux, , nulls] = [0, 4, 8, 12, 16, 20].map((k) => u32(d + 16 + k));
    const isNull = (r) => nulls !== 0 && ((blob[nulls + (r >> 3)] >> (r & 7)) & 1) === 0;
    const out = [];
    if (type === 7) {
      for (let r = 0; r < rows; r++) out.push(text(blob, aux + u32(at + 4 * r), aux + u32(at + 4 * r + 4)));
    } else {
      const count = u32(aux);
      const base = aux + 8 + 4 * count;
      const words = [];
      for (let k = 0; k < count; k++) words.push(text(blob, base + u32(aux + 4 + 4 * k), base + u32(aux + 8 + 4 * k)));
      for (let r = 0; r < rows; r++) out.push(words[type === 11 ? view.getUint16(at + 2 * r, true) : u32(at + 4 * r)] ?? "");
    }
    return out.map((s, r) => (isNull(r) ? null : s));
  }
  throw new Error(`no column ${name}`);
}
