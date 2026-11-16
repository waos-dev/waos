# The inspector, programs with roles, and the interface's compatibility

Written 2026-11-15, for the lead to decide. It records the eight decisions the lead has taken, then designs what follows from them: one layer where every guarded use is decided and told, a summary kept always and a trace sent only while someone listens, an inspection protocol with the inspector, `cargo waos inspect` and remote inspection on top of it, programs with roles, and the promise WaOS makes to programs built against an older interface. What was planned on this date, not what is.

## The problem

Today nobody can see what a program does with what it was allowed. A user who turned the network on for a reader cannot tell which sites it reached this week; a user who gave a program a folder cannot tell which files it changed; a developer whose request fails cannot tell whether the server refused it or WaOS did, and by which setting. Every capability already checks its grant in the engine, each in its own way and silently, so the facts exist for an instant and are gone.

And once WaOS is released, programs built today must keep running on the WaOS of next year, while the interface keeps growing. Nothing says yet what may change, how a new interface is tried before it is promised, or what happens to a program when something must break.

## The lead's decisions

1. **WaOS has an inspector, as Chrome has DevTools, for two audiences**: developers, to see why a program behaves as it does, and users auditing, to see what a program actually does with what it was allowed (the files it touches, its connections, the camera and the microphone, machines, the clipboard). On desktops and on phones. A program pays nothing for it unless someone is inspecting it.
2. **One access layer at the capability boundary.** Every capability call already crosses the engine; one layer there both checks the grant and emits a structured event. This is a refactor of how the capabilities check permissions today.
3. **Two levels.** An audit summary, always on and cheap: per program and per capability, counts, first and last use and similar small facts, kept for 7 days on the device only. And a detailed live trace, sent only while an inspector is attached.
4. **The protocol comes first**, as Chrome's DevTools Protocol does, with UIs on top: the inspector app, a view on the phone, and `cargo waos inspect …`, so developers and coding agents can read it. Inspecting a phone's WaOS from the desktop follows from it.
5. **The inspector is itself a WaOS program, built with Waft**, which verifies WaOS's architecture: if the inspector needs something no program can have, that is found by building it.
6. **Programs with special roles** (the inspector now; later a desktop shell or launcher that manages other apps): each role's privileged API is a WIT package of its own (`wasdk:inspect`, later `wasdk:shell`); a program declares the role in its package; WaOS starts it only once the user granted that role, an extension of the permission system. **No special signing rule for any role**: anyone can build a shell.
7. **The interface's compatibility policy.** `wasdk:platform@0.1.0` is frozen at the first release. Later 0.1.x versions only add, each addition marked `@since(version = …)`. A new API starts behind `@unstable(feature = …)`, which only development builds or programs that opted in import. A break is a new major version of the package, served beside the old one for a deprecation period, as WaOS served WASI 0.2 beside 0.3. CI enforces it: at each release a set of real programs built against that release's WIT is kept, and run against the current engine.
8. **The inspector's semantics tree comes later**, after accessibility: Waft's semantics tree and AccessKit first.

### The lead's decisions for the inspector app, 2026-11-16

Taken after the audit summary, the live trace and `cargo waos inspect` were built. They change parts of the design below, noted where.

9. **A program's roles are what its component imports**, for an installed `.wapk` and a development run alike: a component that imports `wasdk:inspect` asks for the inspector role. The package declares no `roles` and `cargo waos run` sends none; one rule, read by the engine from the component before it links. This replaces "a program declares the role in its package" in decision 6 and the manifest field under Roles.
10. **Icons load lazily**: `device` gains `icon: async func(id: string) -> option<list<u8>>`, a PNG, asked for as a row is shown, so the list stays small as the number of programs grows.
11. **What a program has now is a stream**, a snapshot and then each change, not a query read again. Open files become entries of the same registry as the uses that last (a recording, a run, a request, a watch), each with a guard that ends its entry when it drops, so one mechanism adds and ends both. A listener that falls behind is sent a fresh snapshot instead of the changes it missed, so it never shows a file open that was closed; the program never waits for it. It must stay cheap for a program that opens thousands of files, such as an editor indexing a project; if it does not, that is raised.
12. **An inspector sees every program**: installed ones, development runs, and itself.
13. **The record of what programs do follows the permission model**: a global default, on, with an override on each program's page. Off for a program stops its record and clears what was kept of it; the live trace and what it has now still work, since they keep nothing.
14. **The inspector's program page is layered** (2026-11-16): its first tab is the report, a summary of what the program did, as iOS's App Privacy Report and Android's Privacy dashboard show it; the other tabs are the detail, live panels as Chrome DevTools has them (network, files, devices, output, what it uses now), reached from the report. An auditor stops at the report; a developer goes on to the panels.
15. **Each event says why** where that is easy to build and keep organized: the setting that allowed or refused it, with the way to change it.
16. **The widgets the panels need are ported to Waft**, not worked around: WinUI's table and Cupertino's `CupertinoSearchTextField`.

## The design

### What a user and a developer get

A user opens the inspector, or a program's page in WaOS's settings, and sees for the last seven days what each program did: the domains it reached, the folders it read and the files it changed, when it had the camera or the microphone and for how long, what it read from the clipboard without a paste, the commands it ran on a machine, the links it opened and the questions it asked. Each fact sits beside the setting that allowed it, so the user can turn it off from there.

A developer opens the inspector on a running program and watches it live: its output, every lookup, connection and request with what WaOS decided and why ("refused by `local-network`"), every file it opened, its clipboard and devices, its memory, CPU and frames. The same stream reaches a terminal or a coding agent through `cargo waos inspect`, as JSON lines.

### The access layer

**What it is.** One type per program, `ProgramAccess`, through which every guarded use passes: a name looked up, a connection, a request, a file opened or changed, the clipboard read or written, a camera or the microphone opened, a command run, a link opened, a notification posted, a question put. It does three things at once for each use: runs the capability's rule against the user's grants, counts the use and its verdict in the program's audit summary, and, only if an inspector listens to that capability, sends an event. A capability no longer holds the grants; it holds the program's access.

**What it is not.** It does not hold the rules. The network's domain rule and address record stay in `network`, the mount table in `files`, the paste rule in `clipboard`: the layer is handed the rule as a closure and runs it. So the layer stays small, and each rule stays where the code that knows it lives.

**Where it sits today, and what moves.** The grants are made once per store in `crates/waos/src/running/store.rs` (`store_data`, lines 256–320) and handed as `Arc<dyn ProgramGrants>` to ten capability states, each reading them its own way:

| Capability | Where it reads the grants today | What becomes a decided use |
|---|---|---|
| Name lookup | `network/mod.rs` `plan_lookup` and `plan_system_lookup` (107–138), served by `network/host/p3.rs` `resolve_addresses` | `Lookup { name }` |
| TCP and UDP | `network/mod.rs` `allows_socket` and `may_reach` (184–203), called by `wasmtime-wasi`'s `socket_addr_check`, set in `wasi/mod.rs` (65–70) | `Connect { address, protocol }`, `Listen { port }`; datagrams counted only |
| HTTP | `network/http.rs` `send_request` (174) and `http/route.rs` `route`, through `ProgramNetwork::reachable` | `Request { method, uri }`, its response told after |
| A player's URL | `media_sources.rs` (65), `network.reachable` | `Request` |
| Files | `files/mounts.rs` `LiveMounts::now` (411–438), the mount table resolving each path; `files/p3/host.rs` `target_of` (27), `open_at` (306) and the calls that change (234, 330, 343, 359, 373); a machine's root unconfined, `files/mod.rs` `folder_of_machine` (71) | `Open { path, target, mode }`, `Change { path, target, change }` |
| Clipboard | `clipboard/mod.rs` `read` and the write check (243–264) | `ClipboardRead { forms, pasted }`, `ClipboardWrite { forms }` |
| Camera | `cameras/mod.rs` `in_use` and `open` (107–186), asked through `cameras/host.rs` (100) | `OpenDevice { device: Camera, … }`, held until closed |
| Microphone | `audio/mod.rs` (115, 194, 210) | `OpenDevice { device: Microphone, … }`, held until closed |
| Machines | `machines/mod.rs` `granted` and `pick` (87–116); `machines/host.rs` `exec` and `exec_in_terminal` (123–160), unchecked since a machine is all or nothing | `Run { machine, command, in_terminal }`, held until it ends |
| Links | `opener/mod.rs` (65–78) | `OpenLink { url }` |
| Notifications | `notifications/host.rs` (64) | `Post` |
| Questions | `permissions/mod.rs` (`request`, `answer`, `state_of`, 87–230), `files/picks.rs` (21–49), `audio/outputs.rs` (62) | told, not decided: asked, answered, a settings page opened, a pick kept |

`recordings.rs`, which tells the shell's indicator that a program records, stays as it is: it is the indicator's. The layer's held use counts the same session's time for the summary.

**The refactor.** `ProgramAccess` is made once per program in `ProgramInstances::link` (`running/instances.rs`, 64), since the summary is the program's across its main instance and its workers; each store gets a copy that carries its instance's number. Each capability takes `ProgramAccess` where it took `Arc<dyn ProgramGrants>`, and its checks move into `decide`'s rule unchanged. The tests of each capability keep passing, since what is decided does not change; one new test per capability checks that each refusal path is told. The trait `ProgramGrants` in `waos-capabilities` stays as it is: it is what the user set, and the layer is how the engine uses it.

```rust
/// Every guarded use a program makes, decided and told: the one place a capability asks whether
/// the program may, and the one place that says it did.
///
/// The invariant: a grant read that lets or refuses a use is made inside `decide`'s rule, never
/// beside it, so no use is decided without being counted.
#[derive(Clone)]
pub(crate) struct ProgramAccess {
    parts: Arc<AccessParts>,
    /// The instance this copy is in: the main one, 0, or a worker, from 1 as they started.
    instance: u32,
}

struct AccessParts {
    grants: Arc<dyn ProgramGrants>,
    /// Today's counts, kept by the environment's `AuditLog`.
    summary: ProgramSummary,
    /// The live trace's listeners: none while nobody inspects the program.
    taps: Taps,
}

/// One guarded use, as the capability names it. Borrowed: nothing is copied unless an inspector
/// listens.
pub(crate) enum Use<'a> {
    Lookup { name: &'a str },
    Connect { address: SocketAddr, protocol: Protocol },
    Listen { port: u16 },
    Request { method: &'a str, uri: &'a Uri },
    Open { path: &'a str, target: &'a Target, mode: OpenMode },
    Change { path: &'a str, target: &'a Target, change: FileChange<'a> },
    ClipboardRead { forms: &'a [String], pasted: bool },
    ClipboardWrite { forms: &'a [String] },
    OpenDevice { device: Device, name: &'a str, simulated: bool },
    Run { machine: &'a str, command: &'a Command, in_terminal: bool },
    OpenLink { url: &'a Url },
    Post,
}

/// What refused a use: the setting, by the key `cargo waos settings` names its row with
/// (`network`, `local-network`, `clipboard-read`, `camera`…), or `domains` and `folders`, the rules
/// beneath the switches.
pub(crate) struct Refusal {
    pub(crate) setting: &'static str,
}

impl ProgramAccess {
    /// Decides `used` by `rule`, which reads the grants, and tells it: counted in the summary,
    /// and sent to the trace when an inspector listens to its capability.
    pub(crate) fn decide<R>(
        &self,
        used: Use<'_>,
        rule: impl FnOnce(&dyn ProgramGrants) -> Result<R, Refusal>,
    ) -> Result<R, Refusal>;

    /// As `decide`, for a use that lasts (a camera, the microphone, a command): its end is told
    /// when the answer is dropped.
    pub(crate) fn hold<R>(
        &self,
        used: Use<'_>,
        rule: impl FnOnce(&dyn ProgramGrants) -> Result<R, Refusal>,
    ) -> Result<(R, Held), Refusal>;

    /// Tells what happened that decides nothing: a question asked or answered, a pick kept, a
    /// response's status, a lifecycle change.
    pub(crate) fn tell(&self, happened: Happened<'_>);

    /// The grants, for what is no use: a permission's state, the answer set for a picker.
    pub(crate) fn grants(&self) -> &dyn ProgramGrants;
}
```

### Events

Each capability has its own kinds of event, so a listener hears only the capabilities it asked for, and a capability added later is a new stream rather than a change to an old one:

| Capability | Events in the trace | Kept in the summary |
|---|---|---|
| Network | a lookup (name, addresses answered); a connection (address, port, TCP or UDP, the names the address was looked up by); listening (port); a request (an id, method, URL, through the proxy or not) and its response (status, body bytes, time) | per domain: uses, refusals, first and last; the local network per address; datagrams as one count |
| Files | an open (the path as the program named it, where it is on the machine, which mount, read or write); a change (create a folder, remove, rename, link, symlink, set times) | per mount: opened to read, opened to write, created, removed, renamed; the paths changed in the user's folders and picks, up to 100 a day |
| Clipboard | a read (forms, by the user's paste or not); a write (forms). Never what was read or written | reads without a paste, pastes, writes, each with refusals, first and last |
| Camera, microphone | asked and answered; opened (name, simulated); closed (how long); a photo | each session's start and length, simulated or not |
| Machines | a pick; a run (an id, the machine, the program, its arguments, in a terminal or not); how it ended | per machine and program run: runs, first and last; never arguments |
| Links, notifications | a link opened (scheme, host); a notification posted | per scheme and host; posts |
| Permissions | a question asked, its answer, a switch's page opened, a setting the user changed, a folder or file mounted by a pick or a paste | each question asked, when, and its answer |
| Lifecycle | started, ended and how, frozen, thawed, in sight or not, a worker started or ended, a trap | not kept |
| Output | what the program writes to its standard output and error, as it writes it | not kept |
| Resources | memory, fuel, frames presented, hardware buffer bytes, workers, sampled (`stats.rs`'s `Stats`, read by the inspector at the rate it asks) | not kept |

Every event names the instance it came from, so a worker's requests are told apart from the main instance's.

### The audit summary

**What it keeps.** For each program, one record per day, of the facts in the table above: counts, refusals, first and last use, and short lists (domains, mounts, changed paths, sessions, commands, links). A list is capped (256 domains, 100 changed paths, 100 commands, 100 links, 100 sessions a day), and what falls beyond a cap is one count, "and 40 more", so a program that touches a million files costs the same as one that touches a hundred.

**Its size.** A typical program's day: about 30 domains at some 40 bytes each, five mounts at 40, a few sessions at 16, a dozen commands at 40 and the counters, about 2 KB. Its worst case under the caps is about 30 KB. Thirty programs over seven days: about 0.4 MB typical, 6 MB at the worst.

**Where it lives.** The engine counts; the app keeps. The engine writes nothing outside itself (its README), so the counts are handed to a trait the store implements, beside `ProgramGrants`, about once a minute and at the program's end; a crash loses up to a minute. The store writes each program's day as one postcard file, `<store>/audit/<program id>/<date>.postcard`, removes days older than seven, and removes all of them with the program, as a removed program is forgotten whole. Nothing leaves the device unless an inspector the user granted reads it.

```rust
// waos-capabilities

/// Where a program's audit summary is kept: the store, for the WaOS app.
pub trait AuditLog: Send + Sync {
    /// Keeps `day`, `program`'s counts for one day so far, in place of what was kept for that day.
    fn keep(&self, program: &str, day: &AuditDay);
    /// The days kept of `program`, newest first, seven at most.
    fn days(&self, program: &str) -> Vec<AuditDay>;
}

/// One program's guarded uses over one day, from midnight UTC.
pub struct AuditDay {
    pub start: SystemTime,
    pub domains: Vec<Counted<String>>,
    pub local_addresses: Vec<Counted<IpAddr>>,
    pub datagrams: u64,
    pub folders: Vec<FolderUses>,
    pub clipboard_reads: Uses,
    pub pastes: u32,
    pub clipboard_writes: Uses,
    pub camera: Vec<Session>,
    pub microphone: Vec<Session>,
    pub commands: Vec<Counted<RunCommand>>,
    pub links: Vec<Counted<String>>,
    pub notifications: Uses,
    pub questions: Vec<Asked>,
    /// What fell beyond the lists' caps, by list.
    pub beyond_caps: Overflow,
}

/// How often a use happened, how often it was refused, and when it first and last happened.
pub struct Uses {
    pub allowed: u32,
    pub refused: u32,
    pub first: SystemTime,
    pub last: SystemTime,
}
```

`ProgramEnvironment` gains no field for it: the summary is every program's, so the engine is given one `AuditLog` when it is made, as it is given its shell.

**What it costs.** A use is counted under the program's own lock, one uncontended lock and a few additions, beside a check that already reads the grants and, for a lookup, a connection, a request or an open, asks the system for something far slower. Reads and writes through a handle already open are not counted: a handle is checked once, when it is made (the permissions proposal's research), and the summary follows the checks. Step 1 measures this on the hottest guarded path before it is promised.

### The live trace

**It costs nothing while nobody listens.** Each program's `Taps` holds one atomic word, a bit per capability. A use reads that word, once, with a relaxed load; while its capability's bit is clear, no event is made, nothing is copied, and the use goes on as it does today. A listener sets the bit as it subscribes, and the last listener of a capability clears it as it drops its stream.

**While someone listens, the program is never held up.** The event is made from the borrowed `Use` and sent into a bounded channel, 4096 events, without waiting. When the listener falls behind, events are dropped and the next one sent says how many were (`dropped-before`), as a tracing ring buffer does. A program is never slowed by a slow inspector, only seen less fully.

**Only from when it was opened.** As in DevTools' Network panel, the trace shows what happens after a listener attaches. The inspector offers "Start inspected": subscribe, then start the program, so its first lookups are seen.

**Calls, later.** The engine's bindings already make a `tracing` span for every `wasdk` call, with its arguments (`bindings.rs`, `default: tracing`), at the trace level, where a disabled callsite costs one cached check. A later "Calls" panel, a program's calls as `strace` shows a process's, can be a `tracing` layer filtered to one program, with no change to the access layer.

### The inspection protocol

**One protocol, written in WIT.** The protocol is the WIT package `wasdk:inspect`: what an inspector can ask for, and the events it hears. The inspector program imports it as any program imports `wasdk:platform`; `cargo waos inspect` and a desktop WaOS inspecting a phone speak the same values over the dev connection. WIT stays the contract, as the project's rule says, and there is no second schema to drift from it.

**Its shape, as CDP's.** Chrome's DevTools Protocol has targets (pages, workers), a session attached to a target, and domains (Network, Runtime…) whose events flow after `Network.enable` and stop after `disable`. Here a device is a target holding programs; asking for a capability's stream on a program is `enable`; dropping the stream is `disable`; and closing the connection drops them all, so "attached" means exactly "holding a stream". What differs from CDP:

- **Typed, not free JSON.** CDP grows by adding JSON fields old clients ignore. WIT has no such tolerance (a field or case added to a type is a new type), so `wasdk:inspect` grows by new functions and new streams, one per capability, never by new fields, as the compatibility policy below says of every package.
- **Read-only.** CDP drives a page (evaluate, navigate, emulate). The inspector watches; what changes a program stays where it is: the user's settings, and `cargo waos`'s input for a program a run runs (decisions 60 and 64 of `2026-10-17-the-waos-project.md`).
- **The summary is a query, not a session.** It reads seven days of a program that may not be running, with no attachment.
- **Guarded by a role, not a port.** Chrome's remote debugging port is open to any local process, which Chrome 136 narrowed by refusing it for the default profile. Here the program side is a role the user grants (below), and the dev connection is the user's own socket, as today.
- **Subscriptions per capability**, as WebDriver BiDi's `session.subscribe` names the events it wants, rather than CDP's whole-domain switches.

```wit
package wasdk:inspect@0.1.0;

/// The devices whose WaOS can be inspected, and the programs on each: this device, and each
/// phone or computer WaOS reaches now, a phone over USB first.
interface devices {
  /// A program on a device, installed or run in development.
  record program {
    id: string,
    name: string,
    /// Its package's version; none for a development run.
    version: option<string>,
    running: bool,
    /// The roles its package declares, and whether the user granted each.
    roles: list<tuple<string, bool>>,
  }

  variant program-change {
    added(program),
    removed(string),
    started(string),
    ended(string),
  }

  resource device {
    /// Its name, as its user knows it.
    name: func() -> string;
    /// Whether it is the device this program runs on.
    is-this-device: func() -> bool;
    programs: async func() -> list<program>;
    /// Each change from now on, on a stream of its own.
    program-changes: func() -> stream<program-change>;
  }

  variant device-change {
    reached(device),
    lost(string),
  }

  /// This device first, then each device reached now.
  devices: func() -> list<device>;
  device-changes: func() -> stream<device-change>;
}

/// What each program did with what it was allowed, over the last seven days, as the device keeps it.
interface audit {
  use wasi:clocks/system-clock@0.3.0.{instant};
  use devices.{device};

  /// How often a use happened, how often it was refused, and when it first and last happened.
  record uses {
    allowed: u32,
    refused: u32,
    first: instant,
    last: instant,
  }

  record counted {
    /// What was used: a domain, an address, a scheme and host, a machine and program.
    name: string,
    uses: uses,
  }

  enum mount-kind { own-data, own-cache, granted, picked, pasted, machine }

  record folder {
    /// Where it is on the machine.
    path: string,
    kind: mount-kind,
    opened-to-read: u32,
    opened-to-write: u32,
    created: u32,
    removed: u32,
    renamed: u32,
    /// The files changed in it, up to 100 a day; none for the program's own folders.
    changed: list<string>,
  }

  record session {
    start: instant,
    seconds: u32,
    simulated: bool,
  }

  record asked {
    /// The question, by its row's key: `camera`, `microphone`, `notifications`, a link scheme's.
    question: string,
    at: instant,
    /// The answer, by its key; none when nobody answered.
    answer: option<string>,
  }

  /// One day, from midnight UTC.
  record day {
    start: instant,
    domains: list<counted>,
    local-addresses: list<counted>,
    datagrams: u64,
    folders: list<folder>,
    clipboard-reads: uses,
    pastes: u32,
    clipboard-writes: uses,
    camera: list<session>,
    microphone: list<session>,
    commands: list<counted>,
    links: list<counted>,
    notifications: uses,
    questions: list<asked>,
    /// What fell beyond the lists' caps, all lists together.
    beyond-caps: u32,
  }

  /// The days kept of the program `id` names, newest first, seven at most; empty for one the
  /// device does not know.
  days: async func(device: borrow<device>, id: string) -> list<day>;
}

/// What a program does, live, one stream per capability. Holding a stream is being attached:
/// events flow from when it is asked for, and stop when it is dropped.
interface trace {
  use wasi:clocks/system-clock@0.3.0.{instant};
  use wasi:clocks/monotonic-clock@0.3.0.{duration};
  use devices.{device};
  use audit.{mount-kind};

  /// What WaOS decided: allowed, or refused by the setting the key names (`network`,
  /// `local-network`, `clipboard-read`, `camera`…, or `domains` and `folders`, the rules beneath).
  variant verdict {
    allowed,
    refused(string),
  }

  /// Which instance of the program: its main one, 0, or a worker, from 1 as they started.
  type instance = u32;

  record lookup { name: string, addresses: list<string> }
  enum protocol { tcp, udp }
  record connection { address: string, port: u16, protocol: protocol, names: list<string> }
  record request { id: u64, method: string, url: string, proxied: bool }
  record response { request: u64, status: u16, body-bytes: u64, took: duration }

  variant network-use {
    lookup(lookup),
    connect(connection),
    listen(u16),
    request(request),
    response(response),
  }

  record network-event {
    at: instant,
    instance: instance,
    /// Events dropped before this one because the reader fell behind.
    dropped-before: u32,
    kind: network-use,
    verdict: verdict,
  }

  variant file-use {
    open-to-read,
    open-to-write,
    create-directory,
    remove,
    remove-directory,
    /// Renamed to this path, as the program named it.
    rename(string),
    link(string),
    symlink(string),
    set-times,
  }

  record file-event {
    at: instant,
    instance: instance,
    dropped-before: u32,
    /// The path as the program named it.
    path: string,
    /// Where it is on the machine; none where it led nowhere.
    on-machine: option<string>,
    mount: option<mount-kind>,
    kind: file-use,
    verdict: verdict,
  }

  variant clipboard-use {
    /// The forms read, and whether by the user's paste.
    read(tuple<list<string>, bool>),
    write(list<string>),
  }

  record clipboard-event { at: instant, instance: instance, dropped-before: u32, kind: clipboard-use, verdict: verdict }

  variant device-use {
    asked(option<string>),
    /// The device's name, and whether it is WaOS's simulated one.
    opened(tuple<string, bool>),
    closed(duration),
    photo,
  }

  record device-event { at: instant, instance: instance, dropped-before: u32, kind: device-use, verdict: verdict }

  record run { id: u64, machine: string, program: string, arguments: list<string>, in-terminal: bool }

  variant machine-use {
    picked(string),
    run(run),
    /// A run's id, and how it ended, as `machines` says it.
    ended(tuple<u64, string>),
  }

  record machine-event { at: instant, instance: instance, dropped-before: u32, kind: machine-use, verdict: verdict }

  variant permission-use {
    /// A question put to the user, by its row's key.
    asked(string),
    /// Its answer, by key.
    answered(tuple<string, string>),
    /// A switch's page opened for the user.
    settings-opened(string),
    /// A row the user changed, and its value now.
    changed(tuple<string, string>),
    /// A file or folder mounted by a pick or a paste, where it is on the machine.
    mounted(string),
  }

  record permission-event { at: instant, instance: instance, dropped-before: u32, kind: permission-use }

  variant lifecycle-use {
    started,
    ended(string),
    frozen,
    thawed,
    in-sight(bool),
    worker-started(instance),
    worker-ended(instance),
    trapped(string),
  }

  record lifecycle-event { at: instant, dropped-before: u32, kind: lifecycle-use }

  enum output-stream { stdout, stderr }
  record output-chunk { at: instant, instance: instance, dropped-before: u32, channel: output-stream, bytes: list<u8> }

  record usage {
    linear-memory-bytes: u64,
    fuel-consumed: u64,
    frames-presented: u64,
    hardware-buffer-bytes: u64,
    workers: u32,
  }

  /// Each capability's events of the program `id` names, from now on, also across its restarts;
  /// none for a program the device does not know.
  network: func(device: borrow<device>, id: string) -> option<stream<network-event>>;
  files: func(device: borrow<device>, id: string) -> option<stream<file-event>>;
  clipboard: func(device: borrow<device>, id: string) -> option<stream<clipboard-event>>;
  camera: func(device: borrow<device>, id: string) -> option<stream<device-event>>;
  microphone: func(device: borrow<device>, id: string) -> option<stream<device-event>>;
  machines: func(device: borrow<device>, id: string) -> option<stream<machine-event>>;
  permissions: func(device: borrow<device>, id: string) -> option<stream<permission-event>>;
  lifecycle: func(device: borrow<device>, id: string) -> option<stream<lifecycle-event>>;
  output: func(device: borrow<device>, id: string) -> option<stream<output-chunk>>;
  /// The program's resources, sampled every `every` while it runs.
  resources: func(device: borrow<device>, id: string, every: duration) -> option<stream<usage>>;
}
```

Links and notifications are two more streams of the same shape, left out of this sketch to keep it short. The sketch parses: `wasm-tools component wit` reads it with the repository's `wit/` and the world below.

**On this device.** The engine serves `wasdk:inspect` to an inspector program from its own taps, its `AuditLog` and its list of programs. For the app, the engine's public API gains the same, as Rust: `WaOS::subscribe(id, capability) -> Option<Trace>` and `WaOS::audit_days(id)`, which the dev connection serves.

**Over the dev connection.** `cargo waos` and WaOS already speak `waos-dev-protocol`: length-prefixed postcard frames over the user's own socket, agreeing on a protocol number first. An inspection is one more kind of connection, as a run is: `ToWaOS::Inspect` opens it, and the connection is the inspection, so closing it drops every stream. Inside it, requests carry an id, as CDP's do, so one connection holds several streams:

```rust
// waos-dev-protocol, added at the end, the protocol's number raised.
pub enum ToWaOS {
    // …
    /// Opens an inspection; the connection is the inspection from now on.
    Inspect,
    /// A request inside an inspection, answered `Inspected` under the same id.
    InspectRequest { id: u64, request: InspectRequest },
}

pub enum InspectRequest {
    Programs,
    AuditDays { program: ProgramId },
    /// A capability's events of a program, sent as `InspectEvent`s under this request's id until
    /// `Stop` names it or the connection closes.
    Subscribe { program: ProgramId, capability: Capability },
    Stop { subscription: u64 },
}

pub enum FromWaOS {
    // …
    /// The answer to a request, as JSON of `wasdk:inspect`'s values.
    Inspected { id: u64, json: String },
    /// One event of a subscription, as JSON of its `wasdk:inspect` record.
    InspectEvent { subscription: u64, json: String },
}
```

The values cross as JSON of the WIT records, made by serde from the engine's generated types (a `bindgen!` of the `wasdk:inspect` interfaces with `additional_derives: [serde::Serialize, serde::Deserialize]`, which works there because no record of the package holds a resource). So `cargo waos` needs no types of its own: it prints each value as a line. The JSON's names are serde's (fields in snake case, cases by their Rust names); see the open questions.

**`cargo waos inspect`.** For the program of the package in the current folder, or `--program <id>`; on this device, or `--device <name>`:

```text
cargo waos inspect programs                       # the programs, as JSON lines
cargo waos inspect summary [--days 7]             # the audit summary
cargo waos inspect network files clipboard …      # those capabilities' events, until Ctrl-C
cargo waos inspect all                            # every capability's
```

It reaches what `cargo waos`'s other commands reach: the programs a `cargo waos run` on this machine runs or has run (decision 60 of `2026-10-17-the-waos-project.md`, an installed one later with the user's consent per agent).

**A phone from the desktop.** The phone's WaOS listens on the same protocol on a socket only USB debugging reaches: on Android an abstract socket, `waos-dev`, which accepts only adb's shell user and WaOS's own, checked by the peer's uid, as Chrome's `chrome_devtools_remote` does. On the desktop, adb forwards it to a socket in the user's folder (`adb forward localfilesystem:…`, to be checked against adb when built; a loopback port where it is not taken), so decision 30's rule, the system's file permissions decide who connects, still holds. The desktop WaOS lists the phone as a device, which the inspector program shows beside this one; `cargo waos inspect --device` uses the same forward. Pairing over the network, which iPhone needs, comes with `cargo waos run` on phones over the network (decision 30), on the same messages.

### Roles

**The effect.** Most programs see only themselves. A program with a role sees or does more: an inspector sees every program's activity; a shell, later, lists, starts and arranges other programs. The user gives that power to a program by name, once, as they give the camera, and can take it back.

**A role is a WIT package.** `wasdk:inspect` now, `wasdk:shell` later. A program that has the role imports the package; one that does not, never sees it. The program world gains worlds that include them, so wasdk generates their bindings:

```wit
package wasdk:program@0.1.0;

/// A program with the inspector role: everything a program has, and `wasdk:inspect`.
world inspector-base {
  include program-base;
  import wasdk:inspect/devices@0.1.0;
  import wasdk:inspect/audit@0.1.0;
  import wasdk:inspect/trace@0.1.0;
}
```

In the SDK, one `generate!` of a world holding every role package, since a built component imports only the functions it calls (`wit-component` 0.262, `encoding.rs` 570–600): a program that never calls `wasdk::inspect` imports nothing of it. The module `wasdk::inspect` is shown only behind wasdk's Cargo feature `inspector`, so an ordinary program, and an agent writing one, never finds it.

**A program declares it in its package**:

```toml
[package.metadata.waos]
id = "dev.waos.inspector"
name = "Inspector"
icon = "icon.png"
roles = ["inspector"]
```

`cargo waos package` writes `roles` into `manifest.toml`; `Manifest` gains `roles: Vec<Role>`. A component that imports a role's package without declaring it is refused at start, with a message naming the role; a declared role it does not import is harmless. The declaration is what the library reads before installing, to say "Inspector asks to see what your other programs do".

**The user grants it.** At the program's first start, WaOS asks, in words that say what the role sees: "Inspector wants to see what your other programs do: the sites they reach, the files they open and change, when they use the camera and the microphone, what they run. Allow?" The answer is a row in the program's settings, `role.inspector`, allow or don't allow, which `ProgramGrants` gains (`role(&self, role: Role) -> Option<Answer>`). It has no global default: every program is asked for itself. Turning it off ends the role's streams at once. `cargo waos set` sets the row as the user would, which is the user's own socket, as every other row.

**No signing rule.** A role is the user's grant, not WaOS's blessing: any publisher's inspector or shell is asked the same, WaOS's own included. The package's signature still says who published it.

**How the engine links it.** The engine builds a `Linker` for each run (`running/store.rs`, `linker`, called from `run_in_store`). It reads the roles the program holds, declared and granted, and adds a role's interfaces only then:

```rust
/// Every interface the program may import: every program's, and each role's it holds.
fn linker(engine: &Engine, held: &HeldRoles, unstable: &UnstableFeatures) -> wasmtime::Result<Linker<StoreData>> {
    // … every program's, as today, with `unstable`'s `LinkOptions`
    if held.has(Role::Inspector) {
        crate::inspect::host::add_to_linker(&mut linker)?;
    }
    Ok(linker)
}
```

Before linking, the engine reads the component's imports (`Component::component_type`) and checks each role package it imports against what the program holds, so a program missing a grant is told "not granted the inspector role" rather than wasmtime's "missing import". The engine's own `bindgen!` takes a world holding every role package, so the host traits exist; what is linked is decided per program.

### Compatibility

**The promise.** A program built against a released WaOS keeps running on every later WaOS, untouched, for as long as the interface it was built against is served, which is at least the deprecation period after it is replaced.

**What each piece does today, exactly** (wasmtime 50.0.1, wit-bindgen 0.63.0, wit-parser 0.262.0, wit-component 0.262.0, wasm-tools 1.258.0, read from their sources in the cargo registry):

- **Where gates go.** `@since(version = …)`, `@unstable(feature = …)` and `@deprecated(version = …)` go on a package's worlds, a world's imports, exports and includes, interfaces, `use`s, types (resources among them) and functions and methods. Not on a record's field, a variant's or an enum's case or a flag: those carry no attributes in `wit-parser`'s grammar (`ast.rs`, `Field`, `Case`, `EnumCase`, `Flag`). So "adding" in 0.1.x means new interfaces, functions, methods and types, never a field or a case added to a type that exists.
- **`@since` is checked, never acted on.** `wit-parser` refuses a `@since` naming a version later than the package's ("feature gate cannot reference unreleased version"), and a gate with a version in an unversioned package; it filters nothing, and neither bindings generator emits anything for it. It is documentation that the parser keeps honest.
- **`@unstable` filters.** An item whose feature is not enabled is left out of the `Resolve` (`Resolve::features`, `all_features`). wit-bindgen's `generate!` takes `features: [...]`; `wasm-tools component wit` takes `--features` and `--all-features`. wasmtime's `bindgen!` parses with every feature on, and generates a `LinkOptions` with a setter per feature, which `add_to_linker` takes, adding a gated item only when its feature is set (`wasmtime-internal-wit-bindgen` 50.0.1, `LinkOptionsBuilder` and `FeatureGate`). So a host decides per `Linker`, and WaOS, which builds one per run, decides per program.
- **`@deprecated` reaches nobody yet.** It is parsed and kept in the item's `Stability`, but neither wit-bindgen's Rust generator nor wasmtime's emits `#[deprecated]`, so a program's author gets no warning from the compiler. Deprecation is told by the site's reference and by `cargo waos` (below), and a `#[deprecated]` from wit-bindgen is a change to propose upstream.
- **A built component carries no gates.** Its imports are names and types. The engine still knows which unstable items a program uses, by looking its imports up in its own WIT.
- **Versions match by semver track.** wasmtime's `Linker` resolves an import of `wasdk:platform/views@0.1.0` to a definition of `@0.1.4`, and the other way round: same major version, or for a 0.x version the same minor (`wasmtime-environ`, `alternate_lookup_key`). A prerelease matches only itself. Then each import is type-checked against what is defined.
- **A component imports only what it calls.** `wit-component` encodes, of each imported interface, only the functions the code imports. So a WaOS serving more than a program uses links it; a program calling a function added in 0.1.3 fails to link on a WaOS serving 0.1.2, the `IncompatibleProgram` of `running/incompatible.rs`; and a type changed in shape fails every program that uses it, which is why nothing in a released type changes.
- **A type check exists.** `wasm-tools component semver-check --prev <world> --new <world> <wit>` checks that components targeting the old world run against the new one, structurally, with every version stripped: the new may import more and export less.

**The policy.**

1. **Until the first release, nothing is promised**, and `wasdk:platform@0.1.0`, `wasdk:media@0.1.0` and `wasdk:program@0.1.0` change freely.
2. **At the first release they freeze.** Each later 0.1.x only adds: an interface, a function, a method, a type, each marked `@since(version = 0.1.x)`. What was in 0.1.0 needs no mark. A function's meaning does not change either; where it must, a new function is added beside it.
3. **A new API starts unstable**: `@unstable(feature = name)`, linked by WaOS for a development run (`cargo waos run`) and for a program whose package declares the feature (`unstable = ["name"]` in `[package.metadata.waos]`), which the library shows as "uses interfaces that may still change". In wasdk, unstable items come behind the Cargo feature `unstable`, a second `generate!` with the features listed, under `#[cfg]`, so a program opts in where it is built too. When the API settles, `@unstable` becomes `@since`: the same names and types, so programs that used it keep running. An unstable API may change or go in any release, and a program using it then fails to link, told which feature.
4. **A break is a new version of the package**: for a 0.x package the next minor, `wasdk:platform@0.2.0`, which wasmtime treats as a track of its own. WaOS serves both, one `bindgen!` per version, the old one's host written over the same capability state, as `files/p2` and `files/p3` both served one `Descriptor` and `Target` until `657cdf5` removed 0.2. The old version is served for the deprecation period after the new one is released, and `cargo waos build` warns of a program built against it.
5. **WASI's packages are under the same promise.** A program imports `wasi:http@0.3.0`, `wasi:sockets@0.3.0` and the rest as WaOS served them, and `wasi:webgpu@0.3.0-rc.2`, a prerelease, which matches only itself: when WaOS moves to a later `wasi:webgpu`, it keeps serving rc.2 beside it for the same period.
6. **The Rust SDK is versioned on its own.** The promise is that built programs keep running. A program's source compiling against a newer `wasdk` follows Cargo's semver: `wasdk` 0.x pins one wit-bindgen, and a wit-bindgen change to the generated Rust is a new minor of `wasdk`, as Cargo reads 0.x.

**CI enforces it**, in three checks:

- **Every change to `wit/`**: `cargo xtask check-wit` runs `wasm-tools component semver-check` of each released world (kept under `wit/released/<version>/`, copied there at the release) against the current one; and, with `wit-parser`, checks that every item not in the last release carries `@since` of the version being made, or `@unstable`.
- **Every change to the engine**: the release corpus. At each release, the built components of real programs (the built-in apps, the examples, the six apps of `2026-11-13-what-six-apps-found.md`) are kept as release assets, `compat-<version>.tar`, with a short script for each, the inputs `cargo waos` already gives (`tap`, `type`, `key`, `resize`). CI runs every kept release's corpus on the current engine with the test shell: each program must link, start, draw its first frame and take its script's inputs without trapping. A failure names the release and the program.
- **At each release**: the reference marks each item's `@since` and `@unstable` (the site's `read-wit.mjs` already skips gates and keeps their text; it gains reading them), and the corpus of the release is made.

### The inspector's first panels

Organised by the two audiences, with one program's page shared between them, as Chrome's DevTools organise by task, iOS's App Privacy Report by kind of access over seven days, and Android's Privacy Dashboard by permission over a timeline.

**For a user: Activity.** The first page, on every device.

1. **Recently** (the home): the programs that used something guarded in the last seven days, most recent first, each row its icon, name and small marks for what it used (camera, microphone, files, network, clipboard, machines), with "2 minutes ago". Above them, as Android's dashboard opens: a strip each for the camera and the microphone, who had it and when, today. Below, as iOS's report has: the most contacted domains, across programs.
2. **A program's activity**: one section per capability, in plain words, each beside its setting with a link to change it: "Camera: 3 times today, 12 minutes", "Network: 14 sites, refused 2 (local network is off)", "Files: read 120 files in Documents, changed 4: …", "Clipboard: read twice without you pasting", "Ran on This Mac: git (14 times), cargo (2 times)", "Asked: camera, allowed on 12 November". This is also what a program's page in WaOS's settings shows under its switches, in short, linking here.

**For a developer: a program's live panels**, as DevTools' tabs, each a table of the events since it opened, filterable, with refused rows marked and their setting named:

3. **Console**: the program's output, its traps and the engine's warnings about it.
4. **Network**: lookups, connections and requests in one table (time, kind, host, address, verdict, status, size, duration), a request's response joined to it, as DevTools' Network panel.
5. **Files**: opens and changes (time, path, where on the machine, mount, read or write, verdict).
6. **Access**: the clipboard, the camera and microphone, machines, questions and settings changed, on one timeline.
7. **Resources**: memory, CPU (fuel over time), frames a second, hardware buffers, workers, as small charts.

Later: Calls (above); Storage, the program's `/data` and `/cache`; and the semantics tree, after accessibility (decision 8).

On a phone the same program lays the pages out as a phone's settings do: a list, then a page, Activity first. The inspector is opened at a program from that program's page in WaOS's settings, by opening a link the inspector answers (`2026-10-29-opening.md`).

What Waft's Fluent kit has for it: `NavigationView`, `TabView`, `SelectorBar`, `TreeView`, `Expander`, `InfoBar`, the list of decision 16 of `2026-11-13-what-six-apps-found.md`. What it lacks: a table with columns that size, sort and filter, and a chart. These are raised, not built around (open question 9).

## The steps

Each step is built, checked and committed on its own; the WIT and Rust types of each are shown to the lead before code is written.

1. **Measure the summary's cost** (`../experiments/2026-11-15-audit-cost`): a program opening and stat-ing files in a loop, and sending UDP datagrams, with no counting, with the summary's counting, and with a listener attached. Only the rows that decide: is counting each open under 1% of the open; is a listener's cost bounded. It decides whether the summary counts stats and datagrams.
2. **The access layer, a reshape with no new behaviour**: `ProgramAccess` with `decide`, `hold` and `tell`, counting into nothing and with no taps; each capability moved onto it in turn (network, files, clipboard, camera, microphone, machines, links, notifications, questions), its tests unchanged, and one test per capability that every refusal is told. One commit per capability.
3. **The audit summary**: counting in the layer, `AuditLog` and `AuditDay` in `waos-capabilities`, the store's files and their pruning, and a program's settings page showing its last seven days.
4. **The protocol and `cargo waos inspect`**: the `wasdk:inspect` WIT, the taps and the live trace, the engine's public API, the dev protocol's inspection, the command. From here a developer and a coding agent can inspect a run.
5. **Roles**: `roles` in the package and the manifest, the role's question and row, linking the role's interfaces only for a program that holds it, wasdk's `inspector` feature.
6. **The inspector program**, with Waft and the Fluent kit: Activity first, then Console and Network, then Files, Access and Resources; on desktop and phone.
7. **A phone from the desktop**: the Android abstract socket, the forward, the phone as a device in the inspector and in `cargo waos inspect --device`.

**Decided on 2026-11-15**, after step 2's first part was built: the access layer's model is revised as `2026-11-15-access-layer-design.md` advises, and the lead approved it whole. A use is split into what the program asked and what the rule resolved it to, the record made from the rule's answer; an operation nothing decides anew (a stat, a datagram, a handle's first write) is noted, not decided; a lasting use is held by what owns the real thing until it really ends, in one record of what the program holds, which the recording indicator reads too, so `recordings.rs` goes, unlike "Where it sits today" above says; the network's checks are wrapped whole and reshaped later, with the fake-address redesign; and step 1's measurements are those the note lists.

The compatibility work is a track of its own, needed before the first release, whichever of the above is done:

- **C1.** `cargo xtask check-wit`, with `wit/released/` empty until the release.
- **C2.** `@unstable` features linked per program (`LinkOptions` from a development run or the package's `unstable`), wasdk's `unstable` feature, and `cargo waos build` telling which features a program uses.
- **C3.** The release corpus and its CI job, made at the first release.
- **C4.** The site's reference showing `@since` and `@unstable`, and the compatibility page (`packages/waos-site/content/docs/compatibility.mdx`, drafted beside this proposal).

## Open questions for the lead

**Decided by the lead, 2026-11-15:** the design as proposed, with the recommendation on every question below; and where building it hits something the design didn't foresee, it is raised to the lead rather than worked around to keep to these decisions.

1. **How the protocol's values cross the wire.**
   - (a) JSON made by serde from the engine's generated `wasdk:inspect` types: one schema, the WIT; `cargo waos` prints it without types of its own; names are serde's (snake case fields, cases as Rust names).
   - (b) Rust types of the protocol in `waos-dev-protocol`, in postcard, converted to and from the generated ones: compact, but a second definition to keep in step.
   - (c) WAVE, the WebAssembly value text wasmtime's command line uses: WIT's own names, but few tools read it.
   - Recommendation: (a). An agent reads JSON, and the WIT stays the one definition.
2. **What the summary keeps of files.**
   - (a) Counts per mount only.
   - (b) Counts per mount, and the paths changed in the user's folders and picks, up to 100 a day; reads counted, not listed.
   - (c) Every path read and changed, capped.
   - Recommendation: (b). A change is what a user worries about; reads are many, and the folder already says what was readable.
3. **Command arguments.** The trace shows a run's arguments; they can hold a token or a password.
   - (a) In the trace and the summary.
   - (b) In the trace only, the summary keeping the program's name.
   - (c) In neither.
   - Recommendation: (b). The developer needs them live; a week's record does not.
4. **Can the user turn the summary off?** iOS's report is off until turned on; the lead decided always on.
   - (a) Always on, no switch.
   - (b) On by default, with one switch in WaOS's settings that turns it off and clears what is kept, and a "clear" without turning off.
   - Recommendation: (b). The record is itself private (a reader's domains are much of a browsing history), so the user who does not want it kept can say so.
5. **Who may import `@unstable` APIs in a released WaOS.** The decision says "development builds or opted-in programs".
   - (a) Development runs only.
   - (b) Development runs, and a package that declares the features, which the library marks.
   - (c) Both, and a nightly WaOS that links every feature for every program.
   - Recommendation: (b), with (c) later if WaOS has a nightly channel. "Development builds" is read here as `cargo waos run`'s runs; if the lead meant builds of WaOS itself, (c) is that.
6. **How long an old version is served.**
   - (a) Six months after its replacement is released.
   - (b) At least twelve months, and at least two WaOS releases.
   - (c) Until no program in the default source uses it.
   - Recommendation: (b). Long enough for a program nobody maintains to be noticed; the site page is drafted with it.
7. **What the corpus checks.**
   - (a) Links and starts.
   - (b) Links, starts, draws a first frame, and takes its script's inputs without a trap.
   - (c) As (b), and compares screenshots with the release's.
   - Recommendation: (b). Screenshots change with fonts and Valo for reasons that break nothing; they can come later for programs that draw only their own content.
8. **What `cargo waos inspect` reaches.**
   - (a) The programs of `cargo waos run`'s runs, as decision 60 has it, an installed one later with the user's consent per agent.
   - (b) Every program, the dev socket being the user's own.
   - Recommendation: (a). The summary of every program is the user's audit of other publishers' code, and an agent reading it should be one the user named.
9. **The table and the chart the inspector needs, which the Fluent kit lacks.**
   - (a) A table in the Fluent kit over Waft's list with WinUI's `ItemContainer` rows (decision 16 of `2026-11-13-what-six-apps-found.md`), columns sized as WinUI's `Grid` sizes them; charts drawn by the inspector on Waft's canvas.
   - (b) Port the Windows Community Toolkit's `DataGrid`, which Microsoft no longer develops.
   - (c) Port Material's `DataTable`, from the Material port of decision 33.
   - Recommendation: (a). It uses what is being ported already and keeps WinUI's look; the inspector is the first program to need a table, so it is where one is found to be right.
10. **The first remote transport.**
    - (a) USB through adb, on Android first.
    - (b) The network with pairing, for Android and iPhone at once.
    - Recommendation: (a). adb's own authorization is the pairing, and it is how Chrome inspects a phone; the network follows with `cargo waos run` on phones.
11. **Does WaOS's own inspector ask for its role?** The decision is no signing rule.
    - (a) It asks, as any program does.
    - (b) It is granted when the user opens it from WaOS's own menu, the opening being the consent.
    - Recommendation: (a). One rule for every inspector keeps the role's question meaningful.

## Research behind it

- **Chrome DevTools and CDP.** Targets (pages, workers, service workers) found through the `Target` domain and attached to with a session; domains enabled to receive their events; JSON messages with ids over a WebSocket; a phone's Chrome reached over USB through an abstract socket adb forwards. The Network panel is a table of requests joined to their responses, filterable, exportable as HAR. From Chrome 136 the debugging port is refused for the default profile, to stop malware using it to read users' cookies.
- **WebDriver BiDi.** A standardised successor for automation, whose `session.subscribe` names the events and the browsing contexts wanted: the subscription per capability here follows it.
- **iOS's App Privacy Report** (iOS 15.2): off until the user turns it on, then seven days kept on the device: each app's data and sensor access (camera, microphone, location, photos, contacts) with times, the domains each app contacted, and the most contacted domains; exportable as a file.
- **Android's Privacy Dashboard** (Android 12): a timeline per permission of which apps used location, the camera and the microphone over the past 24 hours, opened from settings; with the indicators while in use. For developers, Android 11's data access auditing (`AppOpsManager.setOnOpNotedCallback`) tells an app its own accesses with where in its code they came from, which the developer's panels here do for any program from outside it.
- **macOS** shows which app uses the camera or the microphone now, in Control Center, and keeps no history the user can read.
- **The component tools**, read in `~/.cargo/registry`: `wit-parser` 0.262.0 (`Stability`, `Resolve::include_stability`, the grammar's attributes), `wasmtime-internal-wit-bindgen` and `wasmtime-internal-component-macro` 50.0.1 (`LinkOptions`, `FeatureGate`, `all_features = true`, the per-call `tracing` spans), `wit-bindgen-rust-macro` 0.63.0 (`features`), `wit-component` 0.262.0 (imports encoded per function used; `semver_check`), `wasmtime-environ` 50.0.1 (`alternate_lookup_key`), and `wasm-tools` 1.258.0's `component wit --features` and `component semver-check`.
