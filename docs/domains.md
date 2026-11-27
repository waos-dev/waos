# Who owns what

Each thing WaOS keeps has one owner, a domain, which keeps every rule about it. A domain is handed what it needs from outside when it is made, and the app's startup puts the domains together. *Why:* a rule kept by its callers is forgotten by the next caller.

- **Packages own what is installed and every change to it.** An app is always one whole version. What is checked is what is installed. An update comes from the same publisher. Installed means ready to open. An app is stopped before it changes and does not start while it changes. Every change is announced. An install in flight is a session with its progress and cancel, from its first byte; a caller's newer session for an app replaces its older one, and a removal cancels them all. Handed: a compiler, and a way to stop an app. ([waos-package-manager](/crates/waos-package-manager/src/lib.rs))
  - *Today:* its folders are where `waos-store` kept them: `programs/<id>/` holds the app's versions beside the settings and own folders `waos-store` keeps there, and a removal moves that folder away whole, those with it; `waos-store` forgets what it holds of the app when it hears the removal, wired at the app's startup. Only who asked is recorded of a caller, and no app asks yet.
- **Permissions own what the user lets each app use, and the questions being asked now.** A question is asked once, however many wait for it, and its answer is kept. Handed: where it keeps the answers, and something to ask the user with.
  - *Today:* the answers are in `waos-store` ([grants.rs](/crates/waos-store/src/grants.rs)), the engine does the asking ([asking.rs](/crates/waos/src/asking.rs)), and the WaOS app keeps a second list of what is asked ([asking_now.rs](/apps/waos/src/logic/asking_now.rs)).
- **The running apps own what runs: one per app.** A start of an app that runs already takes its place or leaves it, never runs beside it. Handed: the engine. ([running_apps.rs](/apps/waos/src/logic/running_apps.rs))
  - *Today:* handed nothing: whoever starts an app runs its program, since the engine is made after the running apps, which the inspector's view of the engine's apps reads.
- **The app starter owns the starts under way:** from an app's id to its running, by one path whoever asks, compiling its code first where the engine cannot use what is there. Handed: the running apps, packages, permissions, app storage.
  - *Today:* also handed the engine, to run the program; permissions and app storage reach it through the store's grants and folders. ([app_starter.rs](/apps/waos/src/logic/app_starter.rs))
- **App storage owns each app's own folders,** which the app sees at `/data` and `/cache`.
  - *Today:* inside `waos-store`, in the app's folder beside its versions; `/tmp` is the engine's.
- **The activity record owns what each app did.**
  - *Today:* counted in the engine and kept by `waos-store`, and whether to record is read in both.
- **Sources own the app listings and their downloads.** Handed: packages, to install into.
  - *Today:* inside `waos-store`, handed nothing: the library feeds a download's bytes to a package manager's session.
- **Inspection and the system's entries for an app (the Start menu, app bundles, shortcuts) own nothing.** They follow the domains' changes, and no domain knows them.
  - *Today:* the engine is handed the inspector's service before the engine exists, and the loop is closed by a value set later.
- **Removing an app takes everything that is its.** The package manager takes the app out of its record in one step, and each domain forgets its part when it hears.
  - *Today:* the package manager moves the app's folder away whole, the settings and own folders `waos-store` keeps there with it, and `waos-store` forgets the rest when it hears.

What the engine is handed (the shell, the device, each run's environment) is under [Engine](engine/).
