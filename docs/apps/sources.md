# Sources

- **A source is a list of apps:** an `index.json` at a URL, or a GitHub repository's latest release. ([source.rs](../../crates/waos-store/src/source.rs))
- **A source is read over HTTPS,** or over plain HTTP only from this machine. The list isn't signed, and it says which package a first install trusts. ([source.rs](../../crates/waos-store/src/source.rs))
- **A download must match the list.** The list gives each package's checksum; a file that doesn't match (corrupted in transit, or swapped on the server) isn't installed. The checksum only proves it's the file the list meant; whether it may replace an installed app is still decided by the publisher's key. ([store.rs](../../crates/waos-store/src/store.rs))
- **The official source comes with WaOS, and can be removed.** WaOS adds `list.waos.app` when the user has never had a list of sources; once removed, it never comes back. ([state::library](../../apps/waos/src/state/library/mod.rs))
- **The official source checks who publishes.** It builds nothing: each app releases from its own repository, and the catalog (`waos-dev/apps`) pins each app's publisher key and refuses a package signed by another. Without that, a source compromised upstream would reach every new install, since WaOS trusts the first key it sees. ([catalog.toml](https://github.com/waos-dev/apps))
