// The playground's map: MapLibre GL JS over a style with no external source. The base map is
// every country's outline from the pack; results are circles; names are in the panel and in
// popups, since labels on the map would need a glyph server. MapLibre's worker is the site's
// copy (public/playground/), named with setWorkerUrl.

import type { Feature, FeatureCollection, Geometry, Position } from 'geojson';
import type { GeoJSONSource, LngLatBoundsLike, Map as MapLibreMap, StyleSpecification } from 'maplibre-gl';
import { playgroundBase } from './engine';

export const EMPTY: FeatureCollection = { type: 'FeatureCollection', features: [] };

/** The theme's colours, resolved to what MapLibre parses (`rgb(…)`), from the site's tokens. */
export function themeColors() {
  const style = getComputedStyle(document.documentElement);
  // A colour in any CSS syntax (the tokens are `lab()` and `oklch()`) painted on one pixel and
  // read back as sRGB, which every MapLibre version parses.
  const canvas = document.createElement('canvas');
  canvas.width = canvas.height = 1;
  const ctx = canvas.getContext('2d', { willReadFrequently: true });
  const resolve = (token: string, fallback: string): readonly number[] => {
    const value = style.getPropertyValue(token).trim() || fallback;
    if (!ctx) return [127, 127, 127];
    ctx.clearRect(0, 0, 1, 1);
    ctx.fillStyle = fallback;
    ctx.fillStyle = value;
    ctx.fillRect(0, 0, 1, 1);
    const [r = 0, g = 0, b = 0] = ctx.getImageData(0, 0, 1, 1).data;
    return [r, g, b];
  };
  const rgb = ([r, g, b]: readonly number[], alpha = 1) => `rgba(${r}, ${g}, ${b}, ${alpha})`;
  /** `a` moved towards `b` by `t`: the sea is the land tinted with the primary colour. */
  const mix = (a: readonly number[], b: readonly number[], t: number) =>
    a.map((v, i) => Math.round(v + ((b[i] ?? v) - v) * t));
  const page = resolve('--color-fd-background', '#ffffff');
  const text = resolve('--color-fd-foreground', '#111111');
  const primary = resolve('--color-fd-primary', '#2563eb');
  const muted = resolve('--color-fd-muted-foreground', '#667085');
  // Land a shade towards the text colour, sea a shade towards the primary: apart in both themes.
  return {
    sea: rgb(mix(page, primary, 0.16)),
    land: rgb(mix(page, text, 0.06)),
    border: rgb(muted, 0.6),
    text: rgb(text),
    primary: rgb(primary),
  };
}

export type Colors = ReturnType<typeof themeColors>;

export function style(c: Colors): StyleSpecification {
  return {
    version: 8,
    sources: {
      world: { type: 'geojson', data: EMPTY },
      outline: { type: 'geojson', data: EMPTY },
      places: { type: 'geojson', data: EMPTY },
      pin: { type: 'geojson', data: EMPTY },
    },
    layers: [
      { id: 'sea', type: 'background', paint: { 'background-color': c.sea } },
      { id: 'land', type: 'fill', source: 'world', paint: { 'fill-color': c.land } },
      {
        id: 'borders',
        type: 'line',
        source: 'world',
        paint: { 'line-color': c.border, 'line-width': ['interpolate', ['linear'], ['zoom'], 1, 0.4, 6, 1.2] },
      },
      { id: 'outline-fill', type: 'fill', source: 'outline', paint: { 'fill-color': c.primary, 'fill-opacity': 0.12 } },
      { id: 'outline-line', type: 'line', source: 'outline', paint: { 'line-color': c.primary, 'line-width': 2 } },
      {
        id: 'places',
        type: 'circle',
        source: 'places',
        paint: {
          'circle-radius': ['case', ['==', ['get', 'role'], 'selected'], 7, 5],
          'circle-color': ['case', ['==', ['get', 'role'], 'selected'], c.primary, c.land],
          'circle-stroke-color': c.primary,
          'circle-stroke-width': 2,
        },
      },
      {
        id: 'pin',
        type: 'circle',
        source: 'pin',
        paint: { 'circle-radius': 6, 'circle-color': c.text, 'circle-stroke-color': c.land, 'circle-stroke-width': 2 },
      },
    ],
  };
}

/** Repaints the layers in the theme's current colours (light and dark). */
export function repaint(map: MapLibreMap, c: Colors) {
  map.setPaintProperty('sea', 'background-color', c.sea);
  map.setPaintProperty('land', 'fill-color', c.land);
  map.setPaintProperty('borders', 'line-color', c.border);
  map.setPaintProperty('outline-fill', 'fill-color', c.primary);
  map.setPaintProperty('outline-line', 'line-color', c.primary);
  map.setPaintProperty('places', 'circle-color', ['case', ['==', ['get', 'role'], 'selected'], c.primary, c.land]);
  map.setPaintProperty('places', 'circle-stroke-color', c.primary);
  map.setPaintProperty('pin', 'circle-color', c.text);
  map.setPaintProperty('pin', 'circle-stroke-color', c.land);
}

export async function createMap(container: HTMLElement): Promise<MapLibreMap> {
  const maplibre = await import('maplibre-gl');
  maplibre.setWorkerUrl(`${location.origin}${playgroundBase}/maplibre-gl-worker.mjs`);
  return new maplibre.Map({
    container,
    style: style(themeColors()),
    center: [84.1, 28.3],
    zoom: 5.2,
    attributionControl: false,
    dragRotate: false,
    pitchWithRotate: false,
    renderWorldCopies: false,
  });
}

export function setData(map: MapLibreMap, source: string, data: FeatureCollection) {
  (map.getSource(source) as GeoJSONSource | undefined)?.setData(data);
}

export interface Point {
  id: string;
  name: string;
  kind: string;
  lat: number;
  lon: number;
  role?: 'selected' | 'result';
}

export function points(list: Point[]): FeatureCollection {
  return {
    type: 'FeatureCollection',
    features: list.map(
      (p): Feature => ({
        type: 'Feature',
        geometry: { type: 'Point', coordinates: [p.lon, p.lat] },
        properties: { id: p.id, name: p.name, kind: p.kind, role: p.role ?? 'result' },
      }),
    ),
  };
}

/** The bounds of a geometry, or of points, padded for a single point. */
export function bounds(geometries: Geometry[]): LngLatBoundsLike | null {
  let [w, s, e, n] = [Infinity, Infinity, -Infinity, -Infinity];
  const visit = ([x, y]: Position) => {
    if (x === undefined || y === undefined) return;
    w = Math.min(w, x);
    e = Math.max(e, x);
    s = Math.min(s, y);
    n = Math.max(n, y);
  };
  const walk = (g: Geometry) => {
    switch (g.type) {
      case 'Point':
        visit(g.coordinates);
        break;
      case 'MultiPoint':
      case 'LineString':
        g.coordinates.forEach(visit);
        break;
      case 'MultiLineString':
      case 'Polygon':
        g.coordinates.forEach((r) => r.forEach(visit));
        break;
      case 'MultiPolygon':
        g.coordinates.forEach((p) => p.forEach((r) => r.forEach(visit)));
        break;
      case 'GeometryCollection':
        g.geometries.forEach(walk);
        break;
    }
  };
  geometries.forEach(walk);
  if (!Number.isFinite(w)) return null;
  const pad = e - w < 0.02 && n - s < 0.02 ? 0.05 : 0;
  return [
    [w - pad, s - pad],
    [e + pad, n + pad],
  ];
}
