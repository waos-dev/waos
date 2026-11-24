# Phone

- **On a phone, each app gets its own place where the platform allows it:** on Android a task of its own, with its own Recents card, unless the user chooses tabs; tabs inside WaOS where the platform allows only one app, as iOS does. Installed apps get shortcuts on Android's home screen and on WaOS's icon's menu. ([programs_open.rs](/apps/waos/src/state/library/programs_open.rs), [program_shortcuts.rs](/apps/waos/src/logic/program_shortcuts.rs))
- **In tabs, WaOS is one screen:** the home, and each app opened from it. ([deck](/apps/waos/src/state/deck/mod.rs))
- **In tabs, an app's tab opens at once and waits for the app.** It shows the app starting until the app's first window fills it. If the app ends, the tab keeps its last picture and says how it ended, until the user closes it. ([tabs.rs](/apps/waos/src/state/deck/tabs.rs))
- **In tabs, each further window is a tab of its own, and closing an app's last tab ends the app** without asking, as a phone's app switcher does. ([tabs.rs](/apps/waos/src/state/deck/tabs.rs))
- **Back works as on the phone:** the overview closes, an app that handles Back gets it, any other app's page goes home, and on the home's first page Back goes to the system. ([back.rs](/apps/waos/src/ui/deck/back.rs))
- **A dialog for an app with no screen opens one for it,** closed once the dialog is answered. ([file_dialogs.rs](/apps/waos/src/state/file_dialogs.rs))
- **On Android, This computer is the phone's storage once All files access is on.** WaOS opens Android's page for it when This computer is picked, and says so if it's still off; its settings open the page too. Until then, This computer's home is WaOS's own folder. ([phone_files.rs](/apps/waos/src/state/phone_files.rs))
- **On Android, an app reads what its user picks on the phone.** ([phone_storage.rs](/crates/waos-program-window/src/logic/phone_storage.rs))
  - *Today:* only files on the phone's own storage, and only with All files access on; WaOS opens Android's page for it at the pick. Reading a pick through Android's own handle, with no extra access, is planned. ([TODO](/TODO.md))
- **WaOS asks the system only from the main thread,** and reads again what it needs when WaOS comes back to the front, where the user returns from changing it. Android ends WaOS when another thread asks. ([phone_files.rs](/apps/waos/src/state/phone_files.rs))
