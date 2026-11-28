# Who owns what

Each thing WaOS keeps has one owner, a domain, which keeps every rule about it. A domain is handed what it needs from outside when it is made, and the app's startup puts the domains together. *Why:* a rule kept by its callers is forgotten by the next caller.

- **Packages own what is installed and every change to it.** An app is always one whole version. What is checked is what is installed. An update comes from the same publisher. Installed means ready to open. An app is stopped before it changes and does not start while it changes. Every change is announced. An install in flight is a session with its progress and cancel, from the moment it is asked, and the removals under way are told beside them; a caller's newer session for an app replaces its older one, and a removal cancels them all. Handed: a compiler, and a way to stop an app. ([packages](/crates/waos-services/src/packages/mod.rs))
  - *Today:* its folders are where `waos-store` kept them: `programs/<id>/` holds the app's versions beside the settings Permissions keeps there and the own folders app storage keeps there, and a removal moves that folder away whole, those with it; `waos-store` forgets the app's activity when it hears the removal, wired at the app's startup. Only who asked is recorded of a caller, and no app asks yet.
- **Permissions own what the user lets each app use, and the questions being asked now.** The engine only checks: a run waits on its grants, which answer with what is kept or ask the user. A question is asked once, however many wait for it, and its answer is kept. The questions asked now are one list, which the dev tools report. An app being removed takes no write, and one removed is forgotten. Handed: where it keeps the answers, something to ask the user with, and the package manager's record. ([permissions](/crates/waos-services/src/permissions/mod.rs))
  - *Today:* the WaOS app shows a question over the app's run, which it finds among the running apps, waiting for a run asked about as it starts to be listed. ([prompts.rs](/apps/waos/src/logic/prompts.rs))
- **The running apps own what runs: one per app.** A start of an app that runs already takes its place or leaves it, never runs beside it. Handed: the engine. ([running_apps.rs](/apps/waos/src/logic/running_apps.rs))
  - *Today:* handed nothing: whoever starts an app runs its program, since the engine is made after the running apps, which the inspector's view of the engine's apps reads.
- **The app starter owns the starts under way:** from an app's id to its running, by one path whoever asks, compiling its code first where the engine cannot use what is there. Handed: the running apps, packages, permissions, app storage.
  - *Today:* also handed the engine, to run the program; permissions and app storage reach it through each run's environment, its grants and the store's folders. ([app_starter.rs](/apps/waos/src/logic/app_starter.rs))
- **App storage owns each app's own folders,** which the app sees at `/data` and `/cache`. ([app_storage](/crates/waos-services/src/app_storage/mod.rs))
  - *Today:* they sit in the app's folder beside its versions, which the package manager removes whole, these with it; `/tmp` is the engine's.
- **The activity record owns what each app did.**
  - *Today:* counted in the engine and kept by `waos-store`, and whether to record is read in both, from the app's settings, which `waos-store` follows to remove what it kept of an app turned off.
- **Sources own the app listings and their downloads.** Handed: where they keep the list of sources, a folder for the pictures they read, and packages, to install into. ([sources](/crates/waos-services/src/sources/mod.rs))
  - *Today:* handed no packages: the library feeds a download's bytes to a package manager's session.
- **Inspection and the system's entries for an app (the Start menu, app bundles, shortcuts) own nothing.** They follow the domains' changes, and no domain knows them.
  - *Today:* the engine is handed the inspector's service before the engine exists, and the loop is closed by a value set later.
- **Removing an app takes everything that is its.** The package manager takes the app out of its record in one step, and each domain forgets its part when it hears.
  - *Today:* the package manager moves the app's folder away whole, the settings and own folders kept there with it; Permissions refuses a settings write while the removal is under way and forgets the app before its next read or change, and `waos-store` forgets the app's activity when it hears.

What the engine is handed (the shell, the device, each run's environment) is under [Engine](engine/).
