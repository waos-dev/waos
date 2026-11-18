import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createMDX } from 'fumadocs-mdx/next';

const withMDX = createMDX();
const repositoryRoot = join(dirname(fileURLToPath(import.meta.url)), '../..');

/** @type {import('next').NextConfig} */
const config = {
  reactStrictMode: true,
  // The docs' pages are in `crates/waos-cli/docs`, outside the site's folder, so the build reaches
  // from the repository's root.
  turbopack: { root: repositoryRoot },
  outputFileTracingRoot: repositoryRoot,
  // Next writes its own AGENTS.md and CLAUDE.md into the package on dev start; the repository's
  // AGENTS.md at the root is the one that holds.
  agentRules: false,
  async redirects() {
    // The docs have no page of their own: they open on their first. Waft's pages were under
    // /docs/frameworks before the docs were arranged by what an app does.
    return [
      { source: '/docs', destination: '/docs/concepts', permanent: false },
      { source: '/docs/frameworks/waft', destination: '/docs/waft', permanent: true },
      { source: '/docs/frameworks/waft.md', destination: '/docs/waft.md', permanent: true },
      { source: '/docs/frameworks', destination: '/docs/waft', permanent: true },
      { source: '/docs/frameworks.md', destination: '/docs/waft.md', permanent: true },
    ];
  },
  async rewrites() {
    return [{ source: '/docs/:path*.md', destination: '/llms.mdx/docs/:path*' }];
  },
};

export default withMDX(config);
