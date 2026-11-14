# File dialogs

**Decisions taken (2026-10-29).** The lead took these, one question at a time, starting the file dialogs of step 7, which v1 interface decision 3 (`2026-10-17-waos-v1-interface.md`) first sketched and permissions decision 10 (`2026-10-23-permissions.md`) revised: a file or folder picked while a program runs is mounted at its own path the moment the user confirms it, with no `/granted`, and kept until the user takes it back, for an installed program.

1. **A function per kind, as Flutter's official `file_selector` package has them** (`openFile`, `openFiles`, `getDirectoryPath`, `getSaveLocation`), each with options of its own and answering what its kind means, and the systems' own panels (`NSOpenPanel`, `IFileOpenDialog`, the portal's `OpenFile`) behind them; not one dialog object with setters, whose one answer would serve every kind and which could express a save allowing many. Inset gains the host trait, `Platform::file_dialogs()` none where the system has none; the WIT follows it.

```rust
pub trait FileDialogs {
    fn open_file(&self, options: OpenOptions) -> FileDialogFuture<Option<PathBuf>>;
    fn open_files(&self, options: OpenOptions) -> FileDialogFuture<Vec<PathBuf>>;
    fn open_folder(&self, options: FolderOptions) -> FileDialogFuture<Option<PathBuf>>;
    fn save_file(&self, options: SaveOptions) -> FileDialogFuture<Option<PathBuf>>;
}

pub struct OpenOptions {
    pub types: Vec<FileType>,          // file_selector's XTypeGroup
    pub initial_folder: Option<PathBuf>,
    pub confirm_label: Option<String>,
    pub writable: bool,
}

pub struct FileType {
    pub label: String,
    pub extensions: Vec<String>,
    pub mime_types: Vec<String>,
}
```

2. **A pick grants read only, unless the program asks to write**, as Android's `ACTION_OPEN_DOCUMENT` grants write only with `FLAG_GRANT_WRITE_URI_PERMISSION` and the web's `showOpenFilePicker` reads while `showDirectoryPicker({ mode: "readwrite" })` asks to write; a save grants write to the one file chosen, which may not exist yet. Writable is the mount's own property (`Mount { host, at, writable }`): an option only says which kind of mount the pick makes. macOS's sandbox grants read and write to every pick, which would let every viewer change what it opens.
3. **A program saves safely into a picked file by writing in its own folders and renaming onto it**, as a sandboxed macOS app writes into a replacement folder and calls `replaceItemAtURL:`: a file in the program's own folders (`/data`, `/cache`) may be renamed onto a writable file mount, which WaOS, serving the filesystem, does as one replace on the machine, copying the file beside the target first when the two are on different volumes. A rename across mounts fails on a plain Unix system (`EXDEV`); WaOS allows this one. The program's temporary folder (`TMPDIR`) is in `/cache`, so `NamedTempFile::new()` and `persist(target)` work. Code that creates its temporary file beside the target (`NamedTempFile::new_in(target.parent())`) assumes every folder is writable and is not supported, as Flatpak's document portal supports it by letting an app create files beside a picked one: one rule, and the grant stays the one file.
4. **A pick answers a path where the system gives one, and the system's own handle where it gives none, Android and the web included in this round**, as Flutter's `XFile` carries a path or a blob: macOS and iOS answer a URL with a path, Windows a path, Linux's desktop portal a `file://` URL, while Android answers a `content://` URI and the web a `FileSystemFileHandle`, neither with any path. A copy into the app's cache, as Flutter's `image_picker` makes, could not be written back and would stand in for what the system gives. A WaOS program always gets a path, the mount WaOS made.
5. **A handle reads and writes at any offset, and a pick that can only be read through is a stream**: the web's `File.slice` and its `createWritable({ keepExistingData: true })`, writing at positions and replacing the file only on `close()`, and Android's file descriptor; but Android's `ContentProvider.openFile` answers a file opened read only possibly as "a pipe or socket pair to enable streaming of data", while read-write "implies a file on disk that supports seeking", so Android opens a pick when it is answered and answers a handle where it seeks and a stream where it is a pipe. A system that gives only streams later adds nothing new.

```rust
pub enum PickedFile {
    /// macOS, iOS, Windows, Linux, and every WaOS program.
    Path(PathBuf),
    /// The web; Android when the provider answers a file that seeks (always so for a writable pick).
    Handle(Box<dyn FileHandle>),
    /// Android when the provider answers a pipe: read once, start to end. Read-only picks only.
    Stream(Box<dyn FileStream>),
}

pub trait FileHandle {
    fn name(&self) -> &str;
    fn size(&self) -> SizeFuture;
    fn read_at<'a>(&'a self, offset: u64, buf: &'a mut [u8]) -> ReadFuture<'a>;
    /// None unless the pick is writable.
    fn open_write(&self) -> Option<OpenFuture<Box<dyn FileWriter>>>;
}

pub trait FileWriter {
    fn write_at<'a>(&'a mut self, offset: u64, bytes: &'a [u8]) -> WriteFuture<'a>;
    fn set_len(&mut self, len: u64) -> WriteFuture<'_>;
    /// Ends the writing: on the web the file is replaced only now, and a writer dropped
    /// before abandons it; on Android the writes landed in place as they were made.
    fn finish(self: Box<Self>) -> FinishFuture;
}

pub trait FileStream {
    fn name(&self) -> &str;
    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> ReadFuture<'a>;
}
```
6. **Inset's Android apps gain one small Java class** (2026-10-29): an activity extending `android.app.NativeActivity` that overrides `onActivityResult` and hands the result to native code, which inset-cli compiles into the APK. An Inset app runs on `NativeActivity`, which ships no Java and forwards no activity result to native code, so it could start `ACTION_OPEN_DOCUMENT` and never hear what was picked; Flutter's `FlutterActivity` and android-activity's `GameActivity` are Java for the same reason. It also opens what the clipboard could not do on Android without Java: writes beyond text and promised files, which need a `ContentProvider`.
7. **The WIT follows Inset's: each kind a function, answering paths already mounted at their own place, read only unless asked otherwise, and options as resources with setters**, so that a new option breaks no program, as the clipboard's `copy-options` does; `parent` makes the dialog a sheet on that view's window.

```wit
interface file-dialogs {
  use views.{view};

  resource open-options {
    constructor();
    add-type: func(label: string, extensions: list<string>, mime-types: list<string>);
    initial-folder: func(path: string);
    confirm-label: func(label: string);
    writable: func();
  }
  resource folder-options {
    constructor();
    initial-folder: func(path: string);
    confirm-label: func(label: string);
    writable: func();
  }
  resource save-options {
    constructor();
    add-type: func(label: string, extensions: list<string>, mime-types: list<string>);
    initial-folder: func(path: string);
    suggested-name: func(name: string);
    confirm-label: func(label: string);
  }

  variant dialog-error { failed(string) }

  resource file-dialogs {
    open-file: async func(parent: option<borrow<view>>, options: open-options) -> result<option<string>, dialog-error>;
    open-files: async func(parent: option<borrow<view>>, options: open-options) -> result<list<string>, dialog-error>;
    open-folder: async func(parent: option<borrow<view>>, options: folder-options) -> result<option<string>, dialog-error>;
    save-file: async func(parent: option<borrow<view>>, options: save-options) -> result<option<string>, dialog-error>;
  }

  get-file-dialogs: func() -> option<file-dialogs>;
}
```
8. **A folder pick answers a path where the system gives one, and a folder handle where it gives none** (2026-10-29), as decision 4 has files: the web's `showDirectoryPicker` gives a directory handle, and Android's `ACTION_OPEN_DOCUMENT_TREE` a tree URI, neither with a path nor a file to read at an offset. `PickedFolder { Path, Handle }`, where a `FolderHandle` lists its entries, opens a child file as a `PickedFile` and a child folder as a `PickedFolder`, and creates files and folders in it when the pick is writable. A WaOS program still always gets a path.
