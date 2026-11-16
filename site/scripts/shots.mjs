// The landing page's playground screenshots, taken from the built site: serves `out/`, opens the
// playground on a place with its outline, and writes public/shots/playground-{light,dark}.webp
// (cwebp, from libwebp, encodes them).
// It loads OpenStreetMap's real tiles (one view, a few dozen tiles), so run it by hand when the
// playground's look changes, not in CI.
//
// Usage: node scripts/shots.mjs out   (after `npm run build` and `cargo xtask playground`)

import { createServer } from 'node:http';
import { execFileSync } from 'node:child_process';
import { mkdir, readFile, rm, stat } from 'node:fs/promises';
import { extname, join, normalize, resolve } from 'node:path';
import { chromium } from 'playwright';

const [dir = 'out'] = process.argv.slice(2);
const root = resolve(dir);
const shots = new URL('../public/shots/', import.meta.url);
// Bagmati Province: an outline, its towns, and the panel's stats at a glance.
const view = '/playground/#place=tgd%3Anp%2F2aq';

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript',
  '.css': 'text/css',
  '.json': 'application/json',
  '.wasm': 'application/wasm',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.woff2': 'font/woff2',
};

const server = createServer(async (req, res) => {
  const url = new URL(req.url ?? '/', 'http://localhost');
  let path = normalize(join(root, decodeURIComponent(url.pathname)));
  if (!path.startsWith(root)) return res.writeHead(403).end();
  try {
    if ((await stat(path)).isDirectory()) path = join(path, 'index.html');
    res.writeHead(200, { 'content-type': TYPES[extname(path)] ?? 'application/octet-stream' }).end(await readFile(path));
  } catch {
    res.writeHead(404).end();
  }
});
await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
const origin = `http://127.0.0.1:${server.address().port}`;

await mkdir(shots, { recursive: true });
const browser = await chromium.launch();
try {
  for (const theme of ['light', 'dark']) {
    const context = await browser.newContext({
      viewport: { width: 1440, height: 900 },
      deviceScaleFactor: 1.5,
      colorScheme: theme,
      reducedMotion: 'reduce',
    });
    const page = await context.newPage();
    await page.goto(origin + view);
    await page.getByTestId('load').click();
    await page.getByTestId('ready').waitFor({ timeout: 120_000 });
    await page.locator('path.tg-map-outline').first().waitFor();
    // The fly-to ends, then the tiles of the final view load.
    await page.waitForTimeout(1500);
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(500);
    const png = new URL(`playground-${theme}.png`, shots).pathname;
    const webp = new URL(`playground-${theme}.webp`, shots).pathname;
    // Below the site's navigation bar: the playground alone, as a window on the landing page.
    const nav = await page.evaluate(() => document.querySelector('#nd-nav, header')?.getBoundingClientRect().bottom ?? 0);
    await page.screenshot({ path: png, clip: { x: 0, y: nav, width: 1440, height: 900 - nav } });
    execFileSync('cwebp', ['-quiet', '-q', '80', '-m', '6', png, '-o', webp]);
    await rm(png);
    console.log(`wrote ${webp}`);
    await context.close();
  }
} finally {
  await browser.close();
  server.close();
}
