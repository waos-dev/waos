# The official source

Written 2026-11-22, once Zed's port ran on a phone and there was no place to get it, nor Inspector or any of WaOS's own apps, but `waos run` from their source. Designed with the lead one question at a time. What was planned on this date, not what is.

## The lead's decisions

1. **The official source is a catalog of apps released by their own repositories** (2026-11-22). Each app's repository packages and releases it with `waos package`, signed with its publisher's key, as any GitHub source: WaOS's own repository its built-in apps, a port's fork the port. The catalog, `waos-dev/apps`, builds nothing: it lists which apps from which sources, and collects their latest packages. Why: apps release on their own schedules, each signed by its real publisher, so the catalog never holds another publisher's key; and the catalog is the reference for anyone running a source of their own: publish your app as a source, and a catalog can list it. The lead weighed a builder, which clones each app at a pinned revision and builds and signs it in one place, releasing every app together and signing apps it does not own; and no catalog, a few sources preset in WaOS, with no one place to look.

2. **The catalog is served from R2, at `list.waos.app`** (2026-11-22): its `index.json`, and each package it lists, copied there once at `<id>/<version>/<file>` and never changed. Why: GitHub serves release files slowly in places and limits how often it is asked, R2 quickly and with no charge for the download, as `download.waos.app` serves WaOS's own releases (`mirror-release.yml`). The GitHub repository holds the catalog's list and its workflow alone. The lead weighed the index as a GitHub release pointing at packages on R2, which keeps GitHub on every refresh of the library, and both, two copies to keep in step.

3. **The catalog pins each app's publisher key** (2026-11-22): `catalog.toml` names it, and a package signed with another key is not collected. Why: WaOS takes the key it first sees for a program, so a source compromised upstream would otherwise reach every new installation through the catalog; a catalog's worth is that someone checked what is in it, and the key is what is worth checking. A key changes only through a reviewed change to `catalog.toml`. The lead weighed collecting what each source lists, its SHA-256 checking only the download.

4. **A publisher has one key** (2026-11-22), as decision 43 of `2026-10-17-the-waos-project.md` has it: the WaOS project is a publisher, its key the secret `WAOS_SIGNING_KEY` of `waos-dev/waos`. The lead weighed a key per app, each derived from one secret, which would let one app be handed to another publisher without a reinstall, at the cost of a key format of the source's own.

5. **Updates stay bound to the key** (2026-11-22). A publisher's website may later vouch for its key (`/.well-known/waos-publisher.json`), shown as "From waos.app" where WaOS is online, as Android's Digital Asset Links tie an app to a website; it labels, and never decides what installs or updates, so a website taken over cannot reach a device. Not built now. The lead weighed the website also vouching for a new key, so a key could change without a reinstall, which makes the website a way onto every device; and the old key signing the new one, as Android's v3 signatures do, which nobody needs yet.

6. **WaOS tells it needs a newer version as it installs a package** (2026-11-22): from the interfaces the package's component imports, which name their versions, against what this WaOS provides, with which one is missing. Nothing is added to the index until a catalog needs to hide what a WaOS cannot run. The lead weighed the index listing each package's imports, which lets the library mark a program before it is downloaded, at the cost of a field to keep.

## Choices made along the way

- The index lists each app's latest version; an older package stays on R2, listed no more.
- WaOS comes with `list.waos.app` among its sources, removable as any other.
- The bucket is `waos-list` in the Cloudflare account of `waos-releases`.

## The work, in order

1. This proposal.
2. `waos package verify <file> --publisher <key>`, which verifies a package and that its publisher is the key named; and the store refusing a package whose component imports an interface this WaOS lacks, naming it.
3. WaOS's release packaging its built-in apps with `waos package`, signed with `WAOS_SIGNING_KEY`, their `index.json` and packages attached to the release, so `github.com/waos-dev/waos` is a source of them.
4. `waos-dev/apps`: `catalog.toml`, the step that collects, verifies and copies, run hourly, on a push and by hand, and a README that is the guide to running a source.
5. `list.waos.app` among WaOS's sources.

## Not in this proposal

- "From waos.app" (decision 5).
- Zed's port in the catalog: it needs its port committed where a workflow can build it, a branch of its fork.
- The library's look, settings and getting programs, designed on their own.
