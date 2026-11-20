import { repositoryUrl } from '@/lib/shared';

/** What a release is downloaded as on each system, by the name its release job gives the file. */
export const platforms = {
  macos: /^WaOS_.+_aarch64\.dmg$/,
  windows: /^WaOS-.+-windows-x64-setup\.exe$/,
  linux: /^waos-.+-linux-x86_64\.tar\.gz$/,
  android: /^WaOS-.+\.apk$/,
} as const;

export type Platform = keyof typeof platforms;

export function isPlatform(name: string): name is Platform {
  return Object.hasOwn(platforms, name);
}

/** One file of a release, as waos.app serves it. */
export interface Download {
  name: string;
  size: number;
  /** GitHub's address of the file, which waos.app/download/<platform> sends to. */
  url: string;
}

/** The newest release of WaOS, and its file for each system that has one. */
export interface Latest {
  version: string;
  tag: string;
  publishedAt: string;
  downloads: Partial<Record<Platform, Download>>;
}

/** How long a read of GitHub's newest release is kept before it is read again, in seconds. */
export const latestMaxAge = 300;

interface GitHubRelease {
  tag_name: string;
  published_at: string;
  assets: { name: string; size: number; browser_download_url: string }[];
}

/**
 * WaOS's newest release, read from GitHub and kept for {@link latestMaxAge} seconds, so the site
 * names no version of its own: a new release is what every download and install script gets
 * within minutes of its files being attached.
 */
export async function latestRelease(): Promise<Latest> {
  const repository = new URL(repositoryUrl).pathname.slice(1);
  const token = process.env.GITHUB_TOKEN;
  const response = await fetch(`https://api.github.com/repos/${repository}/releases/latest`, {
    headers: {
      Accept: 'application/vnd.github+json',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    next: { revalidate: latestMaxAge },
  });
  if (!response.ok) {
    throw new Error(`GitHub answered ${response.status} for the latest release`);
  }
  const release = (await response.json()) as GitHubRelease;
  const downloads: Partial<Record<Platform, Download>> = {};
  for (const [platform, pattern] of Object.entries(platforms) as [Platform, RegExp][]) {
    const asset = release.assets.find((asset) => pattern.test(asset.name));
    if (asset) {
      downloads[platform] = { name: asset.name, size: asset.size, url: asset.browser_download_url };
    }
  }
  return {
    version: release.tag_name.replace(/^v/, ''),
    tag: release.tag_name,
    publishedAt: release.published_at,
    downloads,
  };
}

/** waos.app's address of the newest release's file for `platform`. */
export function downloadPath(platform: Platform): string {
  return `/download/${platform}`;
}
