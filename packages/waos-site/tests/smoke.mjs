/**
 * A smoke test for the site: every page and route answers, and with its title. It also checks
 * what the home page's actions and its sample do, what decision 4 asks of the art short of how
 * it looks, that the reference has every interface of the WIT with all of its code, that the
 * icons page shows every built-in app's icon, and that no page logs an error. How the pages look is the lead's to check.
 *
 * Run against a production build: `npm run build && npm test`, which serves it with `next start`;
 * or against a site already serving, the Worker's local preview among them, by its URL in
 * `WAOS_SITE_URL`: `WAOS_SITE_URL=http://localhost:8787 npm test`.
 */
import { spawn } from 'node:child_process';
import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const siteRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const SITE_URL = process.env.WAOS_SITE_URL?.replace(/\/$/, '');
const PORT = Number(process.env.WAOS_SITE_PORT ?? 3312);
const BASE = SITE_URL ?? `http://localhost:${PORT}`;
const ORIGIN = 'https://waos.app';
const GALLERY = 'https://cupertino-gallery.inset.rs';
const WIT_DEPS = join(siteRoot, '../../wit/deps');
const BUILTIN_APPS = join(siteRoot, '../../builtin-apps');

const GUIDE_TOPICS = [
  ['windows', 'Windows'],
  ['back', 'Back'],
  ['system-bars', 'System bars'],
  ['keys', 'Keys'],
  ['images', 'Images'],
  ['webgpu', 'WebGPU'],
  ['compositing', 'Compositing'],
  ['fonts', 'Fonts'],
  ['file-dialogs', 'File dialogs'],
  ['file-watching', 'File watching'],
  ['clipboard', 'Clipboard'],
  ['network', 'Network'],
  ['media-players', 'Media players'],
  ['cameras', 'Cameras'],
  ['audio', 'Audio'],
  ['system', 'System settings'],
  ['lifecycle', 'Lifecycle'],
  ['opener', 'Opener'],
  ['menus', 'Menus'],
  ['notifications', 'Notifications'],
  ['processes', 'Processes'],
  ['workers', 'Workers'],
  ['threads', 'Threads'],
];

/**
 * The WIT packages the reference shows, each with its interfaces and the lines of code each
 * declares, read from `wit/deps` apart from the site's own reader, so a page that leaves
 * something out fails.
 */
const REFERENCE_PACKAGES = [
  ['wasdk-platform', 'wasdk:platform'],
  ['wasdk-media', 'wasdk:media'],
].map(([slug, name]) => ({ slug, name, interfaces: witInterfaces(slug) }));

/** The built-in apps' names, read from their manifests apart from the script that writes their icons. */
const BUILTIN_APP_NAMES = readdirSync(BUILTIN_APPS, { withFileTypes: true })
  .filter((entry) => entry.isDirectory())
  .map((entry) => {
    const manifest = readFileSync(join(BUILTIN_APPS, entry.name, 'Cargo.toml'), 'utf8');
    return /^\[package\.metadata\.waos\]$[^[]*^name = "([^"]+)"$/m.exec(manifest)[1];
  });
const ICON_SIZE = 512;

/** The crates the reference links on docs.rs. */
const DOCS_RS_CRATES = ['wasdk', 'wasdk-adapter-waft', 'waos', 'waos-capabilities'];

/** A line of `windows`, which its page shows as the WIT has it. */
const SET_MIN_SIZE = 'set-min-size: func(size: option<size>);';

/** Each docs page, by its path and its title. */
const DOCS_PAGES = [
  ['/docs/concepts', 'Concepts'],
  ['/docs/start', 'Start'],
  ['/docs/guide', 'Building the app'],
  ...GUIDE_TOPICS.map(([slug, title]) => [`/docs/guide/${slug}`, title]),
  ['/docs/waft', 'Waft'],
  ['/docs/tools', 'The tools'],
  ['/docs/compatibility', 'Compatibility'],
  ['/docs/icons', 'Icons'],
  ['/docs/reference', 'Reference'],
  ...REFERENCE_PACKAGES.flatMap(({ slug, name, interfaces }) => [
    [`/docs/reference/${slug}`, name],
    ...interfaces.map((iface) => [`/docs/reference/${slug}/${iface.name}`, iface.name]),
  ]),
];

function witInterfaces(slug) {
  const [dir] = readdirSync(WIT_DEPS).filter((entry) => new RegExp(`^${slug}-\\d`).test(entry));
  return readdirSync(join(WIT_DEPS, dir))
    .filter((file) => file.endsWith('.wit'))
    .flatMap((file) => interfacesIn(readFileSync(join(WIT_DEPS, dir, file), 'utf8')));
}

/** Each interface in a WIT file, with its lines of code: neither comments nor a body's closing brace. */
function interfacesIn(wit) {
  return wit
    .split(/^(?=interface )/m)
    .slice(1)
    .map((part) => {
      const lines = part.split('\n').map((line) => line.trim());
      const name = /^interface ([\w-]+)/.exec(lines[0])[1];
      const code = lines.slice(1).filter((line) => line !== '' && !line.startsWith('//') && line !== '}');
      return { name, code };
    });
}

let failures = 0;
function check(name, ok, detail = '') {
  console.log(`${ok ? 'ok  ' : 'FAIL'}  ${name}${detail ? `  — ${detail}` : ''}`);
  if (!ok) failures += 1;
}

function startServer() {
  return spawn('npx', ['next', 'start', '-p', String(PORT)], { cwd: siteRoot, stdio: 'ignore' });
}

async function waitForServer() {
  for (let attempt = 0; attempt < 60; attempt += 1) {
    try {
      const response = await fetch(BASE);
      if (response.ok) return;
    } catch {
      /* not listening yet */
    }
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  throw new Error(`the site never came up on ${BASE}`);
}

/** A browser context with the gallery, another site's, kept out, so the test runs offline. */
async function newContext(browser, options = {}) {
  const context = await browser.newContext({ viewport: { width: 1200, height: 900 }, ...options });
  await context.route(`${GALLERY}/**`, (route) => route.abort());
  return context;
}

/** Fetches `path` and checks it answers as `type`, its body starting with `start`. */
async function checkRoute(path, type, start, init) {
  const response = await fetch(`${BASE}${path}`, init);
  const body = await response.text();
  check(`${path} answers`, response.ok, `${response.status}`);
  check(`${path} is ${type}`, (response.headers.get('content-type') ?? '').startsWith(type));
  check(`${path} starts with its title`, body.startsWith(start), body.split('\n')[0]);
  return body;
}

async function checkMarkdownRoutes() {
  const index = await checkRoute('/llms.txt', 'text/markdown', '# WaOS');
  const listed = DOCS_PAGES.every(([path]) => index.includes(`(${path})`));
  check('llms.txt lists every docs page', listed);
  const full = await checkRoute('/llms-full.txt', 'text/markdown', '# ');
  const whole = DOCS_PAGES.every(([path]) => full.includes(`(${ORIGIN}${path})`));
  check('llms-full.txt has every docs page, by its URL on waos.app', whole);
  check('llms-full.txt has the WIT', full.includes(SET_MIN_SIZE));
  for (const [path, title] of DOCS_PAGES) {
    await checkRoute(`${path}.md`, 'text/markdown', `# ${title} (${ORIGIN}${path})`);
  }
  const asked = { headers: { Accept: 'text/markdown' } };
  await checkRoute('/docs/concepts', 'text/markdown', '# Concepts', asked);
  const playbook = await checkRoute('/agents.md', 'text/markdown', '# Build an app on WaOS');
  check("the playbook's icon step links the icons page", playbook.includes(`${ORIGIN}/docs/icons`));
}

/** Each interface's page, as Markdown, holds every line of its WIT's code. */
async function checkReferenceMarkdown() {
  for (const { slug, interfaces } of REFERENCE_PACKAGES) {
    for (const iface of interfaces) {
      const path = `/docs/reference/${slug}/${iface.name}`;
      const markdown = await (await fetch(`${BASE}${path}.md`)).text();
      const missing = iface.code.filter((line) => !markdown.includes(line));
      check(`${path} has all of its WIT`, missing.length === 0, missing.slice(0, 2).join(' | '));
    }
  }
}

async function checkSiteRoutes() {
  const sitemap = await (await fetch(`${BASE}/sitemap.xml`)).text();
  const urls = [ORIGIN, ...DOCS_PAGES.map(([path]) => `${ORIGIN}${path}`)];
  check('the sitemap lists every page on waos.app', urls.every((url) => sitemap.includes(`${url}<`)));
  const robots = await (await fetch(`${BASE}/robots.txt`)).text();
  check('robots.txt points at the sitemap', robots.includes(`Sitemap: ${ORIGIN}/sitemap.xml`));
  const search = await fetch(`${BASE}/api/search?query=cameras`);
  const results = search.ok ? await search.json() : [];
  check('search finds a guide', results.some((result) => result.url === '/docs/guide/cameras'));
  const docs = await fetch(`${BASE}/docs`, { redirect: 'manual' });
  check('/docs opens on its first page', docs.headers.get('location') === '/docs/concepts');
  const moved = await fetch(`${BASE}/docs/frameworks/waft`, { redirect: 'manual' });
  check("Waft's old address leads to its page", moved.headers.get('location') === '/docs/waft');
  const waftSubpage = await fetch(`${BASE}/docs/waft/state`, { redirect: 'manual' });
  check("Waft's framework subpages redirect to waft.rs", waftSubpage.headers.get('location') === 'https://waft.rs/docs/state');
  const unknownPlatform = await fetch(`${BASE}/download/unknown`, { redirect: 'manual' });
  check('/download/unknown returns 404', unknownPlatform.status === 404, `${unknownPlatform.status}`);
  check(
    '/download/unknown is not cached',
    (unknownPlatform.headers.get('cache-control') ?? '').includes('no-store'),
    unknownPlatform.headers.get('cache-control') ?? '',
  );
  const latest = await fetch(`${BASE}/api/latest`);
  if (latest.ok) {
    const cacheControl = latest.headers.get('cache-control') ?? '';
    const cdnCache = latest.headers.get('vercel-cdn-cache-control') ?? '';
    check('/api/latest sets Cache-Control: no-store', cacheControl.includes('no-store'), cacheControl);
    check('/api/latest has no stale-while-revalidate', !cacheControl.includes('stale-while-revalidate') && !cdnCache.includes('stale-while-revalidate'), `${cacheControl} | ${cdnCache}`);
    if (!SITE_URL) {
      check('/api/latest sets 5-minute Vercel edge cache', cdnCache === 'public, s-maxage=300', cdnCache);
    }
    const payload = await latest.json();
    check('/api/latest returns version and all 4 platforms', Boolean(payload.version && payload.downloads?.macos && payload.downloads?.windows && payload.downloads?.linux && payload.downloads?.android), payload.version);
    const mac = await fetch(`${BASE}/download/macos`, { redirect: 'manual' });
    const macCache = mac.headers.get('cache-control') ?? '';
    const macCdn = mac.headers.get('vercel-cdn-cache-control') ?? '';
    check('/download/macos redirects with 302', mac.status === 302, `${mac.status}`);
    check('/download/macos sets Cache-Control: no-store', macCache.includes('no-store'), macCache);
    if (!SITE_URL) {
      check('/download/macos sets 5-minute Vercel edge cache', macCdn === 'public, s-maxage=300', macCdn);
    }
  } else if (SITE_URL) {
    check('/api/latest answers on deployed site', false, `${latest.status}`);
  }
}

/** Loads `path` in the browser and checks it answers with `title` as its heading. */
async function checkPage(page, path, title) {
  const response = await page.goto(`${BASE}${path}`, { waitUntil: 'load' });
  const heading = (await page.locator('h1').first().innerText()).trim();
  check(`${path} answers`, response?.ok() === true, `${response?.status()}`);
  check(`${path} is titled ${title}`, heading === title, heading);
}

async function checkDocsPages(page) {
  for (const [path, title] of DOCS_PAGES) {
    await checkPage(page, path, title);
    check(`${path}'s document is titled ${title} — WaOS`, (await page.title()) === `${title} — WaOS`);
  }
}

async function linksIn(page, selector) {
  return page.locator(`${selector} a`).evaluateAll((links) => links.map((link) => link.getAttribute('href')));
}

async function checkReference(page) {
  await page.goto(`${BASE}/docs/reference`, { waitUntil: 'load' });
  const links = await linksIn(page, 'article');
  const crates = DOCS_RS_CRATES.map((crate) => `https://docs.rs/${crate}/latest/${crate.replaceAll('-', '_')}/`);
  check('the reference links each crate on docs.rs', crates.every((url) => links.includes(url)));
  const packages = REFERENCE_PACKAGES.map(({ slug }) => `/docs/reference/${slug}`);
  check('and each WIT package', packages.every((url) => links.includes(url)));
  for (const { slug, name, interfaces } of REFERENCE_PACKAGES) {
    const urls = interfaces.map((iface) => `/docs/reference/${slug}/${iface.name}`);
    await page.goto(`${BASE}/docs/reference/${slug}`, { waitUntil: 'load' });
    const listed = await linksIn(page, 'article');
    check(`${name}'s page links each of its interfaces`, urls.every((url) => listed.includes(url)));
    const sidebar = await linksIn(page, 'aside');
    check(`the sidebar has ${name}'s interfaces`, urls.every((url) => sidebar.includes(url)));
  }
  await page.goto(`${BASE}/docs/reference/wasdk-platform/windows`, { waitUntil: 'load' });
  const windows = await page.locator('article').innerText();
  check("windows' page shows set-min-size as the WIT has it", windows.includes(SET_MIN_SIZE));
  const windowsLinks = await linksIn(page, 'article');
  check(
    "windows' page links its wasdk::windows module on docs.rs",
    windowsLinks.some((href) => /^https:\/\/docs\.rs\/wasdk\/[^/]+\/wasdk\/windows\/$/.test(href ?? '')),
  );
  await page.goto(`${BASE}/docs/reference/wasdk-platform/worker-entry`, { waitUntil: 'load' });
  const workerEntryLinks = await linksIn(page, 'article');
  check(
    "worker-entry does not link a nonexistent wasdk::worker_entry module",
    !workerEntryLinks.some((href) => (href ?? '').includes('/wasdk/worker_entry/')),
  );
}

/** The icons page shows each built-in app's icon above its name, each a PNG of 512 pixels. */
async function checkIcons(page) {
  await page.goto(`${BASE}/docs/icons`, { waitUntil: 'load' });
  const shown = await page
    .locator('article li')
    .evaluateAll((items) => items.map((item) => [item.textContent.trim(), item.querySelector('img')?.getAttribute('src')]));
  const names = shown.map(([name]) => name);
  check('the icons page shows every built-in app', BUILTIN_APP_NAMES.every((name) => names.includes(name)), names.join(', '));
  for (const [name, src] of shown) {
    const response = await fetch(`${BASE}${src}`);
    const isPng = response.ok && response.headers.get('content-type') === 'image/png';
    const size = isPng ? pngSize(Buffer.from(await response.arrayBuffer())) : [];
    check(`${name}'s icon is a PNG of ${ICON_SIZE} pixels`, `${size}` === `${ICON_SIZE},${ICON_SIZE}`, `${src} ${size}`);
  }
}

/** A PNG's width and height, from its header. */
function pngSize(png) {
  return [png.readUInt32BE(16), png.readUInt32BE(20)];
}

async function checkHome(page) {
  await checkPage(page, '/', 'Apps on demand.\nSafe to run. Built in minutes.');
  check("the home page's document is titled WaOS", (await page.title()) === 'WaOS');
  const bodyText = await page.locator('#nd-home-layout').innerText();
  check('the home page has no middot splits', !bodyText.includes('·'));
  const download = page.getByRole('link', { name: 'Download for macOS' });
  check("the macOS download is the site's newest-release address", (await download.getAttribute('href')) === '/download/macos');
  const androidDownload = page.getByRole('link', { name: 'Android' });
  check("the Android download is the site's newest-release address", (await androidDownload.getAttribute('href')) === '/download/android');
  const linuxDownload = page.getByRole('link', { name: 'Linux' });
  check("the Linux download is the site's newest-release address", (await linuxDownload.getAttribute('href')) === '/download/linux');
  const topics = await page
    .locator('section h3 + ul a')
    .evaluateAll((links) => links.map((link) => [link.getAttribute('href'), link.textContent]));
  const guides = GUIDE_TOPICS.map(([slug, title]) => [`/docs/guide/${slug}`, title]);
  check('every topic links to its guide', JSON.stringify(topics) === JSON.stringify(guides));
  await page.getByRole('button', { name: 'Build your app with an agent' }).click();
  const dialogLinks = await linksIn(page, 'dialog');
  check(
    'the Build dialog lists all four platforms',
    ['/download/macos', '/download/windows', '/download/linux', '/download/android'].every((href) =>
      dialogLinks.includes(href),
    ),
    dialogLinks.join(', '),
  );
  await page.getByRole('button', { name: 'Copy', exact: true }).click();
  const copied = await page.evaluate(() => navigator.clipboard.readText());
  check('the agent prompt points at the playbook', copied.includes(`${ORIGIN}/agents.md`), copied);
  await page.getByRole('button', { name: 'Close' }).click();
  await page.getByRole('button', { name: 'Markdown reader' }).click();
  await page.getByRole('button', { name: 'Copy prompt' }).click();
  const starterCopied = await page.evaluate(() => navigator.clipboard.readText());
  check('starter chip customizes the inline prompt', starterCopied.includes('markdown reader'), starterCopied);
  const commands = await page.locator('pre').first().innerText();
  check('the commands make an app and run it', commands.includes('waos new hello'));
  await page.getByRole('tab', { name: 'Windows' }).click();
  const winCommands = await page.locator('pre').first().innerText();
  check('the Windows tab shows PowerShell install.ps1', winCommands.includes('install.ps1'));
  const docsLinks = await page.locator('section ul a').evaluateAll((links) => links.map((link) => link.getAttribute('href')));
  check('the developers are pointed at Start', docsLinks.includes('/docs/start'));
  check('Concepts remains reachable from the home page', docsLinks.includes('/docs/concepts'));
}

const PLATFORM_CASES = [
  {
    name: 'Windows',
    userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/130.0.0.0 Safari/537.36',
    button: 'Download for Windows',
    href: '/download/windows',
    snippet: 'install.ps1',
  },
  {
    name: 'Linux',
    userAgent: 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 Chrome/130.0.0.0 Safari/537.36',
    button: 'Download for Linux',
    href: '/download/linux',
    snippet: 'install.sh',
  },
  {
    name: 'Android',
    userAgent: 'Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 Chrome/130.0.0.0 Mobile Safari/537.36',
    button: 'Download for Android',
    href: '/download/android',
    snippet: 'install.sh',
  },
  {
    name: 'iPhone',
    userAgent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 Mobile/15E148 Safari/604.1',
    button: 'Download WaOS',
    href: '#downloads',
    snippet: 'install.sh',
  },
];

async function checkPlatforms(browser) {
  const staticContext = await newContext(browser, { javaScriptEnabled: false });
  const staticPage = await staticContext.newPage();
  await staticPage.goto(BASE, { waitUntil: 'load' });
  const staticCta = staticPage.getByRole('link', { name: 'Download WaOS' });
  check('static HTML renders neutral Download WaOS button', (await staticCta.getAttribute('href')) === '#downloads');
  const staticDownloads = await linksIn(staticPage, '#downloads');
  check(
    'static HTML lists all four platforms',
    ['/download/macos', '/download/windows', '/download/linux', '/download/android'].every((href) =>
      staticDownloads.includes(href),
    ),
    staticDownloads.join(', '),
  );
  await staticContext.close();

  for (const item of PLATFORM_CASES) {
    const context = await newContext(browser, {
      userAgent: item.userAgent,
      permissions: ['clipboard-read', 'clipboard-write'],
    });
    const page = await context.newPage();
    await page.goto(BASE, { waitUntil: 'load' });
    const cta = page.getByRole('link', { name: item.button });
    await cta.waitFor({ state: 'visible', timeout: 5000 });
    check(`${item.name} shows "${item.button}" -> ${item.href}`, (await cta.getAttribute('href')) === item.href);
    const snippet = await page.locator('pre').first().innerText();
    check(`${item.name} quickstart defaults to ${item.snippet}`, snippet.includes(item.snippet));
    if (item.name === 'Linux') {
      await page.getByRole('button', { name: 'install.sh' }).click();
      const copied = await page.evaluate(() => navigator.clipboard.readText());
      check('Linux install.sh button copies the curl command', copied === 'curl -fsSL https://waos.app/install.sh | sh', copied);
    }
    await context.close();
  }
}

/** The art's pixels, as an image, `after` milliseconds from now. */
async function artPixels(page, after) {
  await page.waitForTimeout(after);
  return page.locator('canvas').evaluate((canvas) => canvas.toDataURL());
}

async function artColorIn(page, colorScheme) {
  await page.emulateMedia({ colorScheme });
  await page.waitForTimeout(300);
  return page.locator('canvas').evaluate((canvas) => getComputedStyle(canvas).color);
}

/** Where the name sits once the page has loaded, with or without its script. */
async function nameTop(browser, javaScriptEnabled) {
  const context = await newContext(browser, { javaScriptEnabled });
  const page = await context.newPage();
  await page.goto(BASE, { waitUntil: 'load' });
  await page.waitForTimeout(500);
  const top = (await page.locator('h1').boundingBox())?.y;
  await context.close();
  return top;
}

async function checkArt(browser) {
  const context = await newContext(browser, { deviceScaleFactor: 2 });
  const page = await context.newPage();
  await page.goto(BASE, { waitUntil: 'load' });
  check('the art is drawn on one canvas', (await page.locator('canvas').count()) === 1);
  const moving = await artPixels(page, 300);
  check('the art moves', moving !== (await artPixels(page, 600)));
  const pixels = await page.locator('canvas').evaluate((canvas) => [canvas.width, canvas.clientWidth]);
  check("the art is drawn at the screen's pixels", pixels[0] === 2 * pixels[1], `${pixels}`);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  const still = await artPixels(page, 300);
  check('the art is still with reduced motion', still === (await artPixels(page, 600)));
  check('the art follows light and dark', (await artColorIn(page, 'light')) !== (await artColorIn(page, 'dark')));
  await context.close();

  const withoutScript = await nameTop(browser, false);
  check('the art never moves the text', withoutScript === (await nameTop(browser, true)), `${withoutScript}`);
}

const server = SITE_URL ? undefined : startServer();
let browser;
try {
  await waitForServer();
  await checkMarkdownRoutes();
  await checkReferenceMarkdown();
  await checkSiteRoutes();

  browser = await chromium.launch({ headless: true });
  const context = await newContext(browser, {
    userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 Chrome/130.0.0.0 Safari/537.36',
    permissions: ['clipboard-read', 'clipboard-write'],
  });
  const page = await context.newPage();
  const consoleErrors = [];
  page.on('console', (message) => message.type() === 'error' && consoleErrors.push(message.text()));
  page.on('pageerror', (error) => consoleErrors.push(`pageerror: ${error.message}`));

  await checkHome(page);
  await checkPlatforms(browser);
  await checkDocsPages(page);
  await checkReference(page);
  await checkIcons(page);
  await checkArt(browser);

  const noisy = consoleErrors.filter((text) => !/favicon|Failed to load resource/.test(text));
  check('no console errors', noisy.length === 0, noisy.slice(0, 3).join(' | '));
} finally {
  await browser?.close();
  server?.kill();
}

console.log(failures === 0 ? '\nsite smoke: all checks passed' : `\nsite smoke: ${failures} failed`);
process.exit(failures === 0 ? 0 : 1);
