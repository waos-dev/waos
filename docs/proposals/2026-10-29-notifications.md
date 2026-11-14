# Notifications

**Decisions taken (2026-10-29).** The lead took these, one question at a time, starting step 7's notifications. Already settled: a program's first notification asks, and a post without permission answers `denied` without asking (v1 interface decision 7, `2026-10-17-waos-v1-interface.md`); the permission's state is the `permissions` interface's (permissions decision 5); and who posts on each system (v1): the program's own process on macOS (step 10), WaOS under the program's taskbar identity on Windows, WaOS through the desktop's notification service on Linux, WaOS with a channel per program on Android, WaOS with the program's name in the content on iOS. Inset has no notifications, so by the project's rule the capability goes into Inset and WaOS wraps it.

1. **A notification is a value posted under an id; posting the same id again replaces it**, as every system's own model is (`UNNotificationRequest(identifier:)`, Android's `notify(id, …)`, the web's `tag`, Linux's `replaces_id`, a Windows toast's `Tag`), and as menus are values; not an object the app keeps, as Electron's `new Notification().show()`, which a click after a restart could not reach. A click, at launch or while running, arrives by the notification's id and the action chosen.
2. **A typed reply and delivery at a later time are capabilities, each none where the system lacks it**, as a tray icon's menus are: a reply on macOS and iOS (`UNTextInputNotificationAction`), Windows (a toast's text input), Android (`RemoteInput`, through the Java activity) and Linux where the server advertises KDE's `inline-reply`; delivery later, held by the system whether or not the app runs, on macOS and iOS (`UNTimeIntervalNotificationTrigger`, calendar triggers), Windows (`ScheduledToastNotification`) and Android (`AlarmManager` firing a receiver, one more small Java class), none on Linux and the web. For a WaOS program, scheduling is what lets a reminder outlive the program: the system holds it, not the program. Badges, progress, threads and urgency are left for later.

```rust
pub trait Notifications {
    fn post(&self, notification: &Notification) -> PostFuture;
    fn remove(&self, id: &str);
    fn replies(&self) -> Option<&dyn NotificationReplies>;
    fn scheduling(&self) -> Option<&dyn NotificationScheduling>;
}

pub struct Notification {
    pub id: String,
    pub title: String,
    pub body: Option<String>,
    pub image: Option<IconImage>,
    pub actions: Vec<NotificationAction>,
    pub silent: bool,
}

pub trait NotificationReplies {
    fn reply_action(&self, id: &str, label: &str, placeholder: &str) -> NotificationAction;
}

pub trait NotificationScheduling {
    fn schedule(&self, notification: &Notification, at: SystemTime) -> PostFuture;
}

// EmbedderClient::notification_activated(NotificationActivation { id, action, reply });
```
3. **The WIT follows Inset's** (2026-10-29): a notification is a resource with setters, so a new field breaks no program, as the clipboard's `copy-options` is; replies and scheduling are getters answering none where the system lacks them; clicks arrive on a stream of activations, each call a stream of its own, including the click that started the program. The permission stays the `permissions` interface's. Settled with it: ids are each program's own, kept apart by WaOS; until a program has an identity of its own with the system (step 10), WaOS posts under its own name and puts the program's name in the notification, as it always will on iOS; a click for a program that is not running starts it and is delivered once it listens; removing a program removes its notifications and cancels those scheduled.

```wit
interface notifications {
  use menus.{image};
  use wasi:clocks/wall-clock@0.3.0.{datetime};

  resource notification {
    constructor(id: string, title: string);
    body: func(body: string);
    image: func(image: image);
    add-action: func(id: string, label: string);
    silent: func();
  }

  variant notification-error {
    denied,
    failed(string),
  }

  record activation {
    id: string,
    action: option<string>,
    reply: option<string>,
  }

  resource notification-replies {
    add-reply-action: func(notification: borrow<notification>, id: string, label: string, placeholder: string);
  }
  resource notification-scheduling {
    schedule: async func(notification: notification, at: datetime) -> result<_, notification-error>;
  }

  resource notifications {
    post: async func(notification: notification) -> result<_, notification-error>;
    remove: func(id: string);
    replies: func() -> option<notification-replies>;
    scheduling: func() -> option<notification-scheduling>;
    activations: func() -> stream<activation>;
  }

  get-notifications: func() -> option<notifications>;
}
```
4. **A removed program's notifications are left as they are, for now** (2026-10-29): the system holds them and Inset removes a notification only by its exact id, so WaOS cannot find all of a program's once it has quit and started again; a click on one of a program no longer installed tells the user the program was removed, and its scheduled ones that fire do the same. A rare case, not worth an interface yet.
