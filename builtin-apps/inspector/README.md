# Inspector

WaOS's inspector, a program holding the inspector role, which it asks for by calling `wasdk::inspect` (wasdk's `inspector` feature): WaOS asks its user at its start whether it may see what other programs do, and starts it only with the role granted. It lists the programs on this device, itself among them, those that used something guarded lately first, each with its icon, read as its row is first shown and decoded by the system's codecs, its version, whether it runs, when it last used something and what (Camera, Network, Files…). The list is read again at each change the device tells: a program installed or removed, started or ended.

A program's page is layered (decisions 14–16 of `docs/proposals/2026-11-15-inspector-roles-and-compatibility.md`). Its first tab is the report, what it did over the last seven days as iOS's App Privacy Report and Android's Privacy dashboard show an app's: each kind of access with a strip of the days it had it, the domains it contacted ranked with a bar each, the questions it put, and a way on to each live panel. The other tabs are the live panels, as Chrome DevTools has them, each a table of what the program does from when the panel shows: Network (lookups, connections, requests joined to their responses), Files (opens and changes, where each path is on this computer), Devices (the camera, the microphone, the machines and commands), Output (its standard output and errors, a line each) and In use (what it uses now: files open, connections with what they moved, captures). A refused event says which setting refused it and where to change it. Only the panel shown is listened to; it keeps its last 2,000 rows. Each panel can start its program inspected, or restart it: WaOS starts it held before its first instruction until the panel listens, so launch-time events, the camera opening, are caught (decision 18).

In Fluent's controls from 600 logical points wide: the list a selectable `ItemsView` of `ItemContainer`s beside the page, its tabs a `SelectorBar`, the panels tables drawn with what Waft has until WinUI's table is ported. Narrower, as on a phone, in Cupertino's: the list, then the report, then each panel, a page each with a way back.

- **Takes:** `wasdk` with its `inspector` feature; `wasdk-adapter-waft`, Waft, its Fluent and Cupertino kits; `waos-activity`.
- **Provides:** the program `dev.waos.inspector`, named Inspector.
- **Used by:** whoever wants to see what their programs do.

```sh
cargo waos run -p inspector
cargo waos set role.inspector allow -p inspector   # answered ahead, from its next start
```

`icon.png` is WaOS's default program icon for now, until the app has one drawn as the others are.
