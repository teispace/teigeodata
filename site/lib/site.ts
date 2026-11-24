import { basePath, repoUrl } from './shared';

/** How the site describes itself and where things live. */
export const site = {
  name: 'teigeodata',
  tagline: 'Accurate places, offline, in every language.',
  /** The home page's title: what people search for, in plain words. */
  title: 'teigeodata: offline geocoding and place data for every language',
  description:
    'An open-source geodata SDK: search, reverse geocoding, the admin hierarchy to the lowest level, boundaries, local names, exact IANA timezones, elevation and postal codes, in small verified packs that work offline. One API in Rust, C, C++, Swift, Kotlin, Java, Dart, Python, Node.js, WebAssembly, Go and .NET.',
  keywords: [
    'offline geocoding',
    'reverse geocoding',
    'geocoder SDK',
    'place search',
    'gazetteer',
    'admin boundaries',
    'administrative hierarchy',
    'timezone lookup',
    'IANA timezone from coordinates',
    'elevation API',
    'Nominatim alternative',
    'GeoNames',
    'OpenStreetMap',
    'Nepal places',
    'Rust geocoder',
    'WebAssembly geocoding',
    'MCP server',
  ],
  url: 'https://teigeodata.teispace.com',
  github: repoUrl,
  releases: `${repoUrl}/releases`,
  issues: `${repoUrl}/issues`,
  changelog: `${repoUrl}/blob/main/CHANGELOG.md`,
  license: `${repoUrl}/blob/main/LICENSE`,
  /** The library's release the site documents. */
  version: '1.3.0',
  versionDate: '2026-11-24',
  org: {
    name: 'Teispace',
    legalName: 'Teispace Technology Private Limited',
    url: 'https://teispace.com',
    email: 'info@teispace.com',
  },
};

/** A path under the site's base path. */
export function asset(path: string): string {
  return `${basePath}${path}`;
}

/** The absolute URL of a page, as search engines should see it. */
export function absolute(path: string): string {
  return new URL(asset(path), site.url).toString();
}
