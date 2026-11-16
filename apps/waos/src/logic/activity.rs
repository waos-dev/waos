//! What a program did over the days the store keeps of it, summed as `waos-activity` says it, for
//! its settings page to show in short (the inspector's first panels, decision 2).

use waos::{AuditDay, AuditLog};
use waos_activity::{Activity, Counts, Folder};
use waos_package::ProgramId;
use waos_store::Store;

/// What the program `id` did over the days `store` keeps of it.
pub(crate) fn activity_of(store: &Store, id: &ProgramId) -> Activity {
    let mut activity = Activity::default();
    for day in store.days(id.as_str()) {
        add_day(&mut activity, &day);
    }
    activity
}

/// `day` added to `activity`.
fn add_day(activity: &mut Activity, day: &AuditDay) {
    for (captures, sessions) in [
        (&mut activity.camera, &day.camera),
        (&mut activity.microphone, &day.microphone),
    ] {
        for session in sessions {
            Activity::add_session(captures, session.seconds, session.simulated);
        }
    }
    for site in &day.domains {
        Activity::add_named(&mut activity.sites, &site.name, counts(&site.uses));
    }
    for address in &day.local_addresses {
        let name = address.name.to_string();
        Activity::add_named(&mut activity.local_addresses, &name, counts(&address.uses));
    }
    for folder in &day.folders {
        activity.add_folder(Folder {
            path: folder.path.display().to_string(),
            opened_to_read: folder.opened_to_read,
            opened_to_write: folder.opened_to_write,
            changes: folder.created + folder.removed + folder.renamed,
            changed: (folder.changed.iter())
                .map(|path| path.display().to_string())
                .collect(),
        });
    }
    activity.folder_refusals += day.folder_refusals;
    add_counts(&mut activity.clipboard_reads, &day.clipboard_reads);
    activity.pastes += day.pastes;
    add_counts(&mut activity.clipboard_writes, &day.clipboard_writes);
    for command in &day.commands {
        let uses = counts(&command.uses);
        activity.add_command(&command.name.machine, &command.name.command, uses);
    }
    for link in &day.links {
        Activity::add_named(&mut activity.links, &link.name, counts(&link.uses));
    }
    add_counts(&mut activity.notifications, &day.notifications);
    for asked in &day.questions {
        activity.questions.push(waos_activity::Asked {
            question: asked.question.clone(),
            at: seconds_of(asked.at),
            answer: asked.answer.clone(),
        });
    }
    if let Some(last) = last_use(day) {
        activity.used_at(last);
    }
}

/// When the day's last use was, of every kind it counts the time of.
fn last_use(day: &AuditDay) -> Option<i64> {
    let uses = (day.domains.iter().chain(&day.links))
        .map(|named| named.uses.last)
        .chain(day.local_addresses.iter().map(|named| named.uses.last))
        .chain(day.commands.iter().map(|named| named.uses.last))
        .chain([
            day.clipboard_reads.last,
            day.clipboard_writes.last,
            day.notifications.last,
        ])
        .flatten();
    let sessions = (day.camera.iter().chain(&day.microphone)).map(|session| session.start);
    let questions = day.questions.iter().map(|asked| asked.at);
    uses.chain(sessions).chain(questions).map(seconds_of).max()
}

fn counts(uses: &waos::Uses) -> Counts {
    Counts {
        allowed: uses.allowed,
        refused: uses.refused,
    }
}

fn add_counts(counts: &mut Counts, uses: &waos::Uses) {
    counts.allowed += uses.allowed;
    counts.refused += uses.refused;
}

fn seconds_of(at: std::time::SystemTime) -> i64 {
    let since = at.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
}
