import type { MetadataRoute } from 'next';
import { absolute } from '@/lib/site';

export const dynamic = 'force-static';

/**
 * Crawlers that answer questions with the web (AI search and assistants), named so the welcome
 * is explicit: the docs are written to be quoted, and `llms.txt` lists every page with the full
 * text in `llms-full.txt`.
 */
const AI_CRAWLERS = [
  'OAI-SearchBot',
  'ChatGPT-User',
  'GPTBot',
  'Claude-SearchBot',
  'Claude-User',
  'ClaudeBot',
  'PerplexityBot',
  'Perplexity-User',
  'Google-Extended',
  'Applebot-Extended',
  'DuckAssistBot',
  'MistralAI-User',
];

export default function robots(): MetadataRoute.Robots {
  return {
    rules: [
      { userAgent: '*', allow: '/' },
      { userAgent: AI_CRAWLERS, allow: '/' },
    ],
    sitemap: absolute('/sitemap.xml'),
    host: absolute('/'),
  };
}
