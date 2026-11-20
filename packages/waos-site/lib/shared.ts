export const appName = 'WaOS';
/** The first section's statement, in two lines. */
export const headline = ['Apps on demand.', 'Safe to run. Built in minutes.'] as const;
export const subtitle =
  'One lightweight runtime for real GPU apps—built by your agent or opened from a single .wapk file.';
export const tagline = headline.join(' ');
export const description = subtitle;

export function siteOrigin(): string {
  return process.env.NEXT_PUBLIC_SITE_URL ?? 'https://waos.app';
}

export const docsRoute = '/docs';

export const repositoryUrl = 'https://github.com/waos-dev/waos';

/** Waft's Cupertino gallery, deployed for Waft; it will move. */
export const cupertinoGalleryUrl = 'https://cupertino-gallery.inset.rs';

/** One line pointing an agent at its playbook, optionally tailored to a specific app idea. */
export function agentPrompt(idea?: string, origin = siteOrigin()): string {
  const target = idea ? `a ${idea}` : 'an app';
  return `Read ${origin}/agents.md and follow it to build ${target} on WaOS.`;
}
