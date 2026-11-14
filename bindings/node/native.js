// Loads the teigeodata addon (D-294). In order: the file `TEIGEODATA_NODE_ADDON` names, a
// `teigeodata.node` beside this file (a local build), then this machine's platform package,
// `@teispace/teigeodata-<suffix>` (napi-rs layout), which must be this package's version.
"use strict";

const { target } = require("./target.js");

function load() {
  const errors = [];
  const tryLoad = (what) => {
    try {
      return require(what);
    } catch (e) {
      if (e && e.code !== "MODULE_NOT_FOUND") throw e;
      errors.push(`${what}: ${e && e.message}`);
      return null;
    }
  };
  if (process.env.TEIGEODATA_NODE_ADDON) return require(process.env.TEIGEODATA_NODE_ADDON);
  const local = tryLoad("./teigeodata.node");
  if (local) return local;

  const suffix = target();
  if (!suffix) {
    throw new Error(
      `teigeodata has no addon for ${process.platform}-${process.arch}; ` +
        "@teispace/teigeodata-wasm runs everywhere",
    );
  }
  const name = `@teispace/teigeodata-${suffix}`;
  const addon = tryLoad(name);
  if (!addon) {
    throw new Error(
      `teigeodata: ${name} is not installed (an install that left out optional ` +
        `dependencies?). Reinstall with them.\n  ${errors.join("\n  ")}`,
    );
  }
  const want = require("./package.json").version;
  const got = require(`${name}/package.json`).version;
  if (got !== want) {
    throw new Error(`teigeodata ${want} found ${name} ${got}: reinstall so the versions match`);
  }
  return addon;
}

module.exports = load();
