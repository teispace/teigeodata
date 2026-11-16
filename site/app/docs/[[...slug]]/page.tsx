import type { Metadata } from 'next';
import { notFound } from 'next/navigation';
import {
  DocsBody,
  DocsDescription,
  DocsPage,
  DocsTitle,
  MarkdownCopyButton,
  ViewOptionsPopover,
} from 'fumadocs-ui/layouts/docs/page';
import { createRelativeLink } from 'fumadocs-ui/mdx';
import { source } from '@/lib/source';
import { getMDXComponents } from '@/components/mdx';
import { basePath, gitConfig, repoUrl } from '@/lib/shared';
import { JsonLd } from '@/components/json-ld';
import { sectionOf } from '@/lib/og-pages';
import { breadcrumbs, ogImage, pageMetadata, techArticle } from '@/lib/seo';

export default async function Page(props: { params: Promise<{ slug?: string[] }> }) {
  const params = await props.params;
  const page = source.getPage(params.slug);
  if (!page) notFound();

  const MDX = page.data.body;
  const markdownUrl = `${basePath}/llms.mdx${page.url}.md`;

  return (
    <DocsPage toc={page.data.toc} full={page.data.full}>
      <JsonLd
        things={[
          techArticle({
            title: page.data.title,
            description: page.data.description ?? '',
            path: `${page.url}/`,
            image: ogImage(['docs', ...page.slugs]),
          }),
          breadcrumbs([
            { name: 'Docs', path: '/docs/' },
            ...(page.slugs.length > 1 ? [{ name: sectionOf(page.slugs), path: `/docs/${page.slugs[0]}/` }] : []),
            ...(page.slugs.length > 0 ? [{ name: page.data.title, path: `${page.url}/` }] : []),
          ]),
        ]}
      />
      <DocsTitle>{page.data.title}</DocsTitle>
      <DocsDescription className="mb-0">{page.data.description}</DocsDescription>
      <div className="flex flex-row gap-2 items-center border-b pb-6">
        <MarkdownCopyButton markdownUrl={markdownUrl} />
        <ViewOptionsPopover
          markdownUrl={markdownUrl}
          githubUrl={`${repoUrl}/blob/${gitConfig.branch}/site/content/docs/${encodeURI(page.path)}`}
        />
      </div>
      <DocsBody>
        <MDX components={getMDXComponents({ a: createRelativeLink(source, page) })} />
      </DocsBody>
    </DocsPage>
  );
}

export function generateStaticParams() {
  return source.generateParams();
}

export async function generateMetadata(props: {
  params: Promise<{ slug?: string[] }>;
}): Promise<Metadata> {
  const params = await props.params;
  const page = source.getPage(params.slug);
  if (!page) notFound();
  return pageMetadata({
    title: page.slugs.length === 0 ? 'Documentation' : page.data.title,
    description: page.data.description ?? '',
    path: `${page.url}/`,
    image: ogImage(['docs', ...page.slugs]),
    type: 'article',
  });
}
