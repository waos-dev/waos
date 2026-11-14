/**
 * Writes the icons of WaOS's own apps into `public/apps`, from `builtin-apps` as it is when the
 * site builds, so they never drift from it: each app's icon, as its `[package.metadata.waos]`
 * names it, at 512 pixels, as `/apps/<name>.png` (`/apps/video-player.png` for Video Player).
 * `npm run dev` and `npm run build` run it first. What it writes is not committed, and each run
 * replaces it whole, so an app taken out goes from the site too.
 */
import { mkdirSync, readdirSync, readFileSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import sharp from 'sharp';

const ICON_SIZE = 512;

const siteRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const builtinApps = join(siteRoot, '../../builtin-apps');
const iconsDir = join(siteRoot, 'public/apps');

rmSync(iconsDir, { recursive: true, force: true });
mkdirSync(iconsDir, { recursive: true });
for (const app of apps()) await writeIcon(app);

/** Each built-in app, with its name and the path of its icon, from its `Cargo.toml`. */
function apps() {
  return readdirSync(builtinApps, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => {
      const dir = join(builtinApps, entry.name);
      const metadata = waosMetadata(join(dir, 'Cargo.toml'));
      return { name: metadata.name, icon: join(dir, metadata.icon) };
    });
}

/** The `name` and `icon` of a manifest's `[package.metadata.waos]`, each a string on a line of its own. */
function waosMetadata(manifest) {
  const [, table] = readFileSync(manifest, 'utf8').split(/^\[package\.metadata\.waos\]$/m);
  if (table === undefined) throw new Error(`${manifest}: no [package.metadata.waos]`);
  const fields = table.split(/^\[/m)[0];
  const field = (key) => {
    const value = new RegExp(`^${key} = "([^"]+)"$`, 'm').exec(fields)?.[1];
    if (value === undefined) throw new Error(`${manifest}: [package.metadata.waos] has no ${key}`);
    return value;
  };
  return { name: field('name'), icon: field('icon') };
}

async function writeIcon(app) {
  await sharp(app.icon).resize(ICON_SIZE, ICON_SIZE).png().toFile(join(iconsDir, `${iconSlug(app.name)}.png`));
}

/** The app's name as a file's: `video-player` for Video Player. */
function iconSlug(name) {
  return name.toLowerCase().replaceAll(' ', '-');
}
