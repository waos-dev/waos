# Tests

- **A test waits for what it expects, never on the clock,** with one deadline that only a broken build reaches. A "nothing happened" check comes after something later did, never after a sleep. ([test_shell](/crates/waos/tests/all/test_shell/mod.rs))
- **A test moves the engine's time instead of waiting for it,** so a freeze's delay passes at once. ([lifecycle](/crates/waos/src/lifecycle/mod.rs))
- **The engine's tests run side by side in one process,** on a headless shell where the test plays every part of the platform (the user's answers, the clipboard, dialogs, cameras), each reading only its own shell and its own apps' output. ([test_shell](/crates/waos/tests/all/test_shell/mod.rs))
- **Each system tests what concerns it,** listed in one place. ([ci.yml](/.github/workflows/ci.yml))
