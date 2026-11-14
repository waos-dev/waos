//! "Remind Me in 1 Minute": a notification titled with the note shown, which the system holds and
//! shows a minute later whether or not the program runs. The program asks for the notifications
//! permission first, as `notifications` says, since a schedule without it answers `denied` and
//! never asks; the window says when it is denied, and what else kept the reminder from being set.
//!
//! A click on a reminder opens its note in the window brought forward, the click that launched the
//! program among them: the reminder's id names the note by its id, which the library keeps from one
//! run to the next.

use std::time::{Duration, SystemTime};

use waft_embedder::{
    Notification, NotificationActivation, NotificationError, PermissionState, PlatformRef,
};
use waft_foundation::App;
use waft_winui::InfoBarSeverity;

use crate::note::{Note, NoteId};
use crate::notes_app::NotesApp;
use crate::showing::reveal;

/// How long after it is set a reminder shows.
const DELAY: Duration = Duration::from_secs(60);

/// What the window says once a reminder is set, and when it cannot be.
const SET: &str = "You’ll be reminded of this note in a minute.";
const DENIED: &str = "Notes may not show notifications, so it cannot remind you. Allow them for Notes in WaOS’s settings.";
const NO_NOTIFICATIONS: &str = "This system shows no notifications, so Notes cannot remind you.";
const NO_SCHEDULING: &str =
    "This system cannot show a notification later, so Notes cannot remind you.";

/// What a reminder's id begins with, before the id of its note.
const NOTE_PREFIX: &str = "note:";

/// Sets a reminder of the note shown, a minute from now, asking for the permission first.
pub(crate) fn remind_in_a_minute(app: &mut App, notes: &NotesApp) {
    let shown = notes.browsing.read(app).shown_note();
    let Some(note) = shown.and_then(|id| notes.library.read(app).note(id)) else {
        return;
    };
    let reminder = reminder_of(note);
    let platform = app.platform();
    let Some(notifications) = platform.notifications() else {
        return notes.say(app, InfoBarSeverity::Error, NO_NOTIFICATIONS);
    };
    if notifications.scheduling().is_none() {
        return notes.say(app, InfoBarSeverity::Error, NO_SCHEDULING);
    }
    let asked = notifications
        .permission()
        .map(|permission| permission.request());
    let notes = notes.clone();
    app.spawn(async move |cx| {
        let allowed = match asked {
            Some(asked) => asked.await == PermissionState::Granted,
            None => true,
        };
        let (severity, said) = if allowed {
            schedule(cx.update(|app| app.platform()), &reminder).await
        } else {
            (InfoBarSeverity::Warning, DENIED.to_owned())
        };
        cx.update(|app| notes.say(app, severity, said));
    });
}

/// What a click on one of the program's notifications does: opens the note it reminds of.
pub(crate) fn reminder_clicked(
    app: &mut App,
    notes: &NotesApp,
    activation: NotificationActivation,
) {
    if let Some(id) = note_of(&activation.id) {
        reveal(app, notes, id);
    }
}

/// Has the system show `reminder` a minute from now; answers what the window says of it.
async fn schedule(platform: PlatformRef, reminder: &Notification) -> (InfoBarSeverity, String) {
    let Some(scheduling) = platform
        .notifications()
        .and_then(|notifications| notifications.scheduling())
    else {
        return (InfoBarSeverity::Error, NO_SCHEDULING.to_owned());
    };
    let scheduled = scheduling.schedule(reminder, SystemTime::now() + DELAY);
    match scheduled.await {
        Ok(()) => (InfoBarSeverity::Success, SET.to_owned()),
        Err(NotificationError::Denied) => (InfoBarSeverity::Warning, DENIED.to_owned()),
        Err(NotificationError::Failed(why)) => (
            InfoBarSeverity::Error,
            format!("The reminder could not be set: {why}"),
        ),
    }
}

/// The reminder of `note`, titled with its title.
fn reminder_of(note: &Note) -> Notification {
    let id = format!("{NOTE_PREFIX}{}", note.id);
    let mut reminder = Notification::new(id, note.title());
    reminder.body = Some("You asked Notes to remind you of this note.".to_owned());
    reminder
}

/// The note the reminder `id` is of; none for a notification of no note.
fn note_of(id: &str) -> Option<NoteId> {
    id.strip_prefix(NOTE_PREFIX)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reminder_names_its_note_by_its_id() {
        let note = Note::new(NoteId(42), "Call Mum".to_owned(), SystemTime::UNIX_EPOCH);
        let reminder = reminder_of(&note);
        assert_eq!(reminder.id, "note:42");
        assert_eq!(reminder.title, "Call Mum");
        assert_eq!(note_of(&reminder.id), Some(NoteId(42)));
    }

    #[test]
    fn a_notification_of_no_note_names_none() {
        assert_eq!(note_of("greeting"), None);
        assert_eq!(note_of("note:"), None);
        assert_eq!(note_of("file:/Users/me/a.txt"), None);
    }
}
