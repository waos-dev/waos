export const appName = 'WaOS';
/** The first section's statement, in two lines. */
export const headline = ['Apps on demand.', 'Secure. Fast. Efficient.'] as const;
export const tagline = headline.join(' ');
export const description = tagline;

export function siteOrigin(): string {
  return process.env.NEXT_PUBLIC_SITE_URL ?? 'https://waos.app';
}

export const docsRoute = '/docs';

/** The download waits for WaOS's release (decision 9); the repository is public. */
export const downloadUrl = 'about:blank';
export const repositoryUrl = 'https://github.com/waos-dev/waos';

/** Waft's Cupertino gallery, deployed for Waft; it will move. */
export const cupertinoGalleryUrl = 'https://cupertino-gallery.inset.rs';

/** What the home page's second action copies: one line pointing an agent at its playbook. */
export function agentPrompt(): string {
  return `Read ${siteOrigin()}/agents.md and follow it to build an app on WaOS.`;
}
