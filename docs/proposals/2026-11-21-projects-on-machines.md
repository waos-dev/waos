# Projects on machines

Written 2026-11-21, after Zed's port ran on an Android phone: it asked which machine to run its commands on, the phone, then asked the system's folder picker for its project, which on Android answers a folder handle WaOS cannot mount, and which in any case put the project in Zed's sandbox while its terminal, git and language servers ran on the machine. Designed with the lead one question at a time. What was planned on this date, not what is.

## The lead's decisions

1. **An editor's project lives on the machine the user picked for it**, not in the program's sandbox (2026-11-21). Its files, its terminal, git and its language servers are then on one machine, with the same paths, as Zed's SSH projects and VS Code's remote work. A machine already gives its files from its root (`machines.wit`, `machine.root`), which nothing used. The lead weighed keeping the system's folder picker, which mounts the folder into the sandbox, and both together: two worlds of files, matching only by chance on this computer and never on a remote machine.

2. **A program mounts a folder it holds at a path of its own files, and Zed mounts its machine's root at `/`** (2026-11-21, replacing the same day's first answer, a `MachineFs` inside Zed). `wasdk:platform/mounts` shows a folder the program holds, a machine's root or a folder opened from it, or one the user picked, at a path it names, for as long as it keeps the `mounted`; what reaches files by their paths reaches it there, `std::fs` and every library on the system's file calls. The program's own `/data`, `/cache` and `/tmp` stay over what it mounts, and nothing is mounted inside them. Zed, holding "This computer", mounts its root at `/`, so its file layer, the reads of `.git` its git code makes itself, and every other path read its files at the paths the machine's commands name them by. Why: a program ported from a system whose files it reads by their paths keeps doing so, rather than being made to take up the machine's model, which here would have meant a file layer in Zed and its git code changed besides; and a program holding several machines mounts each where it chooses. What it gives up: a project under the machine's own `/data`, `/cache` or `/tmp`, which no system it runs on keeps the user's projects in. The lead weighed WaOS showing the machine in every program's files unasked, which makes no sense for a program holding several machines; Zed's own file layer over the machine's root, routing its own folders to its own files; and that layer for projects alone beside its own for its settings.

3. **Watching a machine's folder needs no new interface** (2026-11-21): `file-watching.watch` takes a folder opened from a machine's root, this computer's watched by the system as any folder is. A machine that cannot watch answers `unsupported`, and a watch that lost changes says `rescan`, so the program reads the folder again; WaOS may later watch on an SSH machine's side, as VS Code's server does, with the interface as it is.

4. **On Android, "This computer" reaches the phone's storage by WaOS holding All files access** (`MANAGE_EXTERNAL_STORAGE`, 2026-11-21), so its root shows `/storage/emulated/0` at its real paths, which Zed's folder prompt opens as any folder. Without it, Android lets an app reach by path only its own folders and the files it made itself, and a folder the user picks only through a handle. The Play Store takes the permission only for some kinds of app, which is no matter while WaOS is installed directly. The lead weighed "This computer" reaching only WaOS's own folders, cut off from the phone's other files and with no git to bring a project in; and leaving the phone to Termux over SSH, which waits for SSH machines (see `TODO.md`).

5. **WaOS asks for All files access when the user picks "This computer" for a program on Android** (2026-11-21), once, in a sheet that opens Android's switch for it and reads it again on the way back; WaOS's settings say whether it is on and open the switch. The lead weighed asking at a program's first read of a phone's file, which cannot wait for the user in Android's settings, and a switch in WaOS's settings alone, which a user would not know to look for.

## Choices made along the way

- On Android, "This computer"'s home, where Zed's folder prompt starts and where a command with no folder of its own runs, is `/storage/emulated/0` once WaOS holds All files access, and WaOS's own folder before.
- The permission is declared in WaOS's `[package.metadata.waft.android]` beside `INTERNET`, and checked and asked for through JNI in WaOS's own Android code (`apps/waos/src/os/android/all_files_access.rs`, beside the folders and certificates it reaches Android for already), `Environment.isExternalStorageManager` and `Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION`: two calls, which can move into Waft's Android host once another app needs them.
- Found while building it (2026-11-22): This computer's home is given by WaOS as each command starts (`waos_local_processes::HomeFolder`), not read once from the login environment, so a command started after the user turned the access on runs in the phone's storage without WaOS starting again. A pick of This computer is answered once the user goes on without the access ("Not now") or comes back to WaOS from Android's page, whatever they chose there; the sheet is shown again at the next program's pick while the access is off, since each program's pick is kept among its grants and asked once.
- Found while building it (2026-11-22): in Zed's port, `~` is the machine's home, which Zed reads once it connects (`util::paths::set_home_dir`, and `$HOME`, which the port's patched `dirs` reads on WASI), while Zed's own config, data and caches stay in its `/data` (`util::paths::own_dir`).
- The language servers and debug adapters Zed downloads go to its own data and run on the machine, which a remote machine cannot run from there, nor a phone at all (Android runs no program an app downloaded). Until tools can be put on a machine (`TODO.md`), they come from what the machine has.

## The work, in order

1. `wasdk:platform/mounts` in WaOS; then Zed's port mounting its machine's root at `/`, with Zed's own folder prompt in place of the system's; checked on a Mac, where "This computer" is the Mac.
2. WaOS: the permission declared, asked for in the machine picker on Android, said in WaOS's settings, and "This computer"'s home on Android.
3. On the phone: "This computer" picked, the switch allowed, a folder in the phone's storage opened, edited and saved.

## Not in this proposal

- A program that opens a folder through the system's folder picker, not a machine, still cannot on Android, where the pick is a handle; designed on its own if an app needs it.
- SSH machines, Termux among them.
- The phone's safe area over a program's view.
