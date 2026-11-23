# When an app runs, pauses and freezes

- **An app's state follows its windows.** Focused window: resumed. On screen but not focused: inactive. None on screen: hidden. WaOS going to the background pauses every app, as a phone pauses an app that leaves the screen. ([state_of_views.rs](/crates/waos/src/lifecycle/state_of_views.rs))
- **In the background, an app runs only as its user allows.** Out of sight, an app is frozen, saving CPU, battery and GPU memory, unless its user lets it keep working there (music, a download). ([freeze_rule.rs](/crates/waos/src/lifecycle/freeze_rule.rs))
  - *Today:* there's no such setting. An app keeps running while it plays or records sound or runs a camera, so an app playing silence stays alive. ([TODO](/TODO.md))
- **A frozen app wakes to answer:** a quit request, a paste of a format it promised, a menu choice, its window closing. It can't answer while frozen. ([lifecycle](/crates/waos/src/lifecycle/mod.rs))
