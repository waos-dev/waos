# Proposals

Designs written for the project lead before something is built, each with the lead's decisions on top. They describe what was planned on their date, not what is: the crates' READMEs and `DESIGN.md`, once something is built, describe what is.

## This project's

- [The waos project](2026-10-17-the-waos-project.md): an embeddable engine, capabilities per program, a shell of small traits, the engine making pictures and the shell showing them, views given or requested, wasdk as generated bindings; the crates and the work order.
- [WaOS's first version](2026-10-16-waos-v1.md), carried over from the experiment: five platforms, package sources and signed packages, programs as apps of their own, the capabilities it adds, custom IO behind WIT-shaped traits.
- [What WaOS's first version gives a program](2026-10-17-waos-v1-interface.md), carried over from the experiment: permissions asked at first use with readable state; networking through WASI over providers; chosen files under `/granted`; Web Audio's graph; surfaces placed with the frame; menus, notifications, opening; processes in the usual model; workers. Rough sketches, each designed with the lead before it is written.

- [Inset: a frame per view](2026-10-20-inset-frames-per-view.md): each view's frame at its own screen's refresh, a frame request naming one view or every view, tickers following their view, build and layout shared, paint per view and present only what painted (Flutter's #192128), hidden windows idle; revised on 2026-10-22 after checking how far Flutter has settled; the work in Inset and in WaOS's embedder.
- [Permissions](2026-10-23-permissions.md): programs as untrusted; rare uses ask, constant ones (the network, files) are switches the user sets with rules beneath; what the platforms ask for, and where WASI's checks sit at no cost per use.
- [The clipboard](2026-10-25-clipboard.md): a full clipboard, not text only; how every platform shapes its clipboard, and how each tells the user's own paste from a read.
- [Menus](2026-10-28-menus.md): a menu as a value rebuilt from state, for the menu bar, context menus, tray icons and the Dock; platform interfaces that tell only what the system provides; Edit's commands provided by the system and sent to the focused view.
- [File dialogs](2026-10-29-file-dialogs.md): a function per kind, as Flutter's file_selector; a pick read only unless the program asks to write; saving safely by renaming from its own folders onto the picked file.
- [Opening](2026-10-29-opening.md): opening links and files and revealing them, as Electron's shell; hearing being opened on every system; one instance as an opt-in crate; an opened file mounted writable.
- [Notifications](2026-10-29-notifications.md): a value posted under an id; replies and delivery later as capabilities none where the system lacks them.
- [Processes](2026-10-29-processes.md): a program's commands, with pipes or in a terminal, running outside its sandbox behind their switch.
- [Sound](2026-10-29-sound.md): Web Audio's graph played by the `web-audio-api` crate, every node but AudioWorklet.
- [Cameras and media](2026-10-29-media.md): players and cameras as Inset's, sent to surfaces and frame readers, on Apple's systems first.
- [Apps of their own on macOS](2026-10-29-macos-apps.md): each running program's windows a small Inset app of its own, the engine staying in WaOS; from the research on Chrome's app shims.
- [Apps of their own on Android](2026-10-29-android-apps.md): a task per program in WaOS's own app, with its own Recents card, home-screen shortcut and notification group; from the research on identity on Android.
- [Apps of their own: the plan](2026-10-29-apps-of-their-own-plan.md): the parts on macOS in three layers (WaOS's, a program's window on any desktop, macOS's alone), what Inset gains, and the steps on both platforms.
- [Native surfaces in Inset's frames](2026-10-30-native-surfaces.md): the scene records each surface at its place in the paint order and the host makes room, as Chromium, GTK and Flutter do; then, from the lead's review, one concept for native content, a composition layer made on its own and adopted by whoever places it, each frame naming its children; hardware buffers and video frames as the platform's own types; video drawn by the framework from a reader asked for the frame of each refresh's display time; with the research on how frameworks composite native surfaces.
- [Inset's scene as Flutter's layer tree](2026-10-30-inset-scene-layer-tree.md): the scene's layers and what reaches a host for a surface (its rectangle and Flutter's mutators), one walk giving the slices between surfaces and the surfaces, valo drawing the slices with a hole at each, the change in five steps; its six questions answered by the native-surfaces proposal's decisions 3 to 8.
- [A program's media across the WIT](2026-11-02-program-media.md): composition layers named with each present; a player's or camera's picture sent to one destination the program makes, a layer or a frame reader; frames as the platform hands them, the hardware buffer in a frame and one conversion to a texture; a media player's tracks, tags and subtitle cues, which the program draws with the user's caption settings merged in, from the research on subtitles (`2026-11-02-subtitles-research.md`); cameras listing their formats and starting with their outputs whole.
- [A machine's facts, file watching and a per-program `/tmp`](2026-11-15-machine-facts-file-watching-tmp.md): what a machine runs and its user's login shell, the systems' watches of a folder the program holds as a stream of batches, and a `/tmp` of each program's own under WaOS's caches; written after they were built, with the lead's decisions and the choices made building them.
- [The dev protocol's first stable shape](2026-11-18-dev-protocol-1.md): a hello that names each build and the requests it serves, CBOR so additions break neither half, an answer type and one refusal per request, a run that skips a component WaOS holds, windows by id; with the lead's decisions.
- [Views a program asks for](2026-11-21-views-a-program-asks-for.md): `request-view` the one way to a view, a screen WaOS opens before the program asks held for its first request, restored cards among them; a program with a UI is one whose component imports `wasdk:platform/views`, and one without is a console program WaOS shows a terminal for, a widget of Waft's that comes after; with the lead's decisions.
- [Projects on machines](2026-11-21-projects-on-machines.md): an editor's project on the machine the user picked for it, its root mounted at `/` in the program's own files (`wasdk:platform/mounts`), a program's own folders staying over it, watched as any folder; on Android, "This computer" reaching the phone's storage by All files access, asked for when the user picks it; with the lead's decisions.
- [Tests without the clock](2026-11-22-tests-without-the-clock.md): five rules for the tests and CI, a test waiting for what it expects and reading only its own program; `waos tap` answering once the input is handed over; one headless shell, `waos-headless`, for the tests, the daemon and the dev tools; the app's tests on Waft's tester.

## The experiment's, in wasdk (frozen)

Earlier designs of wasdk's, and their decisions, still the best account of what was learned:

- `2026-09-26-waos-mobile.md`: the phone form factor, frames into tabs, Android first, Pulley on iOS, lifecycle states and freezing.
- `2026-09-26-remote-machine.md` and `2026-09-26-remote-ux.md`: running programs against another machine over SSH, and its UI.
- `2026-09-26-suspend-model.md`: freezing programs out of sight, and what they keep.
- `2026-09-26-ios-jit.md`: compiled code on iOS only with a debugger attached.
- `2026-09-29-inset-frame-timing.md` and `2026-09-29-inset-macos-backend.md`: Inset's frame timing from a display, and a macOS host of its own.
- `2026-10-06-valo-filter-architecture.md` and `2026-10-08-valo-restructure.md`: valo's renderer.
- `2026-10-15-quit-flow.md` and `2026-10-15-inset-views-and-windows.md`: quitting, and Inset's views and windows, whose model this project's views follow.
- `2026-10-16-zed-on-waos.md`: research on Zed as a program.

The experiment's `DESIGN.md` and `docs/waos.md` describe what it built, including the measured frame path this project ports.

## Research

The maintainers' experiments folder, which is not part of this repository, holds a report for each question, with its commands, numbers and conclusions. The latest, behind the interface sketches, is the interface research of 2026-10-17: one report per capability, on how each system abstracts it.
