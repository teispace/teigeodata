// The browser store's worker (D-375, D-376): `new Worker(new URL("teigeodata-wasm/store/store-worker.js",
// import.meta.url), { type: "module" })`, called with `storeWorker(worker)` from `opfs.js`.
// Hand-written.
//
// Messages are `{ id, op, args }`; each answer is `{ id, ok, value }` or `{ id, ok: false,
// error: { code, message } }`. The first call is `open(root)` (the store's directory in the
// origin's private file system); `init(root, repo, assets, trustedRoot)` sets one up. Calls run
// one at a time, each under the store's Web Lock, on the store opened afresh, so several tabs
// share one store and each sees the others' installs. After a call that changes the packs, the
// worker posts `{ root, changed: true }` on the BroadcastChannel named by `storeLock(root)`.
import { Store, init } from "./index.js";
import { opfsHost, storeLock } from "./opfs.js";

const ready = init();
let root = null;
let host = null;
let queue = Promise.resolve();

const CHANGES = new Set(["install", "update", "remove", "repair"]);
const CALLS = new Set(["catalog", "status", "refresh", "install", "update", "remove", "pin", "repair", "packs", "damaged"]);

async function use(name) {
  if (typeof name !== "string" || name === "") {
    throw Object.assign(new Error("the store's directory must be a non-empty string"), { code: "InvalidArgument" });
  }
  await ready;
  root = name;
  host = await opfsHost(name);
}

async function run(op, args) {
  if (op === "init") {
    const [name, repo, assets, trusted] = args;
    await use(name);
    return navigator.locks.request(storeLock(root), () => Store.init(host, repo, assets, trusted));
  }
  if (op === "open") {
    await use(args[0]);
    return navigator.locks.request(storeLock(root), async () => {
      const s = await Store.open(host);
      try {
        return { damaged: s.damaged(), packs: s.packs() };
      } finally {
        s.free();
      }
    });
  }
  if (!host) throw Object.assign(new Error("open a store first"), { code: "Refused" });
  if (!CALLS.has(op)) throw Object.assign(new Error(`no store call \`${op}\``), { code: "InvalidArgument" });
  const value = await navigator.locks.request(storeLock(root), async () => {
    const s = await Store.open(host);
    try {
      return await s[op](...args);
    } finally {
      s.free();
    }
  });
  if (CHANGES.has(op)) {
    const c = new BroadcastChannel(storeLock(root));
    c.postMessage({ root, changed: true });
    c.close();
  }
  return value;
}

self.addEventListener("message", (e) => {
  const { id, op, args = [] } = e.data ?? {};
  queue = queue
    .then(() => run(op, args))
    .then(
      (value) => {
        const transfer = value instanceof Uint8Array ? [value.buffer] : [];
        self.postMessage({ id, ok: true, value }, transfer);
      },
      (err) => {
        self.postMessage({
          id,
          ok: false,
          error: { code: err?.code ?? "Internal", message: String(err?.message ?? err) },
        });
      },
    );
});
