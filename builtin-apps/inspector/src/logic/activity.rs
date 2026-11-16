//! What a program did over the days its device keeps, `wasdk:inspect`'s days summed as
//! `waos-activity` says them.

use waos_activity::{Activity, Asked, Counts, Folder};
use wasdk::inspect::audit::{Day, Uses};
use wasdk::system_clock::Instant;

/// What `days` say a program did.
pub(crate) fn activity_of(days: &[Day]) -> Activity {
    let mut activity = Activity::default();
    for day in days {
        add_day(&mut activity, day);
    }
    activity
}

/// `day` added to `activity`.
fn add_day(activity: &mut Activity, day: &Day) {
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
        Activity::add_named(
            &mut activity.local_addresses,
            &address.name,
            counts(&address.uses),
        );
    }
    for folder in &day.folders {
        activity.add_folder(Folder {
            path: folder.path.clone(),
            opened_to_read: folder.opened_to_read,
            opened_to_write: folder.opened_to_write,
            changes: folder.created + folder.removed + folder.renamed,
            changed: folder.changed.clone(),
        });
    }
    activity.folder_refusals += day.folder_refusals;
    add_counts(&mut activity.clipboard_reads, &day.clipboard_reads);
    activity.pastes += day.pastes;
    add_counts(&mut activity.clipboard_writes, &day.clipboard_writes);
    for command in &day.commands {
        activity.add_command(&command.machine, &command.command, counts(&command.uses));
    }
    for link in &day.links {
        Activity::add_named(&mut activity.links, &link.name, counts(&link.uses));
    }
    add_counts(&mut activity.notifications, &day.notifications);
    for asked in &day.questions {
        activity.questions.push(Asked {
            question: asked.question.clone(),
            at: asked.at.seconds,
            answer: asked.answer.clone(),
        });
    }
    if let Some(last) = last_use(day) {
        activity.used_at(last);
    }
}

/// When the day's last use was, of every kind it counts the time of, in seconds since the epoch.
fn last_use(day: &Day) -> Option<i64> {
    let named = (day
        .domains
        .iter()
        .chain(&day.local_addresses)
        .chain(&day.links))
    .map(|named| named.uses.last);
    let commands = day.commands.iter().map(|command| command.uses.last);
    let others = [
        day.clipboard_reads.last,
        day.clipboard_writes.last,
        day.notifications.last,
    ];
    let sessions = (day.camera.iter().chain(&day.microphone)).map(|session| Some(session.start));
    let questions = day.questions.iter().map(|asked| Some(asked.at));
    named
        .chain(commands)
        .chain(others)
        .chain(sessions)
        .chain(questions)
        .flatten()
        .map(|at: Instant| at.seconds)
        .max()
}

fn counts(uses: &Uses) -> Counts {
    Counts {
        allowed: uses.allowed,
        refused: uses.refused,
    }
}

fn add_counts(counts: &mut Counts, uses: &Uses) {
    counts.allowed += uses.allowed;
    counts.refused += uses.refused;
}

#[cfg(test)]
mod tests {
    use waos_activity::AccessKind;
    use wasdk::inspect::audit::{CommandUses, DeviceSession, Overflow};

    use super::*;

    fn at(seconds: i64) -> Instant {
        Instant {
            seconds,
            nanoseconds: 0,
        }
    }

    fn uses(allowed: u32, last: Option<i64>) -> Uses {
        Uses {
            allowed,
            refused: 0,
            first: last.map(at),
            last: last.map(at),
        }
    }

    fn day(start: i64) -> Day {
        Day {
            start: at(start),
            domains: Vec::new(),
            local_addresses: Vec::new(),
            udp_datagrams: 0,
            folders: Vec::new(),
            folder_refusals: 0,
            clipboard_reads: uses(0, None),
            pastes: 0,
            clipboard_writes: uses(0, None),
            camera: Vec::new(),
            microphone: Vec::new(),
            commands: Vec::new(),
            links: Vec::new(),
            notifications: uses(0, None),
            questions: Vec::new(),
            beyond_caps: Overflow {
                domains: 0,
                local_addresses: 0,
                folders: 0,
                changed: 0,
                commands: 0,
                links: 0,
                device_sessions: 0,
                questions: 0,
            },
        }
    }

    #[test]
    fn the_days_are_summed_and_their_last_use_kept() {
        let mut monday = day(0);
        monday.commands.push(CommandUses {
            machine: "This Mac".to_owned(),
            command: "git".to_owned(),
            uses: uses(2, Some(100)),
        });
        let mut tuesday = day(86_400);
        tuesday.commands.push(CommandUses {
            machine: "This Mac".to_owned(),
            command: "git".to_owned(),
            uses: uses(3, Some(86_500)),
        });
        tuesday.camera.push(DeviceSession {
            start: at(90_000),
            seconds: 60,
            simulated: false,
        });
        let activity = activity_of(&[tuesday, monday]);
        assert_eq!(activity.commands[0].uses.allowed, 5, "one command, summed");
        assert_eq!(activity.camera.sessions, 1);
        assert_eq!(activity.last_use, Some(90_000));
        assert_eq!(activity.marks(), [AccessKind::Camera, AccessKind::Machines]);
    }
}
