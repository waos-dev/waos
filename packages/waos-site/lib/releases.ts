import { repositoryUrl } from '@/lib/shared';

/** What a release is downloaded as on each system, by the name its release job gives the file. */
export const platforms = {
  macos: /^WaOS-.+-macos-arm64\.dmg$/,
  windows: /^WaOS-.+-windows-x64-setup\.exe$/,
  linux: /^WaOS-.+-linux-x86_64\.tar\.gz$/,
  android: /^WaOS-.+-android-arm64\.apk$/,
} as const;

export type Platform = keyof typeof platforms;

export function isPlatform(name: string): name is Platform {
  return Object.hasOwn(platforms, name);
}

/** One file of a release, as waos.app serves it. */
export interface Download {
  name: string;
  size: number;
  /** Where waos.app/download/<platform> sends: the copy on download.waos.app once the release's
   * files are mirrored there, GitHub's file until then. */
  url: string;
  /** GitHub's address of the file. */
  github: string;
}

/** Where release files are mirrored, as `<tag>/<file>` (`.github/workflows/mirror-release.yml`). */
export const mirrorOrigin = 'https://download.waos.app';

/** The newest release of WaOS, and its file for each system that has one. */
export interface Latest {
  version: string;
  tag: string;
  publishedAt: string;
  downloads: Partial<Record<Platform, Download>>;
}

/** How long a read of GitHub's newest release is cached at the edge before it is read again, in seconds. */
export const latestMaxAge = 300;

/**
 * Response caching for `/api/latest` and `/download/<platform>`: browsers never cache the
 * redirect or JSON, while Vercel's edge caches it for {@link latestMaxAge} seconds with no
 * stale-while-revalidate, so once 5 minutes pass a download waits for the fresh release instead
 * of handing out an older version.
 */
export const releaseCacheHeaders = {
  'Cache-Control': 'no-store',
  'Vercel-CDN-Cache-Control': `public, s-maxage=${latestMaxAge}`,
} as const;

interface GitHubRelease {
  tag_name: string;
  published_at: string;
  draft: boolean;
  prerelease: boolean;
  assets: { name: string; size: number; browser_download_url: string }[];
}

/**
 * WaOS's newest published release, read directly from GitHub (no Next.js Data Cache SWR layer).
 * Reads the last few releases so that while a new release's platform jobs are still uploading,
 * each platform resolves to the newest release that has its file rather than returning 404.
 */
export async function latestRelease(): Promise<Latest> {
  const repository = new URL(repositoryUrl).pathname.slice(1);
  const token = process.env.GITHUB_TOKEN;
  const response = await fetch(`https://api.github.com/repos/${repository}/releases?per_page=5`, {
    headers: {
      Accept: 'application/vnd.github+json',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    cache: 'no-store',
  });
  if (!response.ok) {
    throw new Error(`GitHub answered ${response.status} for the latest releases`);
  }
  const releases = ((await response.json()) as GitHubRelease[]).filter(
    (release) => !release.draft && !release.prerelease,
  );
  const newest = releases[0];
  if (!newest) {
    throw new Error(`No published release found for ${repository}`);
  }

  const downloads: Partial<Record<Platform, Download>> = {};
  await Promise.all(
    (Object.entries(platforms) as [Platform, RegExp][]).map(async ([platform, pattern]) => {
      for (const release of releases) {
        const asset = release.assets.find((item) => pattern.test(item.name));
        if (!asset) continue;
        const mirrored = `${mirrorOrigin}/${release.tag_name}/${asset.name}`;
        const url = (await isMirrored(mirrored)) ? mirrored : asset.browser_download_url;
        downloads[platform] = { name: asset.name, size: asset.size, url, github: asset.browser_download_url };
        return;
      }
    }),
  );
  return {
    version: newest.tag_name.replace(/^v/, ''),
    tag: newest.tag_name,
    publishedAt: newest.published_at,
    downloads,
  };
}

/** Whether the mirror has the file at `url`, checked live without Next.js's stale fetch cache. */
async function isMirrored(url: string): Promise<boolean> {
  try {
    const response = await fetch(url, { method: 'HEAD', cache: 'no-store', signal: AbortSignal.timeout(5000) });
    return response.ok;
  } catch {
    return false;
  }
}

/** waos.app's address of the newest release's file for `platform`. */
export function downloadPath(platform: Platform): string {
  return `/download/${platform}`;
}
