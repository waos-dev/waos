# WaOS

WaOS runs apps: WebAssembly components that draw with WebGPU, each in a sandbox with only what its user allows, using the system's own windows, menus and dialogs. One build runs on every platform WaOS runs on.

- **[Apps](apps/)**: what an app is and how it reaches a user. [Packages](apps/packages.md), [sources](apps/sources.md).
- **[Engine](engine/)**: runs apps and serves them the interface. [Interface](engine/interface.md), [lifecycle](engine/lifecycle.md), [threads](engine/threads.md), [windows](engine/windows.md), [permissions](engine/permissions.md), [files](engine/files.md), [machines](engine/machines.md), [network](engine/network.md), [media](engine/media.md), [system](engine/system.md).
- **[Shell](shell/)**: the WaOS app, showing apps on each platform. [Desktop](shell/desktop.md), [phone](shell/phone.md), [library](shell/library.md).
- **[Tools](tools/)**: building, driving and testing apps. [Dev tools](tools/dev-tools.md), [tests](tools/tests.md).
