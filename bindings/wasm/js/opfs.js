// The browser store over the Origin Private File System (D-375, D-376). Hand-written; it imports
// nothing, so every package carries it.
//
// - `opfsHost(root)`: a `StoreHost` for `Store` (`teigeodata-wasm/store`) in a dedicated worker,
//   whose files are OPFS sync access handles and whose network is `fetch`.
// - `openInstalled(engine, installedSections, root)`: opens the installed packs in any engine,
//   in a page or a worker, by section, each file held by a shared Web Lock while open.
// - `storeWorker(worker)`: calls `store-worker.js` with promises.
//
// A store is a directory of the origin's private file system. Its writer (the store worker)
// holds the Web Lock `teigeodata-store:<root>` for each call; a reader holds
// `teigeodata-store:<root>:<file>` shared for each pack file it has open, and the writer removes
// a pack file only when it can take that lock exclusively, else the next open removes it.

/** The Web Lock of the store `root`'s writer. */
export const storeLock = (root) => `teigeodata-store:${root}`;
/** The Web Lock of one pack file of the store `root`. */
export const fileLock = (root, file) => `teigeodata-store:${root}:${file}`;

const PACKS = "packs/";
const PREFIX = 64 << 10;

const notFound = (e) => e && (e.name === "NotFoundError" || e.name === "TypeMismatchError");

/** The directory `root` of the origin's private file system. */
async function rootDir(root) {
  let dir = await navigator.storage.getDirectory();
  for (const part of root.split("/").filter(Boolean)) {
    dir = await dir.getDirectoryHandle(part, { create: true });
  }
  return dir;
}

/** The directory holding `path` and the file's name, the directories made when `create`. */
async function parent(dir, path, create) {
  const parts = path.split("/").filter(Boolean);
  const name = parts.pop() ?? "";
  for (const p of parts) dir = await dir.getDirectoryHandle(p, { create });
  return [dir, name];
}

async function fileOf(dir, path) {
  try {
    const [d, name] = await parent(dir, path, false);
    return await (await d.getFileHandle(name)).getFile();
  } catch (e) {
    if (notFound(e)) return null;
    throw e;
  }
}

/**
 * A `StoreHost` over the directory `root` of the origin's private file system and `fetch`.
 * Only in a dedicated worker: sync access handles exist nowhere else. A mirror on another
 * origin must allow it (CORS) and expose `Content-Range` and `ETag`.
 */
export async function opfsHost(root, { fetch: get = globalThis.fetch.bind(globalThis) } = {}) {
  const dir = await rootDir(root);
  return {
    async open(path) {
      const [d, name] = await parent(dir, path, true);
      const h = await d.getFileHandle(name, { create: true });
      return h.createSyncAccessHandle();
    },
    async read(path) {
      const f = await fileOf(dir, path);
      return f && new Uint8Array(await f.arrayBuffer());
    },
    async len(path) {
      const f = await fileOf(dir, path);
      return f && f.size;
    },
    async rename(from, to) {
      const [fd, fname] = await parent(dir, from, false);
      const [td, tname] = await parent(dir, to, true);
      const h = await fd.getFileHandle(fname);
      try {
        await h.move(td, tname);
      } catch (e) {
        // Where `move` does not replace a file, the target goes first; a cut between the two
        // leaves the staged journal, which the store's next open commits (D-376).
        await td.removeEntry(tname).catch((r) => {
          if (!notFound(r)) throw e;
        });
        await h.move(td, tname);
      }
    },
    async remove(path) {
      const [d, name] = await parent(dir, path, false).catch((e) => {
        if (notFound(e)) return [null, ""];
        throw e;
      });
      if (!d) return;
      const drop = () =>
        d.removeEntry(name).catch((e) => {
          if (!notFound(e)) throw e;
        });
      if (!path.startsWith(PACKS)) return drop();
      // A pack file a reader holds stays until the next open (D-372).
      return navigator.locks.request(
        fileLock(root, path.slice(PACKS.length)),
        { mode: "exclusive", ifAvailable: true },
        (lock) => {
          if (!lock) throw new Error(`${path}: a reader holds it`);
          return drop();
        },
      );
    },
    async createDir(path) {
      await parent(dir, `${path}/x`, true);
    },
    syncDir() {},
    async list(path) {
      let d;
      try {
        [d] = await parent(dir, `${path}/x`, false);
      } catch (e) {
        if (notFound(e)) return [];
        throw e;
      }
      const names = [];
      for await (const [name, h] of d.entries()) if (h.kind === "file") names.push(name);
      return names;
    },
    async get(url, start, end) {
      const r = await get(url, { headers: { Range: `bytes=${start}-${end - 1}` }, cache: "no-store" });
      return {
        status: r.status,
        body: new Uint8Array(await r.arrayBuffer()),
        contentRange: r.headers.get("content-range"),
        etag: r.headers.get("etag"),
      };
    },
    async fetch(url, max) {
      const r = await get(url, { cache: "no-store" });
      if (r.status !== 200 || !r.body) return { status: r.status, body: new Uint8Array(0) };
      // Read at most one byte past `max`, so the store sees a file over its cap.
      const chunks = [];
      let n = 0;
      const reader = r.body.getReader();
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        chunks.push(value);
        n += value.length;
        if (n > max) {
          await reader.cancel();
          break;
        }
      }
      const body = new Uint8Array(n);
      let at = 0;
      for (const c of chunks) {
        body.set(c, at);
        at += c.length;
      }
      return { status: 200, body };
    },
  };
}

/** Holds the shared lock `name` until `release()`; resolves once it is held. */
function holdShared(name) {
  return new Promise((held, failed) => {
    let release;
    const until = new Promise((r) => (release = r));
    navigator.locks
      .request(name, { mode: "shared" }, () => {
        held(release);
        return until;
      })
      .catch(failed);
  });
}

/**
 * Opens the packs installed in the store `root` in `engine` (any teigeodata engine), reading
 * only the sections each one's installed components hold, and each installed delta, which the
 * engine reads above its base (D-395). `installedSections` is the package's. Each file is held
 * by a shared Web Lock until `close()`, so the store never deletes a file this engine reads.
 * Returns `{ layers, packs, close }`; `packs` lists the deltas after the packs.
 */
export async function openInstalled(engine, installedSections, root) {
  const dir = await rootDir(root);
  const journal = await fileOf(dir, "journal");
  const listed = journal ? JSON.parse(await journal.text()) : {};
  const packs = [...(listed.packs ?? []), ...(listed.deltas ?? [])];
  const releases = [];
  const layers = [];
  try {
    for (const record of packs) {
      releases.push(await holdShared(fileLock(root, record.file)));
      const file = await fileOf(dir, PACKS + record.file);
      if (!file || file.size !== record.pack_len) {
        throw Object.assign(new Error(`${record.file}: missing or cut; repair it`), { code: "Corrupt" });
      }
      let prefix = PREFIX;
      let head;
      let ranges;
      for (;;) {
        head = new Uint8Array(await file.slice(0, Math.min(prefix, file.size)).arrayBuffer());
        ranges = installedSections(head, record);
        if (ranges !== undefined || prefix >= file.size) break;
        prefix *= 2;
      }
      const offsets = new Float64Array(ranges.length / 2);
      const sections = [];
      for (let i = 0; i < ranges.length; i += 2) {
        offsets[i / 2] = ranges[i];
        sections.push(new Uint8Array(await file.slice(ranges[i], ranges[i] + ranges[i + 1]).arrayBuffer()));
      }
      layers.push(engine.openInstalled(head, record, offsets, sections));
    }
  } catch (e) {
    for (const r of releases) r();
    throw e;
  }
  return {
    layers,
    packs,
    close() {
      for (const r of releases.splice(0)) r();
    },
  };
}

/**
 * The store worker (`store-worker.js`) as an object of promises: `call(op, ...args)`, and
 * `init`, `open`, `catalog`, `status`, `refresh`, `install`, `update`, `remove`, `pin`,
 * `repair`, `packs` by name. Results are as `Store`'s; a failure rejects with its `code`.
 */
export function storeWorker(worker) {
  let next = 0;
  const waiting = new Map();
  worker.addEventListener("message", (e) => {
    const { id, ok, value, error } = e.data ?? {};
    const w = waiting.get(id);
    if (!w) return;
    waiting.delete(id);
    if (ok) w.resolve(value);
    else w.reject(Object.assign(new Error(error.message), { code: error.code }));
  });
  const call = (op, ...args) =>
    new Promise((resolve, reject) => {
      const id = next++;
      waiting.set(id, { resolve, reject });
      worker.postMessage({ id, op, args });
    });
  const ops = ["init", "open", "catalog", "status", "refresh", "install", "update", "remove", "pin", "repair", "packs"];
  return Object.assign({ call, worker }, Object.fromEntries(ops.map((op) => [op, (...a) => call(op, ...a)])));
}
