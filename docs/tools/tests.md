# Tests

- **A test waits for what it expects, never on the clock,** with one deadline that only a broken build reaches. A "nothing happened" check comes after something later did, never after a sleep. ([test_shell](/crates/waos/tests/all/test_shell/mod.rs))
- **A test moves the engine's time instead of waiting for it,** so a freeze's delay passes at once. ([lifecycle](/crates/waos/src/lifecycle/mod.rs))
- **A test holds what makes a state pass,** so a state that lasts a moment cannot be missed: the test of "Optimizing…" holds the compile until it has seen it. ([test_installs.rs](/apps/waos/src/logic/test_installs.rs))
- **A test runs the real domains, and stands in only for the slow outside:** the package manager with a compiler that writes a marker file, settings kept in memory, the user's answers given by the test. *Why:* a fake of a domain keeps none of its rules. ([test_installs.rs](/apps/waos/src/logic/test_installs.rs))
- **What is costly to make is made once for a run:** the test apps are compiled once and shared by every test. ([build.rs](/crates/waos/tests/programs/build.rs))
- **The engine's tests run side by side in one process,** on a headless shell where the test plays every part of the platform (the user's answers, the clipboard, dialogs, cameras), each reading only its own shell and its own apps' output. ([test_shell](/crates/waos/tests/all/test_shell/mod.rs))
- **Each system tests what concerns it,** listed in one place. ([ci.yml](/.github/workflows/ci.yml))
