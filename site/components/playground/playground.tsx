'use client';

// The playground: every query of the WebAssembly engine on the pinned pack, in this browser,
// over a Leaflet map of OpenStreetMap's tiles. Each answer shows the calls that produced it, as
// code to copy, and how long they took. Only the tiles come from elsewhere; with the base map
// off, nothing leaves the page.

import 'leaflet/dist/leaflet.css';
import Link from 'next/link';
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import type { Geometry } from 'geojson';
import { DynamicCodeBlock } from 'fumadocs-ui/components/dynamic-codeblock';
import {
  Check,
  ChevronDown,
  Code2,
  Crosshair,
  Layers,
  Link2,
  Loader2,
  LocateFixed,
  MousePointerClick,
  Radius,
  Ruler,
  Search,
  ShieldCheck,
  X,
  Zap,
} from 'lucide-react';
import { fetchManifest, formatBytes, formatMs, load, time, type Loaded, type Manifest, type Progress, type Row } from './engine';
import { createMap, type Basemap, type PlaygroundMap, type Point } from './map';

type Status =
  | { state: 'idle' }
  | { state: 'loading'; progress: Progress }
  | { state: 'ready' }
  | { state: 'error'; message: string };

type View = 'search' | 'point' | 'place' | 'measure' | 'radius';
type Tool = 'inspect' | 'measure' | 'radius';

interface Timed {
  label: string;
  ms: number;
}

/** The last thing the engine did: what the inspector shows. */
interface Action {
  title: string;
  code: string;
  timings: Timed[];
}

interface SearchView {
  query: string;
  results: Row[];
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
}

interface MeasureView {
  a?: { lat: number; lon: number };
  b?: { lat: number; lon: number };
  meters?: number;
  bearing?: number;
}

interface RadiusView {
  lat: number;
  lon: number;
  rows: Row[];
}

interface Settings {
  basemap: Basemap;
  world: boolean;
  lod: number;
  fill: number;
  weight: number;
  children: boolean;
  spokes: boolean;
  near: boolean;
  strict: boolean;
  limit: number;
  radiusM: number;
}

const DEFAULTS: Settings = {
  basemap: 'osm',
  world: false,
  lod: 0,
  fill: 0.14,
  weight: 2,
  children: true,
  spokes: true,
  near: false,
  strict: false,
  limit: 10,
  radiusM: 5000,
};

const SETTINGS_KEY = 'teigeodata-playground';
const SHOWN_CHILDREN = 60;
const MAPPED_CHILDREN = 500;

/** Examples to start from: none assumes one country. */
const EXAMPLES: { label: string; hash: string }[] = [
  { label: 'Pokhara', hash: 'q=Pokhara' },
  { label: 'काठमाडौं', hash: 'q=काठमाडौं' },
  { label: 'Kathmandu, Bagmati', hash: 'q=Kathmandu, Bagmati' },
  { label: 'Tokyo', hash: 'q=Tokyo' },
  { label: 'Everest', hash: 'at=27.98806,86.92528' },
  { label: 'Manhattan', hash: 'at=40.7128,-74.0060' },
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
const wrapLon = (lon: number) => ((((lon + 180) % 360) + 360) % 360) - 180;

function toPoint(r: Row, role: Point['role'] = 'result'): Point | null {
  const lat = num(r.lat);
  const lon = num(r.lon);
  if (!Number.isFinite(lat) || !Number.isFinite(lon)) return null;
  return { id: str(r.id), name: str(r.name), kind: str(r.kind), lat, lon, role };
}

const isPoint = (p: Point | null): p is Point => p !== null;

function compass(deg: number) {
  const names = ['N', 'NE', 'E', 'SE', 'S', 'SW', 'W', 'NW'];
  return names[Math.round((((deg % 360) + 360) % 360) / 45) % 8];
}

function distanceText(m: number) {
  return m < 1000 ? `${m.toFixed(0)} m` : `${(m / 1000).toFixed(m < 10_000 ? 2 : 1)} km`;
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

function readSettings(): Settings {
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    return raw ? { ...DEFAULTS, ...(JSON.parse(raw) as Partial<Settings>) } : DEFAULTS;
  } catch {
    return DEFAULTS;
  }
}

export function Playground() {
  const mapBox = useRef<HTMLDivElement>(null);
  const mapRef = useRef<PlaygroundMap | null>(null);
  const loadedRef = useRef<Loaded | null>(null);
  const pinRef = useRef<{ lat: number; lon: number } | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const clickRef = useRef<(lat: number, lon: number) => void>(() => {});
  const pickRef = useRef<(id: string, at?: { lat: number; lon: number }) => void>(() => {});

  const [manifest, setManifest] = useState<Manifest | null>(null);
  const [status, setStatus] = useState<Status>({ state: 'idle' });
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [settings, setSettings] = useState<Settings>(DEFAULTS);
  const [view, setView] = useState<View>('search');
  const [tool, setTool] = useState<Tool>('inspect');
  const [search, setSearch] = useState<SearchView>({ query: '', results: [] });
  const [point, setPoint] = useState<PointView | null>(null);
  const [place, setPlace] = useState<PlaceView | null>(null);
  const [measure, setMeasure] = useState<MeasureView>({});
  const [radius, setRadius] = useState<RadiusView | null>(null);
  const [action, setAction] = useState<Action | null>(null);
  const [layersOpen, setLayersOpen] = useState(false);
  const [codeOpen, setCodeOpen] = useState(false);
  const [copied, setCopied] = useState(false);

  const settingsRef = useRef(settings);
  settingsRef.current = settings;

  useEffect(() => setSettings(readSettings()), []);
  useEffect(() => {
    try {
      localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
    } catch {
      // A private window: the settings last for this visit.
    }
  }, [settings]);
  const set = useCallback(<K extends keyof Settings>(key: K, value: Settings[K]) => setSettings((s) => ({ ...s, [key]: value })), []);

  // The manifest names what loading costs before anything large is fetched.
  useEffect(() => {
    fetchManifest().then(setManifest, (e: Error) => setStatus({ state: 'error', message: e.message }));
  }, []);

  // The map, drawn before the pack so the page has its shape.
  useEffect(() => {
    let disposed = false;
    if (!mapBox.current) return;
    createMap(mapBox.current, () => disposed).then((m) => {
      if (!m) return;
      if (disposed) {
        m.remove();
        return;
      }
      mapRef.current = m;
      m.setBasemap(settingsRef.current.basemap);
      m.map.on('click', (e) => {
        if (loadedRef.current) clickRef.current(e.latlng.lat, wrapLon(e.latlng.lng));
      });
    });
    return () => {
      disposed = true;
      mapRef.current?.remove();
      mapRef.current = null;
    };
  }, []);

  const pick = useCallback((id: string, at?: { lat: number; lon: number }) => pickRef.current(id, at), []);

  // Settings that only restyle the map apply at once.
  useEffect(() => {
    mapRef.current?.setBasemap(settings.basemap);
  }, [settings.basemap]);
  useEffect(() => {
    mapRef.current?.setWorld(loaded?.world ?? null, settings.world || settings.basemap === 'none');
  }, [loaded, settings.world, settings.basemap]);
  useEffect(() => {
    mapRef.current?.styleOutline({ fillOpacity: settings.fill, weight: settings.weight });
  }, [settings.fill, settings.weight]);

  const runSearch = useCallback(
    (query: string) => {
      const l = loadedRef.current;
      const m = mapRef.current;
      if (!l) return;
      const q = query.trim();
      if (!q) {
        setSearch({ query, results: [] });
        m?.setPlaces([], pick);
        return;
      }
      const { near, strict, limit } = settingsRef.current;
      const center = m?.center();
      const options = {
        ...(near && center ? { focusLat: coord(center.lat), focusLon: coord(center.lon) } : {}),
        ...(strict ? { strict: true } : {}),
      };
      const plain = Object.keys(options).length === 0;
      try {
        const [blob, ms] = time(() => (plain ? l.engine.search(q, limit) : l.engine.searchWith(q, limit, options)));
        const [results, readMs] = time(() => l.rows(blob));
        const call = plain ? `engine.search(${quote(q)}, ${limit})` : `engine.searchWith(${quote(q)}, ${limit}, ${JSON.stringify(options)})`;
        setSearch({ query, results });
        setAction({
          title: 'Search',
          code: `for (const p of new Reader(${call})) {\n  console.log(p.id, p.name, p.kind, p.level, p.lat, p.lon, p.score);\n}`,
          timings: [
            { label: 'search', ms },
            { label: 'read', ms: readMs },
          ],
        });
        const shown = results.map((r) => toPoint(r)).filter(isPoint);
        m?.setPlaces(shown, pick);
        if (!near) m?.fitPoints(shown);
      } catch (e) {
        setSearch({ query, results: [], error: (e as Error).message });
      }
    },
    [pick],
  );

  /** Runs each query with its timing; a failure is shown, not thrown. */
  const runner = (timings: Timed[]) =>
    <T,>(label: string, f: () => T, fallback: T): T => {
      try {
        const [v, ms] = time(f);
        timings.push({ label, ms });
        return v;
      } catch {
        timings.push({ label: `${label} (error)`, ms: 0 });
        return fallback;
      }
    };

  const openPoint = useCallback(
    (latIn: number, lonIn: number, move = false) => {
      const l = loadedRef.current;
      const m = mapRef.current;
      if (!l) return;
      const lat = coord(latIn);
      const lon = coord(wrapLon(lonIn));
      const timings: Timed[] = [];
      const run = runner(timings);
      const admin = run('adminAt', () => l.rows(l.engine.adminAt(lat, lon)), [] as Row[]);
      const [zone] = run('tzAt', () => l.rows(l.engine.tzAt(lat, lon)), [] as Row[]);
      const now = Math.floor(Date.now() / 1000);
      const [offset] = zone ? run('tzOffsetAt', () => l.rows(l.engine.tzOffsetAt(str(zone.tz), now)), [] as Row[]) : [];
      const [height] = run('elevationAt', () => l.rows(l.engine.elevationAt(lat, lon)), [] as Row[]);
      const nearest = run('reverse', () => l.rows(l.engine.reverse(lat, lon, 5)), [] as Row[]);
      pinRef.current = { lat, lon };
      if (m) {
        m.setPin({ lat, lon });
        m.setOutline(null, { fillOpacity: settingsRef.current.fill, weight: settingsRef.current.weight });
        m.setMarks({});
        const shown = nearest.map((r) => toPoint(r, 'near')).filter(isPoint);
        m.setPlaces(shown, pick);
        m.setSpokes(
          settingsRef.current.spokes ? { lat, lon } : null,
          shown.map((p, i) => ({ lat: p.lat, lon: p.lon, label: distanceText(num(nearest[i]?.meters)) })),
        );
        if (move) m.flyTo(lat, lon, 11);
      }
      setPoint({ lat, lon, admin, zone, offset, height, nearest });
      setAction({
        title: 'At a point',
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
      setView('point');
      history.replaceState(null, '', `#at=${lat},${lon}`);
    },
    [pick],
  );

  const openPlace = useCallback(
    (id: string, lodIn?: number) => {
      const l = loadedRef.current;
      const m = mapRef.current;
      if (!l) return;
      const s = settingsRef.current;
      const lod = lodIn ?? s.lod;
      const timings: Timed[] = [];
      const run = runner(timings);
      const [record] = run('get', () => l.rows(l.engine.get(id)), [] as Row[]);
      if (!record) {
        setPlace(null);
        setView('place');
        return;
      }
      const ancestors = run('ancestors', () => l.rows(l.engine.ancestors(id)), [] as Row[]);
      const children = run('children', () => l.rows(l.engine.children(id)), [] as Row[]);
      const nearby = run('nearby', () => l.rows(l.engine.nearby(id, 5)), [] as Row[]);
      const [shape] = run('boundary', () => l.rows(l.engine.boundary(id, lod)), [] as Row[]);
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
          ? { km: l.distanceM(pin.lat, pin.lon, here.lat, here.lon) / 1000, bearing: l.bearingDeg(pin.lat, pin.lon, here.lat, here.lon) }
          : undefined;
      if (m) {
        m.setOutline(outline?.geometry ?? null, { fillOpacity: s.fill, weight: s.weight });
        m.setSpokes(null, []);
        m.setMarks({});
        m.setPlaces(
          [
            ...(s.children ? children.slice(0, MAPPED_CHILDREN).map((r) => toPoint(r, 'child')) : []),
            ...nearby.map((r) => toPoint(r, 'near')),
            here,
          ].filter(isPoint),
          pick,
        );
        if (lodIn === undefined) {
          if (outline) m.fitGeometry(outline.geometry);
          else if (here) m.flyTo(here.lat, here.lon, 12);
        }
      }
      setPlace({ id, place: record, ancestors, children, nearby, outline, fromPin });
      setAction({
        title: str(record.name) || 'A place',
        timings,
        code: [
          `const [place] = new Reader(engine.get(${quote(id)})).records();`,
          `const ancestors = new Reader(engine.ancestors(${quote(id)})).records(); // parent first`,
          `const children = new Reader(engine.children(${quote(id)}));`,
          `const nearby = new Reader(engine.nearby(${quote(id)}, 5)).records();`,
          `const [shape] = new Reader(engine.boundary(${quote(id)}, ${lod})).records(); // level of detail ${lod}`,
          `L.geoJSON(JSON.parse(shape.geojson)).addTo(map); // a GeoJSON Feature`,
        ].join('\n'),
      });
      setView('place');
      history.replaceState(null, '', `#place=${encodeURIComponent(id)}`);
    },
    [pick],
  );

  const runMeasure = useCallback((lat: number, lon: number) => {
    const l = loadedRef.current;
    const m = mapRef.current;
    if (!l) return;
    const at = { lat: coord(lat), lon: coord(lon) };
    setMeasure((prev) => {
      // A third click starts a new line.
      if (!prev.a || prev.b) {
        m?.setMarks({ start: at });
        return { a: at };
      }
      const a = prev.a;
      const [meters, ms] = time(() => l.distanceM(a.lat, a.lon, at.lat, at.lon));
      const [bearing, bms] = time(() => l.bearingDeg(a.lat, a.lon, at.lat, at.lon));
      m?.setMarks({ line: [a, at, `${distanceText(meters)} · ${bearing.toFixed(0)}°`] });
      setAction({
        title: 'Measure',
        timings: [
          { label: 'distanceM', ms },
          { label: 'bearingDeg', ms: bms },
        ],
        code: `const meters = distanceM(${a.lat}, ${a.lon}, ${at.lat}, ${at.lon});\nconst heading = bearingDeg(${a.lat}, ${a.lon}, ${at.lat}, ${at.lon}); // degrees from north`,
      });
      return { a, b: at, meters, bearing };
    });
    setView('measure');
  }, []);

  const runRadius = useCallback(
    (lat: number, lon: number, radiusM?: number) => {
      const l = loadedRef.current;
      const m = mapRef.current;
      if (!l) return;
      const r = radiusM ?? settingsRef.current.radiusM;
      const at = { lat: coord(lat), lon: coord(lon) };
      const [blob, ms] = time(() => l.engine.withinRadius(at.lat, at.lon, r, 200));
      const [rows, readMs] = time(() => l.rows(blob));
      if (m) {
        m.setPin(at);
        m.setSpokes(null, []);
        m.setOutline(null, { fillOpacity: 0, weight: 0 });
        m.setMarks({ circle: { ...at, m: r } });
        m.fitCircle(at, r, radiusM === undefined);
        m.setPlaces(rows.map((row) => toPoint(row, 'near')).filter(isPoint), pick);
      }
      pinRef.current = at;
      setRadius({ ...at, rows });
      setAction({
        title: 'Within a radius',
        timings: [
          { label: 'withinRadius', ms },
          { label: 'read', ms: readMs },
        ],
        code: `const within = new Reader(engine.withinRadius(${at.lat}, ${at.lon}, ${r}, 200)); // nearest first\nconsole.log(within.rows, "places; farthest", within.numbers("meters").at(-1), "m");`,
      });
      setView('radius');
    },
    [pick],
  );

  clickRef.current = (lat, lon) => {
    if (tool === 'measure') runMeasure(lat, lon);
    else if (tool === 'radius') runRadius(lat, lon);
    else openPoint(lat, lon);
  };
  // With Measure or Radius on, a place on the map is a point like any other.
  pickRef.current = (id, at) => {
    if (at && tool !== 'inspect') clickRef.current(at.lat, at.lon);
    else openPlace(id);
  };

  // A new level of detail redraws the open place's outline in place.
  useEffect(() => {
    if (place && view === 'place') openPlace(place.id, settings.lod);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settings.lod, settings.children]);
  useEffect(() => {
    if (radius && view === 'radius') runRadius(radius.lat, radius.lon, settings.radiusM);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settings.radiusM]);

  /** Opens what the URL hash names: `#q=`, `#at=lat,lon` or `#place=id`. */
  const followHash = useCallback(() => {
    const hash = decodeURIComponent(location.hash.slice(1));
    const [key, ...rest] = hash.split('=');
    const value = rest.join('=');
    if (key === 'q' && value) {
      runSearch(value);
      setView('search');
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

  // `/` focuses the search, as on most map apps.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      if (e.key === '/' && target?.tagName !== 'INPUT' && target?.tagName !== 'TEXTAREA') {
        e.preventDefault();
        setView('search');
        searchRef.current?.focus();
      }
      if (e.key === 'Escape') {
        setLayersOpen(false);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const start = useCallback(async () => {
    if (!manifest) return;
    setStatus({ state: 'loading', progress: { phase: 'engine' } });
    try {
      const l = await load(manifest, (progress) => setStatus({ state: 'loading', progress }));
      loadedRef.current = l;
      setLoaded(l);
      setStatus({ state: 'ready' });
      if (location.hash.length > 1) followHash();
      else searchRef.current?.focus();
    } catch (e) {
      setStatus({ state: 'error', message: (e as Error).message });
    }
  }, [manifest, followHash]);

  const chooseTool = (t: Tool) => {
    setTool(t);
    mapRef.current?.setMarks({});
    if (t === 'measure') {
      setMeasure({});
      setView('measure');
    } else if (t === 'radius') {
      setView('radius');
    }
  };

  const locate = () => {
    navigator.geolocation?.getCurrentPosition(
      (p) => openPoint(p.coords.latitude, p.coords.longitude, true),
      () => {},
      { enableHighAccuracy: false, timeout: 8000 },
    );
  };

  const copyLink = () => {
    void navigator.clipboard?.writeText(location.href).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  };

  const ready = status.state === 'ready' && loaded;
  const hint =
    tool === 'measure'
      ? measure.a && !measure.b
        ? 'Click a second point'
        : 'Click two points to measure'
      : tool === 'radius'
        ? `Click to list places within ${distanceText(settings.radiusM)}`
        : 'Click the map for what is at a point';

  return (
    <div className="tg-playground relative min-h-0 flex-1 overflow-hidden" data-testid="playground" data-state={status.state}>
      <div ref={mapBox} className="absolute inset-0 z-0" data-testid="map" data-tool={tool} />

      {!ready && <Intro manifest={manifest} status={status} onStart={start} />}

      {ready && (
        <>
          {/* The panel */}
          <aside className="tg-glass absolute inset-x-2 bottom-2 z-[500] flex max-h-[52%] flex-col overflow-hidden rounded-2xl md:inset-x-auto md:top-3 md:bottom-3 md:left-3 md:max-h-none md:w-[400px]">
            <div className="space-y-3 border-b p-4">
              <Header loaded={loaded} />
              <div className="relative">
                <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-fd-muted-foreground" aria-hidden="true" />
                <input
                  ref={searchRef}
                  type="search"
                  value={search.query}
                  onChange={(e) => {
                    setView('search');
                    runSearch(e.target.value);
                  }}
                  onKeyDown={(e) => {
                    const first = search.results[0];
                    if (e.key === 'Enter' && first) openPlace(str(first.id));
                  }}
                  placeholder="Search places in any script"
                  aria-label="Search"
                  data-testid="search"
                  className="h-11 w-full rounded-xl border bg-fd-background/80 ps-10 pe-12 text-sm shadow-sm outline-none transition-[border-color,box-shadow] placeholder:text-fd-muted-foreground focus:border-fd-primary focus:ring-4 focus:ring-fd-primary/15"
                />
                <kbd className="pointer-events-none absolute top-1/2 right-3 -translate-y-1/2 rounded-md border px-1.5 font-mono text-[11px] text-fd-muted-foreground">/</kbd>
              </div>
              <div className="flex flex-wrap gap-1.5" aria-label="Examples">
                {EXAMPLES.map((e) => (
                  <a
                    key={e.hash}
                    href={`#${e.hash}`}
                    className="rounded-full border bg-fd-background/60 px-2.5 py-0.5 text-xs text-fd-muted-foreground transition-colors hover:border-fd-primary/60 hover:text-fd-foreground"
                  >
                    {e.label}
                  </a>
                ))}
              </div>
              <nav role="tablist" className="-mb-4 flex gap-4 text-sm">
                {(
                  [
                    ['search', 'Search'],
                    ['point', 'Point'],
                    ['place', 'Place'],
                    ...(tool === 'measure' ? [['measure', 'Measure'] as const] : []),
                    ...(tool === 'radius' ? [['radius', 'Radius'] as const] : []),
                  ] as const
                ).map(([key, label]) => (
                  <button
                    key={key}
                    role="tab"
                    type="button"
                    aria-selected={view === key}
                    onClick={() => setView(key)}
                    className={`relative pb-3 font-medium transition-colors ${
                      view === key
                        ? 'text-fd-foreground after:absolute after:inset-x-0 after:-bottom-px after:h-0.5 after:rounded-full after:bg-fd-primary'
                        : 'text-fd-muted-foreground hover:text-fd-foreground'
                    }`}
                  >
                    {label}
                  </button>
                ))}
              </nav>
            </div>
            <div className="tg-scroll min-h-0 flex-1 overflow-y-auto p-4">
              <div key={view} className="tg-fade">
                {view === 'search' && <SearchPanel view={search} onPick={openPlace} settings={settings} set={set} rerun={() => runSearch(search.query)} />}
                {view === 'point' && <PointPanel view={point} onPick={openPlace} />}
                {view === 'place' && <PlacePanel view={place} onPick={openPlace} />}
                {view === 'measure' && <MeasurePanel view={measure} />}
                {view === 'radius' && <RadiusPanel view={radius} settings={settings} set={set} onPick={openPlace} />}
              </div>
            </div>
            <Credits manifest={loaded.manifest} basemap={settings.basemap} />
          </aside>

          {/* Tools */}
          <div className="absolute top-3 left-1/2 z-[500] flex -translate-x-1/2 items-center gap-1 rounded-full p-1 tg-glass md:left-[calc(50%+208px)]" role="toolbar" aria-label="Map tool">
            {(
              [
                ['inspect', 'Inspect', MousePointerClick],
                ['measure', 'Measure', Ruler],
                ['radius', 'Radius', Radius],
              ] as const
            ).map(([key, label, Icon]) => (
              <button
                key={key}
                type="button"
                aria-pressed={tool === key}
                onClick={() => chooseTool(key)}
                className={`inline-flex h-8 items-center gap-1.5 rounded-full px-3 text-xs font-medium transition-colors ${
                  tool === key ? 'bg-fd-foreground text-fd-background' : 'text-fd-muted-foreground hover:bg-fd-accent hover:text-fd-foreground'
                }`}
              >
                <Icon className="size-3.5" aria-hidden="true" />
                <span className="hidden sm:inline">{label}</span>
              </button>
            ))}
          </div>
          <p className="pointer-events-none absolute top-16 left-1/2 z-[400] hidden -translate-x-1/2 rounded-full px-3 py-1 text-xs text-fd-muted-foreground tg-glass md:left-[calc(50%+208px)] md:block">
            {hint}
          </p>

          {/* Map actions */}
          <div className="absolute top-3 right-3 z-[500] flex flex-col gap-2">
            <IconButton label="Layers and style" onClick={() => setLayersOpen((o) => !o)} active={layersOpen}>
              <Layers className="size-4" />
            </IconButton>
            <IconButton label="My location" onClick={locate}>
              <LocateFixed className="size-4" />
            </IconButton>
            <IconButton label={copied ? 'Link copied' : 'Copy a link to this view'} onClick={copyLink}>
              {copied ? <Check className="size-4 text-fd-primary" /> : <Link2 className="size-4" />}
            </IconButton>
          </div>
          {layersOpen && <LayersCard settings={settings} set={set} onClose={() => setLayersOpen(false)} hasPlace={!!place} />}

          {/* The inspector */}
          {action && (
            <div className="absolute right-3 bottom-24 z-[500] hidden w-[min(520px,calc(100%-440px))] md:block">
              <div className="tg-glass overflow-hidden rounded-2xl">
                <button
                  type="button"
                  onClick={() => setCodeOpen((o) => !o)}
                  aria-expanded={codeOpen}
                  className="flex w-full items-center justify-between gap-3 px-4 py-2.5 text-left text-xs"
                >
                  <span className="flex min-w-0 items-center gap-2">
                    <Code2 className="size-3.5 shrink-0 text-fd-primary" aria-hidden="true" />
                    <span className="truncate font-medium">{action.title}</span>
                    <span className="text-fd-muted-foreground">the code</span>
                  </span>
                  <span className="flex items-center gap-2 text-fd-muted-foreground">
                    <Zap className="size-3 text-fd-primary" aria-hidden="true" />
                    <span className="tabular-nums text-fd-foreground">{formatMs(action.timings.reduce((a, t) => a + t.ms, 0))}</span>
                    <ChevronDown className={`size-3.5 transition-transform ${codeOpen ? 'rotate-180' : ''}`} aria-hidden="true" />
                  </span>
                </button>
                {codeOpen && (
                  <div className="tg-fade border-t px-3 pt-1 pb-3">
                    <div className="[&_figure]:my-2 [&_pre]:max-h-60 [&_pre]:text-xs">
                      <DynamicCodeBlock lang="js" code={action.code} />
                    </div>
                    <Timings timings={action.timings} />
                  </div>
                )}
              </div>
            </div>
          )}
        </>
      )}
    </div>
  );
}

function IconButton({ label, onClick, active, children }: { label: string; onClick: () => void; active?: boolean; children: ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      title={label}
      aria-pressed={active}
      className={`tg-glass flex size-10 items-center justify-center rounded-xl transition-colors hover:text-fd-primary ${active ? 'text-fd-primary' : 'text-fd-foreground'}`}
    >
      {children}
    </button>
  );
}

function Intro({ manifest, status, onStart }: { manifest: Manifest | null; status: Status; onStart: () => void }) {
  const loading = status.state === 'loading';
  return (
    <div className="absolute inset-0 z-[600] flex items-center justify-center bg-fd-background/40 p-4 backdrop-blur-[2px]">
      <div className="tg-glass tg-fade w-full max-w-md space-y-5 rounded-3xl p-6 md:p-7">
        <div>
          <p className="font-mono text-xs tracking-[0.18em] text-fd-primary uppercase">Playground</p>
          <h1 className="mt-2 text-2xl font-semibold tracking-tight">Every query, in your browser</h1>
          <p className="mt-2 text-sm text-fd-muted-foreground">
            The WebAssembly engine and a released pack, run in this tab. Search as you type, click the map for what is at a
            point, open a place for its hierarchy and outline, measure, list what is within a radius. Each answer shows its
            code and how long it took.
          </p>
        </div>
        <ul className="space-y-2.5 text-sm">
          <li className="flex gap-2.5">
            <Zap className="mt-0.5 size-4 shrink-0 text-fd-primary" aria-hidden="true" />
            <span>
              <strong className="font-medium">Every query runs here.</strong>{' '}
              <span className="text-fd-muted-foreground">Only the map&apos;s tiles come from OpenStreetMap; turn them off for none.</span>
            </span>
          </li>
          <li className="flex gap-2.5">
            <ShieldCheck className="mt-0.5 size-4 shrink-0 text-fd-primary" aria-hidden="true" />
            <span>
              <strong className="font-medium">Checked.</strong>{' '}
              <span className="text-fd-muted-foreground">The pack&apos;s SHA-256 is checked before it opens; the engine checks each section it reads.</span>
            </span>
          </li>
        </ul>
        {manifest && (
          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 rounded-xl border bg-fd-background/60 p-3.5 text-xs">
            <dt className="text-fd-muted-foreground">Pack</dt>
            <dd>
              <code>{manifest.file}</code> · {formatBytes(manifest.bytes)} · {manifest.release}
            </dd>
            <dt className="text-fd-muted-foreground">Engine</dt>
            <dd>
              <code>{manifest.package}</code> {manifest.version} · {formatBytes(manifest.engine_bytes)}
            </dd>
          </dl>
        )}
        {status.state === 'error' ? (
          <p role="alert" className="rounded-xl border border-red-500/40 bg-red-500/10 p-3 text-sm" data-testid="error">
            {status.message}
          </p>
        ) : (
          <button
            type="button"
            onClick={onStart}
            disabled={!manifest || loading}
            data-testid="load"
            className="inline-flex h-11 w-full items-center justify-center gap-2 rounded-full bg-fd-foreground px-5 text-sm font-medium text-fd-background transition-[opacity,transform] hover:opacity-90 active:scale-[0.99] disabled:opacity-60"
          >
            {loading && <Loader2 className="size-4 animate-spin" aria-hidden="true" />}
            {loading ? 'Loading…' : manifest ? `Start the playground · ${formatBytes(manifest.bytes + manifest.engine_bytes)}` : 'Reading what to load…'}
          </button>
        )}
        {status.state === 'loading' && <ProgressBar progress={status.progress} />}
        <p className="text-xs text-fd-muted-foreground">
          The same calls work in an app: see{' '}
          <Link className="text-fd-primary hover:underline" href="/docs/tutorials/in-the-browser/">
            in the browser
          </Link>{' '}
          and the{' '}
          <Link className="text-fd-primary hover:underline" href="/docs/reference">
            reference
          </Link>
          .
        </p>
      </div>
    </div>
  );
}

function ProgressBar({ progress }: { progress: Progress }) {
  const label = {
    engine: 'Loading the engine',
    download: 'Downloading the pack',
    verify: 'Checking its SHA-256',
    open: 'Opening the pack',
    world: 'Reading every country',
  }[progress.phase];
  const share = progress.phase === 'download' ? progress.loaded / progress.total : progress.phase === 'engine' ? 0.02 : 1;
  return (
    <div className="space-y-1.5" aria-live="polite">
      <div className="flex justify-between text-xs text-fd-muted-foreground">
        <span>{label}</span>
        {progress.phase === 'download' && (
          <span className="tabular-nums">
            {formatBytes(progress.loaded)} of {formatBytes(progress.total)}
          </span>
        )}
      </div>
      <div className="h-1 overflow-hidden rounded-full bg-fd-secondary">
        <div className="h-full rounded-full bg-fd-primary transition-[width] duration-300" style={{ width: `${(share * 100).toFixed(1)}%` }} />
      </div>
    </div>
  );
}

function Header({ loaded }: { loaded: Loaded }) {
  const { manifest, engine, countries } = loaded;
  return (
    <div className="flex items-center justify-between gap-3">
      <h1 className="text-base font-semibold tracking-tight">Playground</h1>
      <p
        className="inline-flex items-center gap-1.5 rounded-full border bg-fd-background/60 px-2.5 py-0.5 text-[11px] text-fd-muted-foreground"
        data-testid="ready"
        title={`${manifest.file} (${manifest.release}) in ${manifest.package} ${manifest.version}; tzdb ${engine.tzVersion()}; ${countries} countries`}
      >
        <span className="relative flex size-1.5">
          <span className="absolute inline-flex size-full animate-ping rounded-full bg-emerald-500 opacity-60" />
          <span className="relative inline-flex size-1.5 rounded-full bg-emerald-500" />
        </span>
        {manifest.release} · tzdb {engine.tzVersion()}
      </p>
    </div>
  );
}

function Timings({ timings }: { timings: Timed[] }) {
  if (!timings.length) return null;
  return (
    <p className="flex flex-wrap gap-1.5 text-[11px] text-fd-muted-foreground">
      {timings.map((t) => (
        <span key={t.label} className="rounded-full border px-2 py-0.5">
          {t.label} <span className="tabular-nums text-fd-foreground">{formatMs(t.ms)}</span>
        </span>
      ))}
    </p>
  );
}

function Section({ title, children, count }: { title: string; children: ReactNode; count?: number }) {
  return (
    <section className="space-y-2">
      <h2 className="flex items-center gap-1.5 text-[11px] font-semibold tracking-wider text-fd-muted-foreground uppercase">
        {title}
        {count !== undefined && <span className="rounded-full bg-fd-secondary px-1.5 font-medium tabular-nums normal-case">{count.toLocaleString('en')}</span>}
      </h2>
      {children}
    </section>
  );
}

function Empty({ icon, children }: { icon: ReactNode; children: ReactNode }) {
  return (
    <div className="flex flex-col items-center gap-2 rounded-xl border border-dashed px-4 py-8 text-center text-sm text-fd-muted-foreground">
      <span className="text-fd-primary">{icon}</span>
      {children}
    </div>
  );
}

function PlaceList({ rows, onPick, extra }: { rows: Row[]; onPick: (id: string) => void; extra?: (r: Row) => ReactNode }) {
  if (!rows.length) return <p className="text-sm text-fd-muted-foreground">None.</p>;
  return (
    <ul className="divide-y overflow-hidden rounded-xl border bg-fd-background/50">
      {rows.map((r, i) => (
        <li key={`${str(r.id)}-${i}`}>
          <button
            type="button"
            onClick={() => onPick(str(r.id))}
            data-id={str(r.id)}
            className="flex w-full items-center justify-between gap-3 px-3 py-2.5 text-left text-sm transition-colors hover:bg-fd-accent"
          >
            <span className="min-w-0">
              <span className="font-medium">{str(r.name)}</span>{' '}
              <span className="text-xs text-fd-muted-foreground">
                {str(r.kind)}
                {r.kind === 'admin' ? ` · level ${str(r.level)}` : ''}
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

function Toggle({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="flex cursor-pointer items-center justify-between gap-3 text-sm">
      <span>{label}</span>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        onClick={() => onChange(!checked)}
        className={`relative h-5 w-9 shrink-0 rounded-full transition-colors ${checked ? 'bg-fd-primary' : 'bg-fd-secondary ring-1 ring-fd-border ring-inset'}`}
      >
        <span className={`absolute top-0.5 left-0.5 size-4 rounded-full bg-white shadow transition-transform ${checked ? 'translate-x-4' : ''}`} />
      </button>
    </label>
  );
}

function Slider({
  label,
  value,
  min,
  max,
  step,
  format,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  format: (v: number) => string;
  onChange: (v: number) => void;
}) {
  return (
    <label className="block space-y-1.5 text-sm">
      <span className="flex justify-between">
        <span>{label}</span>
        <span className="tabular-nums text-fd-muted-foreground">{format(value)}</span>
      </span>
      <input type="range" min={min} max={max} step={step} value={value} onChange={(e) => onChange(Number(e.target.value))} className="tg-range w-full" />
    </label>
  );
}

type Set = <K extends keyof Settings>(key: K, value: Settings[K]) => void;

function LayersCard({ settings, set, onClose, hasPlace }: { settings: Settings; set: Set; onClose: () => void; hasPlace: boolean }) {
  return (
    <div className="tg-glass tg-fade absolute top-3 right-16 z-[600] w-72 space-y-4 rounded-2xl p-4" role="dialog" aria-label="Layers and style">
      <div className="flex items-center justify-between">
        <p className="text-sm font-semibold">Layers and style</p>
        <button type="button" onClick={onClose} aria-label="Close" className="rounded-md p-1 text-fd-muted-foreground hover:bg-fd-accent">
          <X className="size-4" />
        </button>
      </div>
      <div className="space-y-2">
        <p className="text-[11px] font-semibold tracking-wider text-fd-muted-foreground uppercase">Base map</p>
        <div className="grid grid-cols-2 gap-1 rounded-xl border p-1 text-xs">
          {(
            [
              ['osm', 'OpenStreetMap'],
              ['none', 'Pack only'],
            ] as const
          ).map(([key, label]) => (
            <button
              key={key}
              type="button"
              aria-pressed={settings.basemap === key}
              onClick={() => set('basemap', key)}
              className={`rounded-lg px-2 py-1.5 font-medium transition-colors ${settings.basemap === key ? 'bg-fd-foreground text-fd-background' : 'text-fd-muted-foreground hover:text-fd-foreground'}`}
            >
              {label}
            </button>
          ))}
        </div>
        <Toggle label="Country outlines from the pack" checked={settings.world || settings.basemap === 'none'} onChange={(v) => set('world', v)} />
      </div>
      <div className="space-y-3">
        <p className="text-[11px] font-semibold tracking-wider text-fd-muted-foreground uppercase">Polygons</p>
        <Slider
          label="Level of detail"
          value={settings.lod}
          min={0}
          max={6}
          step={1}
          format={(v) => (v === 0 ? '0 · full' : String(v))}
          onChange={(v) => set('lod', v)}
        />
        <Slider label="Fill" value={settings.fill} min={0} max={0.6} step={0.02} format={(v) => `${Math.round(v * 100)}%`} onChange={(v) => set('fill', v)} />
        <Slider label="Stroke" value={settings.weight} min={0.5} max={5} step={0.5} format={(v) => `${v} px`} onChange={(v) => set('weight', v)} />
        <Toggle label="Children of a place" checked={settings.children} onChange={(v) => set('children', v)} />
        {!hasPlace && <p className="text-xs text-fd-muted-foreground">Open a place to see its outline.</p>}
      </div>
      <div className="space-y-2">
        <p className="text-[11px] font-semibold tracking-wider text-fd-muted-foreground uppercase">At a point</p>
        <Toggle label="Lines to the nearest places" checked={settings.spokes} onChange={(v) => set('spokes', v)} />
      </div>
    </div>
  );
}

function SearchPanel({ view, onPick, settings, set, rerun }: { view: SearchView; onPick: (id: string) => void; settings: Settings; set: Set; rerun: () => void }) {
  const toggle = (key: 'near' | 'strict', v: boolean) => {
    set(key, v);
    setTimeout(rerun, 0);
  };
  return (
    <div className="space-y-4" data-testid="search-panel">
      <div className="flex flex-wrap gap-1.5 text-xs">
        <Chip active={settings.near} onClick={() => toggle('near', !settings.near)}>
          <Crosshair className="size-3" aria-hidden="true" /> Near the map centre
        </Chip>
        <Chip active={settings.strict} onClick={() => toggle('strict', !settings.strict)}>
          Exact matches first
        </Chip>
      </div>
      {view.error && <p className="text-sm text-red-600">{view.error}</p>}
      {view.query.trim() && !view.error ? (
        <Section title="Results" count={view.results.length}>
          <PlaceList rows={view.results} onPick={onPick} extra={(r) => fixed(r.score, 0)} />
        </Section>
      ) : (
        <Empty icon={<Search className="size-5" />}>
          <span>
            Search runs on every keystroke. Try a prefix (<em>Pokh</em>), a typo (<em>Kathamandu</em>), another script (
            <em>पोखरा</em>) or a parent after a comma (<em>Kathmandu, Bagmati</em>).
          </span>
        </Empty>
      )}
    </div>
  );
}

function Chip({ active, onClick, children }: { active: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      className={`inline-flex items-center gap-1 rounded-full border px-2.5 py-1 font-medium transition-colors ${
        active ? 'border-fd-primary/60 bg-fd-primary/10 text-fd-primary' : 'text-fd-muted-foreground hover:text-fd-foreground'
      }`}
    >
      {children}
    </button>
  );
}

function Stat({ label, children, testid }: { label: string; children: ReactNode; testid?: string }) {
  return (
    <div className="rounded-xl border bg-fd-background/50 p-3">
      <p className="text-[11px] text-fd-muted-foreground">{label}</p>
      <div className="mt-1 text-sm" data-testid={testid}>
        {children}
      </div>
    </div>
  );
}

function PointPanel({ view, onPick }: { view: PointView | null; onPick: (id: string) => void }) {
  if (!view) return <Empty icon={<MousePointerClick className="size-5" />}>Click anywhere on the map.</Empty>;
  const { zone, offset, height } = view;
  return (
    <div className="space-y-5" data-testid="point-panel">
      <p className="font-mono text-sm">
        {view.lat}, {view.lon}
      </p>
      <div className="grid grid-cols-2 gap-2">
        <Stat label="Timezone" testid={zone ? 'zone' : undefined}>
          {zone ? (
            <>
              <strong className="font-semibold">{str(zone.tz)}</strong>
              {offset && (
                <span className="block text-xs text-fd-muted-foreground">
                  {offsetText(num(offset.seconds))} {str(offset.abbr)}
                  {offset.dst ? ' · summer' : ''}
                </span>
              )}
              <span className="block text-xs text-fd-muted-foreground">{localTime(str(zone.tz))}</span>
            </>
          ) : (
            <span className="text-fd-muted-foreground">None</span>
          )}
        </Stat>
        <Stat label="Elevation" testid={height ? 'height' : undefined}>
          {height ? (
            <>
              <strong className="font-semibold">{fixed(height.meters, 1)} m</strong>
              <span className="block text-xs text-fd-muted-foreground">
                above {str(height.datum)} · {str(height.source)}, {fixed(height.resolution_m, 0)} m grid
              </span>
            </>
          ) : (
            <span className="text-fd-muted-foreground">No terrain here</span>
          )}
        </Stat>
      </div>
      <Section title="Admin units, country first" count={view.admin.length}>
        {view.admin.length ? (
          <ol className="relative space-y-1 border-s ps-4 text-sm">
            {view.admin.map((r) => (
              <li key={str(r.id)} className="relative flex justify-between gap-3">
                <span className="absolute top-2 -left-[21px] size-2 rounded-full border-2 border-fd-primary bg-fd-background" />
                <button type="button" className="text-left transition-colors hover:text-fd-primary" onClick={() => onPick(str(r.id))}>
                  {str(r.name)} <span className="text-xs text-fd-muted-foreground">level {str(r.level)}</span>
                </button>
                <span className="font-mono text-xs text-fd-muted-foreground">{str(r.iso)}</span>
              </li>
            ))}
          </ol>
        ) : (
          <p className="text-sm text-fd-muted-foreground">None: open water, or a unit the pack has no outline for.</p>
        )}
      </Section>
      <Section title="Nearest places" count={view.nearest.length}>
        <PlaceList rows={view.nearest} onPick={onPick} extra={(r) => `${distanceText(num(r.meters))} ${compass(num(r.bearing))}`} />
      </Section>
    </div>
  );
}

function PlacePanel({ view, onPick }: { view: PlaceView | null; onPick: (id: string) => void }) {
  if (!view) return <Empty icon={<Search className="size-5" />}>Pick a place from a search, a point or the map.</Empty>;
  const p = view.place;
  return (
    <div className="space-y-5" data-testid="place-panel">
      <div>
        <nav aria-label="Ancestors" className="flex flex-wrap items-center gap-1 text-xs text-fd-muted-foreground" data-testid="ancestors">
          {[...view.ancestors].reverse().map((a, i) => (
            <span key={str(a.id)} className="flex items-center gap-1">
              {i > 0 && <span>›</span>}
              <button type="button" className="transition-colors hover:text-fd-primary" onClick={() => onPick(str(a.id))}>
                {str(a.name)}
              </button>
            </span>
          ))}
        </nav>
        <h2 className="mt-1.5 text-2xl font-semibold tracking-tight">{str(p.name)}</h2>
        <p className="mt-0.5 text-xs text-fd-muted-foreground">
          {str(p.kind)}
          {p.kind === 'admin' ? ` · level ${str(p.level)}` : ''} · <span className="font-mono">{view.id}</span>
        </p>
      </div>
      <div className="grid grid-cols-2 gap-2">
        <Stat label="Coordinates">
          <span className="font-mono text-xs">
            {fixed(p.lat, 5)}, {fixed(p.lon, 5)}
          </span>
        </Stat>
        <Stat label="Elevation">{p.elevation === null ? '–' : `${fixed(p.elevation, 0)} m`}</Stat>
        <Stat label="Timezone">{str(p.tz) || '–'}</Stat>
        <Stat label={p.iso !== null && p.iso !== undefined ? 'ISO 3166' : 'Population'}>
          {p.iso !== null && p.iso !== undefined
            ? str(p.iso)
            : p.population !== null && p.population !== undefined
              ? `${num(p.population).toLocaleString('en')}${p.population_kind ? ` · ${str(p.population_kind)}` : ''}`
              : '–'}
        </Stat>
      </div>
      {p.iso !== null && p.iso !== undefined && p.population !== null && p.population !== undefined && (
        <p className="text-sm">
          <span className="text-fd-muted-foreground">Population</span> {num(p.population).toLocaleString('en')}{' '}
          <span className="text-xs text-fd-muted-foreground">({str(p.population_kind)})</span>
        </p>
      )}
      <dl className="space-y-1 text-sm">
        {view.fromPin && (
          <div className="flex justify-between gap-3">
            <dt className="text-fd-muted-foreground">From the pin</dt>
            <dd>
              {view.fromPin.km.toFixed(1)} km · {view.fromPin.bearing.toFixed(0)}° {compass(view.fromPin.bearing)}
            </dd>
          </div>
        )}
        <div className="flex justify-between gap-3">
          <dt className="text-fd-muted-foreground">Outline</dt>
          <dd className="text-right">
            {view.outline
              ? `${view.outline.vertices.toLocaleString('en')} vertices · level ${view.outline.lod} · ${formatBytes(view.outline.bytes)}`
              : 'none: a point place'}
          </dd>
        </div>
      </dl>
      <Section title="Nearby" count={view.nearby.length}>
        <PlaceList rows={view.nearby} onPick={onPick} extra={(r) => (r.meters !== undefined ? distanceText(num(r.meters)) : '')} />
      </Section>
      <Section title="Children" count={view.children.length}>
        <PlaceList rows={view.children.slice(0, SHOWN_CHILDREN)} onPick={onPick} />
        {view.children.length > SHOWN_CHILDREN && (
          <p className="text-xs text-fd-muted-foreground">
            The first {SHOWN_CHILDREN} of {view.children.length.toLocaleString('en')}; the map shows up to {MAPPED_CHILDREN}.
          </p>
        )}
      </Section>
    </div>
  );
}

function MeasurePanel({ view }: { view: MeasureView }) {
  if (!view.a) return <Empty icon={<Ruler className="size-5" />}>Click two points on the map: the great-circle distance and the bearing between them.</Empty>;
  if (!view.b || view.meters === undefined || view.bearing === undefined)
    return <Empty icon={<Ruler className="size-5" />}>Now click a second point.</Empty>;
  return (
    <div className="space-y-4">
      <div className="grid grid-cols-2 gap-2">
        <Stat label="Distance">
          <strong className="text-lg font-semibold">{distanceText(view.meters)}</strong>
        </Stat>
        <Stat label="Initial bearing">
          <strong className="text-lg font-semibold">
            {view.bearing.toFixed(1)}° {compass(view.bearing)}
          </strong>
        </Stat>
      </div>
      <dl className="space-y-1 font-mono text-xs">
        <div className="flex justify-between">
          <dt className="text-fd-muted-foreground">From</dt>
          <dd>
            {view.a.lat}, {view.a.lon}
          </dd>
        </div>
        <div className="flex justify-between">
          <dt className="text-fd-muted-foreground">To</dt>
          <dd>
            {view.b.lat}, {view.b.lon}
          </dd>
        </div>
      </dl>
      <p className="text-xs text-fd-muted-foreground">
        Haversine on the WGS84 mean sphere, within about 0.5% of the ellipsoid. Click again to start a new line.
      </p>
    </div>
  );
}

function RadiusPanel({ view, settings, set, onPick }: { view: RadiusView | null; settings: Settings; set: Set; onPick: (id: string) => void }) {
  return (
    <div className="space-y-4">
      <Slider
        label="Radius"
        value={settings.radiusM}
        min={250}
        max={50_000}
        step={250}
        format={distanceText}
        onChange={(v) => set('radiusM', v)}
      />
      {view ? (
        <Section title="Within the radius, nearest first" count={view.rows.length}>
          <PlaceList rows={view.rows.slice(0, 100)} onPick={onPick} extra={(r) => `${distanceText(num(r.meters))} ${compass(num(r.bearing))}`} />
        </Section>
      ) : (
        <Empty icon={<Radius className="size-5" />}>Click the map: every place within the radius, nearest first.</Empty>
      )}
    </div>
  );
}

function Credits({ manifest, basemap }: { manifest: Manifest; basemap: Basemap }) {
  return (
    <p className="border-t px-4 py-2.5 text-[11px] leading-relaxed text-fd-muted-foreground">
      Data: teigeodata lite, {manifest.release}, CC BY 4.0 (
      <Link className="text-fd-primary hover:underline" href="/docs/concepts/licences">
        credits
      </Link>
      ).{' '}
      {basemap === 'osm' ? 'Tiles © OpenStreetMap contributors.' : 'No tiles: the map is the pack’s own outlines.'}
    </p>
  );
}
