# Tests without the clock

Written 2026-11-22, after `main`'s CI went red on three tests that each passed on another run (PR #58). Two waited for a program's output from the wrong moment; the third, `waos tap`'s answer, waited up to two seconds for a picture drawn after the program read the whole tap, which never comes from a program that redraws on the press. The first fix for the third added about 350 lines to the engine so WaOS could tell when a program has finished reacting. The lead stopped it: the goal is tests and CI that say only what they need, and code that does not earn its place goes.

## What is wrong today

- **Tests wait on the clock.** About a dozen sleep before asserting nothing happened, wait for a number of frames, or wait for quiet: `drawing.rs`'s 400 ms of quiet, `video_player.rs`'s 500 ms, `permissions.rs` and `opener.rs`'s 50 to 100 ms, `two_windows.rs`'s settling frames, `TestPhone::wait_until_running_still`'s 500 ms, `headless_entry`'s 150 ms idle timeout. On a busy machine each can lose.
- **The engine's tests read one log every program writes to** (`shell::output()`, `shell::turns`). A test marks where its program's lines start, and a mark taken late misses them (flake 1). Three test binaries (`frames_in_flight`, `two_windows`, `waft_app`) exist only so the frames and glyphs that log records are one program's; each recompiles the test shell and relinks the engine.
- **Three shells stand in for a screen**: the engine's test shell (`crates/waos/tests/all/shell`, about 2,500 lines, fakes for every capability), the app's virtual shell (`logic/virtual_shell.rs`, what `waos --headless` runs on and the app's drawing tests use, refreshing on a 16 ms timer), and Waft's test platform under the app's tests. Each fakes part of the same things.
- **The app has four harnesses of its own** (`state/library/test_desktop.rs`, `state/deck/test_phone.rs`, `state/tests.rs`, `state/test_system.rs`, about 3,300 lines), each implementing Waft's platform for tests and its own pumping, finders and taps. Waft has a tester, `WidgetTester` with `TestPlatform`, since 0.6.0; the harnesses came over in WaOS's first commit, two days before it.
- **The dev tools drive programs their own way**: `waos tap` has a mouse and keys of its own (`logic/dev_input`), apart from what the tests give programs.
- **CI guessed what each system tests** from crates' source (`.github/platform-test-packages.sh`), and so never ran `waos-local-processes`' Windows tests.

## The lead's decisions

1. **What tests and CI need is five rules, and code that keeps them; the rest goes** (2026-11-22). Where changing the old code step by step is hard, it is dropped and written again, rather than carried along.
   1. Red means broken: a test waits for what it expects, with one deadline that only a broken build reaches, never on the clock.
   2. A test reads only its own program: its output, its windows and its pictures.
   3. "Nothing happened" is checked only after something later did: the program answered the next request, so the earlier one was ignored. A sleep never stands in for it.
   4. One way to drive a program: start WaOS headless, run the program, give it input, read what it prints and draws, the same for the engine's tests, the app's tests, the dev tools and program authors.
   5. Each system tests what concerns it, listed in one place, CI's workflow.

2. **Whoever expects a change waits for it; WaOS does not guess when a program is done** (2026-11-22). `waos tap` and the other inputs answer once the input is handed to the program, saying whether the window was in sight; `new_picture` goes, and with it the two-second wait. Only the test or the agent knows what an input should change, so it waits for that, as Playwright's assertions retry and it tells you not to wait for the network to go idle. Knowing that a program will draw nothing more needed the engine to watch its yields, its frame requests and its frames in flight, about 350 lines, for one message the command printed. An agent takes a screenshot, and another if the program has not drawn yet. The protocol's `MAJOR` rose to 2 for it, so an older `waos` says to update rather than read a field that is gone.

3. **The one way to drive a program is a crate over the engine, `waos-headless`, and the dev tools are a thin layer on it** (2026-11-22). Not the app driven through the dev protocol: the engine's tests would then depend on the app, the wrong way round, and every test would pay for a second process. Flutter's split is the model: `flutter_test` over the engine, `flutter drive` a thin layer on top. The dev server answers `waos tap` and `screenshot` through the crate's driving types, on the desktop app as well as headless, so there is one mouse.

4. **One headless shell serves the tests and `waos --headless`** (2026-11-22), so what a program sees on a headless Linux server is what the tests check. It is made of the engine's shell parts, one per job (placement, vsync, compositor), and whoever builds the engine plugs in the rest: the tests their fakes, the daemon the real local capabilities, as the app does. The tests' screen refreshes as soon as a frame is asked for, the daemon's sixty times a second; the tests run on time that moves only when the test says (`EngineTime::Manual`).

5. **The app's UI tests use Waft's tester** (2026-11-22): one `TestApp` over `WidgetTester`, its form factor a parameter, WaOS adding only what is its own, a store in a folder of the test's own and the engine under the app. What the tester lacks is fixed in Waft, for every Waft app: several views at once, and a finger's lift, which `TestPhone` works around today.

## The design

```rust
// crates/waos-headless: a screen with no display, and the way to drive a program
pub struct HeadlessScreen { … }             // placement, vsync, compositor: the engine's shell parts
pub enum Refreshes {
    WhenAsked,                              // tests: a frame as soon as one is asked for
    Every(Duration),                        // the daemon: 60 Hz
}
pub struct Fakes { … }                      // questions, clipboard, cameras…, from today's test shell

pub struct DrivenProgram { … }              // a program a tool drives: its own output, its windows
pub struct DrivenWindow { … }               // over any ProgramView, headless or a real window
impl DrivenWindow {
    pub fn tap(&self, x: f64, y: f64);      // the dev mouse and keys, moved here
    pub fn picture(&self) -> Option<Picture>;
    pub async fn picture_until(&self, check: impl Fn(&Picture) -> bool) -> Picture;
}
impl DrivenProgram {
    pub async fn line_until(&self, line: &str);
}
```

```rust
// a test
let screen = HeadlessScreen::new(display, Refreshes::WhenAsked);
let engine = WaOS::new(screen.shell_with(Fakes::default()))?;
let input = screen.run(&engine, INPUT);
let window = input.window(1).await;
window.tap(40.0, 30.0);
window.picture_until(|picture| picture.middle() == ORANGE).await;

// apps/waos headless_entry: the daemon
let screen = HeadlessScreen::new(display, Refreshes::Every(Duration::from_millis(16)));
let engine = WaOS::new(screen.shell_with(LocalCapabilities::new(..)))?;
```

What goes: the engine's test shell and its shared log, the three extra test binaries, `virtual_shell`, the four app harnesses, the sleeps and settling frames in tests, the CI script and the `[package.metadata.waos] systems` blocks only it read, and Linux CI's filter list of the app's tests.

## The work

Each step a pull request that builds and passes.

1. **PR #58**: the two flakes that marked a program's output late or announced a window before taking its events, fixed in the tests and the examples; `waos tap` answering once the input is handed over (decision 2); CI listing each system's test packages.
2. **This proposal.** The rules go into a "Tests" section of `docs/essentials.md` with the step that makes each true: the dev tools' answer and CI's list with step 1, the tests' rules with step 5.
3. **`waos-headless`**, built from the virtual shell, the engine's test shell and the dev mouse and keys, with tests of its own.
4. **The daemon and the dev server on it**: `virtual_shell` goes, and Linux CI runs the app's dev tests without a filter.
5. **The engine's tests on it**, file by file: the test shell, the shared log and the extra binaries go, and a test that only repeats a unit test is dropped.
6. **Waft's tester's gaps fixed in Waft** (several views, the finger's lift), then the app's tests on `TestApp`, and the four harnesses go.

## Not in this proposal

- `waos resize` waits two seconds for a window whose system never reports a size. Whether the shell can say it kept the size is a question for step 4.
- A way for program authors to use `waos-headless` from their own `cargo test` is a goal of decision 3; its documentation comes once the crate exists.
