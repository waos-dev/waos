# Who owns what

Each thing WaOS keeps has one owner, which keeps every rule about it.

- **Packages** own what is installed, and every change to it. ([packages](/crates/waos-services/src/packages/mod.rs))
- **Permissions** own what the user lets each app use, and the questions being asked. ([permissions](/crates/waos-services/src/permissions/mod.rs))
- **The running apps** own what runs: one per app. ([running_apps.rs](/apps/waos/src/logic/running_apps.rs))
- **The app starter** owns the starts under way. ([app_starter.rs](/apps/waos/src/logic/app_starter.rs))
- **App storage** owns each app's own folders. ([app_storage](/crates/waos-services/src/app_storage/mod.rs))
- **The activity record** owns what each app did. ([activity](/crates/waos-services/src/activity/mod.rs))
  - *Today:* the engine counts the day and hands it over.
- **Sources** own the app listings and their downloads. ([sources](/crates/waos-services/src/sources/mod.rs))
- **Inspection and an app's entries in the system** own nothing: they follow. ([inspection.rs](/crates/waos-capabilities/src/inspection.rs))
- **Removing an app** takes everything that is its. ([manager.rs](/crates/waos-services/src/packages/manager.rs))
