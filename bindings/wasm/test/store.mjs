// The browser store's checks (D-375) on Node.js or Bun, from the `store` variant's package:
//   node store.mjs <package dir> <bindings/fixtures>
// The host is in memory: files as byte arrays behind handles shaped like OPFS sync access
// handles, and a network that serves `fixtures/store-repo/` (a signed repository under the
// keys of `fixtures/store-root.json`). Set up, install, status, the reader (`installedSections`
// and `Engine.openInstalled`) against the whole file, one call at a time, a reopen, removal.
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const [pkg, fixtures] = process.argv.slice(2).map((p) => resolve(p));
const tg = await import(pathToFileURL(join(pkg, "node.js")).href);
const host = globalThis.Bun ? "bun" : "node";

let checks = 0;
function check(ok, what) {
  if (!ok) throw new Error(`store check failed: ${what}`);
  checks += 1;
}
async function rejects(promise, code, what) {
  try {
    await promise;
  } catch (e) {
    check(e.code === code, `${what}: ${e.code} ${e.message}`);
    return e;
  }
  throw new Error(`store check failed: ${what} did not throw`);
}
function strings(bytes, name) {
  return new tg.Reader(bytes).strings(name);
}

/** Files in memory; `waits` counts the promises the store awaited. */
function memoryHost(repo) {
  const files = new Map();
  const open = new Set();
  const stats = { waits: 0, gets: 0, bytes: 0 };
  const later = (v) => {
    stats.waits += 1;
    return Promise.resolve(v);
  };
  const parent = (p) => p.slice(0, Math.max(0, p.lastIndexOf("/")));
  const handle = (path) => ({
    read(buf, { at }) {
      const d = files.get(path);
      const n = Math.max(0, Math.min(buf.length, d.length - at));
      buf.set(d.subarray(at, at + n));
      return n;
    },
    write(buf, { at }) {
      let d = files.get(path);
      if (d.length < at + buf.length) {
        const grown = new Uint8Array(at + buf.length);
        grown.set(d);
        d = grown;
      }
      d.set(buf, at);
      files.set(path, d);
      return buf.length;
    },
    truncate(len) {
      const d = files.get(path);
      const t = new Uint8Array(len);
      t.set(d.subarray(0, Math.min(len, d.length)));
      files.set(path, t);
    },
    getSize: () => files.get(path).length,
    flush() {},
    close: () => open.delete(path),
  });
  return {
    files,
    open,
    stats,
    host: {
      open(path) {
        if (!files.has(path)) files.set(path, new Uint8Array(0));
        open.add(path);
        return later(handle(path));
      },
      read: (path) => later(files.has(path) ? files.get(path).slice() : null),
      len: (path) => later(files.has(path) ? files.get(path).length : null),
      rename(from, to) {
        if (!files.has(from)) return Promise.reject(new Error(`${from}: not found`));
        files.set(to, files.get(from));
        files.delete(from);
        return later();
      },
      remove(path) {
        files.delete(path);
        return later();
      },
      createDir: () => later(),
      syncDir: () => later(),
      list(dir) {
        const names = [...files.keys()]
          .filter((p) => parent(p) === dir)
          .map((p) => p.slice(dir.length + (dir ? 1 : 0)));
        return later(names);
      },
      async get(url, start, end) {
        const file = url.replace(/^a\//, "assets/");
        const body = readFileSync(join(repo, file)).subarray(start, end);
        const total = readFileSync(join(repo, file)).length;
        stats.gets += 1;
        stats.bytes += body.length;
        return {
          status: 206,
          body: new Uint8Array(body),
          contentRange: `bytes ${start}-${end - 1}/${total}`,
          etag: '"fixture"',
        };
      },
      async fetch(url, max) {
        const file = join(repo, url.replace(/^m\//, "meta/"));
        try {
          const body = new Uint8Array(readFileSync(file));
          return { status: 200, body: body.length > max ? body.subarray(0, max + 1) : body };
        } catch {
          return { status: 404, body: new Uint8Array(0) };
        }
      },
    },
  };
}

const repo = join(fixtures, "store-repo");
const root = new Uint8Array(readFileSync(join(fixtures, "store-root.json")));
const mem = memoryHost(repo);

// Not set up: refused. A root that does not verify: refused.
await rejects(tg.Store.open(mem.host), "Refused", "open before init");
await rejects(tg.Store.init(mem.host, ["m"], ["a"], new Uint8Array([123, 125])), "Refused", "a bad root");

await tg.Store.init(mem.host, ["m"], ["a"], root);
check(mem.files.has("config.json") && mem.files.has("root.json"), "init writes config and root");
let store = await tg.Store.open(mem.host);
check(store.damaged() === "", "a new store has nothing damaged");
check(store.packs().length === 0, "a new store has no packs");

// One call at a time: a second call while one runs is refused, the first still answers.
const refreshing = store.refresh("np");
await rejects(store.status(false), "Refused", "a second call while one runs");
const refreshed = new tg.Reader(await refreshing);
check(Number(refreshed.column("timestamp")[0]) === 1, "the refresh reads timestamp 1");

check(strings(await store.catalog(), "path").includes("np.places.tgd"), "the catalog lists np");
await rejects(store.install("np", { components: "pointy" }), "InvalidArgument", "a bad component");

const waits = mem.stats.waits;
const done = await store.install("np", { pin: true });
check(strings(done, "path").join() === "np.places.tgd", "np installs");
check(mem.stats.gets > 0 && mem.stats.waits > waits, "the install fetched and awaited");
check(mem.open.size === 0, "every file handle is closed");
check(strings(await store.status(true), "health").join() === "good", "np is whole");

// The reader: the journal's record, its sections from the file, the same answers as the file.
const [record] = store.packs();
check(record.address === "np" && record.pinned === true && record.set.includes("point"), "the record");
check(JSON.stringify(tg.parseJournal(mem.files.get("journal"))) === JSON.stringify(store.packs()), "parseJournal");
const file = mem.files.get(`packs/${record.file}`);
check(file.length === record.pack_len, "the pack file has the record's length");
check(tg.installedSections(file.subarray(0, 16), record) === undefined, "a short prefix asks for more");
const ranges = tg.installedSections(file.subarray(0, 64 << 10), record);
const offsets = new Float64Array(ranges.length / 2);
const sections = [];
for (let i = 0; i < ranges.length; i += 2) {
  offsets[i / 2] = ranges[i];
  sections.push(file.slice(ranges[i], ranges[i] + ranges[i + 1]));
}
const lazy = new tg.Engine();
lazy.openInstalled(file.subarray(0, 64 << 10), record, offsets, sections);
const whole = new tg.Engine();
whole.openBytes(file);
const q = (e) => strings(e.search("Synthland", 3), "id").join();
check(q(lazy) !== "" && q(lazy) === q(whole), `the installed sections answer as the file (${q(whole)})`);
lazy.free();
whole.free();

// Reopened, the store reads the same journal; removal empties it and deletes the file.
store.free();
store = await tg.Store.open(mem.host);
check(store.packs().length === 1, "a reopen reads the journal");
await rejects(store.remove("zz"), "Refused", "removing a pack not installed");
check(strings(await store.remove("np"), "path").join() === "np.places.tgd", "np is removed");
check(store.packs().length === 0 && !mem.files.has(`packs/${record.file}`), "its file is gone");
store.free();

console.log(`${host}: teigeodata-wasm/store ok (${checks} checks; ${mem.stats.gets} range requests, ${mem.stats.bytes} B)`);
