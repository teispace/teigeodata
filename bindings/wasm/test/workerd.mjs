// Runs the WebAssembly parity checks in workerd, the Workers runtime, through Miniflare, from
// a built package:
//   node workerd.mjs <package dir> <bindings/parity.json>
// The worker imports the package's `workerd.js` entry, so the module is a compiled
// WebAssembly import as Workers give it, and the fixtures are data modules.
import { cp, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { Miniflare } from "miniflare";
import { fixtures as fixturesOf } from "./checks.mjs";

const [pkg, parityPath] = process.argv.slice(2, 4).map((p) => resolve(p));
const here = dirname(fileURLToPath(import.meta.url));
const parity = JSON.parse(await readFile(parityPath, "utf8"));
const name = JSON.parse(await readFile(join(pkg, "package.json"), "utf8")).name;
const fixtures = fixturesOf(parity);

const stage = await mkdtemp(join(tmpdir(), "tg-workerd-"));
try {
  await cp(pkg, join(stage, "pkg"), { recursive: true });
  await cp(join(here, "checks.mjs"), join(stage, "checks.mjs"));
  await cp(parityPath, join(stage, "parity.json"));
  for (const f of fixtures) await cp(join(dirname(parityPath), f), join(stage, f));
  await writeFile(
    join(stage, "worker.mjs"),
    `import * as tg from "./pkg/workerd.js";
import { run } from "./checks.mjs";
import parity from "./parity.json";
${fixtures.map((f, i) => `import f${i} from "./${f}";`).join("\n")}
const files = new Map([${fixtures.map((f, i) => `[${JSON.stringify(f)}, new Uint8Array(f${i})]`).join(", ")}]);
export default {
  async fetch() {
    try {
      await tg.init();
      const n = run(tg, JSON.parse(parity), (rel) => files.get(rel), { full: ${name === "teigeodata-wasm"} });
      return Response.json({ ok: true, n, components: tg.components() });
    } catch (e) {
      return Response.json({ ok: false, error: String((e && e.stack) || e) });
    }
  },
};
`,
  );
  const mf = new Miniflare({
    modules: true,
    modulesRoot: stage,
    scriptPath: join(stage, "worker.mjs"),
    modulesRules: [
      { type: "ESModule", include: ["**/*.js", "**/*.mjs"] },
      { type: "CompiledWasm", include: ["**/*.wasm"] },
      { type: "Data", include: ["**/*.tgd", "**/*.bin"] },
      { type: "Text", include: ["**/*.json"] },
    ],
    compatibilityDate: "2026-07-30",
  });
  try {
    const r = await (await mf.dispatchFetch("http://localhost/")).json();
    if (!r.ok) {
      console.error(`workerd: ${r.error}`);
      process.exitCode = 1;
    } else {
      console.log(`workerd: ${name} parity ok (${r.n} checks; ${r.components})`);
    }
  } finally {
    await mf.dispose();
  }
} finally {
  await rm(stage, { recursive: true, force: true });
}
