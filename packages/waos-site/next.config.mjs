import { createMDX } from 'fumadocs-mdx/next';

const withMDX = createMDX();

/** @type {import('next').NextConfig} */
const config = {
  reactStrictMode: true,
  // Next writes its own AGENTS.md and CLAUDE.md into the package on dev start; the repository's
  // AGENTS.md at the root is the one that holds.
  agentRules: false,
  async redirects() {
    // The docs have no page of their own: they open on their first. Waft's pages were under
    // /docs/frameworks before the docs were arranged by what an app does, and Waft's full framework
    // guides now live on waft.rs/docs.
    return [
      { source: '/docs', destination: '/docs/concepts', permanent: false },
      { source: '/docs/frameworks/waft', destination: '/docs/waft', permanent: true },
      { source: '/docs/frameworks/waft.md', destination: '/docs/waft.md', permanent: true },
      { source: '/docs/frameworks', destination: '/docs/waft', permanent: true },
      { source: '/docs/frameworks.md', destination: '/docs/waft.md', permanent: true },
      { source: '/docs/waft/:slug', destination: 'https://waft.rs/docs/:slug', permanent: true },
    ];
  },
  async rewrites() {
    return [{ source: '/docs/:path*.md', destination: '/llms.mdx/docs/:path*' }];
  },
};

export default withMDX(config);
