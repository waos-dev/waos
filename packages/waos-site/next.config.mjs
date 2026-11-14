import { createMDX } from 'fumadocs-mdx/next';

const withMDX = createMDX();

/** @type {import('next').NextConfig} */
const config = {
  reactStrictMode: true,
  // Next writes its own AGENTS.md and CLAUDE.md into the package on dev start; the repository's
  // AGENTS.md at the root is the one that holds.
  agentRules: false,
  async redirects() {
    // The docs have no page of their own: they open on their first.
    return [{ source: '/docs', destination: '/docs/concepts', permanent: false }];
  },
  async rewrites() {
    return [{ source: '/docs/:path*.md', destination: '/llms.mdx/docs/:path*' }];
  },
};

export default withMDX(config);
