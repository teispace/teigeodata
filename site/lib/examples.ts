import { readFileSync } from 'node:fs';
import { join } from 'node:path';

/**
 * The example programs under `site/examples`: each ran on the released pack, and its output is
 * checked into `examples/out` (the guides include the same regions). Read at build time only.
 */
const root = join(process.cwd(), 'examples');

/** A region (`#region <name>` … `#endregion <name>`) of an example, without its indent. */
export function region(file: string, name: string): string {
  const text = readFileSync(join(root, file), 'utf8');
  const lines = text.split('\n');
  const marker = (kind: string) => new RegExp(`^\\s*(?:#|//)\\s*#?${kind}\\s+${name}\\s*$`);
  const start = lines.findIndex((l) => marker('region').test(l));
  const end = lines.findIndex((l, i) => i > start && marker('endregion').test(l));
  if (start < 0 || end < 0) throw new Error(`examples/${file}: no region "${name}"`);
  const body = lines.slice(start + 1, end);
  const indent = Math.min(
    ...body.filter((l) => l.trim()).map((l) => l.length - l.trimStart().length),
  );
  return body.map((l) => l.slice(indent)).join('\n');
}

/** An example's checked-in output (`examples/out/<name>.txt`). */
export function output(name: string): string {
  return readFileSync(join(root, 'out', `${name}.txt`), 'utf8').trimEnd();
}
