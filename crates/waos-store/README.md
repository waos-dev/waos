# waos-store

The programs installed in WaOS, their updates, and the package sources they come from, kept in a directory the app gives it. An app bundling one program builds none of it.

- **Takes:** a directory, from the app; packages, opened and verified by `waos-package`; sources' indexes and packages, read over HTTPS through the proxy `Store::open` is given (`SourceProxy`: the system's, as an app reads them, or none, as the tests read, whatever the machine's settings); `waos-capabilities`, whose `ProgramGrants` it implements and whose values its settings are made of.
- **Provides:** `Store`, with the installed programs, the rules that install, update and remove them, the component of each to run and its icon, each program's own folders (`folders`, its `/data` and `/cache`, installed or run in development), the programs' audit summaries, as the engine's `AuditLog` (`audit`: a file per program and day, kept seven days, on this device only, removed with the program), and which programs days are kept of (`programs_with_days`), the sources the user added, and reading and installing from a source; `InstalledProgram`, a program's manifest and its publisher's key; `Source`, an index's URL or a GitHub repository; what the user lets programs use, `GlobalSettings`, which every program follows until it has its own, and `ProgramSettings`, a program's own, each setting a `Setting` (the values re-exported from `waos-capabilities`), and `AnswerWith`, the answers its user set for its pickers and file dialogs, with each program's `ProgramGrants` over them; and `StoreError`, `InstallError` and `SourceError`, which say what is wrong in plain words.
- **Used by:** the WaOS app, whose library shows the programs installed, the sources' programs to install or update and the settings, and which gives each program it runs the store's grants and its folders.

```rust
let store = Store::open(&data_directory.join("store"))?;

// A .wapk opened with WaOS.
store.install(Package::open(&path)?)?;

// A source the user added, and what it offers.
let source: Source = "github.com/example/triangle".parse()?;
store.add_source(source.clone())?;
let index = store.read_source(&source).await?;
let triangle = &index.programs[0];
store.install_from(&source, triangle).await?;

let component = store.program(&triangle.id); // what the app runs

// What the user lets it use, which the engine reads at each check, and its own folders.
let grants = store.grants(&triangle.id);
let folders = store.folders(&triangle.id)?;
store.change_program_settings(&triangle.id, |settings| {
    settings.clipboard_read = Setting::Own(false);
})?;
```

## Installing and updating

An installed program is its manifest and its publisher's key, with no record of where it came from (the design's decision 46). `install` takes a package `waos-package` opened, which has verified its signature, and installs it when its id is not installed, or when the installed program has the same id, the package is signed with the same key, and its version is newer by semver's precedence. A different key is refused, naming both keys' fingerprints; a version not newer is refused, the same version included. A refused or failed install leaves the program installed as it was. A removed program is forgotten whole, its settings and its own folders with it: a package of its id installs again as a new one, whatever its key.

An update is any source listing an installed id at a newer version; whether it installs is `install`'s rule, so it installs only when signed with the installed program's key.

## Sources

A source is an `index.json` (the format is `waos-package`'s): an index at its own `https://` URL, or a GitHub repository, `github.com/owner/repository`, whose index and packages are assets of its latest release, read at `https://github.com/owner/repository/releases/latest/download/index.json` with no GitHub API. A listing's `package` is resolved against the index's URL, so a file name is a package beside the index. Since an index is not signed and says which package a first install trusts, an index is read over HTTPS, or over plain HTTP only from this machine (`localhost`, `127.0.0.1`, `[::1]`), such as `target/waos` served while making a package.

`read_source` reads a source's index, up to 4 MiB. `install_from` downloads a listed package to a temporary file, refuses it unless its SHA-256 is the one listed, before anything opens it, refuses it unless it is the program and version listed, and installs it by `install`'s rules; it opens and installs on tokio's threads for blocking work, and the temporary file goes after. It is `download` then `install_download`, which a caller calls apart to end what runs of the program in between, once the package is there. `remove` takes a program's folders with it, so its caller ends the program's runs first. Reading is done with reqwest over rustls, which checks a server's certificate with the platform's own verifier, follows redirects, as GitHub's release assets need, and gives up when a server takes 30 seconds to accept a connection or to send the next part of an answer.

The sources the user added are kept in the order added; adding one already there leaves it where it is, and removing one that is not there is refused.

## The directory

```text
<directory>/
  programs/
    dev.example.triangle/
      current           the name of the folder below that is installed
      1.2.0-Xk3q9a/     what the package unpacked to: manifest.toml, program.wasm, icon.png, publisher.key
      settings.toml     the program's own settings
      data/             the program's own folders, which it sees at /data and /cache
      cache/
  removing/             a program being removed, moved here whole first
  sources.toml          sources = ["github.com/example/triangle", "https://example.com/waos/index.json"]
  settings.toml         the global settings
```

A program's folder is made by its install or by its first run, whichever comes first, or by a setting written before either (`change_program_settings`), as `cargo waos set` does for a development program: `folders` makes `data` and `cache` in it, so a program run from `cargo waos` that is not installed has a folder with them and its settings, and an install of its id keeps them, as the installed program is the one its development runs were (decision 11 of `docs/proposals/2026-10-23-permissions.md`).

Installing is safe against a crash halfway. An install unpacks into a folder of its own under a fresh name, each file on disk before it goes on; then it replaces `current` whole, by writing the new name beside it and renaming it over, which is the one step that changes what is installed; then it removes the folder of the version it replaced. A removal moves the program's folder whole into `removing/`, the one step that removes it, then removes it there. So a crash at any point leaves the earlier install or the new one, the program or none of it, and `Store::open` clears what else it left: in each program's folder all but `current`, the folder it names, its settings and its own folders, and a folder left keeping nothing; and whatever is in `removing/`. `sources.toml` and each `settings.toml` are replaced whole the same way. One store opens a directory; its clones share it, and its changes happen one at a time.

A program whose files cannot be read is left out of `installed` and `program`, and a `sources.toml` that cannot be read leaves `sources` empty, each said as a `tracing` warning; changing the sources then fails rather than overwrite the file.

## Settings

What the user lets programs use is the global settings, which every program follows until the user sets its own, as Chrome's site settings have a default and exceptions per site (decision 8 of `docs/proposals/2026-10-23-permissions.md`). The global settings start as the design's defaults: the network on for any domain, the local network off, the clipboard on both ways, nothing mounted, and every question to be asked. The camera's and the microphone's answer may be `simulated` as well as granted or denied: allowed, with devices of WaOS's own; what a simulated microphone records, a quiet tone by default or silence, is `[simulated] microphone`, a setting of its own beside the answer, global or a program's (`SimulatedSound`). A program's own settings are each a `Setting`, the global one followed until the user sets it, kept in `settings.toml` in its folder, installed or run in development, and go with it when it is removed: a program installed again starts afresh. Both files have the same keys; a key missing from the global file is the default, and one missing from a program's follows the global setting, so only what the user set is written. A program's answers to opening links of a scheme of its own, any but `http`, `https` and `mailto`, are its own alone, under `[link-schemes]`, with no global default: each scheme is asked of its user once. So is its answer to holding a role it asks for by importing the role's interfaces, under `[roles]` (`inspector = "granted"`), asked at its start. Whether a record is kept of what a program does, its audit summary, is `record`, global, on by default, or a program's own (`keeps_record`); a change that turns it off for a program, its own or the global one it follows, removes the days kept of it, and no day is kept of a program while it is off. So are the machines its user picked for it, under `machines`, by their ids (`docs/proposals/2026-10-29-processes.md`, decision 3). And so are the answers its user set for its pickers and file dialogs, given in place of asking, under `[answer-with]`, each missing to ask: `machine`, a machine's id; `audio-output`, a sound output's id; and `open-file`, `open-files` (a list), `open-folder` and `save-file`, absolute paths on the machine (`AnswerWith`).

`global_settings` and `program_settings` read them, `change_global_settings` and `change_program_settings` change them through a closure, under the store's lock, so two changes never interleave; a program the store has no folder for, neither installed, run, nor set before its first run, has none, and reading keeps nothing. The settings are read from their files once and held in memory, so a check reads no file: `grants(id)` is the program's `ProgramGrants`, installed or run in development, its own settings over the global ones, read at each check, its answers kept in its settings, to its questions (`set_answer`) to its link schemes (`set_link_scheme_answer`) and to its roles (`set_role`), the machines picked for it (`keep_machine`, each once, which `machines` lists), and the answers set for its pickers and dialogs (`machine_answer`, `audio_output_answer`, `file_dialog_answer`). Every change, of the global settings, a program's, or an answer kept, is heard by each `changed` future asked for before it, and by `settings_changed`. Settings that cannot be read grant nothing: a program whose own file or the global one is damaged has every switch off, nothing mounted, no machine, every question and link scheme denied and no answer set for a picker or a dialog, rather than more than the user gave it, and the failure is a `tracing` warning.
