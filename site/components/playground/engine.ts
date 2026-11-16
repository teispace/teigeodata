// The playground's engine: the @teispace/teigeodata-wasm package and the pinned pack, both
// served from this site (`cargo xtask playground` writes them to public/playground/). The
// package is imported at run time from its own files, so the page runs exactly what a user
// installs, and nothing is fetched from anywhere but this site.

import type { Feature, FeatureCollection } from 'geojson';
import { basePath } from '@/lib/shared';

/** What `cargo xtask playground` wrote beside the files. */
export interface Manifest {
  release: string;
  file: string;
  bytes: number;
  sha256: string;
  package: string;
  version: string;
  engine_bytes: number;
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
  await timed('SHA-256 of the pack', async () => {
    const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes));
    const hex = Array.from(digest, (b) => b.toString(16).padStart(2, '0')).join('');
    if (hex !== manifest.sha256) {
      throw new Error(`${manifest.file}: SHA-256 ${hex}, expected ${manifest.sha256}`);
    }
  });

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
