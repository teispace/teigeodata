import { createMDX } from 'fumadocs-mdx/next';

const withMDX = createMDX();

// The site is a static export (`out/`), served by GitHub Pages as a project site under
// `/teigeodata/`; `DOCS_BASE_PATH` is set by the docs workflow and empty for `next dev`.
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
