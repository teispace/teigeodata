// How the static search index stores a page (app/api/search/route.ts).

/**
 * A table row as the index stores it: Fumadocs gives each row with its table's header and
 * separator, so a 250-row table would index its header 250 times. Only the row's cells are
 * words to find; the header is the same for every row.
 */
export function rowText(content: string): string {
  const lines = content.split('\n');
  if (lines.length < 3 || !lines.every((l) => l.trimStart().startsWith('|'))) return content;
  return lines
    .slice(2)
    .map((l) =>
      l
        .split('|')
        .map((c) => c.trim().replaceAll('`', ''))
        .filter(Boolean)
        .join(' · '),
    )
    .join('\n');
}
