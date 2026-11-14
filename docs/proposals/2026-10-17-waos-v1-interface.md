# What WaOS's first version gives a program

> Carried over from `../wasdk/docs/proposals/` on 2026-10-17, as written for the experiment, which is frozen. Where `2026-10-17-the-waos-project.md` differs (no native build of programs, remote machines last, this project's crates and work order), that design wins. Its interface sketches are rough: each is designed with the lead before it is written. Paths in it are the experiment's.

**Decisions taken (2026-10-17).** Designed with the lead, one question at a time, after research into how macOS, iOS, Android, Windows, Linux, the web, Flutter and WASI abstract each capability the first version adds (`2026-10-16-waos-v1.md`, decision 4). The research is in `../experiments/2026-10-17-interface-research/`. The lead took these:
1. **A program uses a capability, and the first use asks; each capability the user guards also tells its state** (`not-asked`, `granted`, `denied`) without asking, through a method on its own resource, so a program can explain before asking, or grey out what was refused. No shared list of capabilities, since adding a case to a shared enum breaks every program built before it. The answer is kept among the program's grants.
2. **Networking is WASI's sockets and WASI's HTTP client, served by WaOS over a network provider, and a certificate check that answers as the system does.** `wasdk-host` serves `wasi:sockets` itself over a `Network` trait in `wasdk-io`, as it serves files over `Storage`, since wasmtime's sockets are this computer's alone; a provider for this computer and one for SSH machines. WaOS serves `wasi:http`'s client through wasmtime's hook, with an HTTP client over the same providers, the system's certificate check and the system's proxy, so HTTP needs no TLS or HTTP code in the program and its encryption runs natively, interpreted on iOS otherwise. `trust.verify-server` checks a certificate chain with the system's own checker, for a program that brings its own TLS.
3. **A file or folder chosen while a program runs is a path under `/granted`, one folder WaOS hands every program at start,** because the C library under Rust's `std` reads its folders once, at start, and cannot take a new one later. A program's settings are one list, edited in Show Info: its permissions and its folders (`/data` and `/cache`, its machine's files, folders added in Show Info, which are folders of their own from the next start, and items picked or dropped, which stay under `/granted` so a path the program saved keeps working).
4. **Sound is a strict subset of Web Audio's graph, with inputs of our own.** The program builds a graph of nodes (clips, gain, stereo panner, filter, analyser, its output) and schedules changes on the audio clock; WaOS plays it on its own audio thread and mixes the programs. A program on one thread in wasm cannot answer the system's real-time audio callback in time, so nothing of the program runs there. Sound gets in through inputs of our own: samples the program pushes ahead, a player's sound, the microphone. The HTML media element and `getUserMedia` are not taken: the media element is the web's older shape bridged into Web Audio, and copying it buys nothing. An `AudioWorklet`-like input and a codecs interface in WebCodecs' shape can be added later, as the same family defines them.
5. **Camera and video are sources sent to destinations: a surface, a frame reader, or both.** A player (a file or a link) and a camera are sources. A **surface** is a system layer on a view, beneath the program's drawing where the program leaves it clear, which the system shows itself: overlay planes, HDR and protected video. It is generic, any producer feeds it, in Inset's model. Its position takes effect with the view's next frame and its pictures run on their own clock; the research found every platform's own mechanism does this, and Chromium and Flutter pay to get it. A **frame reader** gives the program the newest picture as an RGBA texture to draw. The system layer is in this version. Where Inset's hosts differ from this model, Inset changes.
6. **Menus are the system's menu bar where the system has one, and native context menus where they exist.** macOS now, the iPad later; elsewhere the program draws its own, as Flutter does. The program describes its whole menu as a flat list, since WIT has no tree types, replaced whole when its state changes, and hears each choice by id.
7. **Notifications add an explicit permission request to decision 1's state.** A program's first notification is often posted when its user is not looking, and iOS, Android and the web all refuse to post without permission granted before; a post without permission answers `denied` and never asks.
8. **A program is opened by files and links its package declares, registered with the system when WaOS installs it.** In the program's window process bundle on macOS, as file associations and a protocol handler on Windows, in its `.desktop` file on Linux; nothing per program on phones in this version, whose systems fix an app's file types when it is built.
9. **`processes` follows the usual process model:** a command (program, arguments, working directory, environment) started either with pipes or in a terminal, answering the started program and its pipes or its terminal. It replaces today's terminal-only interface, which was built for one program, a terminal, and fused a process with its terminal.
10. **Parallel work is workers: instances of the program's own code that share nothing and exchange messages, as the web's workers do.** Components cannot share memory, the parallel threads proposal is at phase 1, and Pulley cannot run shared memory; separate instances ran in parallel under wasmtime 49 and under Pulley. An experiment proves them before they are built. Cooperative threads come with `wasm32-wasip3`, when Rust ships them, so threaded crates run, without running faster.
11. **A worker gets the IO side (files, networking and processes, under the program's grants), clocks, random numbers and WebGPU; the user's side belongs to the program's main instance alone.** The user's side assumes one instance owns it: a view's events have one reader, the quit question goes to the program's listeners. The web draws the same line.

Settled along the way, as the proposer's defaults the lead accepted:
- **Networking:** a name looked up on an SSH machine answers a placeholder address, which the provider turns back into the name when the program connects, so the machine's own SSH server resolves it. A machine grant includes the machine's network. A program on no machine is asked at its first connection, in two scopes: the internet, and the local network with accepting connections. All of WASI's sockets on this computer; over SSH, TCP connections and listening, and UDP answers `not-supported`. russh widens a channel as the program reads it, as OpenSSH does, after an experiment.
- **Files:** a grant is kept until the user takes it back, for an installed program, and for the run, for a program run from its file. A program on an SSH machine gets WaOS's own dialog over the machine's files, with a switch to this computer. A drop is an event on the view, with paths once the user lets go, granted read and write as a pick is. A dialog's options are set by methods. A native build on Android answers no picker.
- **Sound:** WaOS plays each program's graph itself, so a frozen program's scheduled sound and music play out; a program is not frozen while it is audible or recording, which amends `DESIGN.md` decision 24, and needs iOS's background audio mode and an Android foreground service. The microphone asks at first use, and WaOS shows which program records.
- **Camera and video:** pictures are RGBA textures, converted on the GPU by the system or WaOS; WebGPU's external texture comes when `wasi:webgpu` and wgpu both have it. The camera asks at first use, WaOS shows which program uses it, and opens each camera once and shares it.
- **Menus:** a shortcut goes to the menu first, as GTK and Win32 do. Quit in a program's menu is an exit request to that program. Native context menus answer `unsupported` on Linux and phones in this version.
- **Notifications:** who posts, and whose name the system shows, per platform as in "The interface" below. A click on a notification of a program that is not running starts it, and is delivered once it listens; on macOS a program's window process can start WaOS.
- **Opening:** `http`, `https` and `mailto` open freely; any other scheme asks once per program and scheme. Opening a file that lives on an SSH machine is refused in this version, since a copy would lose its user's edits.
- **Workers:** messages are bytes; handles can come later. Programs move to `wasm32-wasip3` when Rust ships cooperative threads.
- **Memory:** a program's memory grows as far as wasm allows; the 256 MiB cap was removed (`9e3aa3b`).

Written against wasdk at `9e3aa3b`, wasmtime 49 and the WIT in `wit/`. The interface breaks freely until its first release; after it, it grows only by new interfaces, resources, methods and streams.

## The rules that cut across everything

- **Getting a capability:** each interface has a getter answering none where the host lacks it, as today.
- **Permissions:** decision 1, and decision 7 for notifications.
- **Two sides:** every interface is wholly on the user's side (`wasdk-gui`: what the user sees and touches, where the user is) or on the IO side (`wasdk-io`: files, networking, processes, served by providers, so a program on an SSH machine reaches that machine's).
- **Growth:** options are set by methods and content is built by setters, so adding one breaks no program; records and variants are kept for what will not grow.

## The interface

Sketches, in the repository's idiom; doc comments trimmed.

### Networking: WASI's, and `trust`

```wit
world program {
  import wasi:sockets/tcp@0.2.12;
  import wasi:sockets/udp@0.2.12;
  import wasi:sockets/ip-name-lookup@0.2.12;
  import wasi:http/outgoing-handler@0.2.12;
  // … and every interface below
}

interface trust {
  variant trust-error {
    untrusted(string),
    failed(string),
  }

  resource trust {
    /// Checks a server's certificate chain, leaf first, as this system checks it: its roots,
    /// those the user or an organization added, revocation and the system's own rules.
    verify-server: async func(chain: list<list<u8>>, server-name: string) -> result<_, trust-error>;
  }

  get-trust: func() -> option<trust>;
}
```

```rust
/// Sockets and name lookup, served by a machine: this computer's network, or a machine's over
/// SSH. Kept in the shape of `wasi:sockets` 0.3; the socket's states and options stay in the host.
#[async_trait]
pub trait Network: Send + Sync + 'static {
    async fn resolve_addresses(&self, name: &str) -> Result<Vec<IpAddr>, LookupError>;
    async fn connect(&self, local: Option<SocketAddr>, remote: SocketAddr, options: &TcpOptions)
        -> Result<TcpConnection, SocketError>;
    async fn listen(&self, local: SocketAddr, backlog: u32, options: &TcpOptions)
        -> Result<TcpListener, SocketError>;
    async fn bind_udp(&self, local: SocketAddr) -> Result<Box<dyn UdpSocket>, SocketError>;
}
```

`Machine` gains `network()`, and `Grants` the network.

### `processes`

```wit
interface processes {
  /// What to start; a terminal's type is the `TERM` variable.
  resource command {
    constructor(program: string);
    add-argument: func(argument: string);
    set-working-directory: func(path: string);
    set-variable: func(name: string, value: string);
  }

  /// A started program.
  resource child {
    signal: func(signal: signal);
    exit-status: async func() -> exit-status;
  }

  /// Its input, output and errors, each a pipe of its own.
  resource pipes {
    write: async func(data: list<u8>) -> result<_, process-error>;
    close-input: func();
    output: func() -> option<stream<u8>>;
    errors: func() -> option<stream<u8>>;
  }

  /// One terminal that is its input, output and errors together.
  resource terminal {
    write: async func(data: list<u8>) -> result<_, process-error>;
    output: func() -> option<stream<u8>>;
    resize: func(size: terminal-size);
  }

  resource processes {
    start-with-pipes: async func(command: borrow<command>) -> result<tuple<child, pipes>, process-error>;
    start-in-terminal: async func(command: borrow<command>, size: terminal-size) -> result<tuple<child, terminal>, process-error>;
  }

  get-processes: func() -> option<processes>;
}
```

Over SSH, pipes are an exec channel, and a terminal is one with a terminal request; the working directory and the environment are given through the machine's shell, quoted by the provider. The SDK's Rust face mirrors `std::process::Command`, which it wraps natively.

### `workers`

```wit
interface workers {
  variant worker-error {
    unknown-entry(string),
    refused(string),
  }

  /// A running instance of this program's own code, sharing nothing with it. Dropping it ends it.
  resource worker {
    messages: func() -> option<stream<list<u8>>>;
    finished: async func() -> result<_, string>;
  }

  resource workers {
    start: async func(entry: string, inbox: stream<list<u8>>) -> result<worker, worker-error>;
    available-parallelism: func() -> u32;
  }

  get-workers: func() -> option<workers>;
}

/// Exported by a program with workers; WaOS calls it in each new instance.
interface worker-entry {
  run: async func(entry: string, inbox: stream<list<u8>>) -> stream<list<u8>>;
}
```

In Rust, `#[wasdk::worker] async fn parse(port: Port)` registers an entry and `wasdk::workers::start(parse)` starts it; natively the SDK runs the entry on a `std::thread` with channels. In `wasdk-host`, a program becomes several stores that share one freezer, one fuel count and one trace span.

### `file-picker`, and drops

```wit
interface file-picker {
  use views.{view};

  enum dialog-kind {
    open-files,
    open-folder,
    save-file,
  }

  variant dialog-error {
    unsupported,
    failed(string),
  }

  resource file-picker {
    dialog: func(kind: dialog-kind) -> file-dialog;
  }

  resource file-dialog {
    set-title: func(title: string);
    set-confirm-label: func(label: string);
    add-filter: func(label: string, extensions: list<string>, media-types: list<string>);
    allow-many: func();
    allow-writing: func();
    set-suggested-name: func(name: string);
    /// The paths the program now reaches, under `/granted`; empty when the user cancelled.
    show: async func(parent: option<borrow<view>>) -> result<list<string>, dialog-error>;
  }

  get-file-picker: func() -> option<file-picker>;
}
```

A view's events gain a drop: `entered`, `moved` and `exited` with the pointer's position, and `dropped` with the paths, granted as a pick is.

### `views`: surfaces

```wit
/// A layer the platform shows beneath the view's drawing, where the program leaves its picture
/// clear, fed by a producer: a player, a camera, later others.
resource surface {
  /// Where it sits in the view, in the view's pixels; takes effect with the view's next present.
  place: func(rect: rect, scale: f32);
  hide: func();
}

// resource view gains:
//   open-surface: func(opaque: bool) -> surface;
```

The SDK's Inset embedder implements Inset's `Compositor` over `open-surface` and `place`, so an Inset program shows a surface with `SurfaceView`.

### `audio`

```wit
interface audio {
  enum permission-state {
    not-asked,
    granted,
    denied,
  }

  /// Web Audio's AudioParam.
  resource audio-param {
    value: func() -> f32;
    set-value-at-time: func(value: f32, time: f64);
    linear-ramp-to-value-at-time: func(value: f32, time: f64);
    exponential-ramp-to-value-at-time: func(value: f32, time: f64);
    set-target-at-time: func(target: f32, start: f64, time-constant: f64);
    cancel-scheduled-values: func(start: f64);
  }

  /// What every node has, Web Audio's AudioNode; each kind of node hands one out.
  resource audio-node {
    connect: func(destination: borrow<audio-node>);
    disconnect: func();
  }

  resource gain-node {
    node: func() -> audio-node;
    gain: func() -> audio-param;
  }

  resource stereo-panner-node {
    node: func() -> audio-node;
    pan: func() -> audio-param;
  }

  resource biquad-filter-node {
    node: func() -> audio-node;
    frequency: func() -> audio-param;
    q: func() -> audio-param;
    gain: func() -> audio-param;
  }

  resource analyser-node {
    node: func() -> audio-node;
    float-frequency-data: func() -> list<f32>;
  }

  resource audio-buffer {
    channel-data: func(channel: u32) -> list<f32>;
    copy-to-channel: func(samples: list<f32>, channel: u32, start: u32);
  }

  /// Web Audio's AudioBufferSourceNode: plays once, from start to stop.
  resource audio-buffer-source-node {
    node: func() -> audio-node;
    start: func(when: f64, offset: f64, duration: option<f64>);
    stop: func(when: f64);
    playback-rate: func() -> audio-param;
  }

  /// Ours: samples the program pushes ahead. A write waits only while it is a latency ahead.
  resource stream-input-node {
    node: func() -> audio-node;
    write: async func(samples: list<f32>) -> result<_, audio-error>;
  }

  /// Ours: the microphone, as a source in the graph.
  resource microphone-node {
    node: func() -> audio-node;
  }

  resource audio-context {
    current-time: func() -> f64;
    sample-rate: func() -> f32;
    destination: func() -> audio-node;
    create-gain: func() -> gain-node;
    create-stereo-panner: func() -> stereo-panner-node;
    create-biquad-filter: func() -> biquad-filter-node;
    create-analyser: func() -> analyser-node;
    create-buffer: func(channels: u32, length: u32, sample-rate: f32) -> audio-buffer;
    decode-audio-data: async func(bytes: list<u8>) -> result<audio-buffer, audio-error>;
    create-buffer-source: func(buffer: borrow<audio-buffer>) -> audio-buffer-source-node;
    create-stream-input: func(channels: u32) -> stream-input-node;
    permission-to-record: func() -> permission-state;
    /// Asks at first use.
    open-microphone: async func() -> result<microphone-node, audio-error>;
    suspend: async func();
    resume: async func();
  }

  resource audio {
    new-context: func() -> audio-context;
  }

  get-audio: func() -> option<audio>;
}
```

### `player` and `camera`

```wit
interface video {
  resource video-frame {
    time: func() -> instant;
    width: func() -> u32;
    height: func() -> u32;
    texture: func(device: borrow<gpu-device>) -> result<gpu-texture, video-error>;
  }

  /// The newest picture each time; older ones are dropped, so a producer never waits.
  resource frame-reader {
    next-frame: async func() -> result<video-frame, video-error>;
  }
}

interface player {
  resource player {
    /// A path the program can read, or a link.
    constructor(source: string);
    play: func();
    pause: func();
    seek: async func(position: duration) -> result<_, player-error>;
    position: func() -> duration;
    show-on: func(surface: borrow<surface>);
    read-frames: func() -> frame-reader;
    /// Its sound, as an input of the program's audio graph.
    sound: func(context: borrow<audio-context>) -> audio-node;
  }
}

interface camera {
  resource camera {
    show-on: func(surface: borrow<surface>);
    read-frames: func() -> frame-reader;
  }

  resource cameras {
    list: func() -> list<camera-info>;
    permission: func() -> permission-state;
    /// Asks at first use.
    open: async func(id: string, size: size) -> result<camera, camera-error>;
  }

  get-cameras: func() -> option<cameras>;
}
```

### `menus`

```wit
interface menus {
  record menu-entry {
    id: u32,
    parent: option<u32>,
    kind: menu-entry-kind,
  }

  variant menu-entry-kind {
    submenu(string),
    item(menu-item),
    /// About, Hide, Quit…: performed by the system itself.
    provided(provided-menu-item),
    separator,
  }

  record menu-item {
    label: string,
    enabled: bool,
    checked: bool,
    shortcut: option<menu-shortcut>,
  }

  resource menu-bar {
    set-menus: func(entries: list<menu-entry>);
    chosen: func() -> stream<menu-choice>;
  }

  resource menus {
    /// None where the system has no menu bar for the program.
    menu-bar: func() -> option<menu-bar>;
    show-context-menu: async func(view: borrow<view>, x: f64, y: f64, entries: list<menu-entry>) -> result<option<u32>, menu-error>;
  }

  get-menus: func() -> option<menus>;
}
```

### `notifications`

```wit
interface notifications {
  resource notification {
    constructor(tag: string, title: string);
    set-body: func(body: string);
    add-action: func(id: string, label: string);
  }

  record activation {
    tag: string,
    action: option<string>,
  }

  resource notifications {
    permission: func() -> permission-state;
    request-permission: async func() -> permission-state;
    post: async func(notification: notification) -> result<_, notification-error>;
    remove: func(tag: string);
    /// Clicks on its notifications, the one that started the program among them.
    activations: func() -> stream<activation>;
  }

  get-notifications: func() -> option<notifications>;
}
```

Who posts, and whose name shows: on macOS the program's window process, in the program's name, the system asking in its name; on Windows WaOS, under the program's taskbar identity; on Linux WaOS, through the desktop's notification service; on Android WaOS, with a channel per program, and on iOS WaOS, each with the program's name and icon in the content, the system asking once for WaOS and WaOS asking per program.

### `opener`, and `lifecycle`'s open requests

```wit
interface opener {
  variant open-error {
    denied,
    no-app,
    failed(string),
  }

  resource opener {
    open-link: async func(link: string) -> result<_, open-error>;
    open-file: async func(path: string) -> result<_, open-error>;
  }

  get-opener: func() -> option<opener>;
}

// lifecycle gains:
variant open-request {
  files(list<string>),
  links(list<string>),
}
// resource lifecycle: open-requests: func() -> stream<open-request>;
```

The package's manifest declares what opens the program:

```toml
[[opens]]
name = "Markdown document"
extensions = ["md", "markdown"]
media-types = ["text/markdown"]

[[links]]
schemes = ["notes"]
```

## Where each part lives

- **`wit/`, then both SDK backends:** every interface above; the native backend wraps the system directly (`Command` over `std::process`, workers over `std::thread`).
- **`wasdk-gui`, the user's side:** the file dialog, surfaces, sound, players and cameras, menus, notifications, the opener and open requests.
- **`wasdk-io`, the IO side:** the `Network` trait with its two providers; processes reshaped to the command model; object stores as a second kind of storage.
- **`wasdk-host`:** its own `wasi:sockets` host, the HTTP hook, a program as several stores for its workers, the `/granted` folder.
- **WaOS:** the permission questions and the settings list in Show Info; its own file dialog for programs on SSH machines; the window process per program on macOS, with its menu bar and notifications; registering file types at install; the audio engine, players and cameras; surfaces in its compositing.
- **Inset**, for the lead's review before any change: surface positions committed with the host's own frame (its macOS host now applies them on the main thread while wgpu presents apart, inferred); surfaces that hold child surfaces beneath their own buffer, with one scale for the group; links that are not files reaching the app on macOS; a port of Flutter's `PlatformMenuBar`.

## The work, in the first version's order

This replaces the order in `2026-10-16-waos-v1.md`, folding the interface in.

1. Signed packages, updates and package sources, with the settings list in Show Info.
2. The IO side: networking (the sockets host, both providers, `trust`, the HTTP hook, the question at first use) and `processes` reshaped, which share the providers.
3. Workers: the experiment first, then a program as several stores.
4. Android.
5. iOS developer builds.
6. Apps of their own: the macOS window process, and Android's task per program.
7. Grants made while a program runs: the file picker, drops, `/granted`, WaOS's dialog for SSH machines.
8. Media: Inset's surface changes first, then surfaces, the audio graph and its inputs, the microphone, players and cameras.
9. Menus, notifications, opening and being opened.
10. Object stores, a second kind of storage.
11. Windows.
12. Linux.

## Experiments, each before its step

- **Networking:** whether WaOS.app opened from the Finder on macOS 15 or later is asked about the local network when it connects over SSH to a machine on the LAN; the effect of russh widening its window as the program reads; TLS from a component against a real server; reqwest 0.13 on `wasm32-wasip2`; what a host-side wait in a synchronous WASI 0.3 call (`listen` over SSH) does to the program's other tasks.
- **Workers:** a worker parses a 0.63 MB file with tree-sitter while gallery draws; frame times, and the cost of sending results back.
- **Files:** a folder whose contents grow while the program runs, opened 1,000 times; the cost of a storage over Android's content links; whether iOS's export dialog answers a save path before the program writes; macOS's open dialog shown from a program's window process.
- **Media:** how far ahead a program must write, which sets the default latency; whether WaOS can drain a program's stream while the program is in a long wasm call; what each chunk of samples costs to cross into WaOS; what Android's YUV-to-RGBA conversion costs per picture; what mixing costs under Pulley; whether macOS keeps a camera or microphone grant for a window process bundle WaOS signs on the user's Mac.
- **Surfaces:** on macOS, a clear hole scrolled over a child layer, counting the refreshes where the background shows, with positions applied with the frame and apart from it; the same for Inset's own drawing and surfaces; a Core Animation transaction committed off the main thread on iOS.
- **The app shell:** whether one WaOS process on Linux can post notifications under several programs' names; whether Windows notifications posted under each program's taskbar identity show its name and icon, with one activator for every click; whether frames keep reaching a window process while its menu is open; whether a menu choice can overtake a key typed just before it.

## Found along the way

Listed in `DESIGN.md`'s known gaps until fixed: WaOS installs any file dropped on any of its windows, a program's windows included; WaOS.app declares no reason for the local network (`NSLocalNetworkUsageDescription`); local storage resolves a path one folder at a time, which is slow (`2026-10-16-zed-on-waos.md`).

## Sources

The research and its sources, with every claim marked read, ran or inferred: `../experiments/2026-10-17-interface-research/` (`reports/networking.md`, `files.md`, `app-shell.md`, `media.md`, `surface-placement.md`, `threads.md`), and the earlier `../experiments/2026-09-24-capability-survey` and `2026-09-24-capability-apis`.
