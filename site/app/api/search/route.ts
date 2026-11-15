import { createFromSource } from 'fumadocs-core/search/server';
import type { StructuredData } from 'fumadocs-core/mdx-plugins';
import { source } from '@/lib/source';
import { rowText } from '@/lib/search';

export const revalidate = false;

// Written once at build time: the site is a static export, searched in the browser, so the
// whole index is downloaded; `scripts/check-search.mjs` holds it to its budget.
export const { staticGET: GET } = createFromSource(source, {
  language: 'english',
  // Results rank by relevance: a sort index of every field would be a quarter of the file.
  sort: { enabled: false },
  async buildIndex(page) {
    const data = page.data as unknown as {
      title?: string;
      description?: string;
      structuredData: StructuredData | (() => Promise<StructuredData>);
    };
    const structured =
      typeof data.structuredData === 'function' ? await data.structuredData() : data.structuredData;
    return {
      id: page.url,
      url: page.url,
      title: data.title ?? page.url,
      description: data.description,
      structuredData: {
        headings: structured.headings,
        contents: structured.contents.map((c) => ({ ...c, content: rowText(c.content) })),
      },
    };
  },
});
