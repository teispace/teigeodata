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
  const marker = (kind: string) =>
    new RegExp(`^\\s*(?:#|//|/\\*)\\s*#?${kind}\\s+${name}\\s*(?:\\*/)?\\s*$`);
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

/** Each language's example program, in the order the docs' tabs show them (`site/scripts/guides.py`). */
export const tours: { label: string; lang: string; file: string }[] = [
  { label: 'Rust', lang: 'rust', file: 'rust/tour.rs' },
  { label: 'Python', lang: 'python', file: 'python/tour.py' },
  { label: 'Node.js', lang: 'js', file: 'node/tour.mjs' },
  { label: 'Browser', lang: 'js', file: 'wasm/tour.mjs' },
  { label: 'Swift', lang: 'swift', file: 'swift/Tour/Sources/Tour/main.swift' },
  { label: 'Kotlin', lang: 'kotlin', file: 'kotlin/Tour.kt' },
  { label: 'Java', lang: 'java', file: 'java/Tour.java' },
  { label: 'Dart', lang: 'dart', file: 'dart/bin/tour.dart' },
  { label: 'Go', lang: 'go', file: 'go/tour.go' },
  { label: 'C', lang: 'c', file: 'c/tour.c' },
  { label: 'C++', lang: 'cpp', file: 'cpp/tour.cpp' },
  { label: 'C#', lang: 'csharp', file: 'dotnet/Program.cs' },
];
