// The Node.js packages as npm installs them (D-294): from the packages `cargo xtask
// package-node` assembled, the main package and every platform package whose `os`, `cpu` and
// `libc` match this machine go into a fresh node_modules, each with only the files its
// package.json lists. Exactly one platform package must match; then the parity test runs
// against the installed package, so the loader, the names and the files are what is tested.
//
// Usage: node bindings/node/test/install.mjs <packages-dir>
import { cpSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const { target } = createRequire(import.meta.url)("../target.js");
const [packages] = process.argv.slice(2);
if (!packages) {
  console.error("usage: node install.mjs <packages-dir>");
  process.exit(2);
}

const libc = target()?.endsWith("-musl") ? "musl" : "glibc";
const allows = (list, value) => !list || list.includes(value);
const matches = (m) =>
  allows(m.os, process.platform) &&
  allows(m.cpu, process.arch) &&
  (process.platform !== "linux" || allows(m.libc, libc));

const root = mkdtempSync(join(tmpdir(), "teigeodata-install-"));
let main = null;
const platform = [];
try {
  for (const dir of readdirSync(packages)) {
    const pkgDir = join(packages, dir);
    const manifest = join(pkgDir, "package.json");
    if (!existsSync(manifest)) continue;
    const m = JSON.parse(readFileSync(manifest, "utf8"));
    const isMain = !m.os;
    if (!isMain && !matches(m)) continue;
    const dest = join(root, "node_modules", m.name);
    mkdirSync(dest, { recursive: true });
    cpSync(manifest, join(dest, "package.json"));
    for (const f of m.files) {
      if (!existsSync(join(pkgDir, f))) throw new Error(`${m.name} lists ${f}, which it lacks`);
      cpSync(join(pkgDir, f), join(dest, f));
    }
    if (isMain) main = dest;
    else platform.push(m.name);
  }
  if (!main) throw new Error(`no main package in ${packages}`);
  if (platform.length !== 1) {
    throw new Error(`${platform.length} platform packages match this machine: ${platform}`);
  }
  console.log(`installed the main package and ${platform[0]}`);
  const env = { ...process.env, TEIGEODATA_NODE_PACKAGE: main };
  delete env.TEIGEODATA_NODE_ADDON;
  const run = spawnSync(process.execPath, ["--test", join(here, "parity.test.js")], {
    env,
    stdio: "inherit",
  });
  if (run.status !== 0) process.exitCode = 1;
  // The ES module entry names the same exports as the CommonJS one.
  const esm = `import tg, * as named from ${JSON.stringify(pathToFileURL(join(main, "index.mjs")).href)};
    const missing = Object.keys(tg).filter((k) => named[k] !== tg[k]);
    if (missing.length) throw new Error("index.mjs lacks " + missing);
    console.log("index.mjs: " + Object.keys(tg).length + " exports, version " + named.version());`;
  const esmRun = spawnSync(process.execPath, ["--input-type=module", "-e", esm], { env, stdio: "inherit" });
  if (esmRun.status !== 0) process.exitCode = 1;
} finally {
  rmSync(root, { recursive: true, force: true });
}
