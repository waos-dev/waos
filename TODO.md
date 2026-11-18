# Toward WaOS 1.0

What stands between today and a first stable release, in the order it blocks one. Decisions are the lead's; the design behind each lives in `docs/proposals/`.

## Getting it to people
- [ ] A signed, notarized WaOS.app for macOS, built by CI on a tag (needs the Apple Developer account and signing identity).
- [ ] waos.app's Download points at the release (`packages/waos-site/lib/shared.ts`, `downloadUrl`).
- [ ] WaOS updating itself (for example Sparkle on macOS): decide whether 1.0 has it.
- [ ] `cargo waos docs` serving a version's docs from its release: the release workflow writes them on a tag, and no release carries one yet.
- [x] The SDK, `cargo waos` and the engine published from a tag by CI (decision 67).
- [x] The `wasdk:*` interface at 1.0.0, and CI refusing a changed package under the same version.

## The interface
- [ ] Programs with no window: command-line programs, desktop widgets, work that runs on a schedule or when a file changes.
- [ ] Fonts read whole into each program (78 MB for PingFang SC, the Zed port): let a program map a font file rather than copy it. Changes the WIT; shown to the lead first.
- [ ] Threads that share memory, for parallel CPU work (Zed's background executor: parsing, wrapping, search over shared snapshots). Waits on the component model's shared-everything threads in wasmtime; workers cover message-shaped work meanwhile.
- [ ] Accessibility: a program's semantics tree, which changes the views interface. Later.

## Platforms
- [x] macOS: the engine, the shell, programs as apps of their own.
- [ ] Android: a developer build that installs packages end to end; each program a task of its own.
- [ ] iOS: developer builds (the App Store refuses downloaded code).
- [ ] Windows, then Linux (`docs/proposals/2026-11-13-windows-and-linux.md`).
- [ ] Remote machines over SSH (work order step 12).

## A developer, or an agent, succeeding alone
The trials' ranked list: `../experiments/2026-11-17-app-trials/RANKED.md` (the maintainers' experiments folder).
- [ ] Pixiv moved onto Waft's HTTP and files, as the real-app check.
- [ ] `wasdk-adapter-waft`'s HTTP client on Waft's `Body` trailers and `try_clone`, once Waft has them (waos-dev/waft, in progress).
- [ ] A setting answers a question a program is waiting on: the engine re-checks waiting questions when a setting changes, and the program's dialog closes as Waft's dropped dialog future already does (waos-dev/waft#101).
- [ ] A stack for a hung program (`cargo waos inspect`).
- [ ] `block_on` waiting on tasks it did not start (wit-bindgen's shared queue).
- [ ] Screenshots at a size larger than the screen.

## Solid
- [x] CI green in Waft, WaOS and Valo; each releases from a tag.
- [ ] Every built-in app on a release build, end to end.
- [ ] The engine's audit days are UTC days, so the Inspector's report counts an event just after local midnight in a zone east of UTC in the previous day's column: make the buckets the user's.
- [ ] The trial apps rebuilt from crates.io's release alone, with no preview config, as the last outside check.
