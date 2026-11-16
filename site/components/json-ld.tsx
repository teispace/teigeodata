import { serializeJsonLd, type Thing } from '@/lib/seo';

/** Structured data for search engines. */
export function JsonLd({ things }: { things: Thing[] }) {
  // Escaped by serializeJsonLd, so the data cannot close the script.
  return <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: serializeJsonLd(things) }} />;
}
