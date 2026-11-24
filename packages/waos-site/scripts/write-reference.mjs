/**
 * Writes the reference's pages on the WIT into `content/docs/reference`, from `wit/deps` as it is
 * when the site builds, so they never drift from it: for each package, a page per interface, its
 * doc comment, its Rust API link on docs.rs, and then each item's doc comment and WIT, and a page
 * listing its interfaces. `npm run dev` and `npm run build` run it first. What it writes is not
 * committed, and each run replaces it whole, so an interface taken out of the WIT goes from the
 * site too.
 */
import { mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { readPackage } from './read-wit.mjs';

/** The packages the reference shows; `content/docs/reference/meta.json` orders them. */
const PACKAGES = ['wasdk:platform', 'wasdk:media'];
const REFERENCE_ROUTE = '/docs/reference';
/** WIT interfaces a program exports rather than imports, which have no module in `wasdk`. */
const EXPORT_ONLY_INTERFACES = new Set(['worker-entry']);

const siteRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const repoRoot = join(siteRoot, '../..');
const witDeps = join(repoRoot, 'wit/deps');
const referenceDir = join(siteRoot, 'content/docs/reference');

const version = workspaceVersion();
const wasdkModules = wasdkReexportedModules();
const generatedDocsRsModules = [];

for (const name of PACKAGES) writePackage(readPackage(packageDir(name)));
verifyDocsRsLinks(generatedDocsRsModules, wasdkModules);

/** The workspace version from the root `Cargo.toml`. */
function workspaceVersion() {
  const manifest = readFileSync(join(repoRoot, 'Cargo.toml'), 'utf8');
  const found = /\[workspace\.package\][^[]*?^version = "([^"]+)"/m.exec(manifest);
  if (!found) throw new Error('the workspace names no version in Cargo.toml');
  return found[1];
}

/**
 * The `wasdk:platform` and `wasdk:media` module names re-exported at the root of `sdk/wasdk/src/lib.rs`,
 * so generated `docs.rs/wasdk/<version>/wasdk/<module>/` links are checked against the real crate.
 */
function wasdkReexportedModules() {
  const source = readFileSync(join(repoRoot, 'sdk/wasdk/src/lib.rs'), 'utf8');
  const modules = new Set();
  for (const match of source.matchAll(/pub use bindings::wasdk::(?:platform|media)::\{([^}]+)\};/g)) {
    for (const raw of match[1].split(',')) {
      const name = raw.trim().replace(/^.*?\bas\s+/, '');
      if (name) modules.add(name);
    }
  }
  if (modules.size === 0) throw new Error('no wasdk re-exports found in sdk/wasdk/src/lib.rs');
  return modules;
}

/** Verifies every generated `docs.rs/wasdk` module link exists as a re-export in `sdk/wasdk/src/lib.rs`. */
function verifyDocsRsLinks(generated, exported) {
  const missing = generated.filter(({ moduleName }) => !exported.has(moduleName));
  if (missing.length > 0) {
    const names = missing.map(({ iface, url }) => `${iface} -> ${url}`).join(', ');
    throw new Error(`generated docs.rs link(s) missing from sdk/wasdk/src/lib.rs re-exports: ${names}`);
  }
}

/** The folder of `wit/deps` that holds the package, `wasdk-platform-2.2.0` for `wasdk:platform`. */
function packageDir(name) {
  const prefix = `${packageSlug(name)}-`;
  const dirs = readdirSync(witDeps).filter((dir) => dir.startsWith(prefix) && /^\d/.test(dir.slice(prefix.length)));
  if (dirs.length !== 1) throw new Error(`${witDeps}: ${dirs.length} folders for ${name}, where one was expected`);
  return join(witDeps, dirs[0]);
}

function writePackage(pkg) {
  const dir = join(referenceDir, packageSlug(pkg.name));
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  const meta = { title: pkg.name, pages: pkg.interfaces.map((iface) => iface.name) };
  writeFileSync(join(dir, 'meta.json'), `${JSON.stringify(meta, null, 2)}\n`);
  writeFileSync(join(dir, 'index.md'), packagePage(pkg));
  for (const iface of pkg.interfaces) writeFileSync(join(dir, `${iface.name}.md`), interfacePage(pkg, iface));
}

/** The package's page: its interfaces, each with the first sentence of its doc comment. */
function packagePage(pkg) {
  const list = pkg.interfaces.map((iface) => {
    const summary = firstSentence(iface.docs);
    return `- [\`${iface.name}\`](${interfaceUrl(pkg, iface)})${summary ? `: ${summary}` : ''}`;
  });
  return page({ title: pkg.name, description: pkg.id }, [pkg.docs, list.join('\n')]);
}

/** An interface's page: its doc comment, its Rust API link on docs.rs, its `use`s, then each item in the order the WIT has them. */
function interfacePage(pkg, iface) {
  const uses = iface.uses.length > 0 ? blocks(['## use', witBlock(iface.uses.join('\n'))]) : '';
  return page({ title: iface.name, description: interfaceId(pkg, iface) }, [
    iface.docs,
    rustApiNote(pkg, iface),
    uses,
    ...iface.items.map(itemSection),
  ]);
}

/** The docs.rs link for an interface's Rust module in `wasdk`, or its export macro where it is export-only. */
function rustApiNote(pkg, iface) {
  const moduleName = iface.name.replaceAll('-', '_');
  if (EXPORT_ONLY_INTERFACES.has(iface.name)) {
    if (wasdkModules.has(moduleName)) {
      throw new Error(`${iface.name} is listed in EXPORT_ONLY_INTERFACES but re-exported as wasdk::${moduleName}`);
    }
    return `Rust API: exported by the program through [\`#[wasdk::worker]\`](https://docs.rs/wasdk/${version}/wasdk/attr.worker.html) rather than imported as a module.`;
  }
  const url = `https://docs.rs/wasdk/${version}/wasdk/${moduleName}/`;
  generatedDocsRsModules.push({ iface: `${pkg.name}/${iface.name}`, moduleName, url });
  return `Rust API: [\`wasdk::${moduleName}\`](${url}) on docs.rs.`;
}

/** An item, with a resource's functions after it, each under a heading of its own. */
function itemSection(item) {
  const members = (item.members ?? []).map((member) =>
    blocks([`### \`${item.name}.${member.name}\``, member.docs, witBlock(member.wit)]),
  );
  return blocks([`## ${item.kind} \`${item.name}\``, item.docs, witBlock(item.wit), ...members]);
}

function page(frontmatter, sections) {
  const fields = Object.entries(frontmatter).map(([key, value]) => `${key}: ${JSON.stringify(value)}`);
  return `${blocks([['---', ...fields, '---'].join('\n'), ...sections])}\n`;
}

function blocks(parts) {
  return parts.filter(Boolean).join('\n\n');
}

function witBlock(wit) {
  return `\`\`\`wit\n${wit}\n\`\`\``;
}

/** The doc comment's first sentence: up to the first full stop that ends one, outside code. */
function firstSentence(docs) {
  const paragraph = docs.split(/\n\s*\n/)[0].replace(/\s*\n\s*/g, ' ');
  let inCode = false;
  for (let at = 0; at < paragraph.length; at += 1) {
    if (paragraph[at] === '`') inCode = !inCode;
    else if (!inCode && paragraph[at] === '.' && (paragraph[at + 1] ?? ' ') === ' ') return paragraph.slice(0, at + 1);
  }
  return paragraph;
}

/** The name a world imports the interface by, `wasdk:platform/windows@2.2.0`. */
function interfaceId(pkg, iface) {
  return `${pkg.name}/${iface.name}${pkg.version ? `@${pkg.version}` : ''}`;
}

function interfaceUrl(pkg, iface) {
  return `${REFERENCE_ROUTE}/${packageSlug(pkg.name)}/${iface.name}`;
}

/** The package's name as a path's segment: `wasdk-platform` for `wasdk:platform`. */
function packageSlug(name) {
  return name.replace(':', '-');
}
