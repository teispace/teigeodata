// The playground, run: serves the built site (`out/`) under its base path, opens the playground
// in headless Chromium, loads the pack and checks a search, a place and two points against the
// answers the tours print (site/examples/out/), so the page shows what every language shows.
// It fails on a console error, a failed request, or any request to another origin except
// OpenStreetMap's tiles, which it answers itself with an empty tile (CI does not load OSM's
// servers): every query runs in the page (docs/02-research/notes/docs-playground.md).
//
// Usage: node scripts/check-playground.mjs out /teigeodata   (after `npm run build`)

import { createServer } from 'node:http';
import { readFile, stat } from 'node:fs/promises';
import { extname, join, normalize, resolve } from 'node:path';
import { chromium } from 'playwright';

const [dir = 'out', base = ''] = process.argv.slice(2);
const root = resolve(dir);
const tour = (section) => readFile(new URL(`../examples/out/${section}.txt`, import.meta.url), 'utf8');

// What the tours print: the first search hit, the hierarchy, the zone and height at their point.
const firstHit = (await tour('search')).split('\n')[0].split(/\s+/)[0];
const hierarchy = (await tour('hierarchy'))
  .split('\n')
  .filter((l) => /^(admin|country) /.test(l))
  .map((l) => l.split(/\s{2,}/)[1]);
const zone = (await tour('timezone')).split(/\s+/)[0];
const height = (await tour('elevation')).split(/\s+/)[0];
// The tours' point (site/examples/*/tour.*: reverse, timezone and elevation at Kathmandu).
const tourPoint = '27.7172,85.324';

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript',
  '.mjs': 'text/javascript',
  '.css': 'text/css',
  '.json': 'application/json',
  '.wasm': 'application/wasm',
  '.svg': 'image/svg+xml',
  '.txt': 'text/plain; charset=utf-8',
  '.woff2': 'font/woff2',
};

const server = createServer(async (req, res) => {
  const url = new URL(req.url ?? '/', 'http://localhost');
  if (!url.pathname.startsWith(`${base}/`)) {
    res.writeHead(404).end();
    return;
  }
  let path = normalize(join(root, decodeURIComponent(url.pathname.slice(base.length))));
  if (!path.startsWith(root)) {
    res.writeHead(403).end();
    return;
  }
  try {
    if ((await stat(path)).isDirectory()) path = join(path, 'index.html');
    const body = await readFile(path);
    res.writeHead(200, { 'content-type': TYPES[extname(path)] ?? 'application/octet-stream' }).end(body);
  } catch {
    res.writeHead(404).end();
  }
});
await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
const origin = `http://127.0.0.1:${server.address().port}`;

const problems = [];
const browser = await chromium.launch();
try {
  const page = await browser.newPage();
  // A 1×1 transparent PNG for every OpenStreetMap tile.
  const tile = Buffer.from(
    'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=',
    'base64',
  );
  let tiles = 0;
  await page.route('https://tile.openstreetmap.org/**', (route) => {
    tiles++;
    return route.fulfill({ status: 200, contentType: 'image/png', body: tile });
  });
  page.on('console', (m) => {
    if (m.type() === 'error') problems.push(`console: ${m.text()}`);
  });
  page.on('pageerror', (e) => problems.push(`page: ${e.message}`));
  page.on('requestfailed', (r) => problems.push(`failed: ${r.url()} ${r.failure()?.errorText ?? ''}`));
  page.on('request', (r) => {
    const u = r.url();
    const other = new URL(u).origin;
    if (!u.startsWith('data:') && !u.startsWith('blob:') && other !== origin && other !== 'https://tile.openstreetmap.org') {
      problems.push(`left the page: ${u}`);
    }
  });
  page.on('response', (r) => {
    if (r.status() >= 400) problems.push(`${r.status()}: ${r.url()}`);
  });

  const t = Date.now();
  await page.goto(`${origin}${base}/playground/`);
  await page.getByTestId('load').click();
  await page.getByTestId('ready').waitFor({ timeout: 120_000 });
  const loaded = Date.now() - t;

  const expect = async (what, ok) => {
    if (!(await ok())) problems.push(`not as the tours print: ${what}`);
  };

  // Search: the tours' first hit.
  await page.getByTestId('search').fill('Pokhara');
  const first = page.getByTestId('search-panel').locator('button[data-id]').first();
  await first.waitFor();
  await expect(`search Pokhara answers ${firstHit} first`, async () => (await first.getAttribute('data-id')) === firstHit);

  // A place: its ancestors, country first, are the tours' hierarchy.
  await first.click();
  const ancestors = page.getByTestId('ancestors');
  await ancestors.waitFor();
  const crumbs = await ancestors.locator('button').allTextContents();
  await expect(`ancestors ${crumbs.join(' > ')}`, async () => crumbs.join('|') === [...hierarchy].reverse().join('|'));

  // A point: the zone and height the tours print at their point.
  await page.evaluate((at) => (location.hash = `at=${at}`), tourPoint);
  await page.getByTestId('zone').waitFor();
  const zoneText = await page.getByTestId('zone').textContent();
  await expect(`zone ${zoneText}`, async () => zoneText?.startsWith(zone));
  const heightText = await page.getByTestId('height').textContent();
  await expect(`height ${heightText}`, async () => heightText?.startsWith(`${height} m`));

  // A point outside the country in full: the world's zone areas answer their zone too.
  await page.evaluate(() => (location.hash = 'at=40.7128,-74.006'));
  await page.getByTestId('zone').filter({ hasText: 'America/New_York' }).waitFor({ timeout: 10_000 }).catch(() => {
    problems.push('the zone at 40.7128,-74.006 is not America/New_York');
  });

  await expect('the map is drawn (Leaflet, with tiles)', async () => (await page.locator('.leaflet-container').count()) === 1 && tiles > 0);
  if (!problems.length) {
    console.log(
      `playground ok: loaded in ${loaded} ms; search, place, ${zone}, ${height} m, a world zone; ${tiles} tiles stubbed, no other request left ${origin}`,
    );
  }
} finally {
  await browser.close();
  server.close();
}
if (problems.length) {
  console.error(`playground failed:\n  ${problems.join('\n  ')}`);
  process.exit(1);
}
