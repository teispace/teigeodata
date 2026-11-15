// The static search index's budget: the browser downloads the whole index on the first search
// (Fumadocs' static mode), so it is held to gzip -9 at most 300 KiB, the budget the docs-site
// research set (docs/02-research/notes/docs-site.md §6). Fails above it.
//
// Usage: node scripts/check-search.mjs out [budget-bytes]   (after `npm run build`)

import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { gzipSync } from 'node:zlib';

const [dir = 'out', budget = String(300 * 1024)] = process.argv.slice(2);
const bytes = await readFile(join(dir, 'api', 'search'));
const gz = gzipSync(bytes, { level: 9 }).length;
const limit = Number(budget);
const line = `search index: ${bytes.length} B, ${gz} B gzipped (budget ${limit} B, ${((100 * (limit - gz)) / limit).toFixed(1)}% headroom)`;
if (gz > limit) {
  console.error(`${line}: over its budget`);
  process.exit(1);
}
console.log(line);
