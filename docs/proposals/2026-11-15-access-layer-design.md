# The access layer's model: design advice

Written 2026-11-15 for the lead, and kept here from the note where it was written. **Approved by the lead on 2026-11-15, whole**, removing `recordings.rs` so the recording indicator reads the same record of held uses as the inspector among it (the one place this note departs from `2026-11-15-inspector-roles-and-compatibility.md`). What was planned on its date, not what is. Nothing was built or run for it. It reads `docs/proposals/2026-11-15-inspector-roles-and-compatibility.md`, `crates/waos/src/access.rs` and commit `a4191f1`, and the files and network code the three problems are in. What it says of other systems is from their documentation and sources as I know them, not re-read today; the four facts I would check before relying on them are marked "(check)".

## The answer in short

The layer as built is the right shape: one call that runs the capability's rule and tells the result is how SELinux, Android's AppOps and macOS's Endpoint Security are built too. It needs three additions, and the three problems are each one of them:

1. **A record is made from what the rule resolved, not from what the caller wrote before the rule ran.** A use is split into what the program asked (a path, a name) and what the rule answered (the mount and the file, the addresses). Files and DNS then fit without the check and the record drifting.
2. **A lasting use is held by whatever owns the real thing, and ends when that thing really ends**, not when the program's handle drops. The same set of held things is the live state the inspector shows, and should be what the recording indicator reads.
3. **A second, lighter kind of call for what is not decided anew** (a stat, a datagram, a write on an open file): traced when someone listens, otherwise one atomic read, never in the summary's lists.

For the network: wrap today's checks as they are, at the four functions that already give the verdict. Reshape nothing now, remove nothing.

## What other systems do, and what to take

**Decide on the resolved object, and write the record in the same call.**

- Linux's security hooks (LSM) sit after the kernel resolved the path, on the file itself, just before the operation. SELinux's `avc_has_perm` computes the decision and writes the audit record in one function from the same arguments; the object's description is formatted only if a record is written. By default it records every denial and no grant; grants are recorded only where a rule asks (`auditallow`), and noisy denials are silenced by rule (`dontaudit`).
- AppArmor is path-based like WaOS. It does not trust the string the program passed: at the hook it rebuilds the path from the resolved file and matches its rules against that.
- Landlock is the closest to WaOS's mounts. A rule is a directory the sandboxer opened plus the rights under it. At open the kernel starts from the resolved file and walks up its parents looking for a rule. Rights that matter later (truncate, ioctl) are fixed on the file handle at open and only compared afterwards. It does not mediate `stat` at all. Its audit records (Linux 6.15, check) are denials only, naming what blocked and the object.
- fanotify's permission events hand the deciding program a handle to the already-resolved file, never a path. `FAN_OPEN_PERM` is once per open; the per-read event exists but is opt-in because of its cost. The reply can carry `FAN_AUDIT`, so the verdict and the request to record it are one write.
- macOS's Endpoint Security has two kinds of event for the same operation: AUTH (the client must answer allow or deny) and NOTIFY (after the fact, no answer). Both carry the resolved file. `NOTIFY_CLOSE` carries a `modified` flag: whether the file was written while open is told once, not per write.
- Windows checks access once, when a handle is opened, and stores the granted rights in the handle; later operations compare a mask. Its object auditing records the handle opened, the first real use of each right on it, and the close.

**The decider does the operation itself.** seccomp's user notification lets a supervisor decide a syscall, but it sees pointers into the program's memory, and the program can change the path after the check. The documented fix is that the supervisor opens the file itself and injects the handle (`ADDFD`). The rule that follows: never approve a name and let someone else resolve it again. WaOS already obeys it for files (the engine opens beneath the mount and hands back its own handle) and for direct HTTP (reqwest connects to the addresses the checked lookup answered). It breaks it in two known places: a proxy looks the name up again (the blind spot `route.rs` documents), and raw sockets join an address back to a name through a one-hour record. A fake-IP design ends both, since the engine resolves at the moment it connects.

**One-off and lasting uses are different calls.** Android's AppOps has `noteOp` for a use that happens and is over, and `startOp`/`finishOp` for one that lasts. Three details matter here:

- The service that owns the resource makes the calls (the camera service when a client really connects and disconnects, the audio service when recording really starts), not the app and not the app's handle.
- A started use is tied to the client's liveness: if the client dies, the system finishes its uses.
- The mic and camera indicators and the Privacy Dashboard are both readers of this one fact. The indicator listens to "active changed"; the dashboard reads the history. History is kept in two tiers: counts and total durations per time bucket for every op, and individual timed entries only for a few sensitive ops, rounded to a minute, about seven days (check).

**Always-on state is counted where it happens; events flow only when someone attached.**

- Chrome's Network domain sends nothing until `Network.enable`. A request is an id with a start (`requestWillBeSent`, with its initiator), a response, and an end (`loadingFinished` or `loadingFailed` with a `blockedReason`). The address reached and `connectionReused` arrive later, on the response: the request is identified by its URL, and where it landed is a fact filled in afterwards.
- eBPF tools count inside the kernel in a map and read the map on demand; per-event streams go through a ring buffer that reports how many were lost. Unattached probes cost nothing.
- Windows ETW providers test an enabled mask before building an event: the same thing as the proposal's `Taps` word.
- Linux audit keeps, per record, both the name as given and the resolved inode, filters in the kernel, and counts what it lost.
- Fuchsia's Inspect is live state: a component keeps a small tree of its current values in memory, cheap to update, read by a tool on demand; values that are costly to compute are lazy and computed only when read. Fuchsia's routing also shows that a capability not routed is simply absent from the program's namespace: the program gets "not found", and the system logs the routing failure on its own side.
- iOS's App Privacy Report records sensor and data access with times, and network activity by domain, split by who initiated it (the app itself, or content inside it).
- macOS's TCC is the counter-example: grants are in one system and uses in others, so macOS has an indicator and no history.

**What does not fit an engine in the program's own process.**

- A verdict asked of another process with a deadline (Endpoint Security AUTH, fanotify, seccomp). WaOS's rule is a function call. The only asynchronous verdicts are questions to the user, which `put` already keeps apart.
- Labels on objects (SELinux). WaOS cannot label the machine's files; its rules are by place, as Landlock's and AppArmor's.
- A check on every read (the per-read hooks). A mount change applies at the next open, as today.
- A kernel's completeness. The engine sees only what crosses it. Three things it hands to someone else and so cannot see inside: a command run on a machine (what `git` then reads or reaches is unseen; the summary should say "ran git", and the inspector should say plainly that a command's own activity is not recorded), a proxy's second lookup, and `wasmtime-wasi`'s sockets after the address check.

## The model

**A use** is one thing the program asked for that a grant could let or refuse: an open, a change, a lookup, a connection, a request, a clipboard read, a device opened, a run. It has two parts: what was **asked**, in the program's words, known before the rule runs; and what it **resolved** to, which only the rule knows.

**A rule** is the capability's own function. It answers what the use resolved to, or the setting that refused it. It may read the grants or read something compiled from them: the mount table is a compiled rule, as SELinux's decision cache is. So the layer's invariant should be reworded from "a grant read is made inside the rule" to "an answer that lets or refuses a use comes out of `decide`". The first wording cannot hold for files, where the grant is read when the table is rebuilt.

**A verdict that needs resolution** is expressed by putting the resolution inside the rule and making the rule's answer describe itself. The record cannot disagree with the check, because the record's resolved half is the check's return value.

```rust
/// What the program asked for, in its own words, known before the rule runs.
pub(crate) enum Asked<'a> {
    Open { path: &'a str, mode: OpenMode },
    Change { path: &'a str, change: FileChange<'a> },
    Lookup { name: &'a str, cause: Cause },
    Connect { address: SocketAddr, protocol: Protocol },
    Request { id: RequestId, method: &'a str, uri: &'a Uri },
    // … the seven built ones, unchanged
}

/// What a rule's answer says of itself for the record. Called only when the summary or a
/// listener needs it; `()` and the built capabilities' answers resolve to nothing.
pub(crate) trait Resolved {
    fn resolution(&self) -> Resolution<'_>;
}

pub(crate) enum Resolution<'a> {
    Nothing,
    /// A file or folder: the mount it is in and the path beneath it.
    File { mount: &'a MountUses, kind: MountKind, beneath: &'a Path },
    /// A lookup's answer.
    Addresses(&'a [IpAddr]),
    /// An address's names, as the address record holds them.
    Names(&'a [String]),
}

impl ProgramAccess {
    /// Decides `asked` by `rule` and tells it with what the rule resolved.
    pub(crate) fn decide<R: Resolved>(
        &self,
        asked: Asked<'_>,
        rule: impl FnOnce(&dyn ProgramGrants) -> Result<R, Refusal>,
    ) -> Result<R, Refusal>;

    /// Tells an operation nothing decides anew: a stat, a datagram on a checked address, the
    /// first write on an open file. One atomic read when nobody listens.
    pub(crate) fn note(&self, noted: Noted<'_>);

    /// Registers something the program holds from now until `Held` is ended or dropped.
    pub(crate) fn hold(&self, holding: Holding) -> Held;
}
```

`decide` keeps its signature but for the bound; the seven built capabilities change one word (`Use` to `Asked`) and nothing else. `hold` stops being "decide, then hold": the decision and the holding are often made at different moments (a camera is allowed when opened, and runs when a session starts), so they are two calls.

**Lasting uses and live state are one registry.** `ProgramAccess` keeps the set of what the program holds now: each entry a kind, a short description, when it started. `hold` adds one; ending the `Held` removes it and, for the kinds the summary lists as sessions, writes the duration. Three readers:

- the summary, at each end and at each minute's flush (a session still running is kept with its length so far, so a crash loses a minute, not the session);
- the inspector's live state, as a snapshot on demand, the way `lsof` reads the kernel's table and Inspect is read;
- the recording indicator, as "a camera or microphone entry appeared or went", the way Android's indicator listens to active ops.

It costs something when nobody looks, but little: a held thing is rare (a session, a run, a request), and each start already does work thousands of times slower than one lock and one small entry. Open files are the one frequent kind and are handled apart (below), so the registry never sits on a hot path.

## Problem 1: files

**The effect.** An open is recorded as: the path the program named, the mount it landed in and where that is on the machine, read or write, allowed or refused. A path outside every mount is recorded as refused by `folders` with no place on the machine, while the program still gets `no-entry` as today. Listing a folder of ten thousand files adds ten thousand atomic reads and nothing to the summary.

**Why it stuck.** `Use::Open { path, target, mode }` asked the caller to name the target before the rule that finds it. And the mount table is two things at once: a namespace (it translates the program's path, and a path with no mount does not exist, as in Fuchsia or a WASI preopen) and a rule (a read-only mount refuses a change, as in Landlock). Both are fine; the type just has to let the rule produce the target.

**The shape.** Resolution stays one function, used by every path call, so nothing can resolve one way for the check and another for the record. The calls differ only in how they are told:

```rust
// files/mod.rs: the one place a path is led through the mounts.
impl ProgramFiles {
    /// Opens what `path` names from `from`: decided and told as one use.
    pub(crate) fn open(&mut self, from: &Descriptor, path: &str, request: &OpenRequest)
        -> Result<Descriptor, ErrorCode>
    {
        let asked = Asked::Open { path, mode: request.mode() };
        let target = self.access.clone().decide(asked, |_| self.permitted(from, path, request.change()))?;
        let opened = target.open(request)?;          // the system's own errors are not verdicts
        self.access.opened(&opened);                 // live state, below
        Ok(opened)
    }

    /// Where `path` leads and whether the mount allows `change` there: the rule. Outside every
    /// mount is `Refusal::by("folders")`; a change beneath a read-only mount is too.
    fn permitted(&mut self, from: &Descriptor, path: &str, change: Option<Change>)
        -> Result<Target, FilesRefusal>;

    /// What a metadata call names: the same resolution, noted, not counted.
    pub(crate) fn named(&mut self, from: &Descriptor, path: &str) -> Result<Target, ErrorCode> {
        let target = self.target(from, path);
        self.access.note(Noted::Named { path, target: target.as_ref().ok() });
        target
    }
}

impl Resolved for Target { /* Beneath and MountPoint: File { mount, kind, beneath }; else Nothing */ }
```

`FilesRefusal` carries the `Refusal` for the layer and the `ErrorCode` the program is answered with, so the table in `target.rs` (what each refusal answers) stays the files' own. `target.rs` already separates what a change is (`Change`) from doing it; the rule needs its refusal table callable without making the change, which is the one reshape inside `files`.

**What changes in the built code.** `FilesCtx::target_of` in `files/p3/host.rs` goes: `open_at` calls `files.open`; the seven calls that change (`create_directory_at`, `set_times_at`, `link_at`, `rename_at`, `symlink_at`, `remove_directory_at`, `unlink_file_at`) call a `files.change`; `stat_at`, `readlink_at` and `metadata_hash_at` call `files.named`. `ProgramFiles` takes the `ProgramAccess` where it took the grants, and `LiveMounts` reads its grants from it. `Mounted` gains its counters and its kind (own data, own cache, granted, picked, pasted), which `MountTable::new` knows as it builds from `own`, `granted` and `handed`. A machine's root (`Target::OnMachine`) resolves to its machine path with kind `machine`.

**Decide once per handle, count later operations without deciding.** WaOS already does the first half: `Opened` carries `writable`, and a write compares it. For the summary's "files changed", take Endpoint Security's and Windows' idea: a file is changed when it is first written through its handle, not when it is opened for writing. `write_via_stream`, `append_via_stream` and `set_size` each `note` a first write, once per handle (one flag in `Opened`). The summary's list of changed paths is fed by those and by creates, removes and renames.

**Live state.** WaOS knows it without hacks: every open file is an `Opened` holding an `Arc<fs::File>`. `access.opened` keeps a `Weak` to that file with its mount and mode, in a list swept of dead entries as it grows. No guard, no `Drop`, no change to how streams share the handle; a stream that outlives the descriptor still shows, correctly, as open. The path is not stored: on demand the system is asked for the handle's path (`F_GETPATH` on Apple systems, `/proc/self/fd` on Linux and Android, `GetFinalPathNameByHandle` on Windows), which is what `lsof` and Activity Monitor show and stays true after a rename. That needs a check on each system; the fallback is storing the machine path at open, one allocation.

## Problem 2: the network

**What I would do: wrap the checks as they are, and reshape nothing.** Each of today's limits already comes out of one function, and each function already holds its own resolution. Putting `decide` around the whole function makes the record from what that function resolved, so there is no drift to design away:

| Today's function | Becomes | Resolved |
|---|---|---|
| `ProgramNetwork::reachable`, and the lookup `network/host/p3.rs` makes from `plan_lookup` and `resolve` | one `looked_up(name, cause)`: `decide(Asked::Lookup, …)` whose rule is the plan, the system's lookup and the address filter together | the addresses answered |
| `allows_socket`, in the closure `wasi/mod.rs` gives `socket_addr_check` | `decide(Asked::Connect or Listen, …)` for TCP connect, accept and listen; `note` for each datagram, a refused one counted | the names `AddressRecord` holds for the address |
| `check_host` in `send_request` | `decide(Asked::Request, …)`, then a `Held` for the request, carried in the response body's wrapper (`BetweenBytes`) and ended when the body ends or is dropped | nothing yet; the address reached is noted with the response |
| `media_sources::check_link` | `decide(Asked::Request, …)` with cause `Player` | as a lookup |

```rust
// network/mod.rs
pub(crate) fn reachable(&self, host: &str, cause: Cause) -> Result<Vec<IpAddr>, LookupError> {
    self.access.decide(Asked::Lookup { name: host, cause }, |grants| self.reachable_by(grants, host))
        .map_err(LookupError::from)
}
```

The rule blocks while the system resolves; that is fine, since `decide` is only a wrapper and already runs where `reachable` runs, off the program's thread. `Refusal` here names `network`, `domains` or `local-network`, which `plan_lookup` and `resolve` already tell apart by their branches.

**DNS-dependent verdicts.** "Local network only" is decided after the answer, inside `resolve`. With the lookup as one use whose rule includes the resolving, the trace shows `lookup example.test → [192.168.1.4]` allowed, or refused by `local-network` with the reason known, and nothing has to be told twice.

**reqwest resolving on its own, and its pool.** A request is decided by its name every time, in `check_host`, pooled connection or not: that is its `Request` use. When reqwest does resolve, it does so through `ProgramResolver`, which calls `reachable`: a second use, a `Lookup` whose cause is that request. When it reuses a connection, no lookup happens and none is told, which is the truth; the response's event carries the address it came from where reqwest gives it (`Response::remote_addr`), as Chrome's does. The summary counts a domain once per request and does not count the lookups a request caused.

One thing to know, not a change I propose: a pooled connection to a local address is reused after the user turns the local-network switch off, as long as the name still passes the domain rule. That follows the module's stated invariant ("a connection open already carries on"), and it exists today; the layer neither adds nor removes it. If the lead wants an idle pooled connection to count as closed, that is a rule change for the rework.

**Cause.** `Cause::{Program, Request(id), Player}` is iOS's "who initiated" and Chrome's `initiator`. It is what keeps a request's own lookup from looking like the program probing names.

**Why not reshape now, given the fake-IP redesign.** What survives that redesign is exactly what the wrap adds: the vocabulary (lookup, connect, request), the summary by domain, the events. What it replaces is what a reshape would touch: `AddressRecord` and its one-hour guess, the resolver's pin, the route's double lookup. With fake IPs the address the program holds maps back to its name exactly, the engine resolves when it connects, and the "local network" rule is applied to the address actually dialled, so the checks get simpler on their own. Simplifying them now would be work done twice.

**No protection is removed.** Every check keeps its body. In particular the path "network off, local network on: resolve any name and answer only its local addresses" stays, since it is what makes the local-network switch usable alone.

**Live connections: not known today.** `wasmtime-wasi` owns TCP and UDP sockets; WaOS is asked whether an address may be used and never hears the socket close. So the trace can show every connection attempt with its verdict, but the inspector cannot honestly list open sockets, nor a connection's length or bytes, without wrapping `wasmtime-wasi`'s socket implementation, which is a hack worth not building: under fake IPs the engine makes the real connection itself and will own that state. HTTP requests in flight are known now, through the `Held` above.

## Problem 3: held uses

**The effect.** "Camera: 12 minutes" means the camera ran for 12 minutes. A command the program started and let go of shows as running until it exits. The inspector's live page lists what each program holds now.

**Why the durations are wrong today.** `Held` sits in the program's handle: `Camera._open`, `Microphone._open`, `Child._run`. But the camera runs while a `CaptureSession` exists, which the program can keep after dropping its `Camera`; and `waos-capabilities` says of a child "dropping it leaves the command running". Meanwhile `recordings.rs` already holds the right span for the camera (`CaptureSession._recording`). Two guards for one fact, in two places, is how they came apart.

**The rule, from AppOps.** The one who owns the real thing holds the use, and the real end ends it.

```rust
// cameras: opening a camera is decided; a session is what is held.
let in_use = access.decide(Asked::OpenCamera { id, simulated }, allowed)?;           // at open
let session = CaptureSession::new(started, state, access.hold(Holding::Camera { simulated }), capturing);

// machines: the run ends when the command does, wherever the program's handle went.
access.decide(Asked::Run { machine, command, in_terminal }, |_| Ok(()))?;
let run = access.hold(Holding::Run { machine: machine.into(), program: command.program().into() });
let exited = child.exit_status();
engine_runtime.spawn(async move { run.ended(Ended::Exited(exited.await)) });
```

- **Camera and microphone.** The `Held` moves from `Camera` and `Microphone` to the session and the microphone's recording stream, exactly where `Recording` is. Then `Recording` and `Held` are the same span, and I would make them the same object: the indicator becomes a listener of the registry's camera and microphone entries, and `recordings.rs` goes. The proposal says `recordings.rs` stays as it is; this is the one place I advise against it, because keeping both is keeping the bug's cause. The indicator's invariant (told once at the first, once at the last) becomes the registry's.
- **Commands.** The watcher runs on the engine's runtime, not in the program's store, so it outlives the program. `AccessParts` is already an `Arc`, so the summary stays alive until the last held use ends; the day's record is kept again then. If WaOS quits first, the run is kept as "still running when WaOS quit".
- **Dropped without an end.** `Held`'s `Drop` stays as the backstop: a program that ends, a store dropped, a session torn down all end the use, as AppOps finishes a dead client's ops.

**Live state: yes, show it, and WaOS mostly knows it already.**

| Held thing | Does WaOS know it now? | From |
|---|---|---|
| Open files and folders | Yes | the weak list above; the path from the system on demand |
| Camera and microphone sessions | Yes | the registry |
| Running commands and terminals | Yes | the registry, ended by `exit_status` |
| HTTP requests in flight | Yes | the registry, the `Held` in the body wrapper |
| File watches, pickers and questions open | Yes | the registry, one `hold` each where they are made |
| TCP and UDP sockets | No | `wasmtime-wasi`'s; comes with the network redesign |
| reqwest's idle pooled connections | No, and they are not the program's | leave out |

In the protocol this is a query like the summary, not a stream: `holdings(device, id) -> list<holding>`, answered from the registry and the weak list at the moment it is asked. An inspector that wants it live asks again, or joins it with the trace's start and end events, as DevTools joins a request's events.

A side effect worth a decision later: once the layer knows who holds what, turning a switch off can end the uses it allowed (Android's camera service disconnects a client whose op is revoked). Whether a camera turned off mid-session stops the session is the lead's rule to set; today it carries on.

## What is counted always, and what is only traced

| | Summary, always | Trace, only while listened to |
|---|---|---|
| Files | opens per mount, to read and to write; creates, removes, renames; first write on a handle; the changed paths in the user's folders, capped; refusals: outside the folders, and read-only | every stat, readlink and metadata hash; the path and place of each open |
| Network | per domain: requests and the program's own lookups and connections, refusals by setting; local addresses; datagrams as one count | lookups a request caused; each datagram; responses' bytes and timing |
| Held | each camera and microphone session with its true length; each run by machine and program, with its length | starts and ends as events; requests in flight |
| The rest | as the proposal has it | as the proposal has it |

Two rules behind the table, both SELinux's and Landlock's defaults: every refusal is counted, because refusals are rare and are what an audit is for; allowed uses are counted in aggregate, never listed, except changes.

**The cheap way to count.**

- **On the object the rule already found.** Each mount carries its counters as atomics (`MountUses`, shared by an `Arc` so a rebuilt table keeps them). Counting an open is one `fetch_add` on the mount the resolution just returned: no lock, no map, no contention between a program's workers. This is counting at the source, as an eBPF map does.
- **No clock on the hot path.** `first` and `last` are set at the minute's flush for any counter that moved since the last one. A minute is the precision Android keeps for the same purpose.
- **The lock only where the operation is slow.** The per-domain map and the capped lists are under the program's lock, taken for a lookup, a request or a file change, each of which costs far more.
- **`note` is one relaxed load** of the taps word and a return.

## Risks, and what to measure

Add these rows to the planned `2026-11-15-audit-cost` experiment; each decides one thing.

1. Open and close a file in a loop: bare, with the mount's atomic count, with the weak entry added. Decides whether open files are registered always or only on demand.
2. `stat_at` over a large folder with `note` and no listener, against today. Should be unmeasurable; if not, the taps check is in the wrong place.
3. A program with four workers opening files at once: the atomics against one lock. Decides whether per-mount atomics are worth their code.
4. Whether the system answers a handle's path on macOS, Linux, Windows, Android and iOS, also after a rename and for a deleted file.
5. Whether `exit_status` resolves after the program that started the command has ended, and when an SSH machine is lost; else runs never end in the registry.
6. Whether `wasmtime-wasi` 50 calls `socket_addr_check` for every datagram on a connected UDP socket, which sets what "datagrams as one count" counts.

Risks that are not measurements: the sweep of dead weak entries must be bounded (sweep when the list doubles); a session's length should say whether interruptions by the system count (I would count the whole session, as the indicator shows it); and the registry makes `ProgramAccess` hold state that outlives the program, so its flush at the program's end must not be the last one.

## What changes in the built code, in order

1. `access.rs`: `Use` becomes `Asked`; `decide` gains `R: Resolved`; `hold` takes a `Holding` and no rule; `note` is added; the registry and `Held::ended`. The invariant's wording changes as above. The seven capabilities' rules are untouched.
2. Cameras, microphone, machines: the `Held` moves to the session, the recording stream and an exit watcher; the indicator reads the registry and `recordings.rs` is removed (the lead's call).
3. Files: `ProgramFiles::open`, `change` and `named` over the one resolution; counters and kinds on `Mounted`; the first-write note; the weak list.
4. Network: the four wraps, with `Cause`; the request's `Held`. Nothing else until the redesign.
