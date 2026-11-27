# Who owns what

Each thing WaOS keeps has one owner, a domain, which keeps every rule about it. A domain is handed what it needs from outside when it is made, and the app's startup puts the domains together. *Why:* a rule kept by its callers is forgotten by the next caller.

- **Packages own what is installed and every change to it.** An app is always one whole version. An update comes from the same publisher. Installed means ready to open. An app is stopped before it changes and does not start while it changes. Handed: a compiler, and a way to stop an app. ([package_manager.rs](/apps/waos/src/logic/package_manager.rs), [store.rs](/crates/waos-store/src/store.rs))
  - *Today:* split between the WaOS app's package manager and `waos-store`. What is being done to an app is one label, not an install under way with its own progress and cancel, so removing an app while its update downloads brings it back.
- **Permissions own what the user lets each app use, and the questions being asked now.** A question is asked once, however many wait for it, and its answer is kept. Handed: where it keeps the answers, and something to ask the user with.
  - *Today:* the answers are in `waos-store` ([grants.rs](/crates/waos-store/src/grants.rs)), the engine does the asking ([asking.rs](/crates/waos/src/asking.rs)), and the WaOS app keeps a second list of what is asked ([asking_now.rs](/apps/waos/src/logic/asking_now.rs)).
- **The running apps own what runs: one per app.** A start of an app that runs already takes its place or leaves it, never runs beside it. Handed: the engine. ([runs.rs](/apps/waos/src/logic/runs.rs))
- **The app starter owns the starts under way:** from an app's id to its running, by one path whoever asks, compiling its code first where the engine cannot use what is there. Handed: the running apps, packages, permissions, app storage.
  - *Today:* three entrances (`open`, `launch`, `start_held`) on one type whose fields its callers reach into ([installed_runs.rs](/apps/waos/src/logic/installed_runs.rs)).
- **App storage owns each app's own folders,** which the app sees at `/data` and `/cache`.
  - *Today:* inside `waos-store`, in the app's folder beside its versions; `/tmp` is the engine's.
- **The activity record owns what each app did.**
  - *Today:* counted in the engine and kept by `waos-store`, and whether to record is read in both.
- **Sources own the app listings and their downloads.** Handed: packages, to install into.
  - *Today:* inside `waos-store`.
- **Inspection and the system's entries for an app (the Start menu, app bundles, shortcuts) own nothing.** They follow the domains' changes, and no domain knows them.
  - *Today:* the engine is handed the inspector's service before the engine exists, and the loop is closed by a value set later.
- **Removing an app takes everything that is its.** The package manager takes the app out of its record in one step, and each domain forgets its part when it hears.
  - *Today:* `Store::remove` moves the app's folder away whole and calls the others itself.

What the engine is handed (the shell, the device, each run's environment) is under [Engine](engine/).
