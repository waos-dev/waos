# Dev tools

- **`waos` drives a running WaOS from the terminal,** for people and agents alike: install, launch, attach and stop; screenshots, input, settings and inspection; and Waft's own tools through `waos ext`. ([dev_server.rs](/apps/waos/src/logic/dev_server.rs))
  - *Why:* an agent should be able to build and check an app with nobody at the screen.
- **A run is install, launch, attach and stop,** as Android, iOS and Flutter run apps. `waos run` does all four, and Ctrl-C stops the app; closing an attach alone leaves the app running. ([dev_launch.rs](/apps/waos/src/logic/dev_launch.rs), [dev_attach.rs](/apps/waos/src/logic/dev_attach.rs))
- **The tools reach any running app,** however it was installed or started: the socket is its user's alone. ([dev_server.rs](/apps/waos/src/logic/dev_server.rs))
- **Input from the tools is the user's input,** so the app can't tell the two apart. It's answered once handed over, saying whether the window was in sight; whoever expects a picture waits for it. While WaOS asks the user something, the input waits behind the question, as a click would. ([dev_input.rs](/apps/waos/src/logic/dev_input.rs))
- **While attached, an app's windows count as in sight,** even when covered, so screenshots and input meet windows that draw. ([dev_attach.rs](/apps/waos/src/logic/dev_attach.rs))
- **Waft's tools see WaOS as one more platform:** `waos ext` carries their calls to the app and back, reading nothing of them. ([dev_extensions.rs](/apps/waos/src/logic/dev_extensions.rs))
- **A `waos` and a WaOS that don't match refuse each other clearly,** naming both versions and which to update, before anything is sent. ([waos-dev-protocol](/crates/waos-dev-protocol/src/lib.rs))
