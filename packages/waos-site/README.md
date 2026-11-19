# waos-site

WaOS's website, `waos.app`, as `docs/proposals/2026-11-06-website.md` decides it: a Fumadocs site, built as `valo-site` is in valo. Its home page says what WaOS is, with the art above the name; follows one `.wapk` from prompt to desktop and phone; summarizes why WaOS apps are built this way in four bento blocks; and shows how to build one with an agent, with the sample; its docs are arranged by what an app does: the start, the concepts, building the app (a page per thing a program has, under headings: windows and input, drawing, files and data, network, media, the system), Waft, the tools, the reference, compatibility and the icons of WaOS's own apps. Each release carries the same pages as one archive, which `cargo waos docs` downloads: `npm run docs-bundle` writes it. An agent reads it as Markdown: `llms.txt`, `llms-full.txt`, every docs page at its URL with `.md` or asked for as `text/markdown`, and `agents.md`, the agent's playbook.

- **Takes:** Next.js and Fumadocs (`fumadocs-core`, `fumadocs-mdx`, `fumadocs-ui`) 16, with Tailwind, from npm; the pages in `content/docs` and the playbook in `content/agents.md`; `examples/hello`'s `src/main.rs`, read at build time as the home page's sample; the WIT packages `wasdk:platform` and `wasdk:media` in `wit/deps`, read at build time into the reference's pages; the built-in apps' icons, each as its `Cargo.toml` names it under `builtin-apps`, made at build time into the icons page's pictures, with `sharp`; Waft's Cupertino gallery, embedded live from where Waft deploys it (`cupertinoGalleryUrl` in `lib/shared.ts`).
- **Provides:** the site: `/`; `/docs/start`, `/docs/concepts`, `/docs/guide`, `/docs/guide/<topic>`, `/docs/waft`, `/docs/waft/<topic>`, `/docs/tools`, `/docs/compatibility`, `/docs/icons`, `/docs/reference`, `/docs/reference/<package>` and `/docs/reference/<package>/<interface>` (`wasdk-platform/windows` for `wasdk:platform/windows`), with `/docs` opening on the concepts, and `/docs/frameworks` and `/docs/frameworks/waft`, Waft's addresses before, leading to `/docs/waft`; each built-in app's icon at `/apps/<name>.png` (`/apps/video-player.png` for Video Player), 512 pixels square; `/docs/<page>.md`, `/llms.txt`, `/llms-full.txt` and `/agents.md`; `/sitemap.xml`, `/robots.txt` and Fumadocs' search at `/api/search`; and, from `npm run docs-bundle`, the docs' archive a release carries.
- **Used by:** whoever reads about WaOS, and the agents they give it to; the release workflow, for the docs' archive, which `cargo waos docs` reads.

```sh
npm install
npm run dev        # at http://localhost:3000
npm run build      # the production build, in .next
npm start          # serves that build
npm run check      # the type check
npm test           # the smoke test, against the production build: every page and route, headless
npm run reference  # writes the reference's WIT pages again; dev and build run it first
npm run icons      # writes the built-in apps' icons into public/apps again; dev and build run it first
npm run docs-bundle [-- <out>]   # the docs of this version as one archive, dist/waos-docs-<version>.tar.gz, as a release attaches it
```

The reference's page for each interface is made from the WIT each time the site is built, so it never drifts from it: `scripts/write-reference.mjs` reads each package with `scripts/read-wit.mjs`, a reader of the WIT's structure of our own, and writes a Markdown page per interface, its doc comment, a verified `docs.rs/wasdk/<version>/wasdk/<module>/` link checked against `sdk/wasdk/src/lib.rs`, and then each item's doc comment and WIT as written, and one per package listing its interfaces, into `content/docs/reference/<package>/`, which is not committed. From there Fumadocs takes them as it takes the written pages, into the sidebar, the search, `llms-full.txt` and each page's Markdown. The reader stops on WIT it does not know rather than leave it out; `wasm-tools component wit` is what checks the WIT.

The built-in apps' icons are made the same way: `scripts/write-app-icons.mjs` takes each app's icon from `builtin-apps`, as its `[package.metadata.waos]` names it, and writes it at 512 pixels into `public/apps/`, which is not committed and which Next serves as it is. The icons page, `content/docs/icons.mdx`, which the playbook's Icon step links, lists them by name; the smoke test checks it shows every built-in app.

The docs of a version are one archive too, `waos-docs-<version>.tar.gz`, which the release workflow attaches to the version's GitHub release and `cargo waos docs` downloads: `scripts/write-docs-bundle.mjs` writes the reference's pages as the build does, then every page as plain Markdown under `docs/` (`docs/guide/network.md`), its title as its heading and its description below, the playbook as `agents.md`, and `manifest.json`, the version, the workspace's, and the pages in the sidebar's order with their titles and descriptions, and tars them.

The site's origin is `https://waos.app`, or `NEXT_PUBLIC_SITE_URL` when that is set; the metadata, the sitemap, `llms-full.txt`, each page's Markdown and the agent's prompt name it. The download links point at the macOS Apple Silicon `.dmg` and Android `.apk` assets of the GitHub release (`macDownloadUrl` and `androidDownloadUrl` in `lib/shared.ts`); the GitHub link names `github.com/waos-dev/waos`.

The smoke test uses Playwright's Chromium, which `npx playwright install chromium` fetches where it is missing. It keeps the gallery out, so it runs without the network.

`WAOS_SITE_URL=<origin> npm test` runs the smoke test against a site already running there, such as a deployment, instead of starting one.

## On Vercel

The site deploys to Vercel from the repository's root, with the Vercel project's root directory set to `packages/waos-site`, so its build reads `Cargo.toml`, `sdk/wasdk/src/lib.rs`, `wit/`, `examples/hello` and the built-in apps' icons where they are; `.vercelignore` at the root uploads only those and the site. Connecting the Vercel project to `waos-dev/waos` (`vercel git connect`) auto-deploys production on pushes to `main` and previews on pull requests. Vercel builds it on its side, so a deploy uploads the sources alone. Once, from the repository's root:

```sh
vercel login
vercel link                 # a new project, waos; its code in packages/waos-site
vercel deploy --prod
```

Then add `waos.app` to the project (Settings → Domains), pointing the domain's DNS where Vercel says.
