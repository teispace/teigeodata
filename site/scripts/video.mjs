// The landing page's playground video, recorded from the built site: serves `out/`, drives the
// real playground with a visible cursor (a search, a place, a click with its card, a pack added
// while it runs, a radius, a measured line), records it with Playwright, and encodes
// public/shots/playground-{light,dark}.{mp4,webm} and a poster from its first frame
// (ffmpeg: H.264 and VP9; cwebp for the poster).
// It loads OpenStreetMap's real tiles (one short session, a few hundred tiles), so run it by
// hand when the playground changes, not in CI.
//
// Usage: node scripts/video.mjs out   (after `npm run build` and `cargo xtask playground`)

import { createServer } from 'node:http';
import { execFileSync } from 'node:child_process';
import { mkdir, mkdtemp, readFile, rm, stat } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { extname, join, normalize, resolve } from 'node:path';
import { chromium } from 'playwright';

const [dir = 'out'] = process.argv.slice(2);
const root = resolve(dir);
const shots = new URL('../public/shots/', import.meta.url).pathname;
const W = 1440;
const H = 900;

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

// A cursor drawn into the page, since a recording has none: an arrow that follows the mouse and
// a ring on each click.
const cursor = () => {
  addEventListener('DOMContentLoaded', () => {
    const style = document.createElement('style');
    style.textContent = `
      #tg-cursor{position:fixed;z-index:2147483647;left:0;top:0;width:22px;height:22px;pointer-events:none;
        transform:translate(-3px,-2px);filter:drop-shadow(0 2px 3px rgb(0 0 0/.35))}
      .tg-ring{position:fixed;z-index:2147483646;width:34px;height:34px;margin:-17px 0 0 -17px;border-radius:50%;
        border:2px solid #10b981;pointer-events:none;animation:tg-ring .5s ease-out forwards}
      @keyframes tg-ring{from{transform:scale(.3);opacity:1}to{transform:scale(1.4);opacity:0}}`;
    document.head.append(style);
    const el = document.createElement('div');
    el.id = 'tg-cursor';
    el.innerHTML =
      '<svg viewBox="0 0 24 24" width="22" height="22"><path d="M4 2l15 11.5-6.6.9 3.9 7.6-3.1 1.5-3.9-7.7L4 20z" fill="#fff" stroke="#111" stroke-width="1.4" stroke-linejoin="round"/></svg>';
    document.body.append(el);
    addEventListener('mousemove', (e) => (el.style.left = `${e.clientX}px`, el.style.top = `${e.clientY}px`), true);
    addEventListener(
      'mousedown',
      (e) => {
        const ring = document.createElement('div');
        ring.className = 'tg-ring';
        ring.style.left = `${e.clientX}px`;
        ring.style.top = `${e.clientY}px`;
        document.body.append(ring);
        setTimeout(() => ring.remove(), 600);
      },
      true,
    );
  });
};

/** The tour, in the page: each step moves the cursor as a hand would and waits for the answer. */
async function tour(page) {
  let at = { x: W / 2, y: H / 2 };
  const move = async (x, y, ms = 650) => {
    const steps = Math.max(8, Math.round(ms / 16));
    await page.mouse.move(x, y, { steps });
    at = { x, y };
  };
  const center = async (locator) => {
    const b = await locator.boundingBox();
    if (!b) throw new Error(`not on screen: ${locator}`);
    return { x: b.x + b.width / 2, y: b.y + b.height / 2 };
  };
  const click = async (locator, ms) => {
    const c = await center(locator);
    await move(c.x, c.y, ms);
    await page.mouse.down();
    await page.mouse.up();
  };
  const clickAt = async (x, y, ms) => {
    await move(x, y, ms);
    await page.mouse.down();
    await page.mouse.up();
  };
  const pause = (ms) => page.waitForTimeout(ms);

  // Search as you type, in any script.
  await move(at.x - 200, at.y - 120, 300);
  await click(page.getByTestId('search'), 800);
  await page.keyboard.type('Pokhara', { delay: 120 });
  await pause(1400);
  // Open the metropolitan city: its outline, children and nearby places.
  await click(page.getByTestId('search-panel').locator('button[data-id]').filter({ hasText: 'admin' }).first(), 700);
  await page.locator('path.tg-map-outline').first().waitFor();
  await pause(2200);
  // Click the map: the card at the point, with sharper terrain on offer.
  await clickAt(1010, 470, 900);
  await page.getByTestId('popover').waitFor();
  await pause(1600);
  // Add the terrain while it runs: downloaded, checked, kept, and the height answers from it.
  await click(page.getByTestId('popover-add'), 700);
  await page.getByTestId('height').filter({ hasText: '31 m grid' }).waitFor({ timeout: 30_000 });
  await pause(2200);
  // Everything within a radius.
  await page.keyboard.press('Escape');
  await click(page.getByRole('button', { name: 'Radius' }), 800);
  await clickAt(1060, 560, 800);
  await pause(2200);
  // A measured line.
  await click(page.getByRole('button', { name: 'Measure' }), 800);
  await clickAt(860, 380, 700);
  await pause(300);
  await clickAt(1180, 640, 900);
  await pause(2400);
}

await mkdir(shots, { recursive: true });
const scratch = await mkdtemp(join(tmpdir(), 'tg-video-'));
const browser = await chromium.launch();
try {
  for (const theme of ['light', 'dark']) {
    const context = await browser.newContext({
      viewport: { width: W, height: H },
      colorScheme: theme,
      recordVideo: { dir: scratch, size: { width: W, height: H } },
    });
    await context.addInitScript(cursor);
    const page = await context.newPage();
    const started = Date.now();
    await page.goto(`${origin}/playground/`);
    await page.getByTestId('load').click();
    await page.getByTestId('ready').waitFor({ timeout: 120_000 });
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(600);
    // The recording starts at the page's creation; the film starts once the playground is ready.
    const skip = (Date.now() - started) / 1000;
    const nav = await page.evaluate(() => document.querySelector('header')?.getBoundingClientRect().bottom ?? 0);
    await tour(page);
    const video = page.video();
    await context.close();
    const raw = await video.path();

    // Below the site's navigation bar, 1280 wide: the playground alone, as on the landing page.
    const top = Math.round(nav);
    const filter = `crop=${W}:${H - top}:0:${top},scale=1280:-2:flags=lanczos,fps=30`;
    const base = join(shots, `playground-${theme}`);
    const run = (args) => execFileSync('ffmpeg', ['-hide_banner', '-loglevel', 'error', '-y', ...args]);
    run(['-ss', skip.toFixed(2), '-i', raw, '-vf', filter, '-an', '-c:v', 'libx264', '-preset', 'slow', '-crf', '26', '-pix_fmt', 'yuv420p', '-movflags', '+faststart', `${base}.mp4`]);
    run(['-ss', skip.toFixed(2), '-i', raw, '-vf', filter, '-an', '-c:v', 'libvpx-vp9', '-crf', '36', '-b:v', '0', '-row-mt', '1', '-deadline', 'good', '-cpu-used', '2', `${base}.webm`]);
    const png = join(scratch, `poster-${theme}.png`);
    run(['-i', `${base}.mp4`, '-frames:v', '1', png]);
    execFileSync('cwebp', ['-quiet', '-q', '80', '-m', '6', png, '-o', `${base}-poster.webp`]);
    console.log(`wrote ${base}.{mp4,webm} and its poster`);
  }
} finally {
  await browser.close();
  server.close();
  await rm(scratch, { recursive: true, force: true });
}
