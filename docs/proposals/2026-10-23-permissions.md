# Permissions

**Decisions taken (2026-10-23).** The lead took these, one question at a time, starting step 7:

1. **Programs are untrusted.** In the AI era software is more untrusted, so a program gets no more than the user gave it. What a program uses rarely asks the user at its first use (notifications, with decision 7 of `2026-10-17-waos-v1-interface.md`); what a program uses constantly, such as the network and files, is a switch the user sets, with rules beneath it, rather than a question at each use.
2. **The network is a switch per program, off until the user turns it on (revised by decision 8), and a rule of domains beneath it:** any domain, only some, or all but some. Only the user turns it on, in the program's settings in the library; a program reads its state and may ask WaOS to open that setting for the user, as iOS apps send the user to their page in Settings. Nothing asks by itself: asking at each new domain, as Deno and Little Snitch do, interrupts a program that reaches many domains again and again.
3. **A program sees only the folders the user allowed, mapped into its filesystem.**
4. **Each capability is free, asks, or is a switch:** its own `/data` and `/cache` are free, as is a file the user picks and opening links and files, sound playback, menus and dialogs; notifications, the camera and the microphone ask at their first use; the network, processes (with a rule of commands, any or only some, as Deno's `--allow-run`), reading the clipboard and writing it are switches. The user's own paste reads the clipboard whatever its switch, decided with the clipboard (`2026-10-25-clipboard.md`, decision 4).
5. **A program reads its permissions through one `permissions` interface:** a getter for each guarded capability, each answering a shared `permission` resource that tells its state (`not-asked`, `granted`, `denied`), asks for it (the prompt, or the switch's page in WaOS's settings opened for the user) and reports its changes. A new capability is a new getter, which breaks no program, where a case added to a shared enum would.
6. **A program's files are one table of mounts**, each a path on the machine at a path inside the program, read-only or writable: a folder the user grants in settings is mounted at its own path unless the user changes it, as Flatpak's `--filesystem` grants are, and a file or folder picked while the program runs under `/granted`. "Mount" is Docker's word for its bind mounts.
7. **The engine reads a program's grants live**, through `ProgramGrants` in `waos-capabilities`, which the store implements: a switch the user turns off refuses new handles at once, and the program hears of the change.
8. **Every setting follows a global default until the user sets it for one program**, as Chrome's site settings have a default and exceptions per site. The defaults: the network on for any domain, the clipboard on both ways, processes off, nothing mounted, and every question not asked yet. *This revises decision 2's default: the network starts on, through the global setting the user can turn off.*
9. **For now a program's files are WASI's own fixed folders**, so a mount the user sets applies at the program's next start; how a pick reaches a running program is settled with files. A WASI open is timed against a native one in an experiment, to decide whether WaOS later serves the mounts itself. *Replaced by decision 10.*
10. **WaOS serves a program's files itself**, porting `wasmtime-wasi`'s filesystem host for WASI 0.2, the version Rust's standard library calls (0.3's comes over the same core when the standard library moves to it): one root, `/`, resolving every path through the program's mounts, each path kept inside its mount in one call where the system has one (`O_RESOLVE_BENEATH` on Apple's, `openat2` on Linux) and by `cap-std`'s walk elsewhere. Measured, `wasmtime-wasi`'s open on macOS is 3.4 to 10 times a native one, from its walk, which opens every folder on the path, and its hop to a blocking pool, and that the one-call flag on the program's own thread would bring it near native. So a mount applies the moment it is added, and a file or folder the user picks is mounted at its own path, as a folder from settings is: no `/granted`. A folder above a mount shows only what is mounted under it, and a path outside every mount is not found, so a program cannot probe what the machine holds. *This revises decision 6's `/granted` and v1 interface decision 3.*
11. **A development run is the program its id names**: `cargo waos run` sends the id from `[package.metadata.waos]`, and the run has that program's settings and its `/data` and `/cache`, installed or not, as Xcode's and Android Studio's runs share the app's container and permissions.
12. **The local network is a switch of its own, off by default**: this computer (`127.0.0.1`, `::1`) and the local network (private ranges, link-local, multicast), where devices and services often trust whoever reaches them and a direct IP connection needs no name a domain rule could refuse; the network switch is the internet. macOS 15 and iOS ask for the local network on its own too.
13. **For now WaOS serves the name lookup itself and checks the rest through `wasmtime-wasi`'s hooks**: its own `ip-name-lookup`, which refuses a name the domain rule refuses (`access-denied`) and records the addresses an allowed one resolves to, for this program; the stock TCP and UDP, checked by `socket_addr_check` against the local-network switch, or the network switch and that record; and HTTP through `wasmtime-wasi-http`'s `send_request` hook, checked by the URL's host and sent with the system's proxy and certificate check. Stock sockets already run natively, so the `Network` provider of v1 interface decision 2 comes with SSH machines, replacing the stock TCP and UDP then.
14. **Through a system proxy, WaOS still looks up a program's HTTP host itself before it picks the route** (2026-10-27): an address on this computer or the local network is connected to directly, under the local-network switch, and an internet address goes through the proxy, under the network switch and its domain rule. Handing the proxy the name had skipped every check that needs the address: with the network switch off and the local-network switch on, a name the domain rule refuses is let through only if it lands locally, and the proxy, looking it up itself, fetched it from the internet. Both switches stay about where a connection lands, as Little Snitch's local-network rules and macOS's Local Network permission match the destination's address, whatever its name. Without a proxy nothing changes. The proxy still looks the name up again, so a DNS answering it differently the second time can slip past, the blind spot macOS's own check has with a proxy.

## Research behind it

### What the platforms ask an app for

| Capability | macOS | iOS | Android | Browser |
|---|---|---|---|---|
| Its own files | no | no | no | no (storage) |
| A file the user picked | no: picking is consent | no | no | no |
| Network | no; asks for the local network (macOS 15) | no; asks for the local network | no | no |
| Clipboard: write | no | no | no | no |
| Clipboard: read | asks when pasting from another app (macOS 15.4+) | asks on paste from another app | no; shows a notice | asks |
| Notifications | asks | asks | asks (13+) | asks |
| Camera, microphone | ask | ask | ask | ask |
| Running commands | no prompt (not sandboxed) | not possible | not possible | not possible |
| Opening links and files, sound playback, menus, dialogs | no | no | no | no |

### Where the checks sit, and what they cost

WASI's interfaces are capability based: a program acts only through handles (a directory, a socket, a request), so the host checks once, when a handle is made, and what is done with the handle runs unchecked. The permissions cost one check per open, lookup, connection, request or spawn, and nothing per byte.

| IO | Where the check sits | Cost |
|---|---|---|
| Files | Only allowed folders are mapped; other paths do not exist for the program | None per read or write |
| DNS | The name lookup answers only for allowed domains | One per lookup |
| TCP and UDP | The address check at connect or bind | One per connection |
| HTTP | The outgoing request handler, which sees the URL's host | One per request |
| Processes | At spawn, the command | One per spawn |

On raw sockets the address check sees an IP address, not a name, so a domain rule needs the host to record which addresses each program's own lookups resolved, for which domain, and check a connection's address against that record, as Little Snitch does; HTTP sees the domain itself.

`wasmtime-wasi` 49 already has the hooks, so permissions sit on its own implementation: `WasiCtxBuilder::preopened_dir(host, guest, dir_perms, file_perms)` maps a folder; `socket_addr_check(addr, use)` is called for every connect and bind; `allow_ip_name_lookup`, `allow_tcp` and `allow_udp` turn those on or off. Replacing an interface's implementation is the component model's own design (each interface is added to the linker on its own, and a host implements the generated traits), and comes with the providers of decision 5 of `2026-10-16-waos-v1.md`, files, processes and networking served by this computer or an SSH machine, not with permissions.

Turning a switch off while a program runs refuses new handles at once; whether those already open are closed is left for when it is built, since the host holds them.
