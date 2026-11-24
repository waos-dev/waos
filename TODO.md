# Toward WaOS 1.0

What stands between today and a first stable release, in the order it blocks one.

## Getting it to people
- [ ] A signed, notarized WaOS.app for macOS, built by CI on a tag. Needs the Apple Developer account.
- [ ] waos.app's Download points at the release (`packages/waos-site/lib/shared.ts`, `downloadUrl`).
- [ ] WaOS updating itself (Sparkle on macOS, for example): decide whether 1.0 has it.
- [ ] `waos docs` serving a version's docs from its release: the release workflow writes them, and no release carries them yet.

## The interface
- [ ] A tool opened from its icon runs in a terminal of WaOS's own instead of being refused (Waft's terminal widget); then desktop widgets, and work that runs on a schedule or when a file changes.
- [ ] Watching a single file: today only folders can be watched, so an app watching `settings.json` is refused.
- [ ] Fonts are copied whole into each app (78 MB for PingFang SC): let an app map a font file instead. Changes the WIT.
- [ ] Threads that share memory, for parallel CPU work; waits on wasmtime. Workers cover message-shaped work meanwhile.
- [ ] Limits the user sets on an app's CPU and disk.
- [ ] "Allow once" as an answer to a question.
- [ ] Opening a link or a file in another app asks the user, as a link of another scheme does today.
- [ ] Rework how the user controls an app's network: today's switches, `localhost` and per-connection checks are naive.
- [ ] A per-app setting for running in the background (music, a download), replacing today's rule that sound, recording or a camera keeps an app running, which an app playing silence abuses.
- [ ] Accessibility: an app's semantics tree. Changes the views interface.

## Platforms
- [ ] Android: a developer build that installs packages end to end; each app a task of its own.
- [ ] Android: read a pick through Android's own handle, so a pick needs no All files access and a cloud folder works. Today a pick in the phone's own storage is mounted by its path, which needs All files access, and any other pick fails.
- [ ] iOS: developer builds (the App Store refuses downloaded code).
- [ ] Windows, then Linux.
- [ ] Machines over SSH, then other kinds; Termux on an Android phone as an SSH machine at `localhost`, for git, compilers and language servers on a phone.
- [ ] Tools an editor downloads (language servers, debug adapters) installed on the app's machine, which can't run them from the app's own data.

## Building apps alone
- [ ] Waft's Android settings (`[package.metadata.waft.android]`) are buried in `waft-cli`'s README: give them a page or a short section at its top.
- [ ] A stack for a hung app (`waos inspect`).
- [ ] `block_on` waiting on tasks it did not start (wit-bindgen's shared queue).
- [ ] Screenshots at a size larger than the screen.
- [ ] A 10-bit decoded frame has no planes an app can read: its samples are 16 bits, a texture format WebGPU keeps behind a feature the interface does not pass on yet.
- [ ] Simplify: starting a run is built three ways that have drifted (development, installed, inspector-held); file dialogs and questions are split by kind and folded back together everywhere; "Simulated" is offered for every kind of answer though only cameras and microphones have one; three counters keep an app thawed, all doing the same; a waiting question has a channel of its own only to say nobody waits any more.

## Solid
- [ ] Every built-in app on a release build, end to end.
- [ ] The Inspector's report counts by UTC day, so an event just after local midnight east of UTC lands in the previous day: count by the user's days.
- [ ] The trial apps rebuilt from crates.io alone, as the last outside check.
- [ ] WaOS's freezing tests hang now and then on Linux CI.
