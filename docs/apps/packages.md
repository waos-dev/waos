# Packages

- **An app, or a tool.** An app has a UI; a tool is a command-line app with none. An app is a tool only when its manifest says `kind = "tool"`. ([manifest.rs](/crates/waos-package/src/manifest.rs))
- **A `.wapk` is the app, its icon, and its publisher's signature.** The signature covers the manifest, the component and the icon (256 px or more). Other files in it are ignored, so a later version of the format can add its own. ([waos-package](/crates/waos-package/src/lib.rs))
- **Nothing in a package is believed before its signature checks.** ([package.rs](/crates/waos-package/src/package.rs))
- **An update must come from the same publisher, and be newer.** WaOS trusts the key an app was first installed with; a package signed by another key is a different publisher and never replaces it. Any source can offer the update; the key decides. ([store.rs](/crates/waos-store/src/store.rs))
- **An install never half-happens.** Switching which version is current is the one step that installs, and moving the app's folder away the one step that removes, so a crash leaves the old install or the new one, never part of either. ([programs.rs](/crates/waos-store/src/programs.rs))
- **Removing an app removes all of it:** its data, cache, settings and activity record. ([waos-store](/crates/waos-store/src/lib.rs))
- **An app's code compiles when it installs, not when it first opens,** as Android does, so the first open is fast. It compiles again at open only after a WaOS update changed the engine. ([installed_code.rs](/apps/waos/src/logic/installed_code.rs))
- **A development install sits beside the published app.** `waos install` installs from source as `<id>.development`, with its own data and settings and " (Development)" after its name. No package can take that id, so no update replaces it. ([store.rs](/crates/waos-store/src/store.rs))
