import { latestMaxAge, latestRelease } from '@/lib/releases';

export const revalidate = 300;

/**
 * The newest release as JSON: `{ version, tag, publishedAt, downloads: { macos, windows, linux,
 * android } }`, each download `{ name, size, url }` where the release has one. The install
 * scripts read it, and anything else that has to name the current version.
 */
export async function GET() {
  const latest = await latestRelease();
  return Response.json(latest, {
    headers: { 'Cache-Control': `public, s-maxage=${latestMaxAge}, stale-while-revalidate=86400` },
  });
}
