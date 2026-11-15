// Checks every internal link and asset reference in the built site (`out/`): each `href` and
// `src` under the base path must name a file the export wrote, and each `#fragment` on a page
// an `id` on that page. Usage: node scripts/check-links.mjs [out] [basePath]
import { readFileSync, readdirSync, statSync, existsSync } from 'node:fs';
import { join, relative } from 'node:path';

const out = process.argv[2] ?? 'out';
const base = process.argv[3] ?? process.env.DOCS_BASE_PATH ?? '';

function* walk(dir) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) yield* walk(p);
    else yield p;
  }
}

/** The file a site path is served from, or null. */
function resolve(path) {
  const clean = decodeURIComponent(path.split('?')[0]);
  const candidates = [join(out, clean), join(out, clean, 'index.html'), join(out, `${clean}.html`)];
  return candidates.find((c) => existsSync(c) && statSync(c).isFile()) ?? null;
}

const ids = new Map();
function idsOf(file) {
  if (!ids.has(file)) {
    const html = readFileSync(file, 'utf8');
    ids.set(file, new Set([...html.matchAll(/\sid="([^"]+)"/g)].map((m) => m[1])));
  }
  return ids.get(file);
}

const problems = [];
let checked = 0;
for (const file of walk(out)) {
  if (!file.endsWith('.html')) continue;
  const html = readFileSync(file, 'utf8');
  for (const m of html.matchAll(/\s(?:href|src)="([^"]+)"/g)) {
    const url = m[1].replaceAll('&amp;', '&');
    if (/^(https?:|mailto:|data:|javascript:)/.test(url)) continue;
    let target = file;
    const [path, fragment] = url.split('#');
    if (path) {
      if (!path.startsWith('/')) continue; // relative assets are the framework's own
      if (base && !path.startsWith(`${base}/`) && path !== base) {
        problems.push(`${relative(out, file)}: ${url} is outside the base path ${base}`);
        continue;
      }
      const found = resolve(path.slice(base.length) || '/');
      checked++;
      if (!found) {
        problems.push(`${relative(out, file)}: ${url} does not exist`);
        continue;
      }
      target = found;
    }
    if (fragment && target.endsWith('.html') && !idsOf(target).has(decodeURIComponent(fragment))) {
      problems.push(`${relative(out, file)}: ${url}: no id "${fragment}"`);
    }
  }
}
if (problems.length) {
  console.error([...new Set(problems)].join('\n'));
  console.error(`${problems.length} broken link(s)`);
  process.exit(1);
}
console.log(`links ok: ${checked} internal references`);
