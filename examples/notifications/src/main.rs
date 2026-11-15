//! A program that shows notifications, in order: it says where its permission stands, posts a
//! greeting, which is denied without asking while its user has not allowed it, asks its user, and
//! posts the greeting again, with a button; where the system has replies, it posts the greeting a
//! third time, replacing it, with an action answered by typing; where the system has scheduling,
//! it schedules a silent reminder for the first moment of 2100; and it removes a notification of
//! an id it no longer needs. Then it listens for what its user does to its notifications, on two
//! streams, until it is stopped: the first says each activation, its id, the action chosen and
//! what the user typed, and the second each id and action again.
//!
//! It prints each step as `what: result`.
//!
//! ```text
//! cargo build -p notifications --target wasm32-wasip3
//! ```

use wasdk::notifications::{self, Notification, NotificationError, Notifications};
use wasdk::permissions::{self, State};
use wasdk::system_clock::Instant;

/// The first moment of 2100, in seconds from the Unix epoch.
const YEAR_2100: i64 = 4_102_444_800;

#[wasdk::main]
async fn main() {
    let Some(notifications) = notifications::get_notifications() else {
        println!("notifications: none");
        return;
    };
    let permission = permissions::notifications();
    println!("permission: {}", state(permission.state()));
    let posted = notifications.post(greeting()).await;
    println!("post greeting: {}", outcome(posted));
    println!("permission asked: {}", state(permission.request().await));
    let posted = notifications.post(greeting()).await;
    println!("post greeting again: {}", outcome(posted));
    post_with_a_reply(&notifications).await;
    schedule_a_reminder(&notifications).await;
    notifications.remove("old");
    println!("remove old: done");
    hear_activations(&notifications).await;
}

/// Hello, from the program, with a button to answer later.
fn greeting() -> Notification {
    let greeting = Notification::new("greeting", "Hello");
    greeting.body("From examples/notifications");
    greeting.add_action("later", "Later");
    greeting
}

/// Posts the greeting a third time, replacing it, with an action answered by typing, where the
/// system has replies.
async fn post_with_a_reply(notifications: &Notifications) {
    let Some(replies) = notifications.replies() else {
        println!("replies: none");
        return;
    };
    let greeting = greeting();
    replies.add_reply_action(&greeting, "answer", "Answer", "Say something");
    let posted = notifications.post(greeting).await;
    println!("post greeting with a reply: {}", outcome(posted));
}

/// Schedules a silent reminder for the first moment of 2100, where the system has scheduling.
async fn schedule_a_reminder(notifications: &Notifications) {
    let Some(scheduling) = notifications.scheduling() else {
        println!("scheduling: none");
        return;
    };
    let reminder = Notification::new("reminder", "Remember");
    reminder.silent();
    let at = Instant {
        seconds: YEAR_2100,
        nanoseconds: 0,
    };
    let scheduled = scheduling.schedule(reminder, at).await;
    println!("schedule reminder: {}", outcome(scheduled));
}

/// Says each activation, on two streams, until the program is stopped.
async fn hear_activations(notifications: &Notifications) {
    let mut activations = notifications.activations();
    let mut again = notifications.activations();
    println!("listening for activations");
    wasdk::spawn(async move {
        while let Some(activation) = again.next().await {
            println!("heard again: {} {:?}", activation.id, activation.action);
        }
    });
    while let Some(activation) = activations.next().await {
        println!(
            "activated: {} {:?} {:?}",
            activation.id, activation.action, activation.reply
        );
    }
}

fn state(state: State) -> &'static str {
    match state {
        State::NotAsked => "not asked",
        State::Granted => "granted",
        State::Denied => "denied",
    }
}

/// What a post or a schedule answered, in a word or two.
fn outcome(answer: Result<(), NotificationError>) -> String {
    match answer {
        Ok(()) => "ok".to_owned(),
        Err(NotificationError::Denied) => "denied".to_owned(),
        Err(NotificationError::Failed(why)) => format!("failed ({why})"),
    }
}
