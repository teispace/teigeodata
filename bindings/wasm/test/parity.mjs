// Runs the WebAssembly parity checks on Node.js, Bun or Deno from a built package:
//   node parity.mjs <package dir> <bindings/parity.json>
// Node.js and Bun load the package's `node.js` entry (synchronous); Deno its default entry
// with `await init()`, which fetches the module from a `file:` URL as a browser would.
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { run } from "./checks.mjs";

const [pkg, parityPath] = (globalThis.Deno ? Deno.args : process.argv.slice(2)).map((p) => resolve(p));
const host = globalThis.Deno ? "deno" : globalThis.Bun ? "bun" : "node";
const entry = host === "deno" ? "index.js" : "node.js";
const tg = await import(pathToFileURL(join(pkg, entry)).href);
await tg.init();
const parity = JSON.parse(readFileSync(parityPath, "utf8"));
const read = (rel) => new Uint8Array(readFileSync(join(dirname(parityPath), rel)));
const name = JSON.parse(readFileSync(join(pkg, "package.json"), "utf8")).name;
const n = run(tg, parity, read, { full: name === "@teispace/teigeodata-wasm" });
console.log(`${host}: ${name} parity ok (${n} checks; ${tg.components()})`);
