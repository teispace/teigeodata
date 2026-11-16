import type { Metadata } from 'next';
import { absolute, site } from './site';

/** Structured data (schema.org), rendered by `<JsonLd>`. */
export type Thing = Record<string, unknown>;

/** A page's social card, rendered at build time by `app/og`. */
export function ogImage(slug: string[]): string {
  return absolute(`/og/${[...slug, 'image.png'].join('/')}`);
}

/**
 * A page's metadata with its canonical URL and social cards. `path` is the page's address
 * with its trailing slash, as the static export serves it.
 */
export function pageMetadata({
  title,
  description,
  path,
  image,
  type = 'website',
}: {
  title?: string;
  description: string;
  path: string;
  image: string;
  type?: 'website' | 'article';
}): Metadata {
  const card = { url: image, width: 1200, height: 630, alt: title ?? site.name };
  return {
    ...(title ? { title } : {}),
    description,
    alternates: { canonical: absolute(path) },
    openGraph: {
      type,
      siteName: site.name,
      locale: 'en_US',
      url: absolute(path),
      title: title ?? site.title,
      description,
      images: [card],
    },
    twitter: {
      card: 'summary_large_image',
      title: title ?? site.title,
      description,
      images: [image],
    },
  };
}

export const organization: Thing = {
  '@type': 'Organization',
  '@id': `${site.org.url}/#organization`,
  name: site.org.name,
  legalName: site.org.legalName,
  url: site.org.url,
  logo: absolute('/brand/icon.png'),
  sameAs: ['https://github.com/teispace'],
};

export const website: Thing = {
  '@type': 'WebSite',
  '@id': `${site.url}/#website`,
  name: site.name,
  url: site.url,
  description: site.description,
  inLanguage: 'en',
  publisher: { '@id': organization['@id'] },
};

/** The library, as a search engine shows software: what it is, its version and its code. */
export const softwareLibrary: Thing = {
  '@type': ['SoftwareApplication', 'SoftwareSourceCode'],
  '@id': `${site.url}/#library`,
  name: site.name,
  description: site.description,
  url: site.url,
  image: absolute('/brand/icon.png'),
  applicationCategory: 'DeveloperApplication',
  applicationSubCategory: 'Geocoding',
  operatingSystem: 'macOS, Linux, Windows, iOS, Android, web browsers',
  programmingLanguage: [
    'Rust',
    'C',
    'C++',
    'Swift',
    'Kotlin',
    'Java',
    'Dart',
    'Python',
    'JavaScript',
    'Go',
    'C#',
  ],
  softwareVersion: site.version,
  datePublished: site.versionDate,
  releaseNotes: site.changelog,
  codeRepository: site.github,
  license: 'https://www.apache.org/licenses/LICENSE-2.0',
  isAccessibleForFree: true,
  offers: { '@type': 'Offer', price: '0', priceCurrency: 'USD' },
  publisher: { '@id': organization['@id'] },
  author: { '@id': organization['@id'] },
};

export function faqPage(items: { q: string; a: string }[]): Thing {
  return {
    '@type': 'FAQPage',
    mainEntity: items.map(({ q, a }) => ({
      '@type': 'Question',
      name: q,
      acceptedAnswer: { '@type': 'Answer', text: a },
    })),
  };
}

export function breadcrumbs(items: { name: string; path: string }[]): Thing {
  return {
    '@type': 'BreadcrumbList',
    itemListElement: items.map((item, index) => ({
      '@type': 'ListItem',
      position: index + 1,
      name: item.name,
      item: absolute(item.path),
    })),
  };
}

export function techArticle({
  title,
  description,
  path,
  image,
}: {
  title: string;
  description: string;
  path: string;
  image: string;
}): Thing {
  return {
    '@type': 'TechArticle',
    headline: title,
    description,
    url: absolute(path),
    mainEntityOfPage: absolute(path),
    image: [image],
    inLanguage: 'en',
    isPartOf: { '@id': website['@id'] },
    about: { '@id': `${site.url}/#library` },
    author: { '@id': organization['@id'] },
    publisher: { '@id': organization['@id'] },
  };
}

/** Serializes structured data for a `<script>`; `<` is escaped so it cannot close the tag. */
export function serializeJsonLd(things: Thing[]): string {
  const graph = { '@context': 'https://schema.org', '@graph': things };
  return JSON.stringify(graph).replace(/</g, '\\u003c');
}
