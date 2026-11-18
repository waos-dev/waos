# The dev protocol's first stable shape

Written 2026-11-18, before building it. `cargo waos` and WaOS speak the dev protocol (`crates/waos-dev-protocol`). After a release, WaOS.app is downloaded and `cargo waos` is installed with cargo, so the two are often of different releases. Today every change to a message raises the protocol's number, and any mismatch refuses everything: the number reached 10 in a month, and the error does not say which half to update. A design review of the protocol (2026-11-18) found the rest of its shape sound: a connection per command, length-prefixed frames with the cap checked before allocating, a component in 8 MiB parts with its length and digest, logs that leave out secrets and payloads, inspection as JSON of the WIT. This records the lead's decisions and the shape to build before WaOS 0.3.0.

## The lead's decisions

1. **Additions don't break the other half** (the review's option A over add-only postcard or a strict match): a hello first, and a self-describing encoding. "A feels ok as current break do cause trouble."
2. **Built now, before 0.3.0**, beside the rest of the release's work.
3. **The shape below**, as shown to the lead.

## The shape

**The hello.** The first frame each way, its shape frozen: it only ever gains fields.

```rust
struct Hello {
    major: u32,          // 1; raised only by a real break
    build: String,       // "WaOS 0.3.0", "cargo-waos 0.3.0"
    serves: Vec<String>, // WaOS's: the requests it answers, "run", "screenshot", "resize", …
}
```

`cargo waos` reads WaOS's hello before it sends a request. A different `major` is refused with both builds named and which one to update; a request WaOS does not serve is refused before it is sent ("WaOS 0.3.0 has no `resize`: update WaOS"), with the update command.

**CBOR in place of postcard** (`ciborium`), through the same serde types, so there is no second schema. A field added later is `#[serde(default)]`; a build ignores fields it does not know. A newer WaOS's answer an older `cargo waos` does not know is caught by `#[serde(other)] Unknown` on each answer enum (adjacently tagged, as serde requires for `other`), and skipped or reported, never a broken connection. An older WaOS never meets a request it does not know, since `serves` gates them.

**An answer type per request.** One connection is one command, so each request has its own answers rather than one `FromWaOS` every command must sort through:

```rust
enum Request {
    Run(Run),
    Screenshot { id: ProgramId, window: Option<WindowId> },
    Input { id: ProgramId, window: WindowId, input: Input },
    Resize { … },
    Settings { … },
    Set { … },
    Inspect { … },
    Open { … },
}
// Run → Upload, then RunEvent…; Screenshot → Result<Vec<WindowPicture>, Refusal>;
// Input → Result<InputDone, Refusal>; and so on.
```

**One refusal**, a kind an agent can act on and a message for a person, in place of `Refused(String)` beside `Unreached`:

```rust
struct Refusal {
    kind: RefusalKind,
    message: String,
}

enum RefusalKind {
    NoDevRunNow,     // was NotRunByCargoWaos
    NeverDevRun,     // was NeverRunByCargoWaos
    NoWindow { window: WindowId },
    OutsideWindow { … },
    InNoWindow { … },
    Asking(Asked),
    BadInput,        // an unknown key, nothing being edited
    CannotLoad,      // not a component, or one that does not load
    LocalOnly,       // `Open` over a network transport
    #[serde(other)]
    Unknown,
}
```

**A run that skips an unchanged component**, and takes arguments:

```rust
struct Run {
    id: ProgramId,
    args: Vec<String>, // `cargo waos run -- args`
    variables: Vec<(String, String)>,
    length: u64,
    digest: [u8; 32],
}

enum Upload {
    Send,             // the parts follow, as since protocol 10
    Have,             // WaOS holds this digest: nothing is sent
    Refused(Refusal), // before any upload
}
```

**Windows by id.** Each window of a run gets a `WindowId` as it opens, never reused in the run; screenshots list them, so `--window 2` names one window however others close.

**Smaller rules.**
- A run's output in flight is bounded (1 MiB): a program writing faster than the connection carries waits, as one writing to a pipe does.
- `Screenshot` takes an optional window, so a picture of one window sends one.
- `Open` names this machine's paths, so it is local only: refused with `LocalOnly` on a network transport.
- Inspection stays one stream per connection, without the request ids the 2026-11-15 inspector proposal sketched: a connection per command is simpler and enough.

## Not in this change

The network transport's security, for phones: in the transport, not the messages (USB through adb's forward or SSH's socket forwarding, which authenticate the peer; elsewhere TLS 1.3 with keys pinned at pairing, as `adb pair`), with keepalive from the transport, so a half-open connection ends its run. Its own proposal, with phones.

## Work

1. The hello, CBOR, an answer type per request and `Refusal`, through `waos-dev-protocol`, `cargo waos` and WaOS's dev server: protocol major 1.
2. `Upload` with `Have`, a run's arguments, window ids, bounded output, a screenshot of one window, `Open` local only.

Tests: each request's round trip; a newer WaOS's unknown answer met by an older client (skipped), and a request an older WaOS does not serve (refused by `serves` before it is sent); the dev server's tests as they are.

## Built

Every enum inside a message (both requests and answers, and settings rows/asks) uses shared `"kind"` and `"data"` keys rather than per-enum tag names, and derives `#[serde(remote = "Self", tag = "kind", content = "data")]` with `#[serde(skip)] Unknown` instead of `#[serde(other)]` alone: `serde`'s `#[serde(other)]` rejects unknown variants carrying content and allows only one variant per enum, so `adjacent.rs` probes `"kind"` generically against the derived deserializer via `MapDeserializer` before falling back to `Unknown` — with no hand-written tag lists.
