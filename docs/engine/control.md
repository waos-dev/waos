# What the user controls

- **The user decides what each app can reach:** what it may use, and also how much CPU and disk it takes and whether it runs in the background. WaOS is open, and the user is in control, not a vendor. ([access](/crates/waos/src/access/mod.rs))
  - *Today:* the user controls what an app may use (below); CPU and disk have no limits yet. ([TODO](/TODO.md))
- **What an app uses constantly is a switch; what it uses rarely is a question.** The network, the local network, and reading and writing the clipboard are switches in the app's settings in WaOS: asking for one opens that page, as an iOS app sends its user to Settings, and only the user turns it on. Notifications, the camera and the microphone ask at first use. ([permissions.wit](/wit/deps/wasdk-platform-2.2.0/permissions.wit))
- **An app's own things need no permission:** its `/data`, `/cache` and `/tmp`, a file its user picks, watching a folder it holds, sound, menus and dialogs. A machine is given by the user picking it. ([permissions.wit](/wit/deps/wasdk-platform-2.2.0/permissions.wit))
- **Opening a link or a file in another app needs the user's say.** ([opener](/crates/waos/src/opener))
  - *Today:* web links and files open freely; a link of another scheme asks once. ([TODO](/TODO.md))
- **Each app follows WaOS's defaults until its user sets its own:** the network on for any domain, the local network off, the clipboard on both ways, nothing mounted, every question asked. ([settings.rs](/crates/waos-store/src/settings.rs))
- **A change holds from the app's next call.** Nothing keeps a copy of a grant: each use reads the user's settings as they are, so turning something off works at once. ([permissions](/crates/waos/src/permissions/mod.rs))
- **Settings that can't be read grant nothing.** A damaged settings file turns every switch off and denies every question, rather than giving more than the user gave. ([grants.rs](/crates/waos-store/src/grants.rs))
- **A question can be answered for now or for good:** allow once, allow, deny. Asking again while it shows doesn't stack a second question; both asks get the one answer. ([asking.rs](/crates/waos/src/asking.rs))
  - *Today:* every answer is kept until the user changes it; there's no "allow once". ([TODO](/TODO.md))
- **The app's settings answer a question that's waiting.** Turn the camera on in the app's settings while "Allow Camera?" shows, and the dialog goes, the app gets its answer. ([asking.rs](/crates/waos/src/asking.rs))
- **A permission has two layers, and an app gets a thing only when both allow it:** what its user allows in WaOS, and what the system allows WaOS. When the user allows something the system also guards, WaOS asks the system at once, in the system's own way, and tells the user if the system still refuses. Settings show both. ([system_permissions.rs](/crates/waos/src/permissions/system_permissions.rs))
  - *Why:* no system tells an app when its user changes a permission in the system's settings, so WaOS reads the system's answer each time rather than keeping a copy.
- **Every guarded use is decided in one place and recorded.** What an app uses, and how often it's started, is counted day by day, kept seven days on this device, for the inspector, the app's settings page and the home's often-used apps. ([access](/crates/waos/src/access/mod.rs))
- **A role comes only from what the app's code imports,** such as the inspector's, and the user grants it by name at the app's start. Refused, the app doesn't start, and is told why. ([roles.rs](/crates/waos/src/roles.rs))
  - *Why:* what an app can call is what it imports, so the role can't claim more or less than the code uses, and there's no declaration to keep in step with the code.
