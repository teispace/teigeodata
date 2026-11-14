// The entry for Node.js, Bun and Deno's npm resolution (the `node` condition): the module is
// read from the file beside this one and loaded at import, as these hosts cannot fetch a
// `file:` URL (Node.js) or need not wait. Hand-written; the API is index.js's.
import { readFileSync } from "node:fs";
import { initSync } from "./teigeodata_wasm.js";

initSync({ module: readFileSync(new URL("./teigeodata_wasm_bg.wasm", import.meta.url)) });

export * from "./index.js";
