# App freeze (design)

How the engine stops an app that is out of sight, designed again with the lead on 24 and 25 November 2026 and built on the branch `app-freeze`. What the systems do, which each decision follows, is in [docs/research/app-freeze](../../research/app-freeze/README.md).

## Vocabulary

One word for each concept, taken from the systems.

| Concept | Name | From |
|---|---|---|
| None of an app's views is on screen | hidden | the interface's `lifecycle-state`, Flutter |
| The system sends WaOS itself away, and back | suspend, resume | iOS, Windows |
| The engine stops an app's code, and lets it go on | freeze, unfreeze | Android's freezer, the web's Page Lifecycle |
| The time between being told and being frozen | grace period | Kubernetes, systemd |
| What prevents a freeze | freeze inhibitor | systemd-logind, GNOME, Wayland |
| What the engine lets an app do now | execution state | Windows, Apple |
| A system's settings for this | freeze policy | Chromium's `FreezingPolicy` |

"Thaw" is not used. "Wake lock" was considered and dropped: on Android a wake lock does not prevent the freezer, so the name would mislead.

## The model

1. A part of the engine that knows an app must keep running takes a freeze inhibitor, for as long as that is true.
2. One rule turns the app's lifecycle state, the clock, its inhibitors and the system's policy into one execution state: running, background or frozen.
3. The lifecycle publishes that state. Every part that treats an app differently when it is not running reads it at its own door. The app's thread is one of them: it does not poll a frozen app's run.
4. An app is told `hidden` or `paused` first, runs on for a time, then is frozen.

## What was decided, and why

- **An app is told first, runs for a fixed time, then is frozen.** Every system tells an app before it stops it, and a frozen app may be ended with no further word, so saving has to happen in that time. A fixed time, with no "I am done" from the app, is Android's shape; the call can be added later.
- **Each part asks one registry to prevent a freeze.** iOS's background-task assertions, macOS's activities and Chromium's `CannotFreezeReason` are this shape. Before, each reason had its own pair of calls and guard type, and the rule a branch for each.
- **The rule is one function of what it is handed.** It decides the same whoever applies it and whenever, and is tested as a table.
- **A freeze takes where the app's code is entered.** Each app instance's thread polls one future, its run; frozen, it does not. Before, the freeze took at an epoch tick inside the app's code, so it stopped only an app that was running, and a waiting one that was woken ran until its next check. Two tests hung on that gap.
- **A freeze does nothing to what an app waits for.** A timer or a read is polled inside the same run, so it is picked up when the app is unfrozen: a timer long due completes once, a read returns what arrived or the peer's close. This is a suspended process on iOS and a page in a browser's back/forward cache. Android's burst of missed timer ticks cannot happen, since each wait is one future. Closing a frozen app's sockets, as Android does to save the radio, can be added for phones.
- **No app draws while WaOS is suspended.** Nothing is on screen then, and iOS refuses GPU work from a suspended app. It is one rule for every system, read where GPU work is submitted.
- **The decision reaches the rest of the engine as one published value.** The lifecycle knows no reader. Before, it handed out a hook for each need: a callback at each freeze, two waits and a gate for the GPU, two more waits for tests. This is how Blink's objects pause themselves on `ContextLifecycleStateChanged`.
- **A system's rules are two numbers.** How long a hidden app runs on, and the grace period once WaOS is suspended. What may keep an app running on a system needs no table: each part inhibits while it is doing the thing, and the system enforces its own rules through that part, as iOS interrupts a camera in the background.
- **A desktop does not freeze a hidden app.** macOS, Windows and Linux leave a hidden window at work: a build, a render, a local server.
- **The WaOS app tells the engine when the system suspends it.** It follows its own lifecycle through Waft's listener. Before, only tests told the engine, so on a phone WaOS froze nothing.

## Found in building

- **`suspend` resolves once no GPU work is in flight,** and not once every app is frozen, as first sketched: an app whose freeze is inhibited never is, and what a shell needs before the system suspends WaOS is that nothing is on its way to the GPU.
- **An app started while WaOS is suspended runs nothing until it resumes.** It used to run until its first tick.
- **A hidden app's delay runs through a suspension.** Hidden, suspended and resumed, it has been out of sight all along.
- **The view's spare hardware buffers read the state at the engine's tick** they already had, where they were called back at each freeze.

## Left out, each addable without changing the above

- **Asking the system to keep WaOS running** while an app's freeze is inhibited: a foreground service on Android, a background task on iOS, an activity on macOS, where App Nap slows a hidden WaOS as a whole. To be shaped with its first implementation, which needs new services in Waft. Until then sound stops when a phone suspends WaOS.
- **The user's setting** for which apps may run hidden.
- **A time limit** on answering a request, closing a frozen app's sockets on phones, slowed timers for a hidden app.
- **The released interface's comments** still say "thawed"; they change with the interface's next version.
