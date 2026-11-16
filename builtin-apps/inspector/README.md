# Inspector

WaOS's inspector, a program holding the inspector role, which it asks for by calling `wasdk::inspect` (wasdk's `inspector` feature): WaOS asks its user at its start whether it may see what other programs do, and starts it only with the role granted. It lists the programs on this device, itself among them, those that used something guarded lately first, each with its icon, read as its row is first shown, its version, whether it runs, when it last used something and what (Camera, Network, Files…); and says for the one chosen what it did over the last seven days, a section per capability, in the words `waos-activity` says them in, which WaOS's settings page says the same days in, in short. The list is read again at each change the device tells: a program installed or removed, started or ended. In Fluent's controls from 600 logical points wide, the list a selectable `ItemsView` of `ItemContainer`s beside what the program selected did; narrower, as on a phone, in Cupertino's, the list, then a page of each program with a way back.

- **Takes:** `wasdk` with its `inspector` feature; `wasdk-adapter-waft`, Waft, its Fluent and Cupertino kits; `waos-activity`.
- **Provides:** the program `dev.waos.inspector`, named Inspector.
- **Used by:** whoever wants to see what their programs do.

```sh
cargo waos run -p inspector
cargo waos set role.inspector allow -p inspector   # answered ahead, from its next start
```

`icon.png` is WaOS's default program icon for now, until the app has one drawn as the others are.
