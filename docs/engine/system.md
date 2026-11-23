# System

- **An app uses the system's own services, not lookalikes:** its clipboard, menus and tray icons, notifications, file dialogs and fonts, and the user's settings (light or dark, languages, time zone). ([system](/crates/waos/src/system/mod.rs))
  - *Why:* a WaOS app should have the full power of a native app.
- **The user's own copy and paste always work,** whatever the app's clipboard switches say: they're the user's act. Otherwise an app reads the clipboard, and hears it change, only while one of its windows has the focus. A pasted file is mounted read-only. ([clipboard](/crates/waos/src/clipboard/mod.rs))
- **An app that copies can offer formats it hasn't made yet.** An app copying an image can offer PNG and JPEG and make the one a paste asks for only then, even if it has since been frozen: the paste wakes it. ([clipboard](/crates/waos/src/clipboard/mod.rs))
- **A notification is posted under the app's own id;** posting the same id replaces it. A click that starts the app waits until the app listens. ([notifications](/crates/waos/src/notifications/mod.rs))
- **An app sees only the font faces it was handed,** by ids of its own, so it can't reach a face another app was given. ([fonts](/crates/waos/src/fonts/mod.rs))
- **Opening links and files** follows [what the user controls](/docs/engine/control.md).
