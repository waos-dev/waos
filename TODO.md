# Toward WaOS 1.0

What stands between today and a first stable release, in the order it blocks one. Decisions are the lead's; the design behind each lives in `docs/proposals/`.

## Getting it to people
- [ ] A signed, notarized WaOS.app for macOS, built by CI on a tag (needs the Apple Developer account and signing identity).
- [ ] waos.app's Download points at the release (`packages/waos-site/lib/shared.ts`, `downloadUrl`).
- [ ] WaOS updating itself (for example Sparkle on macOS): decide whether 1.0 has it.
- [ ] `waos docs` serving a version's docs from its release: the release workflow writes them on a tag, and no release carries one yet.
- [x] The SDK, `waos` and the engine published from a tag by CI (decision 67).
- [x] The `wasdk:*` interface at 1.0.0, and CI refusing a changed package under the same version.

## The interface
- [x] Views only on request, held screens, and a program declaring whether it is an app or a tool (`docs/proposals/2026-11-21-views-a-program-asks-for.md`).
- [ ] Programs with no window: Waft's terminal widget, so a tool opened from an icon runs in a terminal of WaOS's own rather than being refused (decision 5 there, shown to the lead first); then desktop widgets, and work that runs on a schedule or when a file changes.
- [ ] Watching a single file: `file-watching.watch` takes folders, so a program watching a file (Zed's `settings.json`, `~/.gitconfig`) is refused; watch the file's folder for it, or say in the interface that a program does so.
- [ ] Fonts read whole into each program (78 MB for PingFang SC, the Zed port): let a program map a font file rather than copy it. Changes the WIT; shown to the lead first.
- [ ] Threads that share memory, for parallel CPU work (Zed's background executor: parsing, wrapping, search over shared snapshots). Waits on the component model's shared-everything threads in wasmtime; workers cover message-shaped work meanwhile.
- [ ] Accessibility: a program's semantics tree, which changes the views interface. Later.

## Platforms
- [x] macOS: the engine, the shell, programs as apps of their own.
- [ ] Android: a developer build that installs packages end to end; each program a task of its own.
- [x] Android: This computer reaches the phone's files through All files access, asked as it is picked and said in WaOS's settings (`docs/proposals/2026-11-21-projects-on-machines.md`).
- [ ] Android: a folder a program picks through the system's picker, which answers a document tree with no path, which WaOS cannot mount (a program on a machine opens its folders from the machine instead): a pick in the phone's own storage turned into its path while WaOS holds All files access, a cloud folder failing clearly. Designed with the lead on its own.
- [ ] Phones: the safe area over a program's view, as GPUI reads it (`wasdk-adapter-gpui` giving the view's padding and keyboard as GPUI's window insets, and Zed's port keeping its workspace clear of them).
- [ ] Phones: touch in `wasdk-adapter-gpui`, a finger given to GPUI as a touch, which it recognizes into taps and scrolls, not as a mouse, which drags a selection.
- [ ] iOS: developer builds (the App Store refuses downloaded code).
- [ ] Windows, then Linux (`docs/proposals/2026-11-13-windows-and-linux.md`).
- [ ] Remote machines over SSH (work order step 12).
- [ ] Termux on an Android phone as an SSH machine at `localhost` (Termux's `sshd`), so a phone has git, compilers and language servers: an idea, after SSH machines (`docs/proposals/2026-11-21-projects-on-machines.md`).
- [ ] Putting tools on a machine: the language servers and debug adapters an editor downloads run on its machine, which cannot run them from the program's own data, nor a phone at all (`docs/proposals/2026-11-21-projects-on-machines.md`).

## A developer, or an agent, succeeding alone
The trials' ranked list: `../experiments/2026-11-17-app-trials/RANKED.md` (the maintainers' experiments folder).
- [ ] Waft's Android settings (`[package.metadata.waft.android]`: `permissions`, `dependencies`, the icon) are said only in `waft-cli`'s README, deep in long paragraphs, and were found faster in its source: a page of their own on Waft's site, or a short section at the top of that README.
- [x] `wasdk-adapter-waft`'s HTTP client on Waft's `Body` trailers and `try_clone`, once Waft has them (waos-dev/waft, in progress).
- [ ] A setting answers a question a program is waiting on: the engine re-checks waiting questions when a setting changes, and the program's dialog closes as Waft's dropped dialog future already does (waos-dev/waft#101).
- [ ] A stack for a hung program (`waos inspect`).
- [ ] `block_on` waiting on tasks it did not start (wit-bindgen's shared queue).
- [ ] Screenshots at a size larger than the screen.
- [ ] What the audit of 2026-11-21 found buried, after the two bugs it found were fixed: starting a run is built three ways that have drifted (development, installed and inspector-held runs); file dialogs and questions are split into methods by kind that every implementation folds back together; "Simulated" is allowed for every kind of answer, though only cameras and microphones have one, with a likely crash for a link scheme's; three counters keep a program thawed, all doing the same; a waiting question has a channel of its own only to say nobody waits any more; the engine's and the app's READMEs are each over 100 KB in single paragraphs, which buries their rules.

## Solid
- [x] CI green in Waft, WaOS and Valo; each releases from a tag.
- [ ] Every built-in app on a release build, end to end.
- [ ] The engine's audit days are UTC days, so the Inspector's report counts an event just after local midnight in a zone east of UTC in the previous day's column: make the buckets the user's.
- [ ] The trial apps rebuilt from crates.io's release alone, with no preview config, as the last outside check.
