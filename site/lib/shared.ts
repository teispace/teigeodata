export const appName = 'teigeodata';
export const docsRoute = '/docs';

/** The path the site is served under: empty at https://teigeodata.teispace.com (D-353). */
export const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? '';

export const gitConfig = {
  user: 'teispace',
  repo: 'teigeodata',
  branch: 'main',
};

export const repoUrl = `https://github.com/${gitConfig.user}/${gitConfig.repo}`;

/** The data release the examples and the playground run on, pinned by SHA-256 in `site/data.json`. */
export const dataRelease = 'data-2026.11.14.3';
