# How each WaOS program could appear as an app of its own on Android

Research for the lead, 2026-10-29. Read only: nothing under `/Users/mac/code` was changed. Every claim is marked **[read: source]** or **[inferred]**. AOSP sources are `main` at frameworks/base unless stated; Chromium sources are `main`.

## In short

- **One APK (option A)** gives a program its own Recents card (icon and colours through `TaskDescription`), its own home-screen icon (a pinned shortcut, probably badged with WaOS's) and its own notification group, from API 26. Recents' "App info", the desktop window header, the notification header, the app drawer and Settings still say WaOS: these follow the package, and no public API lends them.
- **A shell APK per program (B)** gives everything, as Chrome's WebAPKs do, without any cross-process drawing: the shell's activity starts WaOS's activity in the shell's own task. But on-device minting meets Play's ban on the install permission for this use, developer verification from 2027, and a system dialog per install. Chrome and Samsung manage it only because Google and Samsung sign the shells.
- **A shell that owns the window (C)** adds little over B, at API 35 and an unverified keyboard path.
- **Recommendation:** A for v1, built so that B can be added later.

## Starting point (local)

- Inset's Android app is one `NativeActivity` subclass, `rs.inset.InsetActivity`, plus a non-exported `InsetFileProvider` and `InsetNotificationReceiver`; notifications go on two fixed channels, "Notifications" and "Silent notifications", with the app's icon as small icon **[read: `/Users/mac/code/inset-rs/crates/inset-cli/android/rs/inset/*.java`]**.
- v1 decision 3: on Android, "an identity per task in WaOS's one process" **[read: `/Users/mac/code/waos/docs/proposals/2026-10-16-waos-v1.md`]**. Notifications decision: "WaOS with a channel per program on Android" **[read: `/Users/mac/code/waos/docs/proposals/2026-10-29-notifications.md`]**.
- Inset's default `minSdkVersion` is 26; WaOS's mobile minimum is 36 for convenience, 31 workable; surfaces need 29, geometry 31, `ASurfaceControl_fromJava` 34 **[read: `/Users/mac/code/waos/docs/proposals/2026-10-29-media-research.md`, `/Users/mac/code/wasdk/docs/proposals/2026-09-26-waos-mobile.md`]**.

## Findings

### Tasks and Recents (AOSP)

- A task's description is assembled from its activities top-down to the root: label, icon, icon file and colours each come from the topmost activity that set them **[read: AOSP `services/core/java/com/android/server/wm/Task.java`, `updateTaskDescription` / `setTaskDescriptionFromActivityAboveRoot`]**.
- `relinquishTaskIdentity` hands a task's identity (base intent, effective uid) to the next activity only when that activity has the same uid, unless the root is a system app **[read: AOSP `Task.java`, `FindRootHelper.test` and `setIntent`]**. So a shell APK's launcher activity cannot hand its task's identity to WaOS's activity, nor WaOS's to a shell's **[inferred from the same]**.
- `ActivityManager.TaskDescription`: label and colours since API 21; a `Bitmap` icon through the `TaskDescription(String, Bitmap[, int])` constructors since API 21, deprecated ("Use Builder instead") but still mapped to `Icon.createWithBitmap` on AOSP main; an icon as a drawable resource since 28; the `Builder` since 33, whose `setIcon(int iconRes)` takes a resource only; `Builder.setIcon(Icon)` and `setBadge(Icon)` (a small badge "displayed on bottom corner of the icon in places like the taskbar and launcher") only since API 37 **[read: https://developer.android.com/reference/android/app/ActivityManager.TaskDescription and `.Builder`; AOSP `core/java/android/app/ActivityManager.java`]**. So a program's own icon, which comes from its `.wapk` and is not a resource of WaOS's APK, needs the deprecated `Bitmap` constructor on API 31 to 36, and `setIcon(Icon)` from 37 **[inferred]**.
- AOSP Launcher3's Recents takes a task's icon from the description's in-memory `Bitmap` or icon file; a resource icon is not loaded ("TODO: Load icon resource (b/143363444)") and falls back to the base activity's icon. Its content description is "<application label> <task label>"; with the `enable_overview_icon_menu` flag on, the title shown beside the icon is the base activity's label, not the task's **[read: AOSP `packages/apps/Launcher3/quickstep/src/com/android/quickstep/TaskIconCache.java`, `aconfig/launcher_overview.aconfig`]**. Recents' "App info" opens the app of the task's base intent **[read: Launcher3 `TaskShortcutFactory.java` `APP_INFO`; inferred: that is WaOS's page for a task in WaOS's APK]**.
- In desktop windowing (tablets, external displays), WM Shell's window header takes the icon from the base activity's `ActivityInfo` and the name from `getApplicationLabel` of the base activity's application: a TaskDescription is not consulted **[read: AOSP `libs/WindowManager/Shell/src/com/android/wm/shell/windowdecor/DesktopModeWindowDecoration.java`, `loadAppInfoIfNeeded`]**. So every task in WaOS's APK shows "WaOS" and WaOS's icon in its desktop window header, while a task whose base activity is a shell APK's shows the shell's **[inferred]**. Pixel Launcher is a Google build of Launcher3 and may differ in detail **[inferred]**.
- Document mode, API 21: `FLAG_ACTIVITY_NEW_DOCUMENT` makes the started activity a task of its own in Recents, found again by "the intent component name and the intent data"; with `FLAG_ACTIVITY_MULTIPLE_TASK` a new task is always made. `android:documentLaunchMode` sets the same in the manifest (`intoExisting`, `always`, `none`, `never`) and needs `launchMode="standard"`. `android:maxRecents` caps an app's tasks in Recents: "The default is 16, and the maximum value is 50 (25 on low memory devices)". `ActivityManager.AppTask` removes or moves a task **[read: https://developer.android.com/guide/components/activities/recents]**. So a task per program is `FLAG_ACTIVITY_NEW_DOCUMENT` with the program's id as the intent's data (one task per program, found again), on API 21+ **[inferred]**.
- Inset's activity is declared `android:launchMode="singleTask"` **[read: `/Users/mac/code/inset-rs/crates/inset-cli/src/android.rs`, the manifest template]**, which document mode does not allow **[inferred from the guide above]**. Inset's Android host runs one loop for the one `AndroidApp` that `android_main` is handed **[read: `inset-embedder-android/src/lib.rs`, `host.rs`]**. `android-activity` 0.6 calls `android_main` per activity instance and says that "Theoretically you could even run multiple `Activity` instances at the same" time **[read: `~/.cargo/registry/src/*/android-activity-0.6.1/README.md`, `src/lib.rs`]**. So a task per program needs Inset's Android host to hold one window per activity instance, several at once, each its own view **[inferred]**.

### Launcher shortcuts

- Pinned shortcuts, API 26: they "appear in supported launchers as separate icons"; the user "receives a confirmation dialog asking their permission to pin the shortcut"; `isRequestPinShortcutSupported()` says whether the launcher allows it; the app can update a pinned shortcut later with `updateShortcuts()`; "the system performs backup and restore on pinned shortcuts automatically" **[read: https://developer.android.com/develop/ui/views/launch/shortcuts/creating-shortcuts]**. Static and dynamic shortcuts (long-press on WaOS's icon): "up to 15 static and dynamic shortcuts", Google recommends four **[read: https://developer.android.com/develop/ui/views/launch/shortcuts/best-practices]**.
- A pinned shortcut is not an app: it is not in the app drawer, not in Settings' app list, and long-pressing it shows WaOS's menu **[inferred: a shortcut belongs to its publishing package, `ShortcutInfo.getPackage()`, as AOSP Launcher3 `CacheableShortcutInfo.kt` keys it]**.

### Chrome's WebAPKs

| | What it is | Source |
|---|---|---|
| Minting | "Chrome requests a specialized server (WebAPK minting server) to generate a real Android APK for the web app. This APK is then installed on the device." The Java side "performs the actual install (via Play Store or download)" | **[read: Chromium `components/webapps/docs/android_architecture.md`]** |
| Trust | Chrome checks "whether a WebAPK is signed by the WebAPK Minting Server": a V1 RSA signature, or a signature in the APK's zip comment checked against a public key; validation can be turned off "only for development with unsigned WebApks" | **[read: Chromium `components/webapk/android/libs/client/.../WebApkValidator.java`]**; dev installs also need Play's `finsky.webapk.do_webapk_install_package_check` and `..._signing_check` turned off, and Chrome's `--skip-webapk-verification` **[read: `chrome/android/webapk/README.md`]** |
| What the shell holds | A manifest with the start URL, scope, colours, icons and share targets as meta-data; intent filters for its URL scope; share activity-aliases; a splash activity; an exported `WebApkServiceFactory` service; `POST_NOTIFICATIONS`; `minSdkVersion` 24 | **[read: Chromium `chrome/android/webapk/shell_apk/AndroidManifest.xml`]** |
| Code | A "runtime library which is stored in the Chrome APK's assets and which is extracted from the Chrome APK by the WebAPK at runtime", so most WebAPK logic "can be updated as often as Chrome" | **[read: `chrome/android/webapk/README.md`]** |
| Launch | The WebAPK sends Chrome an intent (`ACTION_START_WEBAPP` if bound to Chrome, `VIEW` if unbound); Chrome's `WebappLauncherActivity` routes it to `SameTaskWebApkActivity` or `WebappActivity` | **[read: `android_architecture.md`]** |
| Two task models | `WebappActivity` is declared `documentLaunchMode="intoExisting"` and started with `NEW_TASK \| NEW_DOCUMENT` and data `webapp://<id>`: a Chrome-owned task per web app. When the WebAPK shows its own splash, Chrome starts `SameTaskWebApkActivity` with no `NEW_TASK` (`CLEAR_TOP \| NO_ANIMATION \| FORWARD_RESULT`), so Chrome's activity runs on top of the WebAPK's own activity, in the WebAPK's task | **[read: Chromium `chrome/android/java/AndroidManifest.xml`; `WebappLauncherActivity.java`, `createIntentToLaunchForWebapp`]** |
| Notifications | Chrome binds the WebAPK's service (`IWebApkApi`): "Chrome hands over notifications to the WebAPK to display, so they look like they come from the WebAPK", and asks the WebAPK for notification permission on Android 13+ | **[read: `android_architecture.md`]** |
| Other permissions | Location and notifications are "delegated" from the installed app to the origin; on uninstall Chrome resets location and re-points notifications | **[read: `components/webapps/docs/android_registration_and_permissions.md`]**. Web permissions otherwise "work in the same way as other web apps"; WebAPKs "are not granted [notifications] at install time" **[read: https://web.dev/articles/webapks]** |
| Storage | "Chrome uses the current profile to store any data, and it will not be segregated away" | **[read: https://web.dev/articles/webapks]** |
| What the user gets | It shows "in the app launcher, in Android's app settings" and registers intent filters | **[read: https://web.dev/articles/webapks]** |
| Fallback | Without a WebAPK, Chrome adds a home-screen shortcut; from Android 8 such shortcuts carry the browser's badge | **[read, secondary: https://frontendmasters.com/courses/pwas-v2/application-icons and a Brave community report; not checked in a primary source]** |

- The task model is the key precedent for WaOS. In the same-task model the task's root is the WebAPK's activity, so Recents, "App info" and the desktop header name the WebAPK, while the content is Chrome's activity in Chrome's process **[inferred from `Task.java` and `DesktopModeWindowDecoration.java` above, and `WebappLauncherActivity.java`]**. No cross-process surface is involved: two apps' activities share one task **[inferred]**.
- Android 17 (API 37) adds `android.content.pm.webapp.WebAppManager`, a "System service that manages the installation and querying of Web Apps", whose `install` takes a title and a web manifest URL and answers the installed package's name; Chrome uses it for "auto-minted TWAs", installed by "Android's `WebAppManager` / `IWebAppService` (Mainline module)" **[read: https://developer.android.com/reference/android/content/pm/webapp/WebAppManager; Chromium `components/webapps/browser/android/.../TwaInstaller.java`; `android_architecture.md`]**. It installs a web app from a URL, which then runs in a browser as a TWA, so it does not mint shells for WaOS's programs **[inferred]**.

### Installing APKs that WaOS makes

| Question | Answer | Source |
|---|---|---|
| May an app install APKs? | It must hold `REQUEST_INSTALL_PACKAGES` (API 23) and, from Android 8, the user must allow it per source app ("Install unknown apps"); `canRequestPackageInstalls()` tells whether they have (API 26) | **[read: https://developer.android.com/reference/android/Manifest.permission; https://developer.android.com/reference/android/content/pm/PackageManager]** |
| Does every install ask? | A new install asks: with `REQUEST_INSTALL_PACKAGES`, an unspecified session behaves as `USER_ACTION_REQUIRED` and the installer gets `STATUS_PENDING_USER_ACTION`. An **update** need not ask (API 31, `setRequireUserAction(USER_ACTION_NOT_REQUIRED)`) when the installer is the update owner or installer of record of the existing version and the APK targets a recent API level: 29+ on Android 12, 30+ on 13, 31+ on 14, 33+ on 15, 34+ on 16, 35+ on 17, a bar that "will advance in future Android versions" | **[read: https://developer.android.com/reference/android/content/pm/PackageInstaller.SessionParams, `setRequireUserAction`]** |
| Can the user approve before the APK exists? | `Session.requestUserPreapproval` (API 34) asks the user before `commit` | **[read: https://developer.android.com/reference/android/content/pm/PackageInstaller.Session]** |
| Play: may WaOS hold the permission? | Only with "a core purpose to enable installation of packages"; the permitted uses are web browsing or search, communication with attachments, file sharing, enterprise device management, backup and restore, device migration; disallowed "When the required task can be done with a less intrusive method". The policy exists against "apps that self update ... or download harmful APKs" | **[read: https://support.google.com/googleplay/android-developer/answer/12085295]** |
| Play: may WaOS run downloaded programs? | An app "may not download executable code (such as dex, JAR, .so files) from a source other than Google Play. This restriction does not apply to code that runs in a virtual machine or an interpreter where either provides indirect access to Android APIs" | **[read: https://support.google.com/googleplay/android-developer/answer/9888379]** |
| Sideloaded, certified devices | Developer verification: from 2026-09-30, "App registration becomes required for participating stores" (Google Play, Galaxy Store and five other stores) in Brazil, Indonesia, Singapore and Thailand, on certified devices running Android 7+; Google will "expand these protections globally for all apps on certified Android devices in 2027". "Unregistered apps can be sideloaded with Android Debug Bridge (adb) or advanced flow", an opt-in "for installing apps from unverified developers, which includes security checkpoints to resist coercion scams". Registration is of package names, also through an API; limited-distribution accounts share apps "to up to 20 devices" | **[read: https://android-developers.googleblog.com/2026/06/android-developer-verification.html; https://developer.android.com/developer-verification]** |

- In 2026 the check covers installs from the participating stores, which a shell installed by WaOS is not; from 2027 it covers all apps, and a shell minted on the phone has a package name nobody registered, so it would install only by adb or the advanced flow **[inferred from the post above]**. WaOS could register a package name for each shell through the Android Developer Console API as it mints it, but then each program's install depends on a service of WaOS's, which the v1 decision on package sources ("no service of ours") rules out **[inferred; `2026-10-16-waos-v1.md` decision 2]**. Whether the advanced flow, once turned on, also covers installs that WaOS itself requests through `PackageInstaller` is **not documented** in what was read.
- Minting is free of a server only if WaOS signs the shell on the phone. The key must outlive the shells, since an update must carry the same signature; a key kept in WaOS's data is lost with WaOS, after which its shells can only be uninstalled and installed again **[inferred]**.

### How a shell would reach WaOS, and how frames would reach a shell

| Route | What it is | API | Source |
|---|---|---|---|
| **Shell's task, WaOS's activity** | The shell's launcher activity starts WaOS's activity without `NEW_TASK`, so WaOS's activity runs in WaOS's process on top of the shell's activity, in the shell's task: Chrome's same-task WebAPK model | any | **[read: Chromium `WebappLauncherActivity.java`; inferred for WaOS]** |
| **WaOS's task, shell only launches** | The shell starts WaOS's activity with `NEW_TASK \| NEW_DOCUMENT` and the program's id: the task's base activity is WaOS's, so Recents' App info and the desktop header name WaOS | any | **[read: Chromium `WebappActivity` model; `DesktopModeWindowDecoration.java`]** |
| **Bound service** | WaOS binds the shell's exported service to have it act in its own name, as Chrome binds a WebAPK's `IWebApkApi` to post notifications | any | **[read: `android_architecture.md`]** |
| **Content provider** | Read-only metadata (which program, which WaOS), as the WebAPK's `SplashContentProvider` serves its splash | any | **[read: shell `AndroidManifest.xml`; inferred use]** |
| **Embedded views** | `SurfaceControlViewHost` (API 30) puts a View hierarchy from one process into another's `SurfaceView` ("The primary usage of this class is to embed a View hierarchy from one process in to another"); the embedded hierarchy receives input only if the `SurfaceView` is above its host surface | 30 | **[read: https://developer.android.com/reference/android/view/SurfaceControlViewHost; `SurfaceView.setChildSurfacePackage`]** |
| **Layers across processes** | `SurfaceControl` is `Parcelable` (API 29), so a shell can hand WaOS its `SurfaceView`'s layer (`getSurfaceControl`, API 29) and WaOS can parent its buffer layers under it; from API 35 WaOS can take input on its own `SurfaceControl` with `WindowManager.registerBatchedSurfaceControlInputReceiver`, linked to the host's `InputTransferToken` | 29, input 35 | **[read: https://developer.android.com/reference/android/view/SurfaceControl, `SurfaceView`, `WindowManager`; inferred: the combination]** |

- Of the two cross-process routes, `SurfaceControlViewHost` carries Views, which Inset's `NativeActivity` host does not have; the layer route fits the experiment's frame path (an `AHardwareBuffer` per frame on an `ASurfaceControl`) and the media research's own `SurfaceControl` **[inferred from `2026-09-26-waos-mobile.md` and `2026-10-29-media-research.md`]**. Text input to an embedded window is not settled: AOSP has focus grants for embedded windows and moves the IME target to them, but no public document was found that says an embedded `SurfaceControl` input receiver gets the keyboard **[read, partial: AOSP change https://android.googlesource.com/platform/frameworks/base/+/0cfda55b95e8; not verified]**.
- Same-task needs neither: the shell's process draws nothing and the program draws in WaOS's own window, exactly as in a task of WaOS's **[inferred]**.

### What other runtimes do

| Runtime | How its apps appear | Source |
|---|---|---|
| Chrome with Google services | WebAPK minted and signed by Google's server, installed through Play ("the browser installs that app silently"): launcher icon, Settings › Apps, link capture, notifications in its name; fallback a browser-badged shortcut | **[read: https://web.dev/learn/pwa/installation; Chromium docs above]** |
| Chrome's legacy web apps | A Chrome activity per web app in a document task (`WebappActivity`, `documentLaunchMode="intoExisting"`, data `webapp://<id>`); an older scheme of fixed slots `WebappActivity0`–`9` is still named in the code | **[read: Chromium `AndroidManifest.xml`; `WebappLauncherActivity.java` comment "WebappActivity, WebappActivity0-9"]** |
| Chrome on Android 17 desktop | "Auto-minted TWA" through the system's `WebAppManager` | **[read: above]** |
| Samsung Internet | WebAPKs from Samsung's own minting server, "but only on Samsung-manufactured devices"; shortcuts elsewhere | **[read: https://web.dev/learn/pwa/installation]** |
| Firefox, Edge, Opera, Brave | No trusted minting server, so a pinned shortcut: "a browser-badged icon on the home screen", no launcher or Settings entry. Firefox's shortcut starts its launcher activity with `FLAG_ACTIVITY_NEW_DOCUMENT` and the start URL as data, and the web app's activity sets `TaskDescription(name, icon, themeColor)` with the deprecated `Bitmap` constructor: a task per web app with its own icon and colour in Recents | **[read: https://web.dev/learn/pwa/installation; Firefox `mobile/android/android-components/components/feature/pwa/.../WebAppShortcutManager.kt`, `ext/WebAppManifest.kt`, `feature/WebAppActivityFeature.kt` on GitHub `mozilla-firefox/firefox` main]** |
| Huawei, ZTE | QuickApps: like shortcuts, badged with the QuickApp icon, also listed in a "QuickApp Center" | **[read: https://web.dev/learn/pwa/installation]** |
| Android Instant Apps | Gone: "Starting December 2025, Instant Apps cannot be published through Google Play, and all Google Play services Instant APIs will no longer work" | **[read: https://developers.google.com/android/reference/com/google/android/gms/instantapps/package-summary]** |
| Game streaming | Not established: no primary source was found that GeForce NOW or Xbox Cloud Gaming put per-game icons on a phone's home screen; GeForce NOW adds tiles on SHIELD TV only | **[searched; not found]** |

- The pattern across runtimes: a real app per program only where a party the system already trusts signs it (Google through Play, Samsung on its devices, and from API 37 the system's own web-app service); everyone else uses a pinned shortcut plus a document task with its own `TaskDescription` **[inferred from the rows above]**.

### Notifications, the share sheet, Settings

- **Channel groups**, API 26: "If you want to further organize the appearance of your channels in the settings UI, you can create channel groups", one per account in Google's example, which "help users differentiate and control multiple notification channels that have identical names"; `NotificationChannelGroup.isBlocked` and `setDescription` since API 28; once a channel is made, "The user has complete control" of its behaviour **[read: https://developer.android.com/develop/ui/views/notifications/channels; https://developer.android.com/reference/android/app/NotificationChannelGroup]**. So a group per program, with its channels beneath it, gives each program its own section and switch in WaOS's notification settings **[inferred]**.
- A notification's small icon can be an `Icon` made at run time (API 23), and its large icon a bitmap **[read: https://developer.android.com/reference/android/app/Notification.Builder]**. The app name in a notification's header is the posting package's: the extra that substitutes it, `EXTRA_SUBSTITUTE_APP_NAME`, is `@hide @SystemApi` and needs `SUBSTITUTE_NOTIFICATION_APP_NAME` **[read: AOSP `core/java/android/app/Notification.java`]**. So every notification from WaOS's APK reads "WaOS" in its header, whatever its group, and only a shell that posts it itself shows the program's name there **[inferred]**.
- **Share sheet**: Direct Share targets are dynamic sharing shortcuts with categories matching a `<share-target>` declared in the app's XML (Android 10+), the only route from Android 11 **[read: https://developer.android.com/training/sharing/direct-share-targets]**. A program could appear as a Direct Share target with its own name and icon, but beneath WaOS's identity, and the share targets' MIME types are fixed in WaOS's resources **[inferred]**. The app row of the share sheet lists activities that accept `ACTION_SEND`, which are manifest declarations, so only a shell APK puts a program there **[inferred]**.
- **Settings › Apps**, the app drawer, uninstalling, storage, battery and per-app language: all are per package. A program inside WaOS's APK appears in none of them; a shell APK appears in all, with its own label and icon (Chrome's WebAPKs "show up in the app launcher, in Android's app settings") **[read for WebAPKs: https://web.dev/articles/webapks; inferred for the rest]**.

### Permissions

| | One APK (WaOS's identity) | Shell APK per program |
|---|---|---|
| Who the system asks for | WaOS: runtime permissions belong to an app's uid **[inferred: Android's model]**; Chrome's own example is one prompt for Chrome, with sites managed by Chrome's site settings **[read: https://web.dev/articles/webapks, "Permissions work in the same way as other web apps"]** | The shell only if the shell itself does the work. Work done in WaOS's process is WaOS's access, unless WaOS chains attribution **[inferred]** |
| Notifications | `POST_NOTIFICATIONS` (API 33) once for WaOS; per program, a channel group the user can block | The shell holds `POST_NOTIFICATIONS` and posts what WaOS hands it through a bound service, as WebAPKs do; Chrome asks the WebAPK for the permission on Android 13+ **[read: Chromium `android_architecture.md`]** |
| Camera, microphone | WaOS's grant; the system's indicator and privacy dashboard show WaOS | Attribution chains (API 31): "Attribution sources can be chained to represent cases where the protected data would flow through several applications", e.g. "a speech recognizer using the mic so it can provide recognition to a calling app"; the data source checks "whether every app in the attribution chain has permission" and records the access by each **[read: https://developer.android.com/reference/android/content/AttributionSource; `ContextParams.Builder.setNextAttributionSource`, API 31]**. So the shell and WaOS would both need the grant: two system prompts for one program, unless the shell opens the camera in its own process **[inferred]**. Whether the camera service and the mic indicator honour a chain from a third-party proxy was **not verified** |
| Attribution tags | `<attribution android:tag android:label>` (API 30) lets an app tag parts of itself, but the tags are declared in the manifest, so they cannot be made per installed program **[read: https://developer.android.com/guide/topics/data/audit-access]** | n/a |
| WaOS's own permissions | Unchanged: WaOS asks the system once for itself, then asks the user per program at first use as `2026-10-23-permissions.md` decides (notifications, camera, microphone ask; network, processes, clipboard are switches) **[read: that proposal; inferred: how they combine]** | Two layers that can disagree: the system's grant to the shell and WaOS's switch for the program. WaOS's switch must still govern, since WaOS's process does the IO; the system's grant to the shell would then be a second gate **[inferred]** |

- Chrome's precedent is the same split: the system grant is the shell's only for notifications (and location, which it delegates); everything else is Chrome's own grant with Chrome's per-site settings on top **[read: Chromium `android_registration_and_permissions.md`; https://web.dev/articles/webapks]**.

## Options for WaOS

Four options, from least to most separate. D is listed to close the range: a full app per program is a native build, which `2026-10-17-the-waos-project.md` leaves out of the first version ("No native build of programs in the first version") **[read]**.

- **A. A task per program in WaOS's APK.** Each running program is a document task of WaOS's own activity, with the program's `TaskDescription`. A pinned shortcut is its launcher icon, and a notification channel group gives it its notifications.
- **B. A minted shell APK per program, with WaOS's activity in the shell's task.** WaOS writes a small APK per program, signs it and installs it with `PackageInstaller`. The shell's launcher activity starts WaOS's program activity in the shell's own task, as Chrome's same-task WebAPKs do. The shell posts notifications that WaOS hands it through a bound service.
- **C. A minted shell APK that shows the program's window in its own process.** As B, but the shell owns the window and WaOS draws into it across processes (`SurfaceControl` parcelled to WaOS, input through `SurfaceControlInputReceiver`). The shell could then hold camera and microphone grants and use them itself.
- **D. A real APK per program, built by its developer.** A native build of the program, out of scope.

### What the user sees

| | A: task in WaOS's APK | B: shell, WaOS's activity in its task | C: shell owns the window |
|---|---|---|---|
| Home screen | A pinned shortcut with the program's icon and name, after a launcher dialog. It is probably badged with WaOS's icon, as browsers' shortcuts are **[read for browsers: web.dev; inferred for WaOS]** | A real app icon, no badge | the same as B |
| App drawer | absent ("Don't have an icon in the Launcher") **[read: web.dev, for shortcuts]** | present | present |
| Recents | Its own card with the program's icon and colours **[read: Launcher3]**. The visible title is WaOS's activity label wherever Launcher3 shows a title. "App info" opens WaOS | Its own card, and every attribute is the shell's, so "App info" opens the shell | the same as B |
| Desktop windowing header | "WaOS" and WaOS's icon **[read: WM Shell]** | the program's | the program's |
| Settings › Apps | WaOS only. The program has a channel group under WaOS's notifications | Its own entry: uninstall, notifications, the permissions the shell holds. Storage is near zero, since data stays in WaOS **[inferred]** | the same as B |
| Notifications | The header says "WaOS". The large icon and content are the program's, and the program has its own group switch | The header says the program's name; the shell's own notification switch | the same as B |
| Share sheet | A Direct Share target per program is possible, beneath WaOS's identity | An app row of its own, if its `SEND` filters are written in at minting | the same as B |
| Permission prompts | WaOS's, once each, then WaOS's own per-program questions | Notifications the shell's; camera and microphone still WaOS's (or both, if chained) | The shell's, if the shell does the capture itself |
| Install and update | Instant, no system dialog except the shortcut's | A system install dialog per program, after "Install unknown apps" is turned on for WaOS. Updates are silent from API 31 if the shell's target level is kept current | the same as B |

### What it needs from Inset

| | A | B | C |
|---|---|---|---|
| Windows | **Several activity instances, one window each.** Today Inset runs one `AndroidApp` loop per `android_main`, and its activity is `singleTask` **[read]**. A program window needs a `standard` activity whose instances attach to the running app as windows, instead of each starting the app again. It also needs a window the system restores from Recents, after WaOS's process died, to reach the app with its intent **[inferred]** | as A | as A, plus windows whose layer lives in another process |
| Java | A window's task description: title, a bitmap icon, colours (`setTaskDescription`; the deprecated `Bitmap` constructor up to API 36, `Builder.setIcon(Icon)` from 37). `ShortcutManager.requestPinShortcut`, `updateShortcuts`, `disableShortcuts` (framework API 26, no AndroidX). A channel group per caller-chosen key in `InsetNotificationReceiver`, which today has two fixed channels **[read]** | A's, plus: the program activity exported, or a launcher activity, so a shell can start it in its task. A way for an app to add its own Java (a bound service client, `PackageInstaller`), since inset-cli compiles only Inset's classes **[read: media research §4]** | B's, plus parcelling `SurfaceControl`s, `ASurfaceControl_fromJava` (34) and input receivers (35) |
| Packaging | none new | inset-cli builds a shell template APK beside WaOS's: fixed dex, with manifest and resources patched per program | as B |

### What it needs from WaOS

| | A | B | C |
|---|---|---|---|
| Programs ↔ tasks | A program id as the task's intent data, one task per program (`NEW_DOCUMENT` without `MULTIPLE_TASK`); freezing a program when its window stops, waking it when it resumes; finishing the task when the program quits (`AppTask.finishAndRemoveTask`); `maxRecents` raised to 50 **[read: Recents guide; inferred use]** | A's, plus minting: write the shell's binary manifest and resources (package name, label, icon, program id), zip, sign with APK Signature Scheme v2/v3, install, and re-mint when the program's name or icon changes | B's, plus input and text input forwarded across processes |
| Trust | none new | Each side checks the other: WaOS serves only shells signed with its key, and a shell launches only WaOS's package with WaOS's certificate, as Chrome checks WebAPKs' signatures **[read: `WebApkValidator.java`; inferred for WaOS]** | as B |
| Lifecycle | Uninstalling a program removes its shortcut and notifications | Uninstalling a program uninstalls its shell (the user confirms again). Uninstalling WaOS orphans the shells, which then launch nothing. A shell uninstalled from Settings must uninstall the program, or leave it in the library **[inferred]** | as B |
| The deck | The phone design's deck of tabs becomes WaOS's own task (library, launcher), while programs live in Android's Recents. Keeping both means two task switchers **[inferred]** | as A | as A |

### Risks

| | A | B | C |
|---|---|---|---|
| Policy | none beyond today's: the interpreter exception covers programs **[read: Device and Network Abuse policy]** | `REQUEST_INSTALL_PACKAGES` is not a permitted use for this on Play **[read]**, so B is for sideloaded WaOS only, or a Play build without shells **[inferred]** | as B |
| Developer verification | none: WaOS is one registered app | From 2027, on certified devices, unregistered shell packages install only by adb or the advanced flow, unless WaOS registers each through an API, which needs a service **[read: 2026-06 post; inferred]** | as B |
| Each install | none | The system dialog per program, and the one-time "Install unknown apps" switch. Play Protect's treatment of unknown-developer APKs was not checked | as B |
| Signing | none | A key the shells' updates depend on. A Keystore key cannot leave the device and dies with WaOS **[inferred]** | as B |
| Updates | none | Silent only while each shell targets the rising API bar (35+ on Android 17), so old shells need re-minting **[read: `setRequireUserAction`]** | as B |
| Identity gaps | Recents title, "App info", desktop header, notification header and Settings all say WaOS | small: storage, battery and data are WaOS's, behind a shell that looks whole | as B |
| Engineering | Inset's multi-window Android host, which Android's own desktop windowing also wants **[inferred]** | A, plus an APK writer and signer on the phone | B, plus cross-process layers, input and the unverified keyboard |

### Minimum API level

| | Needs | Against 26 (Inset) | Against 31 (workable) | Against 36 (experiment's choice) |
|---|---|---|---|---|
| A | 21 for document tasks and `TaskDescription`; 26 for pinned shortcuts and channel groups; the program's icon through a deprecated constructor until 37 **[read]**. The frame path already needs 29 and 31 | Fits | Fits | Fits; at 37 the icon moves to the non-deprecated `setIcon(Icon)` |
| B | 26 for "Install unknown apps" per source; 31 for silent shell updates; 34 for approval before minting **[read]** | Works, but every update asks | Fits | Fits |
| C | 29 to parcel a `SurfaceControl`; 34 for `ASurfaceControl_fromJava`; 35 for input on a `SurfaceControl` **[read]**; the keyboard unverified | No | No | Fits, if the keyboard works |

## Recommendation

**A for this version: a task per program in WaOS's APK, with a pinned shortcut and a notification channel group per program.** This is v1 decision 3 as written, and the research shows what that decision buys and what it does not.

- **What A gives:** a program's own card in Recents (icon, colours), its own icon on the home screen, its own notification switch, all with no install dialog, no policy question and nothing above API 26 that WaOS does not already need. Firefox's web apps are built exactly this way.
- **What A cannot give:** a program in the app drawer or Settings, or under its own name in Recents' "App info", the desktop window header and the notification header. Only a separate package changes those. Android has no public API that lends one app's identity to another's task (`relinquishTaskIdentity` stops at the uid; `EXTRA_SUBSTITUTE_APP_NAME` is a system API).
- **Why not B now:** B is the only way to a fuller identity, but it costs the most where nothing is settled yet. Play forbids the install permission for this use. Developer verification will block unregistered shells on certified devices from 2027, by rules for on-device minting that are not published. And each program costs the user a system install dialog. Chrome and Samsung reach B only because Google and Samsung sign the shells; no other browser does it.
- **The path from A to B:** build A so that B is an addition, not a rewrite.
  1. The program activity, launched by an intent carrying the program's id, is the one a shell would start in its own task. Same-task launch needs no other change in WaOS.
  2. "Post this program's notification" is one seam: a channel group in A, the shell's bound service in B.
  3. Shortcut creation and shell installation are two answers to one step, "give this program a launcher icon".
  4. B then waits for two things: the verification rules for apps that one app writes and installs, and the lead's choice of how WaOS is distributed on Android (Play, or sideloaded).
- **C is not worth building.** Its one gain over B is camera and microphone prompts in the program's name. It costs cross-process input and an unverified keyboard path, and needs API 35. Attribution chains do not remove WaOS's own grant, so the user would answer two prompts anyway.
- **On the API level:** A needs nothing above 31, so the choice between 31 and 36 stays where the experiment left it, set by the frame path and not by identity.

## Decisions for the lead

1. **How programs appear on Android in v1.**
   - **A (recommended):** a task per program in WaOS's APK, with a pinned shortcut and a channel group.
   - B: a minted shell APK per program, sideloaded WaOS only.
   - C: a shell that owns the program's window.
2. **What becomes of the phone deck on Android under A.**
   - **(a) Recommended:** programs are Android tasks, and WaOS's own task is the library and launcher. Android's Recents is the overview.
   - (b) Keep the deck in WaOS's task as well, so a program opened from the library runs as a tab and one from its shortcut as a task. This gives two switchers.
   - (c) The deck only, as the experiment decided. This drops the per-task identity of decision 3.
3. **When a program gets its home-screen icon.**
   - **(a) Recommended:** WaOS offers the shortcut when a program is installed, and the launcher asks the user.
   - (b) Only when the user asks, from the library.
   - (c) Either of these, plus dynamic shortcuts of recent programs on WaOS's own icon (at most 15, 4 recommended).
4. **Notifications per program.** This refines the decided "channel per program".
   - **(a) Recommended:** a channel group per program, holding Inset's two channels, sound and silent.
   - (b) One channel per program, which loses the silent choice per program.
5. **Inset's shape for this.**
   - **(a) Recommended:** windows on Android are activities. Several `standard` instances attach to one running app, each with its own task description (title, icon, colours), and the system may restore one from Recents. This is a general Inset feature that Android's desktop windowing also needs.
   - (b) A WaOS-only activity outside Inset's model. This breaks the rule that capabilities go into Inset.
6. **For B later: who signs shells, and where.**
   - (a) A key made on the phone. Free of any service, but lost with WaOS.
   - (b) A WaOS publisher key on a server, which mints as Chrome's does. This contradicts "no service of ours".
   - (c) Defer until verification's rules for on-device minting are published (recommended).
7. **WaOS's Android distribution.** Shells only make sense for a sideloaded WaOS.
   - (a) Sideloaded only.
   - (b) Play, without shells.
   - (c) Both builds.

   This is not needed for A, and is recommended to stay open until B is reconsidered.

## Gaps in this research

- Not checked on a device: what Pixel Launcher (as opposed to AOSP Launcher3) shows as a Recents title, whether it badges every pinned shortcut, and whether NativeActivity tolerates several instances in one process with `android-activity` 0.6.
- Not found in a primary source: whether the developer-verification advanced flow covers installs another app requests; Play Protect's handling of minted shells; whether the keyboard reaches an embedded `SurfaceControl` input receiver; whether the camera service and microphone indicator honour an attribution chain from a third-party app; game-streaming apps' per-game icons.
- `developer.android.com` could not be read with the web-fetch tool, and was read with `curl` instead; the same text.
