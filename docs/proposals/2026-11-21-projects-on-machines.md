# Projects on machines

Written 2026-11-21, after Zed's port ran on an Android phone: it asked which machine to run its commands on, the phone, then asked the system's folder picker for its project, which on Android answers a folder handle WaOS cannot mount, and which in any case put the project in Zed's sandbox while its terminal, git and language servers ran on the machine. Designed with the lead one question at a time. What was planned on this date, not what is.

## The lead's decisions

1. **An editor's project lives on the machine the user picked for it**, not in the program's sandbox (2026-11-21). Its files, its terminal, git and its language servers are then on one machine, with the same paths, as Zed's SSH projects and VS Code's remote work. A machine already gives its files from its root (`machines.wit`, `machine.root`), which nothing used. The lead weighed keeping the system's folder picker, which mounts the folder into the sandbox, and both together: two worlds of files, matching only by chance on this computer and never on a remote machine.

2. **Zed reaches the machine's files through its own file layer**, a `MachineFs` over the machine's root, Zed's `Fs` trait being where a project's files are already reached from, for a local project and an SSH one alike (2026-11-21). WaOS's rule stays: a program's own `/` is its mounts, and a machine is a handle the user gave it apart from them, a program holding as many as it was given. The lead weighed WaOS showing the machine's files inside the program's own file system, which every program would use with no port work, but which hides the machine's `/data`, `/cache` and `/tmp` under the program's own, and makes no sense for a program holding several machines. Zed opens a folder with its own folder prompt over `MachineFs`, as it does on an SSH machine. The few places in Zed's project code that read a project's files around `Fs` (a `Cargo.toml` beside a file in `git_store`, the project's `direnv` in `environment`) go through it.

3. **Watching a machine's folder needs no new interface** (2026-11-21): `file-watching.watch` takes a folder opened from a machine's root, this computer's watched by the system as any folder is. A machine that cannot watch answers `unsupported`, and a watch that lost changes says `rescan`, so the program reads the folder again; WaOS may later watch on an SSH machine's side, as VS Code's server does, with the interface as it is.

4. **On Android, "This computer" reaches the phone's storage by WaOS holding All files access** (`MANAGE_EXTERNAL_STORAGE`, 2026-11-21), so its root shows `/storage/emulated/0` at its real paths, which Zed's folder prompt opens as any folder. Without it, Android lets an app reach by path only its own folders and the files it made itself, and a folder the user picks only through a handle. The Play Store takes the permission only for some kinds of app, which is no matter while WaOS is installed directly. The lead weighed "This computer" reaching only WaOS's own folders, cut off from the phone's other files and with no git to bring a project in; and leaving the phone to Termux over SSH, which waits for SSH machines (see `TODO.md`).

5. **WaOS asks for All files access when the user picks "This computer" for a program on Android** (2026-11-21), once, in a sheet that opens Android's switch for it and reads it again on the way back; WaOS's settings say whether it is on and open the switch. The lead weighed asking at a program's first read of a phone's file, which cannot wait for the user in Android's settings, and a switch in WaOS's settings alone, which a user would not know to look for.

## Choices made along the way

- On Android, "This computer"'s home, where Zed's folder prompt starts and where a command with no folder of its own runs, is `/storage/emulated/0` once WaOS holds All files access, and WaOS's own folder before.
- The permission is declared in WaOS's `[package.metadata.waft.android]` beside `INTERNET`, and checked and asked for through JNI in WaOS's own Android code (`crates/waos/src/os/android`, which reaches Android so already), `Environment.isExternalStorageManager` and `Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION`: two calls, which can move into Waft's Android host once another app needs them.
- The language servers and debug adapters Zed downloads go to its own data and run on the machine, which a remote machine cannot run from there, nor a phone at all (Android runs no program an app downloaded). Until tools can be put on a machine (`TODO.md`), they come from what the machine has.

## The work, in order

1. `MachineFs` in Zed's port, its folder prompt and its startup on it, and the two reads of a project's files around `Fs` through it; checked on a Mac, where "This computer" is the Mac.
2. WaOS: the permission declared, asked for in the machine picker on Android, said in WaOS's settings, and "This computer"'s home on Android.
3. On the phone: "This computer" picked, the switch allowed, a folder in the phone's storage opened, edited and saved.

## Not in this proposal

- A program that opens a folder through the system's folder picker, not a machine, still cannot on Android, where the pick is a handle; designed on its own if an app needs it.
- SSH machines, Termux among them.
- The phone's safe area over a program's view.
