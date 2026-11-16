// Runs the WebAssembly parity checks in browsers through Playwright, from a built package:
//   node browser.mjs <package dir> <bindings/parity.json> [chromium,firefox,webkit]
// A local server gives the package, the checks, parity.json and the fixtures; the page loads
// the default entry and `await init()`s it, as a site would.
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { dirname, extname, join, normalize, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium, firefox, webkit } from "playwright";

const [pkg, parityPath] = process.argv.slice(2, 4).map((p) => resolve(p));
const wanted = (process.argv[4] ?? "chromium,firefox,webkit").split(",");
const here = dirname(fileURLToPath(import.meta.url));
const bindings = dirname(parityPath);
const name = JSON.parse(await readFile(join(pkg, "package.json"), "utf8")).name;
const full = name === "teigeodata-wasm";

const page = `<!doctype html><meta charset="utf-8"><script type="module">
import * as tg from "/pkg/index.js";
import { fixtures, run } from "/checks.mjs";
try {
  await tg.init();
  const parity = await (await fetch("/parity.json")).json();
  const files = new Map();
  for (const rel of fixtures(parity)) {
    files.set(rel, new Uint8Array(await (await fetch("/" + rel)).arrayBuffer()));
  }
  const n = run(tg, parity, (rel) => files.get(rel), { full: ${full} });
  window.result = { ok: true, n, components: tg.components() };
} catch (e) {
  window.result = { ok: false, error: String((e && e.stack) || e) };
}
</script>`;

const types = { ".js": "text/javascript", ".mjs": "text/javascript", ".wasm": "application/wasm", ".json": "application/json" };
/** The file a path names, only inside `root`. */
const within = (root, rel) => {
  const path = normalize(join(root, rel));
  return path.startsWith(root + sep) ? path : null;
};
const server = createServer(async (req, res) => {
  const url = decodeURIComponent(new URL(req.url, "http://localhost").pathname);
  const path =
    url === "/" ? null
    : url === "/checks.mjs" ? join(here, "checks.mjs")
    : url === "/parity.json" ? parityPath
    : url.startsWith("/pkg/") ? within(pkg, url.slice(5))
    : url.startsWith("/fixtures/") ? within(join(bindings, "fixtures"), url.slice(10))
    : undefined;
  try {
    if (path === null) return res.writeHead(200, { "content-type": "text/html" }).end(page);
    if (path === undefined) return res.writeHead(404).end();
    const body = await readFile(path);
    res.writeHead(200, { "content-type": types[extname(path)] ?? "application/octet-stream" }).end(body);
  } catch {
    res.writeHead(404).end();
  }
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
      await tab.goto(origin);
      await tab.waitForFunction(() => window.result, null, { timeout: 60_000 });
      const r = await tab.evaluate(() => window.result);
      if (r.ok) console.log(`${browser} ${b.version()}: ${name} parity ok (${r.n} checks; ${r.components})`);
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
