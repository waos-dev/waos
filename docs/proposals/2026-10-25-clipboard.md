# The clipboard

**Decisions taken (2026-10-25).** The lead took these, one question at a time:

1. **The clipboard is full from the start, not text only**, since what it carries shapes its interface.
2. **The clipboard holds items, each offered in several forms named by MIME types**, the shape of Apple's, Android's and the web's clipboards: a program reads what is offered without reading content, then one item in one form, and writes items, each in its forms. WaOS maps MIME to Apple's UTIs and Windows' formats, as Chrome maps the web's types on each system; on a system with one item (Windows, X11, Wayland) it writes the first, as Chrome does, and joins copied files into one list.
3. **A program gives each form's content at copy or promises it, form by form**, as macOS and Windows let it: a promised form is produced when someone pastes it, through the copy's requests, and content is streamed both ways. So a costly form is made only if pasted, a file can exist only once pasted (as `NSFilePromiseProvider` and Windows' `CFSTR_FILECONTENTS` make one), and big content never sits whole in memory: what native programs can do and web pages cannot. WaOS bears what promises cost, as the systems do: a paste reaching a frozen program thaws it to answer; an answer not given in time fails the paste, as WinRT's deadline does; a program that ends is first asked for every promised form, as Windows' `WM_RENDERALLFORMATS` asks, and what it does not give in time is dropped; and a program learns when and in which form its copy was pasted, as native programs do, never by whom.

```wit
variant content {
  data(stream<u8>),
  promise,
}

resource clipboard {
  /// What is on the clipboard now, read without its content: each item's types.
  offers: func() -> list<offer>;
  read: async func(item: u32, %type: string) -> result<stream<u8>, clipboard-error>;
  /// Replaces the clipboard; the copy answers the requests for its promised forms.
  write: async func(items: list<list<tuple<string, content>>>) -> result<copy, clipboard-error>;
}

/// What this program put on the clipboard, until something else replaces it.
resource copy {
  requests: func() -> stream<request>;
}

resource request {
  item: func() -> u32;
  %type: func() -> string;
  provide: func(content: stream<u8>);
}
```

4. **The user's paste comes from WaOS**, as on Apple's systems and the web, where only an action the system itself sees counts as the user's paste: WaOS owns the platform's paste command (⌘V as its Edit menu's key equivalent, Paste in the context menu) and sends the focused view a `paste` event, carrying a handle that reads the clipboard whatever the read switch says, as the web's `clipboardData` reads inside its `paste` event; a read at any other time is the switch's. Inset's text fields paste from the event rather than reading on ⌘V, as Flutter's text fields on the web get pasted text, and WaOS's program windows get an Edit menu.

```wit
variant view-event {
  // … as today
  /// The user pasted, through the platform's paste command.
  paste(paste),
}

/// The clipboard as the user pasted it: readable while the program handles this paste.
resource paste {
  offers: func() -> list<offer>;
  read: async func(item: u32, %type: string) -> result<stream<u8>, clipboard-error>;
}
```

5. **The user's copy and cut come from WaOS too**, as `copy` and `cut` events carrying a handle that writes whatever the write switch says, as the web's `copy` and `cut` events carry their `clipboardData`; a write at any other time is the switch's. **The program has every key first**, as AppKit offers a key equivalent to the key window's views (`performKeyEquivalent:`) before the main menu: ⌘C, ⌘X and ⌘V reach the program as keys, and only one it leaves unhandled runs WaOS's Edit menu, which sends the event. So a program that wants those keys for something else takes them, and the Edit menu is only what happens when it does not; Inset's text fields leave them unhandled in WaOS, as Flutter's text fields on the web leave paste to the browser, and the menus capability will let a program change its Edit menu.

```wit
variant view-event {
  // … as today
  /// The user copied, or cut, through the platform's command: write what is selected.
  copy(copy-request),
  cut(copy-request),
}

resource copy-request {
  /// Replaces the clipboard with what the user copied; the copy answers its promised forms.
  write: async func(items: list<list<tuple<string, content>>>) -> result<copy, clipboard-error>;
}
```

6. **Files travel as references, through the mounts**, as every platform carries them, with the grant a sandbox attaches being WaOS's mount: copying out, a program puts a file by its path inside the program, and WaOS writes that file's real path for a path inside the program's mounts (as `NSURL` writes `public.file-url`); on the user's paste, each pasted file is mounted read-only at its own path, as a pick is, and the program reads the paths from `text/uri-list` (as macOS grants a sandboxed app the pasted file, and Flatpak's portal read-only unless the sender allows writing); a program's own read sees the paths and gets no access, since handing files over is the user's act. A file that exists only once pasted is named by the program and its content streamed on paste, as `NSFilePromiseProvider` writes a promised file where the receiver chose, for a paste as well as a drop. A reference serves a receiver that wants only the bytes as well as one that opens the file in place.

```wit
variant content {
  data(stream<u8>),
  /// Produced when someone pastes it, through the copy's requests.
  promise,
  /// A file the program has, by its path inside the program.
  file(string),
  /// A file that exists only once pasted: its name; its content produced on paste.
  file-promise(string),
}
```

7. **A copy can say how it is to be treated**, through options set by methods so a new one breaks no program: `sensitive`, a password or a code kept out of previews, clipboard history and sync (Android's `EXTRA_IS_SENSITIVE`, the `org.nspasteboard.ConcealedType` convention macOS's clipboard managers honour, Windows' `ExcludeClipboardContentFromMonitorProcessing`); `this-device-only`, kept off other devices' clipboards (iOS's `localOnly`, macOS's `CurrentHostOnly`, Windows' `CanUploadToCloudClipboard`); and `expires-after`, cleared after a while (iOS's `expirationDate`), which WaOS does itself where the system cannot, and only while the copy is still the program's. The content's forms are named in the platforms' words: `data`, `promise`, `file`, `file-promise`.

```wit
resource copy-options {
  constructor();
  /// A password or a code: kept out of previews, clipboard history and sync.
  sensitive: func();
  /// Kept off other devices' clipboards.
  this-device-only: func();
  /// Cleared after `duration`, if still on the clipboard.
  expires-after: func(duration: duration);
}

// `clipboard.write` and `copy-request.write` take them:
write: async func(items: list<list<tuple<string, content>>>, options: copy-options)
    -> result<copy, clipboard-error>;
```

8. **What is on the clipboard, and its changes, are seen only while one of the program's views has focus**, types only, no content: `offers()` answers empty otherwise, and `changes()` streams the offers at each change while focused, as Android's listener needs focus, iOS reads types only in the foreground, Wayland tells only the focused client and the web's change event needs a focused page. A program in the background cannot watch the user copy.

```wit
resource clipboard {
  /// What is on the clipboard now, read without its content: each item's types.
  /// Empty unless one of the program's views has focus.
  offers: func() -> list<offer>;
  /// The offers at each change, while one of the program's views has focus.
  changes: func() -> stream<list<offer>>;
}
```

9. **Inset's platform clipboard becomes the full one**, replacing its text-only `Clipboard` (`set_text`, `text`, `has_strings`), and Flutter's text `Clipboard` service becomes a layer over it reading and writing `text/plain`, so one clipboard serves Inset's apps and WaOS's shell alike, on every platform Inset's hosts reach.
10. **An item is a handle**, as the web's `ClipboardItem` is: `items()` answers what is on the clipboard, each with its types and a `read` of one, which fails if the clipboard changed since, so a read never lands on another copy; `offers`, Wayland's word alone, gives way to the items and types every other platform names. In Inset each promise carries what produces it, as the web's `ClipboardItem` maps a type to a promise, so a promise without a producer cannot be written; and a change reaches the framework as the host's report to its client, which Flutter's `ClipboardStatusNotifier` turns into a notifier. The WIT keeps a stream of changes and a promise answered through the copy's `requests`, its own idiom where a closure cannot cross.

```rust
pub trait Clipboard {
    /// What is on the clipboard now: each item, to read in one of its types.
    fn items(&self) -> ClipboardItemsFuture;                       // Vec<Box<dyn PastedItem>>
    /// Replaces the clipboard.
    fn write(&self, items: Vec<ClipboardItem>, options: CopyOptions) -> ClipboardWriteFuture;
}

pub trait PastedItem {
    fn types(&self) -> &[String];                                  // MIME
    /// Fails with `ClipboardError::Changed` if the clipboard changed since `items`.
    fn read(&self, mime: &str) -> ClipboardReadFuture;
}

pub struct ClipboardItem {
    pub representations: Vec<(String, Content)>,
}

pub enum Content {
    Data(Vec<u8>),
    /// Produced when someone pastes it, each time.
    Promise(Rc<dyn Fn() -> ContentFuture>),
    File(PathBuf),
    /// A file that exists only once pasted: its name, and what writes it where the receiver chose.
    FilePromise { name: String, write: Rc<dyn Fn(&Path) -> WriteFuture> },
}

pub trait EmbedderClient {
    fn clipboard_changed(&mut self);
}
```

```wit
resource clipboard-item {
  types: func() -> list<string>;
  /// Fails if the clipboard changed since this item was listed.
  read: async func(%type: string) -> result<stream<u8>, clipboard-error>;
}

resource clipboard {
  /// What is on the clipboard now; empty unless one of the program's views has focus.
  items: func() -> list<clipboard-item>;
  /// The items at each change, while one of the program's views has focus.
  changes: func() -> stream<list<clipboard-item>>;
  write: async func(items: list<list<tuple<string, content>>>, options: copy-options)
      -> result<copy, clipboard-error>;
}

/// The clipboard as the user pasted it: readable while the program handles this paste.
resource paste {
  items: func() -> list<clipboard-item>;
}
```

11. **A key carries the program's reply**, handled or not, and a reply dropped unanswered counts as not handled, as Flutter's embedder API answers each key through a callback (`FlutterEngineSendKeyEvent`'s `handled`) and Chrome's pages acknowledge each key consumed or not. The systems want the answer before their key call returns (`performKeyEquivalent:`, `dispatchKeyEvent`, a window procedure), so WaOS claims the key at once and, if the program did not handle it, sends it on past the program, as Flutter's embedders redispatch a key their framework left. A handle rather than an id answered through the view, so a program that forgets cannot hold a key.

```wit
variant view-event {
  key(key-input),
  // …
}

record key-input {
  event: key-event,
  reply: key-reply,
}

/// Dropped without an answer counts as not handled.
resource key-reply {
  handled: func(handled: bool);
}
```

12. **What a program put on the clipboard is its `source`**, Wayland's `wl_data_source`, which is asked for a form when someone pastes (`send`) and told when it is replaced (`cancelled`), as `source.requests()` streams the requests and ends when another copy replaces it. Not `copy`: the resource is the program's side of a copy while it lasts, not the copy itself, and as a Rust type `Copy` hides the standard trait (it breaks wasmtime 49's generated derives). Not `clip`, Android's `ClipData`, which names only the data. *This renames decision 3's and decision 5's `copy`.*
13. **Inset's `K_IS_WEB` is true in the browser only** (`target_family = "wasm"` and `target_os = "unknown"`), not in a WaOS program (`wasm32-wasip2`), where it had left the arrows and Backspace to a browser that is not there. So a WaOS program's text fields take their editing keys themselves, as the program embedder's `handles_editing_keys` answers, and ⌘C, ⌘X and ⌘V as Flutter's native apps do; which of these they leave to WaOS's Edit menu is decided with menus.
14. **The clipboard is built now, and the user's commands come with menus**: the clipboard of decisions 1 to 3 and 6 to 10 first; the Edit menu's copy, cut and paste events (decisions 4 and 5), the key reply (decision 11) and the keys a text field leaves to WaOS with the menus capability, which is designed on its own: one model for the menu bar, popups and a tray icon's menu, with submenus, checkbox and radio items, shortcuts and icons, covering what nativeapi covers (`Application::SetMenuBar`, `Menu::Open`, a tray icon's menu) and replacing Inset's popup list as the full clipboard replaced its text one. Until then a program's ⌘C and ⌘V are its own write and read, under switches that start on; only a paste with the read switch off waits.

*Decisions 15 to 17 were taken on 2026-10-26.*

15. **Inset's promised file produces its bytes, as the WIT's does**: the app pushes them into a sink the host hands it, as super_clipboard's `VirtualFileProvider` pushes into an `EventSink`, and Inset's macOS host writes them at the URL the receiver chose. Of the platforms, macOS and X11's direct save have the source write at a path the receiver chose, iOS has it write a file of its own, and Windows (`CFSTR_FILECONTENTS` as an `IStream`) and Android (a content provider's pipe) take a stream; from a stream, writing at a path is a copy, while from a path a stream needs a temporary file first, so the stream serves every platform, and a WaOS program never writes outside its mounts nor learns where the receiver put the file. No size is given up front: the WIT has none, and macOS and Android need none. A promised form stays whole bytes, as the platforms take a form whole (`provideDataForType` an `NSData`, `WM_RENDERFORMAT` one `HGLOBAL`). *This revises decision 10's `FilePromise`.*

```rust
pub enum Content {
    // …
    /// A file that exists only once pasted: its name, and what writes its bytes into the
    /// sink the host hands it. Whole when the future resolves `Ok`; an `Err` fails the paste.
    FilePromise {
        name: String,
        produce: Rc<dyn Fn(Box<dyn FileSink>) -> WriteFuture>,
    },
}

/// Where a promised file's bytes go, in order.
pub trait FileSink {
    /// Appends `bytes`; resolves once the host has taken them.
    fn write<'a>(&'a mut self, bytes: &'a [u8]) -> Pin<Box<dyn Future<Output = io::Result<()>> + 'a>>;
}
```

16. **A request is answered with its content and how it ended**, WASI 0.3's own pairing of a stream with a future, as wasi:http's body pairs `contents` with `trailers`: the paste fails unless `done` says ok, so a producer that breaks half-way never passes for a whole, short file.

```wit
resource request {
  item: func() -> u32;
  %type: func() -> string;
  provide: func(content: stream<u8>, done: future<result<_, string>>);
}
```

17. **A program that ends on its own first flushes the clipboard**, as AppKit asks for every promised type inside `terminate:`, Windows sends `WM_RENDERALLFORMATS` as the owner window goes and OLE apps call `OleFlushClipboard()`, and GTK hands the clipboard to a clipboard manager at quit; where nothing asks (Wayland, or a crash anywhere), lazy data is lost. WaOS cannot ask once `main` has returned, since no program code runs then, so the framework asks on its way out, as AppKit does: Inset's embedder calls `flush` after its event loop ends, and a program on the WIT alone calls it itself. A flush, like a stop, asks for promised forms only: a promised file is promised because it is costly, and needs a place to write that nobody has chosen at quit, so it ends with the program. This completes decision 3's "a program that ends is first asked".

```wit
resource clipboard {
  // …
  /// Asks for every form this program's copy still promises, through its source's requests,
  /// so what it gives outlives the program; resolves once each is answered or its time is up.
  flush: async func();
}
```

## Research behind it

Read in each platform's documentation, its SDK headers (macOS and iOS 27) or AOSP's source; what was only inferred is marked so.

### The shape every platform shares

One current clipboard per user (or seat), holding one or more items, each with one or more representations keyed by a type; a reader lists the types cheaply, then pulls one representation; a writer gives the data at once or produces it when asked; files travel as references (a URL, a URI, a key), never as bytes; a counter or an event says the content changed. Drag and drop uses the same container on every platform below, except the web's async API.

| | Items | Type naming | Data produced when asked | Files | Change signal |
|---|---|---|---|---|---|
| macOS (`NSPasteboard`) | many | UTI, with conformance | data provider per type | file URL, one item per file | `changeCount` only |
| iOS (`UIPasteboard`) | many | UTI (any string) | `NSItemProvider` | item providers, URLs | count and notification |
| Android (`ClipData`) | many, one description for the clip | MIME | `content://` provider | content URI, granted on read | listener, focus-gated |
| Win32 | one | integer id, registered by name | `WM_RENDERFORMAT` | `CF_HDROP` path list, virtual files | listener and sequence number |
| WinRT (`DataPackage`) | one | string id | `SetDataProvider`, with a deadline | storage items | `ContentChanged` |
| X11 | one | atom, MIME by convention | always | `text/uri-list` | XFixes |
| Wayland | one | MIME | always (`send` into a pipe) | `text/uri-list`, portal key | `selection` while focused |
| Web | many in the API, Chrome writes one | MIME, plus `"web "` custom | promises, not truly lazy | files only through events | `clipboardchange` (spec) |

Where they differ, an interface must choose: type naming (MIME; Apple's UTIs, where `public.png` conforms to `public.image`; Win32's registered formats, some a different encoding of the same content, as its "HTML Format" wraps HTML in a header of offsets); more than one item (only Apple, Android and the web's API); data produced when asked (always on X11 and Wayland, optional elsewhere; it keeps the source alive, needs a timeout, and tells the source when and in which form someone pasted, which the W3C TAG raised for the web); files as references with an access grant attached (macOS's sandbox extension, Android's per-read grant, Flatpak's portal key); synchronous calls (Apple, Win32, Android) or asynchronous ones (X11, Wayland, WinRT, the web); and flags on a write (Android's sensitive content and its hour-long auto-clear, iOS's `localOnly` and expiry, macOS's `CurrentHostOnly`, Windows' history and cloud exclusions).

### How a read is gated

| | macOS 15.4+ | iOS 16+ | Android 12+ | Win32 | Wayland | X11 | Web |
|---|---|---|---|---|---|---|---|
| Who can read at all | anyone | the foreground app | the focused app or input method | anyone | the focused client | anyone | a focused document with user activation |
| Prompt | an alert, unless the user pasted | an alert, unless the user pasted | none (a notice after the read) | none | none | none | Chrome a permission; Safari and Firefox their own Paste menu |
| What counts as the user's paste | input on an element the system knows is for pasting; its own menu (inferred) | the edit menu's Paste, ⌘V, `UIPasteControl` | no such notion | none | none | none | the browser's `paste` event; a Paste element the browser draws |
| Inspecting without reading | detect patterns and metadata | types, count, `has…`, detect | `getPrimaryClipDescription` | the formats | the offer's MIME types | `TARGETS` | `clipboardchange`'s types |
| Its own content exempt | yes | yes | yes (no notice) | – | – | – | the same origin (Safari, Firefox) |

Only an action the system itself sees counts as the user's paste where one counts at all: its own menu, its own shortcut, or a control it draws or verifies. A program that draws its own UI, as Flutter does and as every WaOS program does, is the case Flutter met on iOS: its own Paste menu is a programmatic read, which iOS 16 alerts on, and its `SystemContextMenu` shows the system's menu instead.

Sources: the macOS and iOS 27 SDK headers (`NSPasteboard.h`, `NSPasteboardItem.h`, `UIPasteboard.h`, `UIPasteControl.h`, `UIDragItem.h`); Apple's `NSPasteboard.AccessBehavior` and `UIPasteControl` documentation and WWDC22 session 10096; Android's copy-and-paste and Android 10 privacy guides and AOSP's `ClipData.java`, `ClipboardManager.java` and `ClipboardService.java`; Microsoft's Win32 clipboard and WinRT `Clipboard`/`DataPackage` documentation; the ICCCM; the Wayland protocol, `ext-data-control-v1` and Flatpak's FileTransfer portal; the W3C Clipboard APIs draft, MDN, Chrome's posts on web custom formats and unsanitized HTML, WebKit's posts on the async clipboard, and the TAG's review 925; Flutter's `Clipboard` and `SystemContextMenu`, and the `super_clipboard` package.
