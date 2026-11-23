// Types of `opfs.js`: the browser store over the Origin Private File System (D-375, D-376).
// Hand-written.

/** The Web Lock of the store `root`'s writer; also the name of its BroadcastChannel. */
export function storeLock(root: string): string;
/** The Web Lock a reader holds shared on one pack file of the store `root`. */
export function fileLock(root: string, file: string): string;

/** A pack as the store's journal records it. */
export interface InstalledRecord {
  address: string;
  component: string;
  release: string;
  set: string[];
  file: string;
  pack_len: number;
  head: string;
  deps: string[];
  asked: boolean;
  pinned: boolean;
  used: number;
  parts?: { languages?: number[]; outline?: number };
}

/** A delta as the store's journal records it (D-395): read above its base, `of` its release. */
export interface InstalledDelta {
  address: string;
  component: string;
  of: string;
  release: string;
  set: string[];
  file: string;
  pack_len: number;
  head: string;
  parts?: { languages?: number[]; outline?: number };
}

/** A `StoreHost` (`teigeodata-wasm/store`) over the directory `root`, in a dedicated worker. */
export function opfsHost(
  root: string,
  options?: { fetch?: typeof globalThis.fetch },
): Promise<unknown>;

/** What `openInstalled` opened: a layer per file, its record (the packs, then the deltas), and `close()` to let go of the files. */
export interface Installed {
  layers: number[];
  packs: (InstalledRecord | InstalledDelta)[];
  close(): void;
}

/** Opens the packs installed in the store `root` in `engine`, by section. */
export function openInstalled(
  engine: {
    openInstalled(
      head: Uint8Array,
      record: InstalledRecord,
      offsets: Float64Array,
      sections: Uint8Array[],
    ): number;
  },
  installedSections: (prefix: Uint8Array, record: InstalledRecord) => Float64Array | undefined,
  root: string,
): Promise<Installed>;

/** The store worker's calls; results are `Store`'s (blobs as `Uint8Array`). */
export interface StoreWorker {
  worker: Worker;
  call(op: string, ...args: unknown[]): Promise<unknown>;
  init(root: string, repo: string[], assets: string[], trustedRoot: Uint8Array): Promise<void>;
  open(root: string): Promise<{ damaged: string; packs: InstalledRecord[] }>;
  catalog(): Promise<Uint8Array>;
  status(deep: boolean): Promise<Uint8Array>;
  refresh(countries: string): Promise<Uint8Array>;
  install(
    packs: string,
    options?: {
      components?: string;
      languages?: string;
      outlineLevel?: number;
      outlineToleranceM?: number;
      pin?: boolean;
    },
  ): Promise<Uint8Array>;
  update(packs: string): Promise<Uint8Array>;
  remove(pack: string): Promise<Uint8Array>;
  pin(pack: string, on: boolean): Promise<void>;
  repair(pack: string): Promise<Uint8Array>;
  packs(): Promise<InstalledRecord[]>;
}

/** Calls the store worker with promises. */
export function storeWorker(worker: Worker): StoreWorker;
