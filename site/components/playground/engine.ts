// The playground's engine: the teigeodata-wasm package, the pinned pack and the
// catalog of packs a reader can add at run time, all served from this site (`cargo xtask
// playground` writes them to public/playground/). The package is imported at run time from its
// own files, so the page runs exactly what a user installs, and nothing is fetched from anywhere
// but this site. An added pack is checked by SHA-256, kept in Cache Storage for the next visit
// (offline too), and opened as a layer above the base pack (D-357).

import type { Feature, FeatureCollection } from 'geojson';
import { basePath } from '@/lib/shared';

/** A pack of the catalog (`site/packs.json`, as `cargo xtask playground` cut it). */
export interface CatalogEntry {
  id: string;
  title: string;
  summary: string;
  /** Where it holds data: west, south, east, north. */
  bbox: [number, number, number, number];
  contents: string[];
  bytes: number;
  sha256: string;
  /** Relative to the playground's directory. */
  file: string;
  release: string;
  licence: string;
}

/** What `cargo xtask playground` wrote beside the files. */
export interface Manifest {
  release: string;
  file: string;
  bytes: number;
  sha256: string;
  package: string;
  version: string;
  engine_bytes: number;
  catalog: CatalogEntry[];
}

/** A row of a result, by column name (see the reference's result columns). */
export type Row = Record<string, string | number | bigint | boolean | null>;

export interface SearchOptions {
  focusLat?: number;
  focusLon?: number;
  focusScaleM?: number;
  strict?: boolean;
}

/** The engine methods the playground calls; each answers a result blob. */
export interface Engine {
  openBytes(bytes: Uint8Array): number;
  openBytesWith(bytes: Uint8Array, options?: { priority?: number }): number;
  remove(layer: number): boolean;
  layers(): number;
  tzVersion(): string;
  get(id: string): Uint8Array;
  ancestors(id: string): Uint8Array;
  children(id: string): Uint8Array;
  nearby(id: string, limit: number): Uint8Array;
  search(query: string, limit: number): Uint8Array;
  searchWith(query: string, limit: number, options?: SearchOptions): Uint8Array;
  reverse(lat: number, lon: number, limit: number): Uint8Array;
  withinRadius(lat: number, lon: number, radiusM: number, limit: number): Uint8Array;
  adminAt(lat: number, lon: number): Uint8Array;
  elevationAt(lat: number, lon: number): Uint8Array;
  tzAt(lat: number, lon: number): Uint8Array;
  tzOffsetAt(zone: string, unixSeconds: number): Uint8Array;
  postalAt(lat: number, lon: number): Uint8Array;
  byIso(code: string): Uint8Array;
  boundary(id: string, lod: number): Uint8Array;
  distance(from: string, to: string): Uint8Array;
}

interface ReaderClass {
  new (blob: Uint8Array): { rows: number; columns: string[]; records(): Row[] };
}

interface Module {
  init(): Promise<void>;
  Engine: new () => Engine;
  Reader: ReaderClass;
  distanceM(lat1: number, lon1: number, lat2: number, lon2: number): number;
  bearingDeg(lat1: number, lon1: number, lat2: number, lon2: number): number;
}

/** The loaded engine, with a reader that turns a result into rows. */
export interface Loaded {
  engine: Engine;
  rows: (blob: Uint8Array) => Row[];
  /** Great-circle metres and initial bearing between two points (free functions). */
  distanceM: Module['distanceM'];
  bearingDeg: Module['bearingDeg'];
  manifest: Manifest;
  /** The world: every country's outline, a GeoJSON FeatureCollection. */
  world: FeatureCollection;
  countries: number;
  vertices: number;
  timings: { label: string; ms: number }[];
}

export type Progress =
  | { phase: 'engine' }
  | { phase: 'download'; loaded: number; total: number }
  | { phase: 'verify' }
  | { phase: 'open' }
  | { phase: 'world' };

export const playgroundBase = `${basePath}/playground`;

export async function fetchManifest(): Promise<Manifest> {
  const response = await fetch(`${playgroundBase}/manifest.json`);
  if (!response.ok) {
    throw new Error(
      `The playground's files are not built (${response.status} for manifest.json): run \`cargo xtask playground --fetch\`, then reload.`,
    );
  }
  return (await response.json()) as Manifest;
}

/** Loads the engine and the pack, checks the pack's SHA-256, and opens it. */
export async function load(manifest: Manifest, onProgress: (p: Progress) => void): Promise<Loaded> {
  const timings: { label: string; ms: number }[] = [];
  const timed = async <T>(label: string, f: () => Promise<T> | T): Promise<T> => {
    const t = performance.now();
    const value = await f();
    timings.push({ label, ms: performance.now() - t });
    return value;
  };

  onProgress({ phase: 'engine' });
  const url = `${playgroundBase}/engine/index.js`;
  const mod = await timed(
    'engine: import and instantiate',
    async () => {
      const m = (await import(/* webpackIgnore: true */ /* turbopackIgnore: true */ url)) as Module;
      await m.init();
      return m;
    },
  );

  const bytes = await timed(`download ${manifest.file}`, () =>
    download(`${playgroundBase}/${manifest.file}`, manifest.bytes, (loaded) =>
      onProgress({ phase: 'download', loaded, total: manifest.bytes }),
    ),
  );

  onProgress({ phase: 'verify' });
  await timed('SHA-256 of the pack', () => check(bytes, manifest.sha256, manifest.file));

  onProgress({ phase: 'open' });
  const engine = new mod.Engine();
  await timed('engine.openBytes(pack)', () => engine.openBytes(bytes));
  const rows = (blob: Uint8Array) => new mod.Reader(blob).records();

  onProgress({ phase: 'world' });
  const { world, countries, vertices } = await timed('every country’s outline', () =>
    worldOutline(engine, rows),
  );
  return {
    engine,
    rows,
    distanceM: mod.distanceM,
    bearingDeg: mod.bearingDeg,
    manifest,
    world,
    countries,
    vertices,
    timings,
  };
}

async function download(
  url: string,
  total: number,
  onBytes: (n: number) => void,
): Promise<Uint8Array<ArrayBuffer>> {
  const response = await fetch(url);
  if (!response.ok || !response.body) throw new Error(`${url}: ${response.status}`);
  const out = new Uint8Array(new ArrayBuffer(total));
  const reader = response.body.getReader();
  let at = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    if (at + value.length > total) throw new Error(`${url}: more than the ${total} bytes expected`);
    out.set(value, at);
    at += value.length;
    onBytes(at);
  }
  if (at !== total) throw new Error(`${url}: ${at} bytes, expected ${total}`);
  return out;
}

async function sha256(bytes: Uint8Array<ArrayBuffer>): Promise<string> {
  const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes));
  return Array.from(digest, (b) => b.toString(16).padStart(2, '0')).join('');
}

async function check(bytes: Uint8Array<ArrayBuffer>, expected: string, name: string) {
  const hex = await sha256(bytes);
  if (hex !== expected) throw new Error(`${name}: SHA-256 ${hex}, expected ${expected}`);
}

/** Where added packs are kept between visits; a new name drops every older copy. */
const STORE = 'teigeodata-playground-packs-v1';

/** Cache Storage, where the browser has it (a secure context); otherwise packs last a visit. */
async function store(): Promise<Cache | null> {
  try {
    return 'caches' in globalThis ? await caches.open(STORE) : null;
  } catch {
    return null;
  }
}

/** A stored copy is keyed by its file and SHA-256, so a new cut of a pack is a new key. */
const keyOf = (entry: CatalogEntry) => `${playgroundBase}/${entry.file}?sha256=${entry.sha256}`;

/** The IDs of the catalog's packs this browser has kept. */
export async function storedPacks(catalog: CatalogEntry[]): Promise<Set<string>> {
  const cache = await store();
  const ids = new Set<string>();
  if (!cache) return ids;
  for (const entry of catalog) {
    if (await cache.match(keyOf(entry))) ids.add(entry.id);
  }
  return ids;
}

export type PackProgress =
  | { phase: 'download'; loaded: number; total: number }
  | { phase: 'verify' }
  | { phase: 'open' };

/** An added pack: its layer in the engine and what it took. */
export interface Added {
  layer: number;
  /** Read from this browser's store, not the network. */
  stored: boolean;
  timings: { label: string; ms: number }[];
}

/**
 * Adds a pack of the catalog: from this browser's store when it holds it, otherwise downloaded;
 * checked by SHA-256 either way; kept for the next visit; opened above the base pack (priority
 * 1), so where both hold a place the added, newer one answers.
 */
export async function addPack(engine: Engine, entry: CatalogEntry, onProgress: (p: PackProgress) => void): Promise<Added> {
  const timings: { label: string; ms: number }[] = [];
  const timed = async <T>(label: string, f: () => Promise<T> | T): Promise<T> => {
    const t = performance.now();
    const value = await f();
    timings.push({ label, ms: performance.now() - t });
    return value;
  };
  const cache = await store();
  const key = keyOf(entry);
  let bytes: Uint8Array<ArrayBuffer> | null = null;
  let stored = false;
  const kept = await cache?.match(key);
  if (kept) {
    bytes = await timed('read from the store', async () => new Uint8Array(await kept.arrayBuffer()));
    stored = true;
  }
  if (!bytes || bytes.length !== entry.bytes) {
    stored = false;
    bytes = await timed(`download ${entry.file}`, () =>
      download(`${playgroundBase}/${entry.file}`, entry.bytes, (loaded) => onProgress({ phase: 'download', loaded, total: entry.bytes })),
    );
  }
  onProgress({ phase: 'verify' });
  try {
    const verified = bytes;
    await timed('SHA-256', () => check(verified, entry.sha256, entry.file));
  } catch (e) {
    await cache?.delete(key);
    throw e;
  }
  if (!stored && cache) {
    try {
      await cache.put(key, new Response(bytes, { headers: { 'content-type': 'application/octet-stream' } }));
    } catch {
      // Over the browser's quota: the pack lasts this visit.
    }
  }
  onProgress({ phase: 'open' });
  const opened = bytes;
  const layer = await timed('engine.openBytesWith(pack, { priority: 1 })', () => engine.openBytesWith(opened, { priority: 1 }));
  return { layer, stored, timings };
}

/** Drops a pack's stored copy. */
export async function forgetPack(entry: CatalogEntry) {
  await (await store())?.delete(keyOf(entry));
}

const LETTERS = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ';

/**
 * Every country the pack holds, found by its ISO 3166-1 code, with its outline. `byIso`
 * rejects a code ISO 3166 does not assign (InvalidArgument): that code is no country.
 */
function worldOutline(engine: Engine, rows: (b: Uint8Array) => Row[]) {
  const features: Feature[] = [];
  let vertices = 0;
  for (const a of LETTERS) {
    for (const b of LETTERS) {
      let found: Row[];
      try {
        found = rows(engine.byIso(a + b));
      } catch {
        continue;
      }
      const country = found.find((r) => Number(r.level) === 0);
      if (!country) continue;
      const [shape] = rows(engine.boundary(String(country.id), 6));
      if (!shape) continue;
      const feature = JSON.parse(String(shape.geojson)) as Feature;
      feature.properties = { id: country.id, name: country.name, iso: a + b };
      features.push(feature);
      vertices += Number(shape.vertices);
    }
  }
  return {
    world: { type: 'FeatureCollection', features } as FeatureCollection,
    countries: features.length,
    vertices,
  };
}

/**
 * Runs `f` and how long one call takes, in milliseconds. Browsers coarsen `performance.now()`
 * (to 0.1 ms in Chrome without cross-origin isolation), so a call faster than 1 ms is run again
 * for about 3 ms and the mean is reported: queries only read, so repeating one is harmless.
 */
export function time<T>(f: () => T): [T, number] {
  const t = performance.now();
  const value = f();
  const once = performance.now() - t;
  if (once >= 1) return [value, once];
  let runs = 0;
  const start = performance.now();
  let elapsed = 0;
  do {
    f();
    runs++;
    elapsed = performance.now() - start;
  } while (elapsed < 3 && runs < 2000);
  return [value, elapsed / runs];
}

export function formatMs(ms: number): string {
  if (ms < 0.1) return `${Math.max(1, Math.round(ms * 1000))} µs`;
  if (ms < 10) return `${ms.toFixed(2)} ms`;
  return `${ms.toFixed(0)} ms`;
}

export function formatBytes(n: number): string {
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)} MB`;
  return `${(n / 1e3).toFixed(0)} kB`;
}
