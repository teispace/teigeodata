// The playground's map: Leaflet over OpenStreetMap's tiles, or over the pack's own country
// outlines with no tiles at all. Everything drawn on it (results, the pin, outlines, lines,
// circles) is SVG with a class, so the site's theme colours it through CSS
// (`.tg-map-*` in app/global.css) and it follows light and dark mode with no repaint.

import type { Feature, FeatureCollection, Geometry } from 'geojson';
import type * as Leaflet from 'leaflet';

type L = typeof Leaflet;

export type Basemap = 'osm' | 'none';

export interface Point {
  id: string;
  name: string;
  kind: string;
  lat: number;
  lon: number;
  /** How it is drawn: a result, the selected place, a child of it, or a place near the pin. */
  role?: 'result' | 'selected' | 'child' | 'near';
}

export interface OutlineStyle {
  fillOpacity: number;
  weight: number;
}

const ESCAPE: Record<string, string> = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };
const escape = (s: string) => s.replace(/[&<>"']/g, (c) => ESCAPE[c] ?? c);

/** The map and its layers; one per playground. */
export class PlaygroundMap {
  readonly map: Leaflet.Map;
  private readonly L: L;
  private tiles: Leaflet.TileLayer;
  private readonly world: Leaflet.GeoJSON;
  private readonly places: Leaflet.LayerGroup;
  private readonly outline: Leaflet.GeoJSON;
  private readonly lines: Leaflet.LayerGroup;
  private readonly marks: Leaflet.LayerGroup;
  private readonly areas: Leaflet.LayerGroup;
  private me: Leaflet.LayerGroup | null = null;
  private popup: Leaflet.Popup | null = null;
  private pin: Leaflet.Marker | null = null;
  /** A flight is under way: a card opens when it ends, so it is placed where the map stops. */
  private flying = false;
  /** The card waiting for a flight to end; a newer card or a close replaces it. */
  private pending: (() => void) | null = null;
  private basemap: Basemap = 'osm';

  constructor(L: L, element: HTMLElement) {
    this.L = L;
    this.map = L.map(element, {
      center: [28.2, 84.1],
      zoom: 6,
      minZoom: 2,
      maxZoom: 18,
      zoomControl: false,
      // Whole zoom levels only: a tile scaled to a fractional zoom shows hairline seams.
      wheelPxPerZoomLevel: 90,
      worldCopyJump: true,
      attributionControl: true,
    });
    this.map.attributionControl.setPrefix('<a href="https://leafletjs.com">Leaflet</a>');
    L.control.zoom({ position: 'bottomright' }).addTo(this.map);
    L.control.scale({ position: 'bottomleft', imperial: false }).addTo(this.map);
    this.tiles = this.osm();
    this.world = L.geoJSON(undefined, { interactive: false, style: { className: 'tg-map-world' } }).addTo(this.map);
    this.outline = L.geoJSON(undefined, { interactive: false, style: { className: 'tg-map-outline' } }).addTo(this.map);
    this.areas = L.layerGroup().addTo(this.map);
    this.lines = L.layerGroup().addTo(this.map);
    this.marks = L.layerGroup().addTo(this.map);
    this.places = L.layerGroup().addTo(this.map);
    this.setBasemap('osm');
  }

  private osm() {
    return this.L.tileLayer('https://tile.openstreetmap.org/{z}/{x}/{y}.png', {
      maxZoom: 19,
      className: 'tg-map-tiles',
      attribution: '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors',
    });
  }

  setBasemap(kind: Basemap) {
    this.basemap = kind;
    if (kind === 'osm') this.tiles.addTo(this.map);
    else this.tiles.remove();
    this.map.getContainer().dataset.basemap = kind;
  }

  setWorld(world: FeatureCollection | null, visible: boolean) {
    this.world.clearLayers();
    if (world && visible) this.world.addData(world);
  }

  setPlaces(list: Point[], onPick: (id: string, at: { lat: number; lon: number }) => void) {
    this.places.clearLayers();
    const L = this.L;
    // Names beside the dots while they are few enough to read; past that, on hover only.
    const named = list.filter((p) => p.role !== 'child').length <= 12;
    for (const p of list) {
      const role = p.role ?? 'result';
      const marker = L.circleMarker([p.lat, p.lon], {
        radius: radiusOf(p),
        className: `tg-map-place tg-map-${role} tg-kind-${p.kind}`,
        bubblingMouseEvents: false,
      });
      if (named && role !== 'child') {
        marker.bindTooltip(escape(p.name), {
          permanent: true,
          className: `tg-map-name tg-map-name-${role}`,
          direction: 'right',
          offset: [radiusOf(p) + 3, 0],
        });
      } else {
        marker.bindTooltip(`<strong>${escape(p.name)}</strong><span>${escape(p.kind)}</span>`, {
          className: 'tg-map-tip',
          direction: 'top',
          offset: [0, -radiusOf(p)],
        });
      }
      marker.on('click', () => onPick(p.id, { lat: p.lat, lon: p.lon }));
      marker.addTo(this.places);
    }
  }

  /**
   * A card on the map at `at` (React renders into `content`); the map pans so it clears the
   * panel. `onClose` runs when it closes by Escape, a click elsewhere or another card.
   */
  openPopup(at: { lat: number; lon: number }, content: HTMLElement, onClose: () => void) {
    this.closePopup();
    if (this.flying) {
      this.pending = () => this.openPopup(at, content, onClose);
      return;
    }
    const left = this.panelInset();
    const popup = this.L.popup({
      className: 'tg-popup',
      closeButton: false,
      maxWidth: 360,
      minWidth: 336,
      offset: [0, -12],
      // Clear of the panel on the left and the tool bar and its hint on top.
      autoPanPaddingTopLeft: [left + 24, 112],
      autoPanPaddingBottomRight: [64, 32],
    })
      .setLatLng([at.lat, at.lon])
      .setContent(content);
    // Closed by Escape or a click elsewhere, not replaced: the page forgets it too.
    popup.on('remove', () => {
      if (this.popup !== popup) return;
      this.popup = null;
      onClose();
    });
    this.popup = popup;
    popup.openOn(this.map);
  }

  /** Re-measures the card after its content changed size. */
  updatePopup() {
    this.popup?.update();
  }

  closePopup() {
    this.pending = null;
    const popup = this.popup;
    this.popup = null;
    popup?.remove();
  }

  /** The reader's position: a dot, and a circle as wide as the fix is uncertain. */
  setLocation(at: { lat: number; lon: number; accuracy: number } | null) {
    this.me?.remove();
    this.me = null;
    if (!at) return;
    const L = this.L;
    const icon = L.divIcon({ className: 'tg-map-me', html: '<span></span>', iconSize: [18, 18], iconAnchor: [9, 9] });
    this.me = L.layerGroup([
      L.circle([at.lat, at.lon], { radius: at.accuracy, className: 'tg-map-accuracy', interactive: false }),
      L.marker([at.lat, at.lon], { icon, interactive: false, keyboard: false }),
    ]).addTo(this.map);
  }

  /** Where the catalog's packs hold data: loaded ones solid, one highlighted. */
  setAreas(list: { id: string; bbox: [number, number, number, number]; state: 'added' | 'available'; highlight: boolean }[]) {
    this.areas.clearLayers();
    for (const a of list) {
      const [w, s, e, n] = a.bbox;
      this.L.rectangle(
        [
          [s, w],
          [n, e],
        ],
        { className: `tg-map-area tg-map-area-${a.state}${a.highlight ? ' tg-map-area-hi' : ''}`, interactive: false },
      ).addTo(this.areas);
    }
  }

  fitBox(bbox: [number, number, number, number], maxZoom = 11) {
    const [w, s, e, n] = bbox;
    this.fly();
    this.map.flyToBounds(
      [
        [s, w],
        [n, e],
      ],
      this.fitOptions(maxZoom),
    );
  }

  setPin(at: { lat: number; lon: number } | null) {
    this.pin?.remove();
    this.pin = null;
    if (!at) return;
    const icon = this.L.divIcon({
      className: 'tg-map-pin',
      html: '<span class="tg-map-pin-ping"></span><span class="tg-map-pin-dot"></span>',
      iconSize: [22, 22],
      iconAnchor: [11, 11],
    });
    this.pin = this.L.marker([at.lat, at.lon], { icon, interactive: false, keyboard: false }).addTo(this.map);
  }

  setOutline(geometry: Geometry | null, style: OutlineStyle) {
    this.outline.clearLayers();
    if (geometry) this.outline.addData({ type: 'Feature', geometry, properties: {} } as Feature);
    this.styleOutline(style);
  }

  styleOutline(style: OutlineStyle) {
    this.outline.setStyle({ fillOpacity: style.fillOpacity, weight: style.weight, className: 'tg-map-outline' });
  }

  /** Lines from a point to others, each labelled with its distance. */
  setSpokes(from: { lat: number; lon: number } | null, to: { lat: number; lon: number; label: string }[]) {
    this.lines.clearLayers();
    if (!from) return;
    for (const t of to) {
      this.L.polyline(
        [
          [from.lat, from.lon],
          [t.lat, t.lon],
        ],
        { className: 'tg-map-spoke', interactive: false },
      )
        .bindTooltip(escape(t.label), { permanent: true, className: 'tg-map-label', direction: 'center' })
        .addTo(this.lines);
    }
  }

  /** A measured line between two points, or a radius around one. */
  setMarks(marks: { line?: [{ lat: number; lon: number }, { lat: number; lon: number }, string]; circle?: { lat: number; lon: number; m: number }; start?: { lat: number; lon: number } }) {
    this.marks.clearLayers();
    const L = this.L;
    if (marks.start) {
      L.circleMarker([marks.start.lat, marks.start.lon], { radius: 5, className: 'tg-map-measure-dot', interactive: false }).addTo(this.marks);
    }
    if (marks.line) {
      const [a, b, label] = marks.line;
      L.polyline(
        [
          [a.lat, a.lon],
          [b.lat, b.lon],
        ],
        { className: 'tg-map-measure', interactive: false },
      )
        .bindTooltip(escape(label), { permanent: true, className: 'tg-map-label tg-map-label-strong', direction: 'center' })
        .addTo(this.marks);
      for (const p of [a, b]) {
        L.circleMarker([p.lat, p.lon], { radius: 5, className: 'tg-map-measure-dot', interactive: false }).addTo(this.marks);
      }
    }
    if (marks.circle) {
      L.circle([marks.circle.lat, marks.circle.lon], { radius: marks.circle.m, className: 'tg-map-radius', interactive: false }).addTo(this.marks);
    }
  }

  /** Pixels the floating panel covers on the left; 0 when it is a bottom sheet. */
  private panelInset() {
    return this.map.getContainer().clientWidth >= 768 ? 412 : 0;
  }

  /**
   * Marks a flight as under way until the map stops. During a flight Leaflet scales everything
   * drawn with the map (a dot becomes a blurred disc many times its size), so the drawing fades
   * out (`.tg-flying` in global.css) and back in, redrawn sharp, where the map lands.
   */
  private fly() {
    this.flying = true;
    const container = this.map.getContainer();
    container.classList.add('tg-flying');
    this.map.once('moveend', () => {
      this.flying = false;
      container.classList.remove('tg-flying');
      const open = this.pending;
      this.pending = null;
      open?.();
    });
  }

  private fitOptions(maxZoom: number) {
    const left = this.panelInset();
    return { paddingTopLeft: [left + 48, 72] as [number, number], paddingBottomRight: [48, 48] as [number, number], maxZoom, duration: 0.7 };
  }

  fitGeometry(geometry: Geometry, maxZoom = 13) {
    const layer = this.L.geoJSON({ type: 'Feature', geometry, properties: {} } as Feature);
    const box = layer.getBounds();
    if (!box.isValid()) return;
    this.fly();
    this.map.flyToBounds(box, this.fitOptions(maxZoom));
  }

  /** Frames a radius; `smooth` flies, otherwise it snaps (for a slider being dragged). */
  fitCircle(at: { lat: number; lon: number }, m: number, smooth: boolean) {
    const box = this.L.latLng(at.lat, at.lon).toBounds(m * 2);
    const o = this.fitOptions(16);
    if (smooth) {
      this.fly();
      this.map.flyToBounds(box, o);
    }
    else this.map.fitBounds(box, { ...o, animate: false });
  }

  fitPoints(list: { lat: number; lon: number }[], maxZoom = 9) {
    if (!list.length) return;
    const box = this.L.latLngBounds(list.map((p) => [p.lat, p.lon] as [number, number]));
    const [best] = list;
    if (best && this.map.getBounds().contains([best.lat, best.lon]) && list.length > 1) return;
    this.fly();
    this.map.flyToBounds(box, this.fitOptions(maxZoom));
  }

  /** Flies so the point lands in the middle of the map the panel leaves visible. */
  flyTo(lat: number, lon: number, zoom: number) {
    const z = Math.max(this.map.getZoom(), zoom);
    const shift = this.panelInset() / 2;
    const at = this.map.unproject(this.map.project([lat, lon], z).subtract([shift, 0]), z);
    this.fly();
    this.map.flyTo(at, z, { duration: 0.7 });
  }

  center() {
    const c = this.map.getCenter();
    return { lat: c.lat, lon: c.lng };
  }

  get currentBasemap() {
    return this.basemap;
  }

  remove() {
    this.map.remove();
  }
}

/** Dot size by what a place is: countries and top-level units largest, localities smallest. */
function radiusOf(p: Point) {
  if (p.role === 'selected') return 8;
  if (p.role === 'child') return 4;
  if (p.kind === 'country') return 8;
  if (p.kind === 'admin') return 7;
  if (p.kind === 'locality') return 5;
  return 6;
}

/** Creates the map once Leaflet is loaded (it touches `window`, so never on the server). */
export async function createMap(element: HTMLElement, cancelled: () => boolean): Promise<PlaygroundMap | null> {
  const L = (await import('leaflet')).default;
  // React mounts effects twice in development: the first call is cancelled by the time Leaflet
  // has loaded, and must not claim the element the second one draws into.
  if (cancelled()) return null;
  return new PlaygroundMap(L, element);
}
