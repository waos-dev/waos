/**
 * Writes the docs of this version as one archive, `waos-docs-<version>.tar.gz`, which the release
 * attaches to its GitHub release and `cargo waos docs` downloads: every page of the site as plain
 * Markdown, the reference's among them, made by `write-reference.mjs` as the site's build makes
 * them; the playbook, `agents.md`; and `manifest.json`, the version and the pages in the
 * sidebar's order, each with its title and one line.
 *
 * `node scripts/write-docs-bundle.mjs [<out>]` writes the folder `waos-docs-<version>` and the
 * archive into `<out>`, `dist/` by default; the version is the workspace's.
 *
 * The invariant: a page is named as the site's address names it, less `/docs/` (`guide/network`
 * for `/docs/guide/network`), and is the file `docs/<name>.md` in the archive, so a link in one
 * page is the name of another, and the order is the sidebar's: each folder's `meta.json`, a
 * `---Name---` in it heading the pages after it, then what it leaves out, by name.
 */
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const siteRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const content = join(siteRoot, 'content');
const SITE = 'https://waos.app';

// The reference's pages, written into content/docs/reference as the site's build writes them.
await import('./write-reference.mjs');

const version = workspaceVersion();
const out = resolve(process.argv[2] ?? join(siteRoot, 'dist'));
const name = `waos-docs-${version}`;
const folder = join(out, name);
rmSync(folder, { recursive: true, force: true });
mkdirSync(folder, { recursive: true });

const entries = [];
addFolder('docs', 0, entries);
for (const entry of entries.filter((entry) => entry.kind === 'page')) {
  const file = join(folder, 'docs', `${entry.name}.md`);
  mkdirSync(dirname(file), { recursive: true });
  writeFileSync(file, entry.markdown);
  delete entry.markdown;
}
writeFileSync(join(folder, 'agents.md'), readFileSync(join(content, 'agents.md'), 'utf8'));
writeFileSync(join(folder, 'manifest.json'), `${JSON.stringify({ version, pages: entries }, null, 2)}\n`);

const archive = join(out, `${name}.tar.gz`);
// COPYFILE_DISABLE keeps macOS's tar from adding its `._` files of extended attributes.
execFileSync('tar', ['-czf', archive, '-C', folder, 'manifest.json', 'agents.md', 'docs'], {
  env: { ...process.env, COPYFILE_DISABLE: '1' },
});
console.log(archive);

/** The version every crate of the workspace has, from its `Cargo.toml`. */
function workspaceVersion() {
  const manifest = readFileSync(join(siteRoot, '../../Cargo.toml'), 'utf8');
  const found = /\[workspace\.package\][^[]*?^version = "([^"]+)"/m.exec(manifest);
  if (!found) throw new Error('the workspace names no version');
  return found[1];
}

/** Adds the pages and folders in `path` (from `content`), in its `meta.json`'s order, at `depth`. */
function addFolder(path, depth, entries) {
  let itemDepth = depth;
  for (const item of order(path)) {
    if (item.separator) {
      entries.push({ kind: 'heading', title: item.separator, depth });
      itemDepth = depth + 1;
    } else {
      addItem(`${path}/${item.name}`, itemDepth, entries);
    }
  }
}

/** Adds the page or folder at `path`: a folder as its index page, or a heading of its title, then what it holds. */
function addItem(path, depth, entries) {
  const page = pageFile(path);
  if (page) {
    entries.push(pageEntry(path, page, depth));
    return;
  }
  const index = pageFile(`${path}/index`);
  if (index) entries.push(pageEntry(path, index, depth));
  else entries.push({ kind: 'heading', title: meta(path).title ?? path.split('/').pop(), depth });
  addFolder(path, depth + 1, entries);
}

function pageEntry(path, file, depth) {
  const { fields, body } = frontMatter(readFileSync(file, 'utf8'));
  const name = path.slice('docs/'.length);
  const title = fields.title ?? name;
  const description = fields.description ?? null;
  const markdown = [`# ${title}`, description, plainMarkdown(body).trim()].filter(Boolean).join('\n\n');
  return { kind: 'page', name, title, description, depth, markdown: `${markdown}\n` };
}

/** The folder's items: those its `meta.json` lists that are there, then the rest by name. */
function order(path) {
  const present = children(path);
  const listed = (meta(path).pages ?? []).flatMap((entry) => {
    const separator = /^---(.*)---$/.exec(entry);
    if (separator) return [{ separator: separator[1] }];
    return present.includes(entry) ? [{ name: entry }] : [];
  });
  const named = new Set(listed.map((item) => item.name).filter(Boolean));
  return [...listed, ...present.filter((child) => !named.has(child)).map((child) => ({ name: child }))];
}

/** The names of the pages and folders in `path`, but its index and `meta.json`. */
function children(path) {
  const names = readdirSync(join(content, path), { withFileTypes: true }).flatMap((entry) => {
    if (entry.isDirectory()) return [entry.name];
    const stem = /^(.*)\.mdx?$/.exec(entry.name)?.[1];
    return stem && stem !== 'index' ? [stem] : [];
  });
  return [...new Set(names)].sort();
}

function pageFile(path) {
  return ['.mdx', '.md'].map((extension) => join(content, `${path}${extension}`)).find(existsSync);
}

function meta(path) {
  const file = join(content, path, 'meta.json');
  return existsSync(file) ? JSON.parse(readFileSync(file, 'utf8')) : {};
}

/** A page's front matter, each field a plain or a double-quoted string, and what follows it. */
function frontMatter(text) {
  const found = /^---\n([\s\S]*?)\n---\n?/.exec(text);
  if (!found) return { fields: {}, body: text };
  const fields = {};
  for (const line of found[1].split('\n')) {
    const field = /^(\w+):\s*(.*)$/.exec(line);
    if (field) fields[field[1]] = field[2].startsWith('"') ? JSON.parse(field[2]) : field[2];
  }
  return { fields, body: text.slice(found[0].length) };
}

/** The page's MDX as plain Markdown: the icons page's components as a list of its pictures. */
function plainMarkdown(body) {
  return body
    .split('\n')
    .filter((line) => !/^\s*<\/?AppIcons>\s*$/.test(line))
    .map((line) => {
      const icon = /^\s*<AppIcon name="([^"]+)" src="([^"]+)" \/>\s*$/.exec(line);
      return icon ? `- ${icon[1]}: ${icon[2].startsWith('/') ? SITE : ''}${icon[2]}` : line;
    })
    .join('\n');
}
