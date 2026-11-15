'use client';

// The playground: every query of the WebAssembly engine on the pinned pack, in this browser.
// Each panel shows the calls that produced it, as code to copy, and how long they took.

import 'maplibre-gl/dist/maplibre-gl.css';
import Link from 'next/link';
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import type { Geometry } from 'geojson';
import type { Map as MapLibreMap, Popup } from 'maplibre-gl';
import { DynamicCodeBlock } from 'fumadocs-ui/components/dynamic-codeblock';
import {
  fetchManifest,
  formatBytes,
  formatMs,
  load,
  time,
  type Loaded,
  type Manifest,
  type Progress,
  type Row,
} from './engine';
import { EMPTY, bounds, createMap, points, repaint, setData, themeColors, type Point } from './map';

type Status =
  | { state: 'idle' }
  | { state: 'loading'; progress: Progress }
  | { state: 'ready' }
  | { state: 'error'; message: string };

type Tab = 'search' | 'point' | 'place';

interface Timed {
  label: string;
  ms: number;
}

interface SearchView {
  query: string;
  near: boolean;
  results: Row[];
  timings: Timed[];
  code: string;
  error?: string;
}

interface PointView {
  lat: number;
  lon: number;
  admin: Row[];
  zone?: Row;
  offset?: Row;
  height?: Row;
  nearest: Row[];
  timings: Timed[];
  code: string;
}

interface Outline {
  vertices: number;
  lod: number;
  tolerance: number;
  bytes: number;
  geometry: Geometry;
}

interface PlaceView {
  id: string;
  place: Row;
  ancestors: Row[];
  children: Row[];
  nearby: Row[];
  outline?: Outline;
  fromPin?: { km: number; bearing: number };
  timings: Timed[];
  code: string;
}

const LIMIT = 10;
const SHOWN_CHILDREN = 60;

/** Examples to start from: none assumes one country. */
const EXAMPLES: { label: string; hash: string }[] = [
  { label: 'Pokhara', hash: 'q=Pokhara' },
  { label: 'काठमाडौं', hash: 'q=काठमाडौं' },
  { label: 'Kathmandu, Bagmati', hash: 'q=Kathmandu, Bagmati' },
  { label: 'Tokyo', hash: 'q=Tokyo' },
  { label: 'Everest summit', hash: 'at=27.98806,86.92528' },
  { label: 'Lower Manhattan', hash: 'at=40.7128,-74.0060' },
  { label: 'Nepal', hash: 'place=tgd:np/1wmc' },
];

const num = (v: Row[string] | undefined) => (v === null || v === undefined ? NaN : Number(v));
const str = (v: Row[string] | undefined) => (v === null || v === undefined ? '' : String(v));
const fixed = (v: Row[string] | undefined, digits: number) => {
  const n = num(v);
  return Number.isFinite(n) ? n.toFixed(digits) : '–';
};
const quote = (s: string) => JSON.stringify(s);
const coord = (n: number) => Number(n.toFixed(5));

function toPoint(r: Row, role: Point['role'] = 'result'): Point | null {
  const lat = num(r.lat);
  const lon = num(r.lon);
  if (!Number.isFinite(lat) || !Number.isFinite(lon)) return null;
  return { id: str(r.id), name: str(r.name), kind: str(r.kind), lat, lon, role };
}

function compass(deg: number) {
  const names = ['N', 'NE', 'E', 'SE', 'S', 'SW', 'W', 'NW'];
  return names[Math.round((((deg % 360) + 360) % 360) / 45) % 8];
}

function offsetText(seconds: number) {
  const sign = seconds < 0 ? '−' : '+';
  const s = Math.abs(seconds);
  return `UTC${sign}${String(Math.floor(s / 3600)).padStart(2, '0')}:${String(Math.floor((s % 3600) / 60)).padStart(2, '0')}`;
}

function localTime(zone: string) {
  try {
    return new Intl.DateTimeFormat(undefined, { timeZone: zone, dateStyle: 'medium', timeStyle: 'short' }).format(new Date());
  } catch {
    return '';
  }
}

export function Playground() {
  const mapBox = useRef<HTMLDivElement>(null);
  const mapRef = useRef<MapLibreMap | null>(null);
  const popupRef = useRef<Popup | null>(null);
  const loadedRef = useRef<Loaded | null>(null);
  const pinRef = useRef<{ lat: number; lon: number } | null>(null);
  const handlers = useRef<{ point: (lat: number, lon: number) => void; place: (id: string) => void }>({
    point: () => {},
    place: () => {},
  });

  const [manifest, setManifest] = useState<Manifest | null>(null);
  const [status, setStatus] = useState<Status>({ state: 'idle' });
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [tab, setTab] = useState<Tab>('search');
  const [search, setSearch] = useState<SearchView>({ query: '', near: false, results: [], timings: [], code: '' });
  const [point, setPoint] = useState<PointView | null>(null);
  const [place, setPlace] = useState<PlaceView | null>(null);

  // The manifest names what loading costs before anything large is fetched.
  useEffect(() => {
    fetchManifest().then(setManifest, (e: Error) => setStatus({ state: 'error', message: e.message }));
  }, []);

  // The map, drawn before the pack so the page has its shape; filled once the pack is open.
  useEffect(() => {
    let disposed = false;
    let observer: MutationObserver | undefined;
    if (!mapBox.current) return;
    createMap(mapBox.current).then(async (map) => {
      if (disposed) {
        map.remove();
        return;
      }
      mapRef.current = map;
      const maplibre = await import('maplibre-gl');
      map.addControl(new maplibre.NavigationControl({ showCompass: false }), 'top-right');
      map.addControl(new maplibre.ScaleControl({ unit: 'metric' }), 'bottom-left');
      popupRef.current = new maplibre.Popup({ closeButton: false, closeOnClick: false, offset: 10 });
      map.on('mouseenter', 'places', (e) => {
        map.getCanvas().style.cursor = 'pointer';
        const f = e.features?.[0];
        if (!f || f.geometry.type !== 'Point') return;
        const [lon, lat] = f.geometry.coordinates;
        if (lon === undefined || lat === undefined) return;
        const el = document.createElement('div');
        el.className = 'text-xs';
        const name = document.createElement('strong');
        name.textContent = String(f.properties?.name ?? '');
        const kind = document.createElement('div');
        kind.textContent = `${f.properties?.kind ?? ''} · ${f.properties?.id ?? ''}`;
        el.append(name, kind);
        popupRef.current?.setLngLat([lon, lat]).setDOMContent(el).addTo(map);
      });
      map.on('mouseleave', 'places', () => {
        map.getCanvas().style.cursor = '';
        popupRef.current?.remove();
      });
      map.on('click', (e) => {
        if (!loadedRef.current) return;
        const hit = map.queryRenderedFeatures(e.point, { layers: ['places'] })[0];
        if (hit?.properties?.id) handlers.current.place(String(hit.properties.id));
        else handlers.current.point(e.lngLat.lat, e.lngLat.lng);
      });
      // Follow the site's light and dark themes.
      observer = new MutationObserver(() => {
        if (map.isStyleLoaded()) repaint(map, themeColors());
      });
      observer.observe(document.documentElement, { attributes: true, attributeFilter: ['class', 'style'] });
    });
    return () => {
      disposed = true;
      observer?.disconnect();
      mapRef.current?.remove();
      mapRef.current = null;
    };
  }, []);

  const drawPlaces = useCallback((list: Point[]) => {
    const map = mapRef.current;
    if (map) setData(map, 'places', points(list));
  }, []);

  // After typing pauses, the map moves to the results unless the best one is already in view.
  const fitTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const fitTo = useCallback((list: Point[]) => {
    clearTimeout(fitTimer.current);
    fitTimer.current = setTimeout(() => {
      const map = mapRef.current;
      if (!map || !list.length) return;
      const view = map.getBounds();
      const [best] = list;
      if (best && view.contains([best.lon, best.lat])) return;
      const box = bounds(list.map((p) => ({ type: 'Point', coordinates: [p.lon, p.lat] })));
      if (box) map.fitBounds(box, { padding: 60, maxZoom: 9, duration: 600 });
    }, 450);
  }, []);

  const runSearch = useCallback(
    (query: string, near: boolean) => {
      const l = loadedRef.current;
      if (!l) return;
      const q = query.trim();
      if (!q) {
        setSearch({ query, near, results: [], timings: [], code: '' });
        drawPlaces([]);
        return;
      }
      const center = mapRef.current?.getCenter();
      const options =
        near && center ? { focusLat: coord(center.lat), focusLon: coord(center.lng) } : undefined;
      try {
        const [blob, ms] = time(() =>
          options ? l.engine.searchWith(q, LIMIT, options) : l.engine.search(q, LIMIT),
        );
        const [results, readMs] = time(() => l.rows(blob));
        const call = options
          ? `engine.searchWith(${quote(q)}, ${LIMIT}, { focusLat: ${options.focusLat}, focusLon: ${options.focusLon} })`
          : `engine.search(${quote(q)}, ${LIMIT})`;
        setSearch({
          query,
          near,
          results,
          timings: [
            { label: 'search', ms },
            { label: 'read', ms: readMs },
          ],
          code: `for (const p of new Reader(${call})) {\n  console.log(p.id, p.name, p.kind, p.level, p.lat, p.lon, p.score);\n}`,
        });
        const shown = results.map((r) => toPoint(r)).filter((p): p is Point => p !== null);
        drawPlaces(shown);
        if (!near) fitTo(shown);
      } catch (e) {
        setSearch({ query, near, results: [], timings: [], code: '', error: (e as Error).message });
      }
    },
    [drawPlaces, fitTo],
  );

  const openPoint = useCallback(
    (latIn: number, lonIn: number, move = false) => {
      const l = loadedRef.current;
      const map = mapRef.current;
      if (!l) return;
      const lat = coord(latIn);
      const lon = coord(((lonIn + 540) % 360) - 180);
      const timings: Timed[] = [];
      const run = <T,>(label: string, f: () => T, fallback: T): T => {
        try {
          const [v, ms] = time(f);
          timings.push({ label, ms });
          return v;
        } catch {
          timings.push({ label: `${label} (error)`, ms: 0 });
          return fallback;
        }
      };
      const admin = run('adminAt', () => l.rows(l.engine.adminAt(lat, lon)), [] as Row[]);
      const [zone] = run('tzAt', () => l.rows(l.engine.tzAt(lat, lon)), [] as Row[]);
      const now = Math.floor(Date.now() / 1000);
      const [offset] = zone
        ? run('tzOffsetAt', () => l.rows(l.engine.tzOffsetAt(str(zone.tz), now)), [] as Row[])
        : [];
      const [height] = run('elevationAt', () => l.rows(l.engine.elevationAt(lat, lon)), [] as Row[]);
      const nearest = run('reverse', () => l.rows(l.engine.reverse(lat, lon, 5)), [] as Row[]);
      pinRef.current = { lat, lon };
      if (map) {
        setData(map, 'pin', points([{ id: 'pin', name: 'pin', kind: 'point', lat, lon }]));
        setData(map, 'outline', EMPTY);
        if (move) map.flyTo({ center: [lon, lat], zoom: Math.max(map.getZoom(), 8) });
      }
      drawPlaces(nearest.map((r) => toPoint(r)).filter((p): p is Point => p !== null));
      setPoint({
        lat,
        lon,
        admin,
        zone,
        offset,
        height,
        nearest,
        timings,
        code: [
          `const [lat, lon] = [${lat}, ${lon}];`,
          `const admin = new Reader(engine.adminAt(lat, lon)).records(); // country first`,
          `const [area] = new Reader(engine.tzAt(lat, lon)).records();`,
          `const [offset] = new Reader(engine.tzOffsetAt(area.tz, Math.floor(Date.now() / 1000))).records();`,
          `const [height] = new Reader(engine.elevationAt(lat, lon)).records();`,
          `const nearest = new Reader(engine.reverse(lat, lon, 5)).records();`,
        ].join('\n'),
      });
      setTab('point');
      history.replaceState(null, '', `#at=${lat},${lon}`);
    },
    [drawPlaces],
  );

  const openPlace = useCallback(
    (id: string) => {
      const l = loadedRef.current;
      const map = mapRef.current;
      if (!l) return;
      const timings: Timed[] = [];
      const run = <T,>(label: string, f: () => T, fallback: T): T => {
        try {
          const [v, ms] = time(f);
          timings.push({ label, ms });
          return v;
        } catch {
          timings.push({ label: `${label} (error)`, ms: 0 });
          return fallback;
        }
      };
      const [record] = run('get', () => l.rows(l.engine.get(id)), [] as Row[]);
      if (!record) {
        setPlace(null);
        setTab('place');
        return;
      }
      const ancestors = run('ancestors', () => l.rows(l.engine.ancestors(id)), [] as Row[]);
      const children = run('children', () => l.rows(l.engine.children(id)), [] as Row[]);
      const nearby = run('nearby', () => l.rows(l.engine.nearby(id, 5)), [] as Row[]);
      const [shape] = run('boundary', () => l.rows(l.engine.boundary(id, 0)), [] as Row[]);
      let outline: Outline | undefined;
      const feature = shape ? (JSON.parse(str(shape.geojson)) as { geometry: Geometry }) : undefined;
      // A place with no outline answers its point; only an area is drawn as one.
      if (shape && feature && feature.geometry.type !== 'Point') {
        outline = {
          vertices: num(shape.vertices),
          lod: num(shape.lod),
          tolerance: num(shape.tolerance_m),
          bytes: str(shape.geojson).length,
          geometry: feature.geometry,
        };
      }
      const here = toPoint(record, 'selected');
      const pin = pinRef.current;
      const fromPin =
        pin && here
          ? {
              km: l.distanceM(pin.lat, pin.lon, here.lat, here.lon) / 1000,
              bearing: l.bearingDeg(pin.lat, pin.lon, here.lat, here.lon),
            }
          : undefined;
      if (map) {
        setData(
          map,
          'outline',
          outline ? { type: 'FeatureCollection', features: [{ type: 'Feature', geometry: outline.geometry, properties: {} }] } : EMPTY,
        );
        const shown = [
          ...children.slice(0, 500).map((r) => toPoint(r)),
          ...nearby.map((r) => toPoint(r)),
          here,
        ].filter((p): p is Point => p !== null);
        drawPlaces(shown);
        const box = outline ? bounds([outline.geometry]) : null;
        if (box) map.fitBounds(box, { padding: 40, maxZoom: 13, duration: 600 });
        else if (here) map.flyTo({ center: [here.lon, here.lat], zoom: Math.max(map.getZoom(), 11) });
      }
      setPlace({
        id,
        place: record,
        ancestors,
        children,
        nearby,
        outline,
        fromPin,
        timings,
        code: [
          `const [place] = new Reader(engine.get(${quote(id)})).records();`,
          `const ancestors = new Reader(engine.ancestors(${quote(id)})).records(); // parent first, up to the country`,
          `const children = new Reader(engine.children(${quote(id)}));`,
          `const nearby = new Reader(engine.nearby(${quote(id)}, 5)).records();`,
          `const [shape] = new Reader(engine.boundary(${quote(id)}, 0)).records();`,
          `const outline = JSON.parse(shape.geojson); // a GeoJSON Feature: a Point for a place with no outline`,
          `map.addSource("outline", { type: "geojson", data: outline });`,
        ].join('\n'),
      });
      setTab('place');
      history.replaceState(null, '', `#place=${encodeURIComponent(id)}`);
    },
    [drawPlaces],
  );

  handlers.current = { point: (lat, lon) => openPoint(lat, lon), place: openPlace };

  /** Opens what the URL hash names: `#q=`, `#at=lat,lon` or `#place=id`. */
  const followHash = useCallback(() => {
    const hash = decodeURIComponent(location.hash.slice(1));
    const [key, ...rest] = hash.split('=');
    const value = rest.join('=');
    if (key === 'q' && value) {
      runSearch(value, false);
      setTab('search');
    } else if (key === 'at') {
      const [lat, lon] = value.split(',').map(Number);
      if (lat !== undefined && lon !== undefined && Number.isFinite(lat) && Number.isFinite(lon) && Math.abs(lat) <= 90) {
        openPoint(lat, lon, true);
      }
    } else if (key === 'place' && value) {
      openPlace(value);
    }
  }, [openPlace, openPoint, runSearch]);

  useEffect(() => {
    if (status.state !== 'ready') return;
    window.addEventListener('hashchange', followHash);
    return () => window.removeEventListener('hashchange', followHash);
  }, [status.state, followHash]);

  const start = useCallback(async () => {
    if (!manifest) return;
    setStatus({ state: 'loading', progress: { phase: 'engine' } });
    try {
      const l = await load(manifest, (progress) => setStatus({ state: 'loading', progress }));
      loadedRef.current = l;
      setLoaded(l);
      const map = mapRef.current;
      if (map) {
        const fill = () => setData(map, 'world', l.world);
        if (map.isStyleLoaded()) fill();
        else map.once('load', fill);
      }
      setStatus({ state: 'ready' });
      if (location.hash.length > 1) followHash();
    } catch (e) {
      setStatus({ state: 'error', message: (e as Error).message });
    }
  }, [manifest, followHash]);

  return (
    <div
      className="grid min-h-0 flex-1 grid-rows-[minmax(45svh,1fr)_auto] md:grid-cols-[minmax(360px,440px)_1fr] md:grid-rows-1"
      data-testid="playground"
      data-state={status.state}
    >
      <aside className="order-2 min-h-0 overflow-y-auto border-t bg-fd-background md:order-1 md:border-t-0 md:border-r">
        <div className="space-y-5 p-4 md:p-5">
          {status.state !== 'ready' || !loaded ? (
            <Intro manifest={manifest} status={status} onStart={start} />
          ) : (
            <>
              <Header loaded={loaded} />
              <div className="flex flex-wrap gap-1.5" aria-label="Examples">
                {EXAMPLES.map((e) => (
                  <a
                    key={e.hash}
                    href={`#${e.hash}`}
                    className="rounded-full border px-2.5 py-0.5 text-xs text-fd-muted-foreground transition-colors hover:border-fd-primary hover:text-fd-primary"
                  >
                    {e.label}
                  </a>
                ))}
              </div>
              <div role="tablist" className="grid grid-cols-3 rounded-lg border p-0.5 text-sm">
                {(
                  [
                    ['search', 'Search'],
                    ['point', 'At a point'],
                    ['place', 'A place'],
                  ] as const
                ).map(([key, label]) => (
                  <button
                    key={key}
                    role="tab"
                    type="button"
                    aria-selected={tab === key}
                    onClick={() => setTab(key)}
                    className={`rounded-md px-2 py-1.5 font-medium transition-colors ${
                      tab === key ? 'bg-fd-primary text-fd-primary-foreground' : 'text-fd-muted-foreground hover:text-fd-foreground'
                    }`}
                  >
                    {label}
                  </button>
                ))}
              </div>
              {tab === 'search' && <SearchPanel view={search} onSearch={runSearch} onPick={openPlace} />}
              {tab === 'point' && <PointPanel view={point} onPick={openPlace} />}
              {tab === 'place' && <PlacePanel view={place} onPick={openPlace} />}
              <Credits manifest={loaded.manifest} />
            </>
          )}
        </div>
      </aside>
      <div className="relative order-1 min-h-[45svh] md:order-2 md:min-h-0">
        <div ref={mapBox} className="absolute inset-0" data-testid="map" />
        {status.state === 'ready' && (
          <p className="pointer-events-none absolute bottom-8 right-2 rounded bg-fd-background/85 px-2 py-1 text-[11px] text-fd-muted-foreground">
            Click the map for what is at a point; click a circle for that place.
          </p>
        )}
      </div>
    </div>
  );
}

function Intro({ manifest, status, onStart }: { manifest: Manifest | null; status: Status; onStart: () => void }) {
  const loading = status.state === 'loading';
  return (
    <div className="space-y-4">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Playground</h1>
        <p className="mt-2 text-sm text-fd-muted-foreground">
          Every query of teigeodata, run by the WebAssembly engine in this tab on a released pack. Search as you
          type, click the map for what is at a point, open a place for its hierarchy and outline. Each answer
          shows the code that produced it and how long it took.
        </p>
      </div>
      <ul className="space-y-1.5 text-sm">
        <li>
          <strong>Nothing leaves this page.</strong>{' '}
          <span className="text-fd-muted-foreground">
            The engine and the pack are files of this site; the map is drawn from the pack itself, with no tile
            server.
          </span>
        </li>
        <li>
          <strong>Checked.</strong>{' '}
          <span className="text-fd-muted-foreground">
            The pack&apos;s SHA-256 is checked here before it is opened, and the engine checks every section it
            reads.
          </span>
        </li>
      </ul>
      {manifest && (
        <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 rounded-lg border bg-fd-card p-3 text-xs">
          <dt className="text-fd-muted-foreground">Pack</dt>
          <dd>
            <code>{manifest.file}</code>, {formatBytes(manifest.bytes)}, {manifest.release}
          </dd>
          <dt className="text-fd-muted-foreground">Engine</dt>
          <dd>
            <code>{manifest.package}</code> {manifest.version}, {formatBytes(manifest.engine_bytes)}
          </dd>
        </dl>
      )}
      {status.state === 'error' ? (
        <p role="alert" className="rounded-lg border border-red-500/40 bg-red-500/10 p-3 text-sm" data-testid="error">
          {status.message}
        </p>
      ) : (
        <button
          type="button"
          onClick={onStart}
          disabled={!manifest || loading}
          data-testid="load"
          className="w-full rounded-lg bg-fd-primary px-4 py-2.5 text-sm font-medium text-fd-primary-foreground hover:opacity-90 disabled:opacity-60"
        >
          {loading
            ? 'Loading…'
            : manifest
              ? `Load the playground (${formatBytes(manifest.bytes + manifest.engine_bytes)})`
              : 'Reading what to load…'}
        </button>
      )}
      {status.state === 'loading' && <ProgressBar progress={status.progress} />}
      <p className="text-xs text-fd-muted-foreground">
        The same calls work in an app: see the{' '}
        <Link className="text-fd-primary" href="/docs/guides/wasm">
          browser guide
        </Link>{' '}
        and the{' '}
        <Link className="text-fd-primary" href="/docs/reference">
          reference
        </Link>
        .
      </p>
    </div>
  );
}

function ProgressBar({ progress }: { progress: Progress }) {
  const label = {
    engine: 'Loading the engine',
    download: 'Downloading the pack',
    verify: 'Checking its SHA-256',
    open: 'Opening the pack',
    world: 'Drawing every country',
  }[progress.phase];
  const share = progress.phase === 'download' ? progress.loaded / progress.total : progress.phase === 'engine' ? 0 : 1;
  return (
    <div className="space-y-1.5" aria-live="polite">
      <div className="flex justify-between text-xs text-fd-muted-foreground">
        <span>{label}</span>
        {progress.phase === 'download' && (
          <span>
            {formatBytes(progress.loaded)} of {formatBytes(progress.total)}
          </span>
        )}
      </div>
      <div className="h-1.5 overflow-hidden rounded-full bg-fd-secondary">
        <div className="h-full bg-fd-primary transition-[width]" style={{ width: `${(share * 100).toFixed(1)}%` }} />
      </div>
    </div>
  );
}

function Header({ loaded }: { loaded: Loaded }) {
  const { manifest, engine, countries, vertices, timings } = loaded;
  return (
    <div className="space-y-1">
      <h1 className="text-lg font-semibold tracking-tight">Playground</h1>
      <p className="text-xs text-fd-muted-foreground" data-testid="ready">
        <code>{manifest.file}</code> ({manifest.release}) open in {manifest.package} {manifest.version}; tzdb{' '}
        {engine.tzVersion()}; {countries} countries, {vertices.toLocaleString('en')} outline vertices.
      </p>
      <details className="text-xs text-fd-muted-foreground">
        <summary className="cursor-pointer">How it loaded</summary>
        <ul className="mt-1 space-y-0.5">
          {timings.map((t) => (
            <li key={t.label} className="flex justify-between gap-3">
              <span>{t.label}</span>
              <span className="tabular-nums">{formatMs(t.ms)}</span>
            </li>
          ))}
        </ul>
      </details>
    </div>
  );
}

function Timings({ timings }: { timings: Timed[] }) {
  if (!timings.length) return null;
  return (
    <p className="flex flex-wrap gap-x-3 gap-y-0.5 text-xs text-fd-muted-foreground">
      {timings.map((t) => (
        <span key={t.label}>
          {t.label} <span className="tabular-nums text-fd-foreground">{formatMs(t.ms)}</span>
        </span>
      ))}
    </p>
  );
}

function Code({ code }: { code: string }) {
  if (!code) return null;
  return (
    <details className="text-sm" open>
      <summary className="cursor-pointer text-xs font-medium text-fd-muted-foreground">The code</summary>
      <div className="mt-1 [&_pre]:max-h-64 [&_pre]:text-xs">
        <DynamicCodeBlock lang="js" code={code} />
      </div>
    </details>
  );
}

function Section({ title, children, count }: { title: string; children: ReactNode; count?: number }) {
  return (
    <section className="space-y-1.5">
      <h2 className="text-xs font-semibold uppercase tracking-wide text-fd-muted-foreground">
        {title}
        {count !== undefined && <span className="ml-1 font-normal normal-case">({count.toLocaleString('en')})</span>}
      </h2>
      {children}
    </section>
  );
}

function PlaceList({ rows, onPick, extra }: { rows: Row[]; onPick: (id: string) => void; extra?: (r: Row) => ReactNode }) {
  if (!rows.length) return <p className="text-sm text-fd-muted-foreground">None.</p>;
  return (
    <ul className="divide-y rounded-lg border">
      {rows.map((r, i) => (
        <li key={`${str(r.id)}-${i}`}>
          <button
            type="button"
            onClick={() => onPick(str(r.id))}
            data-id={str(r.id)}
            className="flex w-full items-baseline justify-between gap-3 px-3 py-2 text-left text-sm hover:bg-fd-accent"
          >
            <span className="min-w-0">
              <span className="font-medium">{str(r.name)}</span>{' '}
              <span className="text-xs text-fd-muted-foreground">
                {str(r.kind)}
                {r.kind === 'admin' ? ` level ${str(r.level)}` : ''}
              </span>
              <span className="block truncate font-mono text-[11px] text-fd-muted-foreground">{str(r.id)}</span>
            </span>
            <span className="shrink-0 text-xs tabular-nums text-fd-muted-foreground">{extra?.(r)}</span>
          </button>
        </li>
      ))}
    </ul>
  );
}

function SearchPanel({
  view,
  onSearch,
  onPick,
}: {
  view: SearchView;
  onSearch: (query: string, near: boolean) => void;
  onPick: (id: string) => void;
}) {
  return (
    <div className="space-y-3" data-testid="search-panel">
      <input
        type="search"
        autoFocus
        value={view.query}
        onChange={(e) => onSearch(e.target.value, view.near)}
        placeholder="A place, in any script: Pokhara, काठमाडौं, Tokyo…"
        aria-label="Search"
        data-testid="search"
        className="w-full rounded-lg border bg-fd-background px-3 py-2 text-sm outline-none focus:border-fd-primary"
      />
      <label className="flex items-center gap-2 text-xs text-fd-muted-foreground">
        <input type="checkbox" checked={view.near} onChange={(e) => onSearch(view.query, e.target.checked)} />
        Rank places near the map&apos;s centre first (a focus point)
      </label>
      <Timings timings={view.timings} />
      {view.error && <p className="text-sm text-red-600">{view.error}</p>}
      {view.query.trim() && !view.error && (
        <Section title="Results" count={view.results.length}>
          <PlaceList rows={view.results} onPick={onPick} extra={(r) => `score ${fixed(r.score, 0)}`} />
        </Section>
      )}
      {!view.query.trim() && (
        <p className="text-sm text-fd-muted-foreground">
          Search runs on every keystroke: try a prefix (<em>Pokh</em>), a typo (<em>Kathamandu</em>), another
          script (<em>पोखरा</em>) or a parent after a comma (<em>Kathmandu, Bagmati</em>).
        </p>
      )}
      <Code code={view.code} />
    </div>
  );
}

function PointPanel({ view, onPick }: { view: PointView | null; onPick: (id: string) => void }) {
  if (!view) {
    return <p className="text-sm text-fd-muted-foreground">Click anywhere on the map.</p>;
  }
  const { zone, offset, height } = view;
  return (
    <div className="space-y-4" data-testid="point-panel">
      <p className="font-mono text-sm">
        {view.lat}, {view.lon}
      </p>
      <Timings timings={view.timings} />
      <Section title="Admin units here, country first" count={view.admin.length}>
        {view.admin.length ? (
          <ol className="space-y-0.5 text-sm">
            {view.admin.map((r) => (
              <li key={str(r.id)} className="flex justify-between gap-3">
                <button type="button" className="text-left hover:text-fd-primary" onClick={() => onPick(str(r.id))}>
                  <span className="text-xs text-fd-muted-foreground">{str(r.level)}</span> {str(r.name)}
                </button>
                <span className="text-xs text-fd-muted-foreground">{str(r.iso)}</span>
              </li>
            ))}
          </ol>
        ) : (
          <p className="text-sm text-fd-muted-foreground">None: open water, or a unit the pack has no outline for.</p>
        )}
      </Section>
      <Section title="Timezone">
        {zone ? (
          <p className="text-sm" data-testid="zone">
            <strong>{str(zone.tz)}</strong>
            {offset && (
              <>
                , {offsetText(num(offset.seconds))} ({str(offset.abbr)}
                {offset.dst ? ', summer time' : ''}) now
              </>
            )}
            <span className="block text-xs text-fd-muted-foreground">{localTime(str(zone.tz))} there</span>
          </p>
        ) : (
          <p className="text-sm text-fd-muted-foreground">None.</p>
        )}
      </Section>
      <Section title="Elevation">
        {height ? (
          <p className="text-sm" data-testid="height">
            <strong>{fixed(height.meters, 1)} m</strong> above {str(height.datum)}{' '}
            <span className="text-xs text-fd-muted-foreground">
              ({str(height.source)}, {fixed(height.resolution_m, 0)} m grid)
            </span>
          </p>
        ) : (
          <p className="text-sm text-fd-muted-foreground">No terrain here.</p>
        )}
      </Section>
      <Section title="Nearest places" count={view.nearest.length}>
        <PlaceList
          rows={view.nearest}
          onPick={onPick}
          extra={(r) => `${(num(r.meters) / 1000).toFixed(1)} km ${compass(num(r.bearing))}`}
        />
      </Section>
      <Code code={view.code} />
    </div>
  );
}

function PlacePanel({ view, onPick }: { view: PlaceView | null; onPick: (id: string) => void }) {
  if (!view) {
    return <p className="text-sm text-fd-muted-foreground">Pick a place from a search, a point or the map.</p>;
  }
  const p = view.place;
  return (
    <div className="space-y-4" data-testid="place-panel">
      <div>
        <h2 className="text-xl font-semibold">{str(p.name)}</h2>
        <p className="text-xs text-fd-muted-foreground">
          {str(p.kind)}
          {p.kind === 'admin' ? `, level ${str(p.level)}` : ''} · <span className="font-mono">{view.id}</span>
        </p>
      </div>
      <nav aria-label="Ancestors" className="flex flex-wrap items-center gap-1 text-sm" data-testid="ancestors">
        {[...view.ancestors].reverse().map((a, i) => (
          <span key={str(a.id)} className="flex items-center gap-1">
            {i > 0 && <span className="text-fd-muted-foreground">›</span>}
            <button type="button" className="hover:text-fd-primary" onClick={() => onPick(str(a.id))}>
              {str(a.name)}
            </button>
          </span>
        ))}
        {view.ancestors.length > 0 && <span className="text-fd-muted-foreground">›</span>}
        <span className="font-medium">{str(p.name)}</span>
      </nav>
      <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-sm">
        <dt className="text-fd-muted-foreground">At</dt>
        <dd className="font-mono">
          {fixed(p.lat, 6)}, {fixed(p.lon, 6)}
        </dd>
        <dt className="text-fd-muted-foreground">Elevation</dt>
        <dd>{p.elevation === null ? '–' : `${fixed(p.elevation, 0)} m`}</dd>
        <dt className="text-fd-muted-foreground">Timezone</dt>
        <dd>{str(p.tz) || '–'}</dd>
        {p.iso !== null && p.iso !== undefined && (
          <>
            <dt className="text-fd-muted-foreground">ISO 3166</dt>
            <dd>{str(p.iso)}</dd>
          </>
        )}
        {view.fromPin && (
          <>
            <dt className="text-fd-muted-foreground">From the pin</dt>
            <dd>
              {view.fromPin.km.toFixed(1)} km, heading {view.fromPin.bearing.toFixed(0)}° ({compass(view.fromPin.bearing)})
            </dd>
          </>
        )}
        <dt className="text-fd-muted-foreground">Outline</dt>
        <dd>
          {view.outline
            ? `${view.outline.vertices.toLocaleString('en')} vertices at level ${view.outline.lod} (tolerance ${view.outline.tolerance.toFixed(0)} m), ${formatBytes(view.outline.bytes)} of GeoJSON`
            : 'none: a point place'}
        </dd>
      </dl>
      <Timings timings={view.timings} />
      <Section title="Nearby" count={view.nearby.length}>
        <PlaceList rows={view.nearby} onPick={onPick} extra={(r) => (r.meters !== undefined ? `${(num(r.meters) / 1000).toFixed(1)} km` : '')} />
      </Section>
      <Section title="Children" count={view.children.length}>
        <PlaceList rows={view.children.slice(0, SHOWN_CHILDREN)} onPick={onPick} />
        {view.children.length > SHOWN_CHILDREN && (
          <p className="text-xs text-fd-muted-foreground">
            The first {SHOWN_CHILDREN} of {view.children.length.toLocaleString('en')}; the map shows up to 500.
          </p>
        )}
      </Section>
      <Code code={view.code} />
    </div>
  );
}

function Credits({ manifest }: { manifest: Manifest }) {
  return (
    <p className="border-t pt-3 text-[11px] text-fd-muted-foreground">
      Data: the teigeodata lite build, {manifest.release}, CC BY 4.0, built from the{' '}
      <Link className="text-fd-primary" href="/docs/data">
        sources on the data page
      </Link>{' '}
      (
      <Link className="text-fd-primary" href="/docs/concepts/licences">
        credits and licences
      </Link>
      ). Map drawn with MapLibre GL JS from the pack&apos;s own outlines.
    </p>
  );
}
