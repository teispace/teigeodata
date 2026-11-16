import { createMDX } from 'fumadocs-mdx/next';

const withMDX = createMDX();

// The site is a static export (`out/`), served by Vercel at the root of
// https://teigeodata.teispace.com (D-353): the landing page at `/`, the docs at `/docs`.
// `DOCS_BASE_PATH` is only for hosting it under a sub-path.
const basePath = process.env.DOCS_BASE_PATH ?? '';

/** @type {import('next').NextConfig} */
const config = {
  output: 'export',
  basePath,
  trailingSlash: true,
  reactStrictMode: true,
  // The site's own lock file, not one found above the repository.
  turbopack: { root: import.meta.dirname },
  images: { unoptimized: true },
  env: { NEXT_PUBLIC_BASE_PATH: basePath },
};

export default withMDX(config);
