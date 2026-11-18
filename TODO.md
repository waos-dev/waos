# Toward WaOS 1.0

What stands between today and a first stable release, in the order it blocks one. Decisions are the lead's; the design behind each lives in `docs/proposals/`.

## Getting it to people
- [ ] A signed, notarized WaOS.app for macOS, built by CI on a tag (needs the Apple Developer account and signing identity).
- [ ] waos.app's Download points at the release (`packages/waos-site/lib/shared.ts`, `downloadUrl`).
- [ ] WaOS updating itself (for example Sparkle on macOS): decide whether 1.0 has it.
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
- [ ] HTTP and files in Waft (waos-dev/waft#99), then the WaOS adapter for them, then pixiv moved onto them as the real-app check.
- [ ] A setting answers a question a program is waiting on, and the dialog closes (Waft first, then the engine).
- [ ] The docs as one hierarchy of information, from what WaOS is to each interface's reference, bundled with `cargo waos` and installable as an agent skill.
- [ ] `Timer.periodic` and `Future.delayed` in Waft, so the template's clock is a few lines.
- [ ] A stack for a hung program (`cargo waos inspect`).
- [ ] `block_on` waiting on tasks it did not start (wit-bindgen's shared queue).
- [ ] Screenshots at a size larger than the screen.

## Solid
- [x] CI green in Waft, WaOS and Valo; each releases from a tag.
- [ ] Every built-in app on a release build, end to end.
- [ ] The trial apps rebuilt from crates.io's release alone, with no preview config, as the last outside check.
