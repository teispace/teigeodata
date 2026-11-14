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
  const sw = p.search_with;
  const focus = { focusLat: sw.focus_lat, focusLon: sw.focus_lon, focusScaleM: sw.focus_scale_m };
  assert.deepEqual(strings(engine.searchWith(sw.query, sw.limit, focus), "id"), sw.ids);
  assert.notDeepEqual(strings(engine.searchWith(sw.query, sw.limit), "id"), sw.ids);
  assert.throws(() => engine.searchWith(sw.query, 1, { focusLat: 1 }), { code: "InvalidArgument" });
  const ow = p.open_with;
  const fixture = join(__dirname, "../..", p.fixture);
  const bad = readFileSync(fixture);
  bad[ow.corrupt_at] ^= 0xff;
  const scratch = new tg.Engine();
  assert.throws(() => scratch.openBytesWith(bad, { verify: "eager" }), { code: "Corrupt" });
  assert.equal(scratch.openBytesWith(bad), 0);
  assert.equal(scratch.openFileWith(fixture, { verify: "eager", load: "read", priority: 2, outlineLevel: 1 }), 1);
  assert.throws(() => scratch.openFileWith(fixture, { outlineLevel: ow.bad_outline_level }), { code: "InvalidArgument" });
  assert.throws(() => scratch.openFileWith(fixture, { verify: "sometimes" }));
  const points = new tg.Engine();
  points.openFileWith(fixture, { components: ow.components, languages: ow.languages });
  assert.equal(strings(points.get(p.get.id), "id").length, 1);
  assert.throws(() => points.search("Synthland", 1), { code: "MissingComponent" });
  assert.throws(() => points.openFileWith(fixture, { components: ow.bad_components }), { code: "InvalidArgument" });
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

test("store", () => {
  // The store (parity.json, store): the fixture root, a mirror that refuses connections.
  const { mkdtempSync, rmSync } = require("node:fs");
  const { tmpdir } = require("node:os");
  const s = parity.store;
  const root = readFileSync(join(__dirname, "../..", s.root));
  const d = mkdtempSync(join(tmpdir(), "tg-store-"));
  try {
    assert.throws(() => tg.Store.open(join(d, "missing")), { code: "Refused" });
    assert.throws(() => tg.Store.create(join(d, "bad"), s.mirror, s.mirror, Buffer.from("{}")), { code: "Refused" });
    tg.Store.create(join(d, "s"), s.mirror, s.mirror, root);
    const store = tg.Store.open(join(d, "s"));
    assert.equal(store.damaged(), "");
    assert.equal(strings(store.catalog(), "path").length, 0);
    assert.equal(strings(store.status(false), "path").length, 0);
    const engine = store.engine();
    assert.equal(engine.layers(), 0);
    assert.equal(strings(store.find(engine, s.pack, 3), "name").length, 0);
    assert.equal(strings(store.locate(engine, 27.7, 85.3, 1000), "name").length, 0);
    assert.throws(() => store.find(engine, s.pack, 10001), { code: "InvalidArgument" });
    assert.throws(() => store.locate(engine, 91, 0, 1000), { code: "InvalidArgument" });
    assert.throws(() => store.install(s.pack, { components: s.bad_components }), { code: "InvalidArgument" });
    assert.throws(() => store.remove(s.pack), { code: "Refused" });
    assert.throws(() => store.installPass(s.pack, ""), { code: "InvalidArgument" });
    assert.throws(() => store.installPass(s.pack, join(d, "dl")).result(), { code: "Network" });
    const job = store.refresh(s.pack);
    assert.throws(() => job.result(), { code: "Network" });
    assert.equal(job.isEnded(), true);
    assert.equal(job.waitProgress(job.seq(), 60000), job.seq());
    assert.equal(job.doneBytes(), job.totalBytes());
    job.cancel();
  } finally {
    rmSync(d, { recursive: true, force: true });
  }
});

test("store calls that block have Promise forms that keep the event loop free", async () => {
  // The mirror is a server in this process: a blocking call would stop it from answering.
  const http = require("node:http");
  const { mkdtempSync, rmSync } = require("node:fs");
  const { tmpdir } = require("node:os");
  const s = parity.store;
  const root = readFileSync(join(__dirname, "../..", s.root));
  let asked = 0;
  const server = http.createServer((req, res) => {
    asked++;
    setTimeout(() => res.writeHead(404).end(), 50);
  });
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  const mirror = `http://127.0.0.1:${server.address().port}/`;
  const d = mkdtempSync(join(tmpdir(), "tg-store-async-"));
  try {
    const store = await tg.Store.createAsync(join(d, "s"), mirror, mirror, root);
    assert.ok(store instanceof tg.Store);
    assert.ok((await tg.Store.openAsync(join(d, "s"))) instanceof tg.Store);
    await assert.rejects(tg.Store.openAsync(join(d, "missing")), { code: "Refused" });
    assert.equal(strings(await store.statusAsync(false), "path").length, 0);
    const engine = await store.engineAsync();
    assert.equal(engine.layers(), 0);
    assert.equal(strings(await store.findAsync(engine, s.pack, 3), "name").length, 0);
    await assert.rejects(store.findAsync(engine, s.pack, 10001), { code: "InvalidArgument" });
    const job = store.refresh(s.pack);
    await assert.rejects(job.resultAsync(), (e) => e.code === "Network" || e.code === "Refused");
    assert.ok(asked > 0, "the mirror in this process answered while the job ran");
    assert.equal(await job.waitProgressAsync(job.seq(), 60000), job.seq());
  } finally {
    server.close();
    rmSync(d, { recursive: true, force: true });
  }
});

test("a job's progress iterates, and an AbortSignal cancels it", async () => {
  // A mirror in this process that trickles a byte every 20 ms: the job runs until cancelled.
  const http = require("node:http");
  const { mkdtempSync, rmSync } = require("node:fs");
  const { tmpdir } = require("node:os");
  const s = parity.store;
  const root = readFileSync(join(__dirname, "../..", s.root));
  const server = http.createServer((req, res) => {
    res.writeHead(200, { "Content-Length": 1000 });
    let sent = 0;
    const tick = setInterval(() => {
      if (sent++ === 1000 || res.destroyed) return clearInterval(tick);
      res.write(" ");
    }, 20);
    res.on("close", () => clearInterval(tick));
  });
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  const mirror = `http://127.0.0.1:${server.address().port}/`;
  const d = mkdtempSync(join(tmpdir(), "tg-store-progress-"));
  try {
    // Progress ends with the job, at its total.
    const quick = tg.Store.create(join(d, "q"), s.mirror, s.mirror, root).refresh(s.pack);
    let last;
    for await (const p of quick.progress()) last = p;
    assert.equal(last.done, last.total);

    const job = tg.Store.create(join(d, "s"), mirror, mirror, root).refresh(s.pack);
    const abort = new AbortController();
    const result = job.resultAsync(abort.signal);
    let moves = 0;
    for await (const p of job.progress()) {
      if (++moves === 3) break;
    }
    assert.equal(moves, 3, "progress moves");
    const at = Date.now();
    abort.abort();
    await assert.rejects(result, { code: "Cancelled" });
    assert.ok(Date.now() - at < 2000, "the abort ends the job at once");
    assert.equal(job.isEnded(), true);
  } finally {
    server.closeAllConnections();
    server.close();
    rmSync(d, { recursive: true, force: true });
  }
});

test("zone rules on the rules-only fixture", () => {
  const p = parity.tz;
  const engine = new tg.Engine();
  engine.openFile(join(__dirname, "../..", p.fixture));
  assert.equal(engine.tzVersion(), p.version);
  assert.deepEqual(strings(engine.tzZones(), "zone"), p.zones);
  const o = p.offset;
  const got = engine.tzOffsetAt(o.zone, o.unix);
  assert.deepEqual([...numbers(got, "seconds")], [o.seconds]);
  assert.deepEqual(strings(got, "abbr"), [o.abbr]);
  for (const kind of ["gap", "overlap"]) {
    const c = p[kind];
    const l = engine.tzLocal(c.zone, ...c.civil);
    assert.deepEqual(strings(l, "kind"), [kind]);
    assert.deepEqual([...numbers(l, "before"), ...numbers(l, "after")], [c.before, c.after]);
  }
  assert.deepEqual([...numbers(engine.tzRulesStart(p.start.zone), "unix")], [BigInt(p.start.unix)]);
  assert.equal(strings(engine.tzOffsetAt(p.unknown, 0), "abbr").length, 0);
  assert.throws(() => engine.tzLocal(o.zone, 2024, 13, 1, 0, 0, 0), { code: "InvalidArgument" });
});
