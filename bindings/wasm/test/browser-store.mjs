// The browser store end to end in browsers through Playwright (D-375, D-376), from the full
// package (its `store/` subpath):
//   node browser-store.mjs <target/wasm/full> <bindings/fixtures> [chromium,firefox,webkit]
// A local server gives the package and a mirror of `fixtures/store-repo` with range requests.
// The page drives `store-worker.js` over the origin's private file system: set up, install,
// the page's reader by section, a removal while the reader holds the file (the file stays),
// the next open removing it once let go, and two workers sharing one store.
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, normalize, resolve, sep } from "node:path";
import { chromium, firefox, webkit } from "playwright";

const [pkg, fixtures] = process.argv.slice(2, 4).map((p) => resolve(p));
const wanted = (process.argv[4] ?? "chromium,firefox,webkit").split(",");
const repo = join(fixtures, "store-repo");

const page = `<!doctype html><meta charset="utf-8"><script type="module">
import * as tg from "/pkg/index.js";
import { fileLock, openInstalled, storeLock, storeWorker } from "/pkg/opfs.js";
let checks = 0;
let step = "start";
const check = (ok, what) => { step = what; if (!ok) throw new Error("store check failed: " + what); checks++; };
const rows = (blob, name) => new tg.Reader(blob).strings(name);
const worker = () => storeWorker(new Worker("/pkg/store/store-worker.js", { type: "module" }));
const fileIn = async (root, path) => {
  let d = await navigator.storage.getDirectory();
  const parts = (root + "/" + path).split("/");
  const name = parts.pop();
  try {
    for (const p of parts) d = await d.getDirectoryHandle(p);
    return await (await d.getFileHandle(name)).getFile();
  } catch { return null; }
};
try {
  await tg.init();
  const root = "teigeodata-test-" + Math.random().toString(36).slice(2);
  const w = worker();
  const changes = [];
  const channel = new BroadcastChannel(storeLock(root));
  channel.onmessage = (e) => changes.push(e.data);

  try { await w.status(false); check(false, "a call before open"); } catch (e) { check(e.code === "Refused", "a call before open: " + e.code); }
  const trusted = new Uint8Array(await (await fetch("/fixtures/store-root.json")).arrayBuffer());
  await w.init(root, [location.origin + "/repo/meta"], [location.origin + "/repo/assets"], trusted);
  const first = await w.open(root);
  check(first.damaged === "" && first.packs.length === 0, "a new store");

  const done = await w.install("np", { pin: true });
  check(rows(done, "path").join() === "np.places.tgd", "np installs");
  check(rows(await w.status(true), "health").join() === "good", "np is whole");
  const [record] = await w.packs();
  check(record.pinned && record.file.endsWith(".tgd"), "the journal's record");

  // The page's reader: by section, the same answers as the whole file.
  const engine = new tg.Engine();
  const held = await openInstalled(engine, tg.installedSections, root);
  check(held.layers.length === 1, "one layer");
  const file = await fileIn(root, "packs/" + record.file);
  const whole = new tg.Engine();
  whole.openBytes(new Uint8Array(await file.arrayBuffer()));
  const q = (e) => rows(e.search("Synthland", 3), "id").join();
  check(q(engine) !== "" && q(engine) === q(whole), "the reader answers as the file");
  whole.free();

  // Removed while the reader holds it: gone from the journal, the file stays.
  check(rows(await w.remove("np"), "path").join() === "np.places.tgd", "np is removed");
  check((await w.packs()).length === 0, "the journal is empty");
  check((await fileIn(root, "packs/" + record.file)) !== null, "a held file stays");
  check(q(engine) !== "", "the reader still answers");
  held.close();
  engine.free();
  // Let go, the next call's open removes it.
  await w.status(false);
  check((await fileIn(root, "packs/" + record.file)) === null, "the file goes once let go");

  // Two workers, one store: calls in parallel are queued by the store's lock.
  const w2 = worker();
  await w2.open(root);
  const [a, b] = await Promise.all([w.install("np"), w2.status(false)]);
  check(rows(a, "path").join() === "np.places.tgd", "an install beside another worker");
  check(rows(await w2.status(false), "path").join() === "np.places.tgd", "the other worker sees it");
  check(changes.filter((c) => c.changed).length >= 3, "changes are announced: " + changes.length);

  w.worker.terminate();
  w2.worker.terminate();
  channel.close();
  await (await navigator.storage.getDirectory()).removeEntry(root, { recursive: true });
  window.result = { ok: true, checks };
} catch (e) {
  // WebKit's stack holds no message: give the step, the code and the message as well.
  window.result = { ok: false, error: 'after "' + step + '": ' + (e && e.code ? e.code + ' ' : '') + (e && e.message ? e.message : e) + ' | ' + ((e && e.stack) || '') };
}
</script>`;

const types = { ".js": "text/javascript", ".wasm": "application/wasm", ".json": "application/json" };
const within = (root, rel) => {
  const path = normalize(join(root, rel));
  return path.startsWith(root + sep) ? path : null;
};
const server = createServer(async (req, res) => {
  const url = decodeURIComponent(new URL(req.url, "http://localhost").pathname);
  if (url === "/") return res.writeHead(200, { "content-type": "text/html" }).end(page);
  const path = url.startsWith("/pkg/")
    ? within(pkg, url.slice(5))
    : url.startsWith("/repo/")
      ? within(repo, url.slice(6))
      : url.startsWith("/fixtures/")
        ? within(fixtures, url.slice(10))
        : null;
  let body;
  try {
    body = path && (await readFile(path));
  } catch {
    body = null;
  }
  if (!body) return res.writeHead(404).end();
  const headers = {
    "content-type": types[extname(path)] ?? "application/octet-stream",
    "access-control-allow-origin": "*",
    "access-control-expose-headers": "content-range, etag",
    etag: '"fixture"',
    "cache-control": "no-store",
  };
  const range = /^bytes=(\d+)-(\d+)$/.exec(req.headers.range ?? "");
  if (range) {
    const [start, end] = [Number(range[1]), Math.min(Number(range[2]), body.length - 1)];
    if (start > end) return res.writeHead(416).end();
    headers["content-range"] = `bytes ${start}-${end}/${body.length}`;
    return res.writeHead(206, headers).end(body.subarray(start, end + 1));
  }
  res.writeHead(200, headers).end(body);
});
await new Promise((done) => server.listen(0, "127.0.0.1", done));
const origin = `http://127.0.0.1:${server.address().port}/`;

let failed = false;
try {
  for (const [browser, kind] of [["chromium", chromium], ["firefox", firefox], ["webkit", webkit]]) {
    if (!wanted.includes(browser)) continue;
    const b = await kind.launch();
    try {
      const tab = await b.newPage();
      tab.on("pageerror", (e) => console.error(`${browser}: ${e}`));
      tab.on("console", (m) => m.type() === "error" && console.error(`${browser}: ${m.text()}`));
      await tab.goto(origin);
      await tab.waitForFunction(() => window.result, null, { timeout: 120_000 });
      const r = await tab.evaluate(() => window.result);
      if (r.ok) console.log(`${browser} ${b.version()}: teigeodata-wasm/store in the browser ok (${r.checks} checks)`);
      else {
        failed = true;
        console.error(`${browser}: ${r.error}`);
      }
    } finally {
      await b.close();
    }
  }
} finally {
  server.close();
}
process.exit(failed ? 1 : 0);
