# How systems stop an app that has left the screen

Research notes, 24 November 2026. Read-only. This describes what iOS, Android, macOS, Windows, Linux desktops, browsers, and Flutter do. It does not propose a design.

A claim is **verified** when it was read this session from a vendor document, a vendor header or API page, or the system's source. A claim is **recalled** when it is from memory and was not opened this session. Where a number differs between versions or between documents, that is said in place.

**Suspend** and **freeze**, in this note, mean the process stays in memory and its threads do not run. **Throttle** means the process still runs, more slowly or less often. **Discard** or **terminate** means the memory is gone and the next launch is a new process. These words are not used the same way by every vendor; each section uses the vendor's word and says which of these it is.

## Summary

| System | What the app is told, and how long it then has | What stops | What keeps it running, and who decides | What the system does meanwhile | What wakes it |
| --- | --- | --- | --- | --- | --- |
| iOS, iPadOS | Scene moves to background, then the system suspends it. `applicationDidEnterBackground` has 5 seconds. A background-task assertion extends that; the current doc does not publish a fixed length. | Threads. New Metal work is refused with an error. Old OpenGL ES work got the app killed. | A declared background mode, and for audio the doc says the content is audible. The system decides. | Background transfers, scheduled tasks, widget timelines, the app-switcher snapshot. | Push, a finished transfer, a scheduled task, location, Bluetooth, a call. Short budgets (refresh: up to 30 seconds). |
| Android | Activity: resumed, paused, stopped. No long budget in `onPause`. A cached process is frozen 10 seconds later on Android 14+. | Every thread in the frozen process. TCP sockets are closed if every process of the app is frozen. | A foreground service of a declared type, or a visible activity. The system decides. The user sees a notification for a foreground service (recalled for the notification itself). | DownloadManager, JobScheduler, WorkManager, widgets. Quotas by standby bucket. | An intent, a job, a manifest broadcast, the user. Context-registered broadcasts wait until unfreeze. |
| macOS | The app is told it resigned active or hid. It is not suspended. | Nothing is stopped. App Nap lowers priority and slows timers and I/O. | The system, from heuristics: visible drawing, audible, an assertion, OpenGL in the archived guide. | Little. The app keeps running, throttled. Automatic termination can quit an opted-in app. | Coming forward, an XPC message, or an Apple event leaves App Nap. |
| Windows | Packaged app: running in background, then suspended after a few seconds. Suspending handler has a deadline (about 5 seconds on desktop in one doc). Win32 is not suspended. | UWP threads. Network events are dropped, not queued. Win32 keeps running; QoS may drop. | Extended execution, background media, a background task. User battery setting. EcoQoS only slows a Win32 process. | Background transfers continue. Tiles and widgets are system UI. | Activation, a background-task trigger. Most tasks get 30 seconds of wall time. |
| Linux desktops | Nothing equivalent was found. Flatpak asks permission to stay running. | Not a per-app freeze when a window is hidden. ChromeOS freezes some processes for machine suspend. | The user, for the Flatpak background permission. | Autostart, a one-line status. | Login autostart, D-Bus activation. Not a thaw. |
| Browsers | `freeze` / `resume`, then maybe discard (`wasDiscarded`). Hidden first. | JS tasks, timers, workers of a frozen document. Before that, timers are slowed (1s, then 1 per minute). | The browser: audible, capture, WebRTC, locks, devices, visibility, and others in Chromium's policy. | bfcache holds a frozen page. Discard reloads later. Edge sleeps idle tabs. | Shown again, back/forward, or a periodic unfreeze in Chromium. |
| Flutter | `resumed`, `inactive`, `hidden`, `paused`, `detached`. Names do not match the OS names. | In `paused` (iOS and Android only) the engine does not call begin-frame or draw-frame. | Flutter does not decide. The OS under it does. | None of its own. | The OS lifecycle events, which Flutter renames. |

### Findings that constrain a design

1. **Stopping and slowing are different, and desktops mostly slow.** iOS, Android's cached-app freezer, UWP, and a frozen browser page stop threads and leave the memory. macOS App Nap and Windows EcoQoS leave the process running, with less CPU. A design that only throttles matches the desktops. A design that stops the thread matches the phones and the packaged Windows app. Verified in the sources in each section.

2. **The app is told on the way out, and the budget is short.** iOS gives `applicationDidEnterBackground` 5 seconds, then an assertion whose length is not a fixed published number. Windows waits a few seconds, then runs a suspending handler with a deadline (one doc says up to 5 seconds on desktop, 10 on mobile). Android's `onPause` is documented as too short to save in. After the stop, app code does not run, so the save has to happen in that callback. Verified.

3. **The system decides who keeps running, from a declaration plus evidence, and the user can see it.** iOS background modes are a plist the app declares ahead; the audio mode is described as audible content. Android foreground services are a manifest type plus a running service, and several types have a hard timeout. macOS looks at whether the app is actually audible or drawing, with no background-mode capability at all. A switch that means "I might play audio" is not what these systems implement. Verified.

4. **A stopped app can still be holding something another party waits on.** Android will not freeze a cached process that holds a file lock a running process needs, and it kills a frozen process that receives a synchronous binder call. Android also closes the app's TCP sockets once every one of its processes is frozen, so the peer is not left talking to a mute modem. Windows drops network events while suspended and does not queue them. iOS refuses new Metal command buffers from a background app with an error; the older OpenGL ES rule was to terminate the app. Verified.

5. **Long work is handed to the system, and the wake is short and quota-limited.** A background URL session, DownloadManager, JobScheduler, BGTaskScheduler, and a Windows background task run outside the app's normal lifetime. The app's own code then gets a slice: up to 30 seconds for an iOS refresh task, several minutes for an iOS processing task, 30 seconds of wall time for most Windows background tasks, and Android standby quotas measured in minutes per several hours. If the work does not finish, the app is suspended or killed. Verified. Exact Android maintenance-window lengths are not published.

6. **Missed time is not the same on resume.** Android documents that a frozen `scheduleAtFixedRate` can run the missed ticks back to back when the process thaws. Windows says the missed network events are simply gone. A browser in the back/forward cache pauses timers and promises and continues them, it does not deliver the ticks that fell due while frozen (web.dev). iOS does not, in the docs read here, say what a timer or a socket does across suspend. Verified where stated; iOS socket and timer catch-up not found.

7. **Surfaces shown while the app is away are drawn by someone else, on a budget.** WidgetKit renders in another process from a timeline; a widget the user looks at often gets on the order of 40 to 70 reloads a day, and entries should be at least about 5 minutes apart. Android will not honor a widget `updatePeriodMillis` below 30 minutes, and the update broadcast is treated as non-responsive after 10 seconds. The app-switcher picture is a snapshot taken around the time the app enters the background, not a live draw. Verified.

8. **A stopped app may be destroyed with no further call, and the user cannot tell.** iOS can disconnect a suspended scene to reclaim memory. Android kills the process, and `onDestroy` is not guaranteed. Windows terminates a suspended app with no notification; the next launch sees `Terminated` and must restore. A browser discard reloads the document and sets `wasDiscarded`. The save has to have happened before the stop. Verified.

9. **Linux desktops do not have this mechanism.** No GNOME or KDE document turned up that freezes an ordinary app because its window is hidden. Flatpak's background portal is a permission to keep running and to autostart, plus a short status line. ChromeOS uses a cgroup freezer when the machine suspends, which is a different event. Verified as absence in the docs that were found; a desktop that does freeze apps was not found.

### Where they agree

The app is told it is leaving before it is stopped. CPU of a background app is the thing being saved. Memory is kept so that coming back is fast, until memory pressure destroys it. Work the user can hear, or that the user was shown a notification for, is the usual reason to keep running. Long transfers and scheduled work are the system's, with a short wake of the app's code. A picture shown while the app is away is prepared data, redrawn on a budget, not the app painting frames. A process that another process is blocked on is a bad thing to freeze.

### Where they differ

Phones and UWP stop the threads. macOS and ordinary Win32 do not. Browsers do both, in stages: throttle timers, then freeze, then discard. Android waits 10 seconds in the cached state before freezing, and closes TCP; the others do not document that. iOS fails a late Metal submission with an error; old OpenGL ES killed the process. Android can run a burst of missed timer ticks; the back/forward cache does not. Widget refresh budgets differ by an order of magnitude (minutes on Android's periodic update, a denser daily budget on WidgetKit). Flutter's `paused` means "not visible" on phones and is never entered on desktop, where `hidden` is the not-visible state and the process keeps running.

## 1. iOS and iPadOS

Sources opened: [Managing your app's life cycle](https://developer.apple.com/documentation/uikit/managing-your-app-s-life-cycle), [Extending your app's background execution time](https://developer.apple.com/documentation/uikit/extending-your-app-s-background-execution-time), [Configuring background execution modes](https://developer.apple.com/documentation/xcode/configuring-background-execution-modes), [Choosing background strategies](https://developer.apple.com/documentation/backgroundtasks/choosing-background-strategies-for-your-app), [Performing long-running tasks](https://developer.apple.com/documentation/backgroundtasks/performing-long-running-tasks-on-ios-and-ipados), [Keeping a widget up to date](https://developer.apple.com/documentation/widgetkit/keeping-a-widget-up-to-date), [Preparing your Metal app to run in the background](https://developer.apple.com/documentation/metal/preparing-your-metal-app-to-run-in-the-background), the archived [OpenGL ES multitasking guide](https://developer.apple.com/library/archive/documentation/3DDrawing/Conceptual/OpenGLES_ProgrammingGuide/ImplementingaMultitasking-awareOpenGLESApplication/ImplementingaMultitasking-awareOpenGLESApplication.html), the archived [Energy Efficiency Guide](https://developer.apple.com/library/archive/documentation/Performance/Conceptual/EnergyGuide-iOS/WorkLessInTheBackground.html), and the WWDC 2019 slides and WWDC 2022 transcript on developer.apple.com. RunningBoard is not in those public docs.

### 1. States and what the app is told

Verified. From iOS 13, each scene has its own lifecycle. UIKit creates a scene in the unattached state. A scene the person asked for moves to the foreground. A scene the system asked for, for example to handle a location event, typically moves to the background. When the person leaves, the scene goes to the background and eventually to suspended. UIKit can disconnect a background or suspended scene at any time to take its memory back, which returns it to unattached. There is no callback that runs after the threads have stopped, because they have stopped.

The older app-delegate path, iOS 12 and earlier, and still the app-wide events: not running, inactive, active, background, suspended. The delegate is told it will resign active, did enter background, will enter foreground, did become active. On entering the background the app is told to finish crucial work, free memory, and prepare for the snapshot.

`applicationDidEnterBackground` has five seconds to return. Shortly after it returns, the app is suspended, unless it has asked for more time. `beginBackgroundTask(withName:expirationHandler:)` extends that. The current page says to read `backgroundTimeRemaining` for how much is left, and that failing to call `endBackgroundTask` gets the app terminated. It does not publish a fixed length. The archived energy guide says the system allows "up to a few more minutes" on request. A third-party crash write-up (not a primary source) says about 30 seconds under normal power, then about one second after the expiration handler. Those three disagree. The only number Apple states as a number, in the current page, is the five seconds.

`BGAppRefreshTask` is given up to 30 seconds. The app calls `setTaskCompleted(success:)` or the system terminates it. `BGProcessingTask` is described as several minutes, at a time the system chooses, often while charging (WWDC 2019 slides and the WWDC 2022 transcript). `BGContinuedProcessingTask` is for work that started in the foreground because the person asked, and continues if they leave; it can run for minutes.

SwiftUI's `scenePhase` was not fetched. The UIKit states above are the ones verified.

### 2. What stops

Verified for the lifecycle and for GPU. Suspended means the app's code is not running. The docs say a background app should do as little as possible, and preferably nothing. They do not, in the pages read, spell out file I/O, in-flight sockets, or timer catch-up. What the app sees on resume is the same process if it was only suspended, or a fresh launch if the scene was disconnected or the app was jetsam'd. A significant-time-change callback exists for clock changes while the app can run; it was not found as a statement about a jump across suspend.

GPU, two different rules. The current Metal page: after the app is in the background, a new command buffer is not scheduled. The completion handler sees `MTLCommandBufferStatus.error` and `MTLCommandBufferError.notPermitted`. The app is told to stop committing buffers when deactivated, and to `waitUntilScheduled()` on already committed buffers inside `applicationDidEnterBackground`. The archived OpenGL ES guide, which Apple marks retired as of the deprecation in iOS 12: an app is terminated if it executes OpenGL ES commands in the background, or if commands it already submitted are flushed to the GPU after it has entered the background. `GLKViewController` pauses its animation timer when the app goes inactive. So Metal refuses the work with an error; OpenGL ES, in the old guide, kills the app.

### 3. What keeps an app running, and who decides

Verified. Normally a background app is suspended. A background mode is declared ahead, in `UIBackgroundModes` in the Info.plist, by turning on a capability. The system then launches or resumes the app to handle that kind of event. The modes in the current table:

| Mode | plist value | What it claims |
| --- | --- | --- |
| Audio, AirPlay, and Picture in Picture | `audio` | Plays audible content |
| Location updates | `location` | Standard location service |
| Voice over IP | `voip` | VoIP, via CallKit |
| External accessory | `external-accessory` | Accessory that delivers data at intervals |
| Bluetooth LE central / peripheral | `bluetooth-central`, `bluetooth-peripheral` | Accessory communication |
| Screen capture | `screen-capture` | Capture and stream |
| Background fetch | `fetch` | Content at intervals |
| Remote notifications | `remote-notification` | Push as a signal to download |
| Background processing | `processing` | Scheduled tasks |
| Nearby interaction | `nearby-interaction` | Nearby devices |
| Push to Talk | `push-to-talk` | Launch from a push and play audible content |

The audio row says "audible content". A runtime check that suspends an app playing silence was not found in these pages. The abuse control that was found is the instruction to use the modes sparingly, and to prefer a cheaper API (significant-change location instead of continuous location). App Review's rule about background modes used for their intended purpose was not re-fetched.

The system decides. Declaring `audio` does not by itself describe a setting the user toggles per app in the pages read. Background App Refresh as a user switch was not re-fetched.

`beginBackgroundTask` is a short assertion for finishing work, not a way to stay up. It is available without a background mode. The expiration handler runs when time runs out; if the app is still in the task after that, it is terminated.

`BGContinuedProcessingTask` with `requiredResources = .gpu` is the documented way to use the GPU in the background, and only if `BGTaskScheduler.supportedResources` contains `.gpu`, and only with the continued-processing GPU entitlement. It has to have been started from a person tapping something in the foreground.

### 4. Work the system does for a stopped app

Verified for tasks and widgets. Partly recalled for transfers.

`BGAppRefreshTask` and `BGProcessingTask`: the app schedules a request, the system launches the app later, the handler runs, the app must complete the task. Refresh is up to 30 seconds. Processing is several minutes, and the app can say it needs power or network so the system waits for a better time.

WidgetKit, below, renders without the app running.

Background `URLSession` (a session the system continues after the app is suspended or killed, relaunching the app for the delegate callbacks) was not re-fetched. Recalled, not checked this session: the configuration is `background(withIdentifier:)`, and the transfer is owned by a system daemon.

Silent push (`content-available`, the `remote-notification` mode) was not re-fetched as a page. The modes page says the app is afforded time to process the event. A fixed number of seconds for that wake was not found.

### 5. What wakes a stopped app, and for how long

Verified where a number exists.

- The person opens it: full run, no budget.
- A background mode event (location, Bluetooth, VoIP, push): the system resumes the app in the background and gives it time. The refresh task's time is up to 30 seconds. Processing is several minutes. The push wake's length was not found as a number.
- `beginBackgroundTask` is not a wake of a suspended app; it is taken before or as the app leaves, and it delays the suspend.
- A continuous processing task is a wake only in the sense that work already started is allowed to continue.
- If the app does not finish and end the task, the system terminates it, for both the assertion and the refresh task.

### 6. Widgets and other surfaces

Verified for widgets. The app-switcher snapshot is the "prepare for your app snapshot" line in the lifecycle overview; the exact moment the pixels are copied was not fetched as its own page.

WidgetKit renders SwiftUI views in a separate process. The extension is not running all the time, even if the widget is on screen. The app provides a timeline of dated entries and a reload policy (`atEnd`, `after(date)`, or `never`). WidgetKit keeps a budget per widget instance over a 24-hour window aligned to the person's use, not to midnight. A widget the person views often typically gets 40 to 70 refreshes a day, roughly every 15 to 60 minutes, and the interval varies. Entries should be at least about 5 minutes apart. Reloads do not count against the budget while the app is in the foreground, has an audio or navigation session, is the Now Playing app, runs a widget intent (a tap), plays a widget animation, or when locale, Dynamic Type, or accessibility settings change. A countdown can be a date text that WidgetKit updates without running the extension.

Live Activities were not fetched. Now Playing and the lock-screen media controls were not fetched as their own page. The widget page treats "current Now Playing app, as reported by `MPNowPlayingInfoCenter`" as a state the system already knows, which is consistent with the system drawing those controls from data the app published, but that drawing path was not read.

### 7. Known trouble

Verified.

- GPU: Metal `notPermitted` if a command buffer is scheduled in the background. OpenGL ES, in the retired guide, terminates the app if any command reaches the GPU after backgrounding, including a flush of work submitted earlier. The fix the old guide gives is `glFinish` in `applicationWillResignActive` and no GL calls after `applicationDidEnterBackground`.
- Not ending a background task or a refresh task terminates the app instead of suspending it.
- A suspended app is a candidate to be destroyed with no callback, to reclaim memory. State that matters has to be saved before suspend.
- The length of `beginBackgroundTask` is not a stable contract. The archived guide says a few minutes; the current guide refuses to name a number; practitioners report about 30 seconds. Code that assumes 30 seconds is not following a number Apple currently publishes.
- Continuous location and an audio session left running are called out in the WWDC 2022 transcript as battery costs. The transcript says to stop the audio engine when playback stops, and mentions `AVAudioEngine.autoShutdownEnabled` so an idle engine releases the hardware. That is the closest verified remark to "silence should not keep the device awake": it is advice to the app, not a documented system detector.

RunningBoard and jetsam: not found as public API documentation. The public statement is that a suspended or background scene can be disconnected to reclaim resources, with no termination callback.

## 2. Android

Sources opened: [Activity lifecycle](https://developer.android.com/guide/components/activities/activity-lifecycle), [Processes and app lifecycle](https://developer.android.com/guide/components/activities/process-lifecycle), [Cached apps freezer](https://source.android.com/docs/core/perf/cached-apps-freezer), [Doze and App Standby](https://developer.android.com/training/monitoring-device-state/doze-standby), [App Standby buckets](https://developer.android.com/topic/performance/appstandby), [Power management resource limits](https://developer.android.com/topic/performance/power/power-details) (page updated 2026-05-19), [Foreground service types](https://developer.android.com/develop/background-work/services/fgs/service-types), [`ServiceInfo`](https://developer.android.com/reference/android/content/pm/ServiceInfo), [Widget updates](https://developer.android.com/develop/ui/views/appwidgets/advanced), and `CachedAppOptimizer.java` on googlesource (the behavior below is from the AOSP doc, which matches that class's role).

### 1. States and what the app is told

Verified. An activity moves created, started, resumed, paused, stopped, destroyed, through `onCreate`, `onStart`, `onResume`, `onPause`, `onStop`, `onDestroy`.

`onPause` runs when the activity is no longer in the foreground. It may still be visible, in multi-window or behind a dialog. The doc says `onPause` is very brief and not the place to save, to make network calls, or to run a database transaction. `onStop` runs when the activity is not visible, and is the place for heavier shutdown, including saving if it was not saved earlier.

The process is a separate ladder. A foreground process is the resumed activity, a running broadcast, or a service in one of its lifecycle callbacks, and is almost never killed. A visible process has a paused but visible activity, a foreground service, a live wallpaper, or an input method. A service process has a started service; the doc says a service that has run a long time, "such as 30 minutes or more", may be demoted into the cached list. A cached process holds stopped activities. It should do no work. The system may kill it at any time, and `onDestroy` is not guaranteed. Starting in Android 13, a process may get little or no execution time until it is in one of the active states.

The freezer, Android 11 and later, on a kernel with cgroup v2: a cached process is moved into a frozen cgroup. Android 14 and later: it is frozen 10 seconds after entering the cached state. A lifecycle event unfreezes it immediately: an intent, starting a job service, or the user resuming an activity. The freezer can be turned off with the developer option "Suspend execution for cached apps" or the device-config flag `activity_manager_native_boot use_freezer`. It is part of the platform, not an app API.

Doze is a device state, not an app state. Screen off, unplugged, stationary: network is suspended, wake locks are ignored, jobs and syncs and ordinary alarms wait for a maintenance window. The window length is not published; the doc says the windows get less frequent the longer the device stays still. Moving, turning the screen on, or plugging in leaves Doze.

App Standby is per app: active, working set, frequent, rare, restricted. It limits how often jobs and alarms run on battery. It is not a freeze.

### 2. What stops

Verified. A frozen process has every thread suspended. It cannot run, cannot collect garbage, and cannot handle a trim callback. Because of that, Android 14 delivers `TRIM_MEMORY_UI_HIDDEN` as soon as an app with a visible activity goes to the background, and may ask the runtime to collect garbage shortly after the process becomes cached, before the freeze. Other trim levels are not delivered, because the process would be frozen by then. While frozen, the system may compact memory, write dirty pages, and swap anonymous pages to ZRAM.

If every process of that app is frozen, the system closes its active TCP sockets, so the server's keepalive does not wake the modem. What a single frozen process does with a socket, when a sibling process of the same app is still running, is not stated.

Timers: a `scheduleAtFixedRate` (or `Timer.scheduleAtFixedRate`) does not fire while frozen. On thaw, the missed runs can execute back to back with almost no delay. The doc's remedy is `scheduleWithFixedDelay`, or WorkManager.

GPU: no separate GPU rule was found. A frozen thread cannot submit GPU work because it cannot run. What a background but not-yet-frozen process may submit was not found.

On resume, if the process was only frozen, memory is intact and the missed fixed-rate timers may burst. If the process was killed, the next start is a new process and the activity restores from saved state. `onDestroy` may never have run.

### 3. What keeps an app running, and who decides

Verified. The system assigns process importance from what is actually running. The app asks by starting a component.

A foreground service is how an app stays out of the cached state while it has no visible activity. From Android 14 the type is declared in the manifest (`android:foregroundServiceType`) and a matching permission is requested, plus `FOREGROUND_SERVICE`. Types read this session include `camera`, `microphone`, `location`, `mediaPlayback`, `mediaProcessing`, `mediaProjection`, `dataSync`, `shortService`. While-in-use types cannot be started from the background: the camera and microphone pages say you cannot create those services while the app is in the background, with a few exceptions. Media projection shows a permission screen via `createScreenCaptureIntent` before the service starts.

Timeouts, from `ServiceInfo` and the types page:

- `shortService`: 3 minutes, then `onTimeout`. The service is not stopped for you; if you do not stop it, it is your bug. Combined with another type, the short type is ignored and the timeout does not apply.
- `dataSync`: 6 hours starting with the Android release codenamed Vanilla Ice Cream, then `onTimeout`.
- `mediaProcessing`: 6 hours out of every 24, shared by all of that app's media-processing services. On timeout, a few seconds to call `stopSelf`, then an ANR. On Android 14 and lower, `onTimeout(int, int)` does not exist, and reaching the limit caches the app immediately.
- `mediaPlayback`: no timeout in the section read. It is "continue audio or video playback from the background."

The ongoing notification that `startForeground` posts was not on the types page. Recalled from `Service.startForeground`: the user sees a notification while the service is in the foreground. Treat that sentence as recalled.

A foreground service is not a blanket exemption from standby quotas. The power page says that starting with Android 16, jobs running alongside a foreground service count against the job quota. Before Android 16 they did not.

The user can restrict an app's battery use, which restricts jobs, alarms, and network, or un-restrict it. Doze still defers ordinary work while the screen is off; a high-priority FCM is not deferred.

Picture-in-picture is named as the alternative to a media-playback service when the video is what should stay on screen. A connected accessory was not given its own foreground type in the sections read.

Playing silence: no detector was found. The control that was found is the type declaration, the notification, the timeouts on the types that have them, and the user's battery restriction.

### 4. Work the system does for a stopped app

Verified for scheduling and quotas. DownloadManager as a system-owned transfer was not re-fetched; the power page says DownloadManager jobs are part of the same quota as other jobs. Recalled, not checked this session: `DownloadManager` keeps the transfer in a system process and broadcasts when it finishes, so the app does not have to stay running.

WorkManager runs on JobScheduler when the app is not visible, so the quotas apply. The system starts the process to run the job.

Standby quotas, from the power page. The page says the numbers are approximate, not a guarantee, and can change. Rows whose labels survived the fetch:

| Bucket | Regular jobs | Expedited jobs | Alarms | Network |
| --- | --- | --- | --- | --- |
| Active | Up to 20 minutes in a rolling 60 minutes (Android 16; before that, no limit) | Up to 30 minutes in a rolling 24 hours | No limit | No restriction |
| Working set | Up to 10 minutes in a rolling 4 hours | Up to 15 minutes in a rolling 24 hours | 10 per hour | No restriction |
| Rare | Up to 10 minutes in a rolling 24 hours | Up to 10 minutes in a rolling 24 hours | 1 per hour | Once a day, up to 10 minutes |

The same table also has Frequent and Restricted. Their row labels were dropped in the fetch, so those cells are not repeated here. The prose of the page says a rare app gets about 10 minutes of jobs per 24 hours, which matches the rare row. While charging, limits are lifted except for the restricted bucket. In Doze, jobs wait for a maintenance window; while-idle alarms are limited to 7 per hour; high-priority FCM is not limited; normal FCM waits for the window.

From Android 13, high-priority FCM is no longer capped by the standby bucket. If an app sends high-priority messages that do not result in a notification, the system downgrades them.

### 5. What wakes a stopped app, and for how long

Verified.

- The user resumes the activity: immediate unfreeze, process brought up if it had been killed.
- A manifest-declared broadcast: the process is unfrozen immediately to deliver it. A context-registered receiver is not: those broadcasts are queued and delivered after some other event unfreezes the app.
- Starting a job service, or delivering an intent: immediate unfreeze.
- The time slice is the job quota, not a single fixed wake. A broadcast receiver is treated as non-responsive after about 10 seconds (stated on the widget page). The process-lifecycle page's classic warning: a receiver that starts a thread and returns is already eligible to be killed, because the system thinks the receiver is done.
- If a frozen process is sent a synchronous binder call, it is not woken to answer. It is killed. See trouble.

### 6. Widgets and other surfaces

Verified for widgets. `updatePeriodMillis` cannot be less than 30 minutes. Zero means no periodic update. A faster cadence is done by the app asking to update, or by WorkManager, which is still under the standby quotas. `AppWidgetProvider.onUpdate` is a broadcast. If it runs past about 10 seconds the system considers the receiver non-responsive. The widget's pixels are `RemoteViews` applied by the system, not the app drawing into the home screen. A full update replaces them; a partial update merges.

The recents thumbnail is the system. No page was fetched that says the app draws it. The activity lifecycle says a stopped activity is invisible and the process may be killed; the card the user sees is therefore not a live frame from the app.

Ongoing notifications and media controls were not fetched as their own page. A foreground service is the usual reason they exist.

### 7. Known trouble

Verified, mostly from the freezer doc, which has a section of failure modes.

- Synchronous binder to a frozen process: the system kills the frozen process so the caller does not block, and the caller gets `RemoteException`. The exit reason is `ApplicationExitInfo.REASON_FREEZER`. This happens when a client unbinds and then uses a stale `IBinder`. The doc's rule: drop the binder when you unbind.
- Asynchronous binder (`oneway`) is buffered. Too many while the target is frozen overflow the buffer, and the frozen process is killed.
- File locks: a cached process that holds a lock a non-cached process needs is not frozen.
- `BIND_WAIVE_PRIORITY`: a process with such an incoming binding can be cached and still not frozen, until its clients are cached too. The doc names browsers using Custom Tabs.
- `scheduleAtFixedRate` bursts on thaw.
- TCP sockets closed when all of the app's processes are frozen. A server the app was talking to sees a dead connection. On resume the app has to notice.
- `onDestroy` is not called when the process is killed. State saved only there is lost.
- A foreground service of a timed type that ignores `onTimeout` gets an ANR, or, on older releases for media processing, is cached immediately.
- High-priority push used as a way to stay awake is downgraded when it does not show a notification.

## 3. macOS

Sources opened: [Extend App Nap](https://developer.apple.com/library/archive/documentation/Performance/Conceptual/power_efficiency_guidelines_osx/AppNap.html) and the timer chapter of the same guide, [The Core App Design](https://developer.apple.com/library/archive/documentation/General/Conceptual/MOSXAppProgrammingGuide/CoreAppDesign/CoreAppDesign.html) (automatic and sudden termination), and the background-modes page, which says the capability is not available for macOS. The App Nap guide is archived. No current page was found that says App Nap has been removed.

macOS does not suspend an ordinary app. Hiding it, or it not being the foreground app, does not stop its threads. What it does instead is App Nap, which throttles, and, if the app has opted in, automatic termination, which quits it. Sudden termination is a logout and shutdown behavior, not a background behavior.

### 1. States and what the app is told

Verified. There is no suspended state in these docs. The app is told, through the notifications the guide points at, when it is no longer active and when its windows are no longer visible. The guide's point is that the app should go idle itself when it gets those, and not wait for App Nap.

App Nap is automatic. The app is not given a callback that means "you are now in App Nap" in the page read. Activity Monitor's Energy tab shows an App Nap column, and Xcode's energy gauge shows it. That is how a developer sees it, not how the app is told.

Timer coalescing is separate: an `NSTimer` tolerance lets the system shift the fire time so timers batch. The guide says at least 10 percent of the interval is a useful tolerance. It does not say App Nap's timer throttle is a particular rate, only that timers fire less often.

### 2. What stops

Verified. Nothing is stopped. App Nap, for an app that is not doing important user work:

- lowers the process priority, so it gets less CPU;
- throttles timers, so they fire less often;
- throttles I/O, so reads and writes slow down while a foreground app wants the disk.

The guide says these mostly protect other apps, and do not by themselves make the napping app idle. Drawing, network, and audio are not described as cut. OpenGL use is a reason not to nap, in that guide; Metal is not mentioned, and the guide is from the OpenGL era.

Coming back is instant in the guide's words: the user brings the app forward, or it receives an XPC message or an Apple event, and it runs at full speed again. No time jump and no dropped sockets are described, which fits a process that never stopped.

### 3. What keeps an app running, and who decides

Verified. The app is already running. App Nap applies when all of these are true, as the guide's "generally":

- it is not the foreground app;
- it has not recently updated the visible part of a window;
- it is not audible;
- it has not taken an IOKit power assertion or an `NSProcessInfo` assertion;
- it is not using OpenGL.

The system decides from what the app is doing. There is no plist background mode. The background-modes capability page says it is not available for macOS. Audible is observed, not declared. An assertion is the app telling the system that a long user-initiated operation should finish now, in which case it is not napping, or that work is discretionary, in which case napping is fine. `NSProcessInfo` activity options were not fetched as their own page; the App Nap page names the assertion as the signal.

No user switch for App Nap was found. No silence detector was found beyond "isn't audible" as a condition for napping.

### 4. Work the system does for a stopped app

There is no stopped app. `NSBackgroundActivityScheduler` schedules maintenance at a time the system chooses from energy, thermals, and CPU. That is the app's code running later, not work done while the app is frozen. It was identified from the energy guide's contents, and the page itself was not fully read.

### 5. What wakes a stopped app

Not applicable to a freeze. Leaving App Nap happens when the user brings the app forward, or an XPC message or an Apple event arrives. No time budget, because the app was never stopped.

### 6. Widgets and other surfaces

WidgetKit on macOS was not fetched separately. The iOS WidgetKit page is the same framework; whether the 40 to 70 budget is the same on macOS was not stated on that page.

The Dock and the app switcher show a running app, because it is running. A badge or progress was not fetched. Automatic termination, below, is the case where the Dock can still look as if the app is there after the process is gone: the guide says the system manages termination transparently, and restores state later. The exact Dock picture was not read.

### 7. Known trouble

Verified.

- App Nap does not stop a misbehaving app. The guide says so. An app that keeps drawing a one-pixel change, or keeps an audio unit "audible", or holds an assertion, will not nap.
- Automatic termination, opt-in via `NSSupportsAutomaticTermination` or `NSProcessInfo`: the system may kill the process when it wants memory, typically some time after the last window closes, and also when the app is hidden or on another space. There is no freeze in between. The app has to have saved. If it has not opted in, this does not happen.
- Sudden termination: the system may kill the process with no chance to run last code, at logout, restart, or shutdown, unless the app has disabled sudden termination because it has unsaved state. An app that supports both can be killed with no involvement at all. An app that supports sudden termination but not automatic termination still gets a Quit event at logout, and must handle it without showing UI.
- The App Nap exemptions name OpenGL and do not name Metal. Whether a Metal app is treated as "using the GPU" for this heuristic was not found. The guide is archived.
- No binder-style "frozen while someone waits" problem exists here, because ordinary apps are not frozen. The related problem is sudden termination while another process expected a clean quit.

## 4. Windows

Sources opened: [UWP application lifecycle](https://learn.microsoft.com/en-us/windows/uwp/launch-resume/app-lifecycle), [Postpone suspension with extended execution](https://learn.microsoft.com/en-us/windows/uwp/launch-resume/run-minimized-with-extended-execution), [Optimize suspend/resume](https://learn.microsoft.com/en-us/windows/uwp/debug-test-perf/optimize-suspend-resume), [Guidelines for background tasks](https://learn.microsoft.com/en-us/windows/uwp/launch-resume/guidelines-for-background-tasks), [Quality of Service](https://learn.microsoft.com/en-us/windows/win32/procthread/quality-of-service), [`SetProcessInformation`](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setprocessinformation).

Two worlds. A Win32 app is running or not. Minimizing it does not suspend it. A packaged UWP or WinUI app, from Windows 8 on, is suspended when it leaves the foreground. The lifecycle page says this is true on every edition that has a visual shell.

### 1. States and what the app is told

Verified. From Windows 10, version 1607, the states that matter are running in background, running in foreground, suspending, suspended, and then either resumed or terminated. Not-running is the start.

The app enters running in background when it is launched, activated, or resumed. `LeavingBackground` fires just before the UI is visible. `EnteredBackground` fires when it is no longer visible: minimized on a desktop, or switched away on a phone. Then, if the user does not come back within a few seconds, and there is no extended execution, background task, or sponsored activity, Windows suspends it. The lock screen also suspends it, under the same condition.

`Application.Suspending` runs on the way into suspend. The handler must return, or complete its deferral, before `SuspendingOperation.Deadline`. The optimize page says the work should finish in under a second, and may take up to 5 seconds on desktop or 10 seconds on mobile; past that the app is terminated. An older sentence on the suspend page says the timeout was extended so that low-priority access after suspend still corresponds to about 5 seconds of normal-priority time on desktop, or between 1 second and something the fetch cut off, on other devices. The deadline property is the number that matters at runtime. A deferral does not add time. It only covers an asynchronous call that has not finished, and the deadline still wins.

The app is not told when it is terminated. The next activation's `PreviousExecutionState` is `Terminated`, or `ClosedByUser` if the user closed it. Both mean the memory is gone and state must be restored. `Suspended` means the memory was kept. `NotRunning` covers a crash, a reboot, or a first launch.

Prelaunch: an opted-in app may be launched in the background and then suspended, so that a later tap resumes it. It gets `Suspending` then, and the user has not seen it.

Debugging disables the time limits. The docs say to test suspend without a debugger, or with Visual Studio's lifecycle controls.

Win32: the lifecycle page says it keeps running when minimized. No suspending event.

### 2. What stops

Verified for UWP. On suspend, threads stop and memory stays. The app can be suspended for hours or days. On resume, variables are as they were. Exclusive resources the app released (camera, devices, files, network) have to be taken again.

Network: while suspended, registered network events are not delivered and are not queued. They are missed. On resume the app should test the network. The page says connections may have gone stale. It does not say the kernel closes the sockets. It says the app does not hear the events.

Timers and in-flight requests: not given a separate sentence. Threads are stopped, so timer callbacks do not run. Catch-up on resume was not described.

GPU: not found.

Win32: the process keeps running. QoS can slow it. The QoS page, for windowed apps the system classifies itself: High if in the foreground and focused, or audible; Medium if visible but not focused, and also the foreground app after inactivity on battery (that inactivity feature marked as a 2025.05 release); Low if not visible and not audible, which on battery picks an efficient frequency and efficient cores. Eco is an explicit opt-in through `SetProcessInformation` / `PROCESS_POWER_THROTTLING_EXECUTION_SPEED`, and it always picks efficient cores, on AC or battery. Utility is for background services. Media and Deadline are for the multimedia scheduler. Efficiency mode in Task Manager is the user-facing EcoQoS plus a lower priority; the priority part is from a Microsoft Q&A answer, which is weaker than the QoS page. EcoQoS does not stop the process and has no time limit.

### 3. What keeps an app running, and who decides

Verified.

Extended execution is one `ExtendedExecutionSession` at a time. The system can deny it. Reasons:

- `Unspecified`, requested in the foreground: the app stays in the running state and does not get `Suspending` while the session lasts. On desktop on battery, up to ten minutes. On AC power, no limit. Revoked when the device enters connected standby. On desktop it keeps going when the lock screen appears. The user can get the long behavior on battery by turning off "managed by Windows" and allowing background tasks, in the per-app battery settings.
- `LocationTracking`: as long as needed, but only one such session on the device, and it must be requested in the foreground so the person is present for it.
- `SavingData`: the only reason allowed inside the suspending handler. After it is revoked, the app has one second to finish. Using it makes the next launch wait until the save finishes, so the doc warns that launch looks slow.

Revocation always fires the revoked handler first. Reasons include the time limit, an energy quota, or memory needed for a foreground app. The app should dispose the session when the work is done; leaving it open spends the energy quota.

Background media: the lifecycle page says the OS keeps the app running if it follows the background media playback steps (`BackgroundMediaEnabled` is named). That page was not fetched. The decision is the system's, from the capability and from playback.

A background task is not "keep running". It is a separate wake, below.

The user controls background activity through the battery settings, and the app can read `BackgroundExecutionManager` for `BackgroundAccessStatus`.

Win32 stays running with no permission. The system may lower its QoS when it is not visible and not audible. The user can set Efficiency mode. No silence detector was found beyond the QoS rule that audible apps stay at High.

### 4. Work the system does for a stopped app

Verified at the lifecycle level. Background transfers continue while the app is suspended or even terminated. The "how to download a file" page was not fetched; the lifecycle page states the fact. The hand-off is: the app starts the transfer, the system finishes it, the app hears about it later by being activated or by a background task.

Tiles were not fetched. The lifecycle page says a terminated app still appears in the taskbar, and the user expects the old state when they click it.

### 5. What wakes a stopped app, and for how long

Verified.

- The user switches back, or the device leaves a low-power state and this was the active app: resume, memory intact, no time limit.
- If it had been terminated: `OnLaunched` with `Terminated`, a new process, no `Resuming`.
- A contract activation (share, file, protocol) resumes first, then activates.
- A background task: most triggers get 30 seconds of wall-clock time. Some get up to 10 minutes. The guidelines page says to see "Support your app with background tasks" for which triggers get the longer quota; that second page was not fetched, so the split is not listed here. If an in-process task does not leave its handler before cancellation, the process is terminated.
- Extended execution is a postponement of suspend, not a wake of an already suspended app. Once the session ends, the app is suspended with no further notification if it was already in the suspending state.

### 6. Widgets and other surfaces

The taskbar and the switcher keep a suspended app's slot. The thumbnail is the system's. No page was fetched that says the app paints it while suspended. Because threads are stopped, it cannot be a live draw.

Windows widgets (the Widgets board) were not fetched. Not reported.

Toasts and notification actions were not fetched as a page. Activation kinds exist; a toast that activates the app would be one of them. The time budget of a toast activation was not found.

### 7. Known trouble

Verified.

- Missing the suspending deadline terminates the app. The user comes back to a cold start and can tell, because it is slow. The doc says so.
- Network events during suspend are dropped. A socket the app thought was alive may be stale. There is no queue to drain.
- Extended execution requested as `SavingData` during suspend blocks the next launch until it finishes.
- One session at a time. A second request throws `0x8007139F`.
- The debugger hides the bug. Suspend does not happen, and the time limits are off, while a debugger is attached.
- Prelaunch can suspend an app that has never been on screen, so a launch path that plays audio will play it invisibly.
- Win32 is not saved by EcoQoS from a runaway background loop. It is only slowed, and only under the QoS rules.
- No Android-style "kill the frozen process because someone called it" was found, because a suspended UWP app is not a binder server in these docs. The related warning in the background-task guidelines is the other way: do not RPC into a process that might be suspended, because the call can hang, and the time counts against the task.

## 5. Linux desktops

Sources opened: [Background portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Background.html), [ChromeOS suspend and resume](https://chromium.googlesource.com/chromiumos/platform2/+/HEAD/power_manager/docs/suspend_resume.md), [Chromium OS cgroups](https://www.chromium.org/chromium-os/chromiumos-design-docs/chromium-os-cgroups/), a 2014 Chromium review that puts non-extension renderers in a freezer cgroup for system suspend, and a Flatpak discussion about a cgroup portal that is a proposal, not a shipped interface.

No document was found in which GNOME, KDE, or a generic Wayland or X11 session suspends an ordinary app because its window was hidden or unfocused. What was found is a permission to keep running, a machine-suspend freezer, and an out-of-memory killer. Those are different mechanisms.

### 1. States and what the app is told

Verified for Flatpak. `org.freedesktop.portal.Background` lets a sandboxed app ask to run in the background, and optionally to be started at login. The app passes a user-visible reason, whether it wants autostart, a command line, and whether to use D-Bus activation. The reply is two booleans: background allowed, autostart allowed. Version 2 adds `SetStatus` with a message of at most 96 characters, one line, for a list the user can look at. This is not a lifecycle. The app is not told it has been frozen, because it has not been.

Nothing was found that tells a normal desktop app "you are hidden, you will be stopped".

### 2. What stops

Not a per-window freeze, as far as the docs found.

ChromeOS, when the machine suspends, freezes userspace in an order, using the cgroup freezer, so that a process blocked in the kernel on a FUSE daemon is not frozen before that daemon. Pref files name the order. A 2014 change puts Chrome renderers that do not host a GCM extension into a freezer cgroup for the duration of system suspend, and thaws them after resume has finished. That is the laptop lid, not an unfocused window. The Chromium OS cgroup note says the freezer group "stops running when we suspend or resume" and exists to limit work at resume.

systemd-oomd was not re-fetched. Recalled: it kills a cgroup under memory pressure. It does not freeze a background app as a lifecycle. The Flatpak discussion says oomd kills a whole browser cgroup because it cannot see inactive tabs. That is a proposal for a portal that would let an app put workers in cgroups and mark one inactive. It is a GitHub discussion, not a specification that shipped.

### 3. What keeps an app running, and who decides

Verified for the portal. The user decides the background and autostart permission, after the app asks and gives a reason. There is no "audible" or "downloading" signal in this interface. An app that does not ask, and is not sandboxed this way, just runs. That is the ordinary desktop.

### 4. Work the system does for a stopped app

Not applicable. Autostart at login is the closest: the portal can start the app from the desktop file's `Exec` or from a command line, or by D-Bus activation. The app's code then runs as a normal process.

### 5. What wakes a stopped app

There is no thaw. D-Bus activation starts a process that was not running. That is a launch, not a timed wake. No time budget was found.

### 6. Widgets and other surfaces

Not found for a common desktop protocol that draws an app's widget while its process is stopped. Status notifiers and tray icons were not fetched. They generally require the app's process.

### 7. Known trouble

Verified only at the level of the documents found.

- The background portal does not stop abuse by a process that is already allowed to run. It is a yes or no.
- ChromeOS system suspend will deadlock or time out if a process is frozen while it holds a kernel lock another frozen process must release. That is why the freezer order exists. It is the same shape as Android's file-lock exemption, applied to machine suspend.
- A cgroup-wide OOM kill takes a whole browser when one tab is the problem. That is the stated motivation of the unshipped portal, so it is a known complaint, not a shipped behavior to copy.
- No socket, timer, or GPU rule for "the window was hidden" was found, because the freeze was not found.

## 6. Web browsers

Sources opened: the [Page Lifecycle spec](https://wicg.github.io/page-lifecycle/) (WICG), [Chrome's timer-throttling note for Chrome 88](https://developer.chrome.com/blog/timer-throttling-in-chrome-88), [web.dev on the back/forward cache](https://web.dev/articles/bfcache) (updated 16 October 2026), Chromium `components/performance_manager/freezing/freezing_policy.cc` at refs/heads/main, the older [freezing opt-in and opt-out note](https://chromium.googlesource.com/chromium/src/+/HEAD/chrome/browser/performance_manager/docs/freezing_opt_out_opt_in.md), and Microsoft's [SleepingTabsTimeout](https://learn.microsoft.com/en-us/deployedge/microsoft-edge-policies/sleepingtabstimeout) and [SleepingTabsEnabled](https://learn.microsoft.com/en-us/deployedge/microsoft-edge-policies/sleepingtabsenabled) policy pages, plus the Edge 152 release note that the default changed.

### 1. States and what the app is told

Verified in the spec. The lifecycle states a page can observe include active, passive, hidden, frozen, and discarded. Frozen means the freeze steps ran: CPU suspension. Discarded means the document is gone; when the user returns, a new document loads with `wasDiscarded` true. The spec's examples, which are examples and not a current Chrome promise: mobile Chrome may freeze a tab that has been in the background at least 5 minutes; desktop Chrome may discard a background tab that has not been used, to save memory. Pages doing work for the user, the example given is playing audio, are generally not frozen or discarded.

The page learns freeze and resume through events. `document.wasDiscarded` is how it learns it was discarded, on the new document. A discarded page does not get a last turn; it is already gone.

Hidden comes first, through the Page Visibility API. Freezing is a further step the browser may take after a grace period. The spec does not require a particular grace period.

Chrome's timer throttling is a stage before freezing, and it does not require the frozen state. From Chrome 88, for chained timers (`setInterval`, or a `setTimeout` that schedules another):

- Minimal: page visible, or it made sound in the last 30 seconds. A silent audio track does not count. Timers under 4 ms are clamped to 4 ms after a chain of 5, which is older than Chrome 88.
- Throttled, checked once a second: the chain is shorter than 5, or the page has been hidden for less than 5 minutes, or WebRTC is in use (an open data channel or a live media track).
- Intensive, checked once a minute: hidden for more than 5 minutes, chain of 5 or more, silent for at least 30 seconds, and WebRTC is not in use.

`requestAnimationFrame` does not run while the page is hidden.

Edge sleeping tabs: the policy page says the default inactivity timeout is 900 seconds, 15 minutes. The Edge 152 release note says the default changed from 2 hours to 15 minutes starting with version 152.0.4188.0. When energy saver is on, Edge uses the shorter of this timeout and the energy-saver timeout: 5 minutes in maximum-savings mode, otherwise 30 minutes, or 15 minutes on machines with less than 6 GB of memory. Sleeping tabs is on by default. If the policy turns it off, moderate memory pressure may still freeze a tab before discarding it.

The back/forward cache is a freeze too. Navigating away can put the page in bfcache instead of destroying it. `pagehide` and `freeze` fire. Coming back fires `resume` and `pageshow` with `persisted` true. The JavaScript heap is the same object. Chrome has had this since version 96; Firefox and Safari have it too (web.dev).

### 2. What stops

Verified in the spec and in web.dev.

A frozen document's tasks are not runnable. Dedicated workers whose owning document is frozen are blocked. Worklets of a frozen document are blocked. So timers, input handlers, and promise reactions do not run. Messages posted to a frozen window sit on the task queue until it thaws; if too many pile up, the browser may discard the page.

web.dev, on bfcache specifically: pending timers and unresolved promises are paused, and almost all pending tasks are paused. They continue when the page is restored. The ticks that came due while the page was in the cache are not delivered as a burst; the timer waits out the rest of its delay, or rather the task is resumed, not replayed. The page says browsers generally refuse to put a page in bfcache in the middle of an IndexedDB transaction, because pausing that task would block other tabs in the origin.

What TCP does while a page is frozen, outside bfcache, was not found in the spec. The spec says tasks do not run. It does not say the socket is closed.

Discard is the opposite of freeze: the memory is released, JS is gone, and the next visit is a load. `wasDiscarded` is true. `clientId` and `lastClientId` exist so the new document can find state the previous one stored.

A time jump: `Date.now()` keeps tracking real time, because the clock is not stopped (this is implied by a paused task queue rather than a stopped clock; a sentence that says "the clock jumps" was not found). A timer paused in bfcache does not see a compressed interval when it resumes, according to web.dev's "paused and resumed" description. Intensive throttling is different: the callback runs late, up to a minute late, and sees that lateness if it measures time itself.

GPU and drawing: `requestAnimationFrame` does not fire when hidden (Chrome's timer article). A frozen document cannot run the script that would paint. The browser keeps showing the last frame. A discarded tab shows whatever the browser uses as a placeholder until it reloads; that placeholder was not fetched.

### 3. What keeps a page running, and who decides

Verified in Chromium source for the current policy, and in the Chrome 88 article for timer throttling. The browser decides. The page does not declare a manifest. Some exemptions are "doing it right now", and one is a permission.

From `freezing_policy.cc` on main, a page is not frozen while any of these hold. A page shares the decision with pages connected through browsing instances: if any connected page cannot be frozen, the group is not frozen.

- Visible.
- Audible. When it stops being audible, `kRecentlyAudible` protects it for `kFreezingAudioProtectionTime`. That duration is a feature parameter. The default was not retrieved (`features.cc` returned 404).
- Recently visible, for `kFreezingVisibleProtectionTime`, except the "infinite tabs" freeze type ignores this one.
- Most recently used, for the freeze type that is not the infinite-tabs one.
- Uses WebRTC.
- Notification permission is granted. This is a permission, not "a notification is showing".
- Holding a Web Lock.
- Holding a blocking IndexedDB lock.
- Loading, loading-timed-out, or loaded-busy. Idle or not-started may be frozen.
- Connected to USB, Bluetooth, HID, or a serial port.
- Capturing video, audio, a window, or a display.
- Being mirrored.
- Opted out by the origin, or by a freezing origin trial, or an enterprise policy (the policy is named in the older doc; the code has an opt-out checker).
- A chrome:// or untrusted WebUI URL.

The older design note (also at HEAD) says freezing is not enabled on desktop Chrome yet, and lists a shorter set: enterprise opt-out, origin trial, the origin was seen playing audio or updating the favicon or title or showing a persistent notification while backgrounded, not enough observation time, capturing user media, being mirrored, playing audio, WebUSB, visible, DevTools, capturing a window or screen, holding a Web Lock or an IndexedDB transaction, sharing a browsing instance. Two disagreements with the current policy file: that note says desktop freezing is off, and it lists DevTools and "observed to update favicon or title", which the policy file's reason list, as read, does not. The policy file is the one that implements the decision. Whether the feature flag is on for users was not determined. DevTools was not found as a reason in the policy file.

Timer throttling, which is not freezing, exempts a page that made any sound in the last 30 seconds, and exempts WebRTC from the once-a-minute stage. Silence does not count as sound.

Edge says it uses heuristics to avoid sleeping a tab that shows notifications, plays sound, or streams video. Sites can be excluded by policy. The user can turn sleeping tabs off, and can pick the timeout if policy allows.

Abuse: a silent audio track does not buy the Chrome 88 timer exemption. Freezing still treats "audible" as a reason; how Chromium decides audible, and whether a silent buffer counts, was not read past the policy using `IsAudible()`. The 30-second rule is only the timer throttle.

### 4. Work the system does for a stopped app

The browser does not run the page's downloads as a separate system service in the docs read. A frozen page's fetches do not have a documented hand-off. Service workers were not reviewed. Push that shows a notification can be the browser's, with the page not running; that path was not fetched.

bfcache is the browser holding the page. Discard is the browser throwing it away. Neither runs the page's code.

### 5. What wakes a stopped app, and for how long

Verified in the spec. The browser unfreezes a page when the user brings it back. The spec also allows a browser to unfreeze in the background when resources are plentiful, and Chromium's policy has a periodic unfreeze phase (`IsInUnfreezePeriod`). The length of that phase was not retrieved.

There is no short "you have 30 seconds" on unfreeze. The page runs normally until it is hidden and frozen again. A discarded page is not woken. It is loaded, and `wasDiscarded` is true.

If a frozen page's queued tasks grow without bound, the spec allows discard instead of a wake.

### 6. Widgets and other surfaces

A tab is the surface. The discarded or sleeping tab stays in the tab strip. The favicon and title are whatever they were; the freezing design note treats updating them in the background as a reason the origin was considered active, in the older policy. The current policy file did not show favicon or title as a cannot-freeze reason.

No home-screen widget belongs to this section. The installed-PWA path was not fetched.

### 7. Known trouble

Verified.

- IndexedDB and Web Locks: freezing or bfcache in the middle of a transaction would block other tabs in the origin. Both the spec's implementation note and Chromium's policy refuse to freeze a page that holds a blocking IndexedDB lock or a Web Lock. web.dev says browsers generally will not bfcache a page mid-transaction.
- Discard loses the JavaScript heap. Anything not stored elsewhere is gone. `wasDiscarded` is the only signal, and it arrives in the new document.
- Timer throttling breaks pages that poll. The Chrome article says so, and points at events and observers instead. A chain of timers in a hidden, silent, non-WebRTC page runs about once a minute after five minutes.
- The two Chromium documents disagree about whether desktop freezing is on, and about DevTools and favicon updates. The code is ahead of the design note, or the note is about a flag that is still off. That was not resolved.
- Edge's default moved from 2 hours to 15 minutes, and energy saver can make it 5. A page written against the old default is wrong.
- Silent audio is explicitly not a timer-throttling exemption. It was not proven, from the code read, whether it counts as `IsAudible()` for freezing.
- Messages queued at a frozen window can cause discard. The spec says so.

## 7. Flutter

Source opened: [`AppLifecycleState`](https://api.flutter.dev/flutter/dart-ui/AppLifecycleState.html) and the [hidden-state migration note](https://docs.flutter.dev/release/breaking-changes/add-applifecyclestate-hidden). Flutter does not freeze the app. It reports a single state machine, and synthesizes the states a platform does not have, so the same transitions exist everywhere. The OS underneath still does whatever section 1 to 6 said. The names do not match the OS names. The doc says this is historical.

The engine starts in `detached`, then moves to the real state, usually `resumed`, on the first lifecycle message. The app should not assume it will see every state. A kill from the task manager, a signal, or power loss sends nothing, and states can be skipped.

### What each state means

Verified from the enum docs.

**resumed.** Default running state: visible and focused, on every platform. Android: the host view has focus (`onWindowFocusChanged(true)`) and the activity is resumed. The app can be `inactive` while Android still considers the activity resumed, if focus was lost but `onPause` has not run. iOS and macOS: foreground and active.

**inactive.** At least one view is visible, and none has focus. The process is running. Desktop, not web: not the foreground app, but a window is still visible. Web: the tab or window is open and does not have focus. iOS and macOS: foreground inactive, including a phone call, Touch ID, the app switcher, Control Center, or a view-controller transition. Android: `onPause` has run, or the activity is resumed but does not have window focus. Examples: partly covered, split screen and not the focused one, a phone call, picture in picture, a system dialog, the notification shade, the app switcher. On Android and iOS the doc says an app in this state should assume it may be hidden and paused at any moment.

**hidden.** Every view is hidden. On iOS and Android this state is synthesized. It is entered briefly on the way from inactive to paused, and on the way from paused back to inactive. It does not change when paused or inactive are entered. On desktop it is the state while the app is not visible (minimized, or on a space that is not showing). On the web it is a tab or window that is not visible. The migration note says the same: hidden is the cross-platform "not visible", and on phones it is only the brief step between inactive and paused.

**paused.** Not visible, and not taking input. Only entered on iOS and Android. While paused, the engine does not call `PlatformDispatcher.onBeginFrame` or `onDrawFrame`. So Flutter stops asking for frames. It does not, in this doc, stop the isolate, timers, or network. Those keep going until the OS suspends or freezes the process, which on iOS is shortly after background, and on Android is the cached-app freezer about 10 seconds after the process is cached, if this process has nothing else keeping it up.

**detached.** The engine is running and has no host view. Default before the first frame. Entered on iOS, Android, and web after the last view is detached. Not entered on desktop as a "views went away" state; on desktop it is only the initial state. The doc says the engine is running without a view.

The transition checks in `AppLifecycleListener` (same library, the method page) only allow: detached or inactive or null to resumed; hidden or resumed to inactive; paused or inactive to hidden; hidden to paused; paused to detached. So the phone path down is resumed, inactive, hidden, paused, and possibly detached. The path up is paused, hidden, inactive, resumed.

### The seven questions, as they apply

Flutter adds no budget, no exemption, and no system work. The answers are the platform's, with these renames.

1. The app is told the Flutter state, which on Android fires `inactive` for `onPause` and `paused` for `onStop`. On iOS, `inactive` is foreground-inactive and `paused` is the background state. On desktop, not being visible is `hidden`, and `paused` is never entered, which matches macOS and Win32 not suspending the process. On the web, `inactive` is unfocused and `hidden` is not visible; freeze and discard are the browser's, and this enum does not have them.

2. What Flutter itself stops: frame callbacks, in `paused` only. Everything else it stops is the OS stopping the process. Desktop `hidden` does not stop frames according to this page; the "no beginFrame" sentence is attached to `paused`.

3. Exemptions are not Flutter's. An app that plays audio is still subject to the OS rules in the earlier sections. Flutter will happily be in `paused` while the OS keeps the process because of audio. The enum does not say that.

4. Nothing. There is no Flutter scheduler that runs work for a paused app.

5. The OS lifecycle message is the wake, delivered as a state change. No Flutter time limit.

6. Widgets are the OS's. Flutter does not draw a home-screen widget from this enum.

7. The trouble that is Flutter's: code that treats `paused` as "the process is frozen" is wrong on the timing, and code that treats `inactive` as "not visible" is wrong on Android, where inactive can still be on screen. Code that waits for `paused` on desktop never sees it. The doc says a hard kill skips the callbacks, so a save in the last state is not guaranteed to run. That matches every OS section above.

## Not found, or not sure

- RunningBoard. Public docs describe suspend and scene disconnection. They do not describe RunningBoard. Jetsam is the same: the effect (a suspended app can be killed to reclaim memory, with no callback) is documented; the daemon's policy is not, in what was read.
- The length of iOS `beginBackgroundTask`. Current doc: read `backgroundTimeRemaining`. Archived energy guide: up to a few more minutes. Not a verified 30 seconds.
- Whether iOS suspends an app that holds the audio background mode but plays silence. The mode's description says "audible content". A runtime detector was not found. The WWDC 2022 advice is that the app should shut the audio engine down.
- iOS sockets, timers, and file I/O across suspend. Not stated in the pages read. GPU is stated.
- The exact moment and size of the iOS app-switcher snapshot.
- Live Activities: not fetched. No budget is reported.
- Background `URLSession` and silent-push wake length: not re-fetched. No number is reported for the push wake.
- SwiftUI `scenePhase`: not fetched.
- Android DownloadManager's process model: not re-fetched. The quota page treats its jobs like other jobs.
- The exact Doze maintenance-window schedule. Documented as existing and getting rarer. Lengths are not published.
- Frequent and Restricted standby rows. They are on the power page; the fetch dropped their labels, so the cells are not copied.
- Whether a not-yet-frozen Android background process may submit GPU work.
- The ongoing-notification requirement of `startForeground`. Recalled, not read off a page this session.
- App Nap's timer rate. "Less often" is the documented phrase. No interval.
- Whether Metal use prevents App Nap the way OpenGL did in the archived guide.
- `NSProcessInfo` activity option flags, as a list.
- WidgetKit's budget on macOS specifically, as opposed to the iOS page.
- Win32 Efficiency mode's priority change, beyond EcoQoS. QoS is documented. The idle-priority half is from a Microsoft Q&A reply.
- Which Windows background-task triggers get 10 minutes rather than 30 seconds. The split is acknowledged and not listed.
- Windows widget board, toast activation budgets, and whether a suspended app's sockets are closed or only unread.
- UWP GPU submission while suspended.
- A Linux desktop that freezes an ordinary app when its window is hidden. Not found, after the portal, ChromeOS suspend, and the unshipped cgroup-portal discussion.
- systemd-oomd's current behavior. Not re-fetched.
- Chromium `kFreezingAudioProtectionTime` and `kFreezingVisibleProtectionTime` defaults, and `kFreezingHighCPUProportion`. The parameters exist. `features.cc` was not retrieved.
- Whether Chromium's freezing feature is enabled for desktop users. The policy code is on main. A design note at HEAD says it is not enabled on desktop. Both were read. They disagree.
- DevTools, favicon updates, and title updates as freeze exemptions. In the old note. Not in the reason list read from `freezing_policy.cc`.
- Whether a silent buffer sets Chromium `IsAudible()`.
- What happens to an in-flight `fetch` or a WebSocket when a page is frozen but not put in bfcache and not discarded.
- The periodic-unfreeze interval in `freezing_policy.cc`.
- Picture-in-picture as a named Chromium cannot-freeze reason. Visibility and mirroring are named. PiP is not, in the list read.
- Flutter on Windows specifically, as opposed to "non-web desktop". The enum doc groups those desktops together.
