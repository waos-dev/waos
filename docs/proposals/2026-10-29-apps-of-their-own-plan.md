# Apps of their own on macOS and Android: the plan

The work that follows from the lead's decisions in `2026-10-29-macos-apps.md` and `2026-10-29-android-apps.md`, written 2026-10-29; the lead gave the go-ahead the same day. Each step builds and passes on its own; Inset's steps are pull requests to `inset-rs/inset`, WaOS's are commits on main.

## The parts, and where each lives

On macOS the code keeps three layers apart (macOS decision 4):

| Layer | What it holds | Where |
|---|---|---|
| WaOS's anyway | the engine, every program's thread, its pictures and players, the store, prompts (drawn in WaOS and hosted like a program's picture) | `crates/waos`, `apps/waos` as today |
| A program's window, on any desktop | the program's windows as the user meets them: the canvas, pointer and keys, text input, edit commands, menus, Dock menu, About, file dialogs, notifications. In WaOS's own windows where a program has no app of its own (Windows and Linux for now), in its own process on macOS | a new crate, `crates/waos-program-window`, moved out of `apps/waos` (the canvas, text input, edit commands, program menus); it talks to its program through `ProgramLink` |
| | the messages between WaOS and a program's window process, as values | a new crate, `crates/waos-window-messages` |
| | the window process: an Inset app over `waos-program-window`, built as the library each program's loader loads | a new app, `apps/waos-window` |
| macOS's alone | the bundle per program (writing, ad hoc signing, its code hash), the loader, Mach (a bootstrap name, the sender's audit token), `CAContext` and `CALayerHost` | `os/macos` modules of `apps/waos` and `apps/waos-window` |

```rust
/// What a program's window needs of its program, wherever the program runs.
/// In WaOS's process: the engine's calls. In a window process: messages to WaOS.
pub trait ProgramLink {
    fn send_pointer(&self, view: ViewId, event: PointerEvent);
    fn send_key(&self, view: ViewId, event: KeyEvent, reply: KeyReply);
    fn send_text_edit(&self, view: ViewId, edit: TextEditingDelta);
    fn send_edit_command(&self, view: ViewId, command: EditCommand);
    fn report_window(&self, view: ViewId, report: WindowReport);
    // ...menu choices, file dialog answers, notification clicks, open requests, quit answers
}

/// WaOS → a program's window process.
pub enum ToWindow {
    OpenWindow { view: ViewId, config: WindowConfig, layers: HostedLayers },
    CloseWindow { view: ViewId },
    SetTitle { view: ViewId, title: String },
    SetCursor { view: ViewId, cursor: MouseCursor },
    SetTextInput { view: ViewId, state: TextInputState },
    KeyReplied { key: KeyId, handled: bool },
    SetMenus(MenuBar),
    PostNotification(Notification),
    ShowFileDialog { request: DialogId, dialog: FileDialog },
    AskToQuit,
}

/// A program's window process → WaOS.
pub enum FromWindow {
    Hello { program: ProgramId, launched_for: LaunchReason, opened: OpenRequest },
    // ...one variant per ProgramLink call, and the window's reports
}
```

`HostedLayers` is the macOS `CAContext` id, with the fence port that lands a resize in one frame on both sides.

## What Inset gains

| | API | Platforms |
|---|---|---|
| Activation left to the system | none: winit's forced activation off | macOS |
| No GPU device until a view paints | none | macOS's host |
| A surface root no view shows, whose layer the app places; `show_layer_on` public | macOS-only, by downcasting to the compositor | macOS |
| The app outlives its activities: one loop per process, each activity attaching as a view | none | Android |
| A task per requested view, views given from intents, `HostWindow` on Android (`set_title`, `close`), `HostWindow::set_icon` | `set_icon(Option<IconImage>)` | Android, Windows, X11 |
| Pinned shortcuts | `AndroidPlatform::pinned_shortcuts()`, below, reached by downcasting the platform | Android |
| Notification channel groups | `AndroidPlatform::notification_channel_groups()`, below | Android |

```rust
// Reached as `platform.downcast_ref::<AndroidPlatform>()`, as WaOS reaches `MacOs`.
impl AndroidPlatform {
    pub fn pinned_shortcuts(&self) -> &PinnedShortcuts;
    pub fn notification_channel_groups(&self) -> &NotificationChannelGroups;
}

impl PinnedShortcuts {
    /// Asks the launcher to pin `shortcut`; the launcher asks the user.
    pub fn request_pin(&self, shortcut: &PinnedShortcut) -> PinFuture;

    /// Changes a pinned shortcut's label or icon.
    pub fn update(&self, shortcut: &PinnedShortcut);

    /// Greys it out with `message`: Android lets no app remove one.
    pub fn disable(&self, id: &str, message: &str);
}

pub struct PinnedShortcut {
    pub id: String,
    pub label: String,
    pub icon: IconImage,
    /// What the view it opens carries, as `View::initial_data`.
    pub initial_data: String,
}

impl NotificationChannelGroups {
    /// Makes the section, or renames it, keeping the user's settings for it.
    pub fn set(&self, id: &str, name: &str);

    /// Takes the section away, with its channels.
    pub fn remove(&self, id: &str);

    /// The app's notifications posted in section `id`: the same `Notifications`
    /// as everywhere, with posting, permission, replies and scheduling.
    pub fn notifications(&self, id: &str) -> Rc<dyn Notifications>;
}
```

## The steps

**Inset** (two worktrees of the pool; the Android steps stack):
1. macOS: activation left to macOS.
2. macOS: no GPU device and no drawing until a view paints something of its own.
3. macOS: a detached surface root, and `show_layer_on` public.
4. Android: the app outlives its activities (a reshaping, no new behaviour), checked with several activities on the emulator.
5. Android: a task per requested view, views given from intents, `HostWindow` on Android, `set_icon` on Android, Windows and X11.
6. Android: pinned shortcuts.
7. Android: notification channel groups.

**WaOS on macOS** (after Inset's 1 to 3):
1. Reshape: the program's window code moves into `crates/waos-program-window` behind `ProgramLink`, with WaOS's in-process link; nothing changes for the user.
2. `crates/waos-window-messages` and the Mach transport, tested over a pair of ports in one process.
3. `apps/waos-window` and the loader; the bundle per program written and signed ad hoc at install and for a development run (under WaOS's caches), deleted at removal, rebuilt when a launch fails or macOS's version changes; launching and the hello, checked by audit token and code hash.
4. Pictures: the program's surfaces under a detached root in a `CAContext`, hosted by a `CALayerHost` in the window; prompts the same way; resizes by the fence.
5. What the window process holds: menus, Dock menu, About, file dialogs, notifications under the program's bundle, opening files and links, quitting through `NSTerminateLater`.

**WaOS on Android** (after Inset's 4 to 7):
1. A task per program: opening a program requests its view, a task brought back by Android starts its program, the task ends when the program quits; the deck behind a setting.
2. The home-screen shortcut offered at install, disabled at removal.
3. A channel group per program.

**Then:** WaOS's side of media, which Inset's player now allows (Inset #8 merged), players staying in WaOS.
