// The platform package this machine needs (D-294): `@teispace/teigeodata-<suffix>`, in the
// napi-rs layout. Used by the loader (native.js) and the install test.
"use strict";

const SUFFIXES = {
  "darwin-arm64": "darwin-arm64",
  "darwin-x64": "darwin-x64",
  "linux-arm64": "linux-arm64-{libc}",
  "linux-x64": "linux-x64-{libc}",
  "win32-arm64": "win32-arm64-msvc",
  "win32-x64": "win32-x64-msvc",
};

/** Whether the C library is musl (Alpine) rather than glibc. */
function isMusl() {
  try {
    // musl's `ldd` is its loader, whose bytes name musl; glibc's is a shell script.
    return require("node:fs").readFileSync("/usr/bin/ldd", "latin1").includes("musl");
  } catch {
    // No readable ldd: ask Node.js, which names glibc's version when it runs on glibc.
  }
  const report = process.report && process.report.getReport();
  return !(report && report.header && report.header.glibcVersionRuntime);
}

/** The platform package suffix for this machine, or `null` when none is built for it. */
function target(platform = process.platform, arch = process.arch) {
  const pattern = SUFFIXES[`${platform}-${arch}`];
  if (!pattern) return null;
  return pattern.replace("{libc}", () => (isMusl() ? "musl" : "gnu"));
}

module.exports = { target, SUFFIXES };
