import { site } from './site';
import { source } from './source';

/** A page with a social card: its eyebrow, title and description. */
export interface Card {
  slug: string[];
  eyebrow: string;
  title: string;
  description: string;
}

/** The docs section a page is in, from its folder ("Docs" for top-level pages). */
export function sectionOf(slugs: string[]): string {
  const folder = slugs.length > 1 ? slugs[0] : undefined;
  if (!folder) return 'Docs';
  return sections[folder] ?? folder.replace(/-/g, ' ').replace(/^\w/, (c) => c.toUpperCase());
}

const sections: Record<string, string> = {
  concepts: 'Concepts',
  data: 'Data',
  guides: 'Guides',
  reference: 'Reference',
  servers: 'Servers',
};

/** Every page's social card (`/og/<slug>/image.png`). */
export function cards(): Card[] {
  return [
    {
      slug: ['home'],
      eyebrow: 'Open source · offline · twelve languages',
      title: site.tagline,
      description:
        'Search, reverse geocoding, the admin hierarchy, boundaries, timezones and elevation in small signed packs.',
    },
    {
      slug: ['playground'],
      eyebrow: 'Playground',
      title: 'Every query, in your browser',
      description: 'The WebAssembly engine on a released pack, in your browser, on an OpenStreetMap map.',
    },
    ...source.getPages().map((page) => ({
      slug: ['docs', ...page.slugs],
      eyebrow: sectionOf(page.slugs),
      title: page.data.title,
      description: page.data.description ?? '',
    })),
  ];
}
