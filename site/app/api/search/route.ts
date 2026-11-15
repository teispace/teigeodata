import { createFromSource } from 'fumadocs-core/search/server';
import { source } from '@/lib/source';

export const revalidate = false;

// Written once at build time: the site is a static export, searched in the browser.
export const { staticGET: GET } = createFromSource(source, { language: 'english' });
