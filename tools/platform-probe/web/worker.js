// The browser prototype's worker (D-184): downloads the byte ranges of a
// `tgd install --downloads` listing with ranged `fetch` into the origin-private file system,
// writing each chunk at its offset through a FileSystemSyncAccessHandle (workers only).
//
// A range is written to `dl/<asset>/<start>-<end>.part` and renamed when whole. A browser has
// no background download: if the page or the browser dies, the next session resumes each
// `.part` from its size with a narrower Range. Each response is held to its request, as in the
// other prototypes: 206 and exactly the `Content-Range` asked for.

async function fetchRange(url, length, r, dir) {
  const want = r.end - r.start;
  try {
    const done = await (await dir.getFileHandle(r.file)).getFile();
    if (done.size === want) return { file: r.file, ok: true, status: "kept", resumedFrom: null };
  } catch (_) {
    // Not downloaded yet.
  }
  const part = await dir.getFileHandle(r.file + ".part", { create: true });
  const h = await part.createSyncAccessHandle();
  let at = h.getSize();
  const resumedFrom = at > 0 ? at : null;
  const from = r.start + at;
  const t0 = performance.now();
  let status = 0;
  let cr = null;
  try {
    const resp = await fetch(url, { headers: { Range: `bytes=${from}-${r.end - 1}` } });
    status = resp.status;
    cr = resp.headers.get("Content-Range");
    if (status !== 206 || cr !== `bytes ${from}-${r.end - 1}/${length}`) {
      return { file: r.file, ok: false, status, contentRange: cr, resumedFrom };
    }
    postMessage({ progress: r.file });
    const reader = resp.body.getReader();
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      h.write(value, { at });
      at += value.length;
      h.flush();
    }
  } finally {
    h.close();
  }
  const ok = at === want;
  if (ok) await part.move(r.file);
  return { file: r.file, ok, status, contentRange: cr, resumedFrom, ms: Math.round(performance.now() - t0) };
}

self.onmessage = async (e) => {
  const root = await navigator.storage.getDirectory();
  const dl = await root.getDirectoryHandle("dl", { create: true });
  const results = [];
  try {
    for (const a of e.data.pending) {
      const dir = await dl.getDirectoryHandle(a.dir.split("/").pop(), { create: true });
      for (const r of a.ranges) results.push(await fetchRange(a.urls[0], a.length, r, dir));
    }
  } catch (err) {
    results.push({ ok: false, error: String(err) });
  }
  postMessage({ results });
};
