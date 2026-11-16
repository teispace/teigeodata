import type { MetadataRoute } from 'next';
import { ogImage } from '@/lib/seo';
import { absolute } from '@/lib/site';
import { source } from '@/lib/source';

export const dynamic = 'force-static';

/** Every page with the social card it shows, so search engines index both. */
export default function sitemap(): MetadataRoute.Sitemap {
  const page = (path: string, priority: number, image: string) => ({
    url: absolute(path),
    changeFrequency: 'weekly' as const,
    priority,
    images: [image],
  });
  return [
    page('/', 1, ogImage(['home'])),
    page('/playground/', 0.8, ogImage(['playground'])),
    ...source.getPages().map((p) =>
      page(
        `${p.url}/`,
        p.slugs.length === 0 ? 0.9 : p.slugs[0] === 'getting-started' ? 0.8 : 0.7,
        ogImage(['docs', ...p.slugs]),
      ),
    ),
  ];
}
