import { isPlatform, latestRelease, releaseCacheHeaders } from '@/lib/releases';

export const dynamic = 'force-dynamic';

/**
 * `/download/macos`, `/download/windows`, `/download/linux`, `/download/android`: the newest
 * release's file for that system, by a redirect to its copy on download.waos.app, or to GitHub
 * before the release is mirrored. A link here never names a version, so it stays right across
 * releases.
 */
export async function GET(_request: Request, { params }: { params: Promise<{ platform: string }> }) {
  const { platform } = await params;
  if (!isPlatform(platform)) {
    return new Response(`No download for "${platform}": macos, windows, linux or android.\n`, {
      status: 404,
      headers: { 'Cache-Control': 'no-store' },
    });
  }
  const download = (await latestRelease()).downloads[platform];
  if (!download) {
    return new Response(`The newest release has no ${platform} download yet.\n`, {
      status: 404,
      headers: { 'Cache-Control': 'no-store' },
    });
  }
  return new Response(null, {
    status: 302,
    headers: {
      Location: download.url,
      ...releaseCacheHeaders,
    },
  });
}
