# Files

- **An app sees only its own files and what its user gave it.** Its `/` holds its own `/data`, `/cache` and `/tmp`, the folders its user granted, and the files they picked. A path outside them isn't found, so an app can't probe what the machine holds. ([files](/crates/waos/src/files/mod.rs))
- **A picked file stays reachable until its user takes it back.** A file or folder the user picks for an app, or opens it with, is mounted at its own path and kept in the app's settings, where they can remove it. It's mounted before the app learns its path, so the app never holds a path it can't reach. ([picks.rs](/crates/waos/src/files/picks.rs))
- **A picked file is given to the app only if WaOS can read it.** If the system keeps WaOS from it, WaOS sends the user to allow it; if they don't, the app is told nothing was picked. ([host.rs](/crates/waos/src/file_dialogs/host.rs))
- **Granting or taking back a folder works at once,** without restarting the app; a file already open stays open. ([files](/crates/waos/src/files/mod.rs))
  - *Why:* plain WASI fixes an app's folders when it starts. WaOS looks up every path in the app's current mounts, so a change in its settings holds at its next call.
- **`/tmp` is emptied when the app ends,** however it ended. After a crash of WaOS, its next start removes what was left. ([temporary_folders.rs](/crates/waos/src/temporary_folders.rs))
- **An app can put a folder it was given at a path of its choosing.** An editor given a project's folder puts it at `/`, so code using plain paths (`/src/main.rs`, a compiler, a language server) finds the files where it expects. It reaches nothing new, only a folder it already holds; its own `/data`, `/cache` and `/tmp` stay on top. ([mount_host.rs](/crates/waos/src/files/mount_host.rs))
- **An app can watch any file or folder it holds,** through the system's own watcher, for as long as it listens. ([file_watching](/crates/waos/src/file_watching/mod.rs))
  - *Today:* only folders can be watched, so an app watching a single file, such as `settings.json`, is refused. ([TODO](/TODO.md))
