// The entry for Cloudflare Workers (the `workerd` condition): Workers import WebAssembly as a
// compiled module and cannot fetch their own files, so the module is loaded at import.
// Hand-written; the API is index.js's.
import module from "./teigeodata_wasm_bg.wasm";
import { initSync } from "./teigeodata_wasm.js";

initSync({ module });

export * from "./index.js";
