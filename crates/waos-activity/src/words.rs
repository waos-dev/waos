//! The sections of what a program did, each in plain words.

use super::{AccessKind, Activity, Captures, Counts, Named, Section};
use crate::dates::date;

/// How many of a list's names a section says before it says how many more.
const NAMES_SAID: usize = 5;

pub(super) fn captures(kind: AccessKind, captures: &Captures) -> Option<Section> {
    if captures.sessions == 0 {
        return None;
    }
    let mut line = format!("{}, {}", times(captures.sessions), length(captures.seconds));
    if captures.all_simulated {
        line.push_str(", WaOS's own, simulated");
    }
    Some(Section {
        kind,
        lines: vec![line],
    })
}

pub(super) fn network(activity: &Activity) -> Option<Section> {
    if activity.sites.is_empty() && activity.local_addresses.is_empty() {
        return None;
    }
    let refused: u32 = (activity.sites.iter().chain(&activity.local_addresses))
        .map(|named| named.uses.refused)
        .sum();
    let mut first = count(activity.sites.len(), "site", "sites");
    if refused > 0 {
        first.push_str(&format!(", refused {refused}"));
    }
    let mut lines = vec![first];
    lines.extend(most_used(&activity.sites));
    if !activity.local_addresses.is_empty() {
        lines.push(format!(
            "This computer and the local network: {}",
            count(activity.local_addresses.len(), "address", "addresses")
        ));
    }
    Some(Section {
        kind: AccessKind::Network,
        lines,
    })
}

pub(super) fn files(activity: &Activity) -> Option<Section> {
    if activity.folders.is_empty() && activity.folder_refusals == 0 {
        return None;
    }
    let mut lines: Vec<String> = activity
        .folders
        .iter()
        .map(|folder| {
            let mut line = format!(
                "{}: read {}, opened to write {}",
                folder.path,
                count_of(folder.opened_to_read),
                count_of(folder.opened_to_write)
            );
            if folder.changes > 0 {
                line.push_str(&format!(", {} changed", count_of(folder.changes)));
            }
            if !folder.changed.is_empty() {
                line.push_str(&format!(": {}", listed(&folder.changed)));
            }
            line
        })
        .collect();
    if activity.folder_refusals > 0 {
        lines.push(format!(
            "Refused {} outside its folders",
            times(activity.folder_refusals)
        ));
    }
    Some(Section {
        kind: AccessKind::Files,
        lines,
    })
}

pub(super) fn clipboard(activity: &Activity) -> Option<Section> {
    let mut lines = Vec::new();
    let reads = activity.clipboard_reads;
    if reads != Counts::default() {
        lines.push(with_refused(
            format!("Read {} without your pasting", times(reads.allowed)),
            reads.refused,
        ));
    }
    if activity.pastes > 0 {
        lines.push(format!("Read {} as you pasted", times(activity.pastes)));
    }
    let writes = activity.clipboard_writes;
    if writes != Counts::default() {
        lines.push(with_refused(
            format!("Written {}", times(writes.allowed)),
            writes.refused,
        ));
    }
    (!lines.is_empty()).then_some(Section {
        kind: AccessKind::Clipboard,
        lines,
    })
}

pub(super) fn commands(activity: &Activity) -> Option<Section> {
    if activity.commands.is_empty() {
        return None;
    }
    let lines = activity
        .commands
        .iter()
        .map(|runs| {
            with_refused(
                format!(
                    "{} on {}, {}",
                    runs.command,
                    runs.machine,
                    times(runs.uses.allowed)
                ),
                runs.uses.refused,
            )
        })
        .collect();
    Some(Section {
        kind: AccessKind::Machines,
        lines,
    })
}

pub(super) fn links(activity: &Activity) -> Option<Section> {
    if activity.links.is_empty() {
        return None;
    }
    let mut lines = vec![count(
        activity.links.len(),
        "kind of link",
        "kinds of links",
    )];
    lines.extend(most_used(&activity.links));
    Some(Section {
        kind: AccessKind::Links,
        lines,
    })
}

pub(super) fn notifications(activity: &Activity) -> Option<Section> {
    let posted = activity.notifications;
    (posted != Counts::default()).then(|| Section {
        kind: AccessKind::Notifications,
        lines: vec![with_refused(
            format!("Posted {}", times(posted.allowed)),
            posted.refused,
        )],
    })
}

pub(super) fn questions(activity: &Activity) -> Option<Section> {
    if activity.questions.is_empty() {
        return None;
    }
    let lines = activity
        .questions
        .iter()
        .map(|asked| {
            let answer = match asked.answer.as_deref() {
                Some("allow") => "allowed",
                Some("deny") => "not allowed",
                Some("simulated") => "simulated",
                Some(_) => "answered",
                None => "not answered",
            };
            format!("{}, {answer} on {}", asked.question, date(asked.at))
        })
        .collect();
    Some(Section {
        kind: AccessKind::Questions,
        lines,
    })
}

/// The most used of `list`, each with how often, and how many more there are.
fn most_used(list: &[Named]) -> Vec<String> {
    let mut sorted: Vec<&Named> = list.iter().collect();
    sorted.sort_by_key(|named| std::cmp::Reverse(named.uses.allowed + named.uses.refused));
    let mut lines: Vec<String> = sorted
        .iter()
        .take(NAMES_SAID)
        .map(|named| {
            with_refused(
                format!("{}: {}", named.name, times(named.uses.allowed)),
                named.uses.refused,
            )
        })
        .collect();
    if sorted.len() > NAMES_SAID {
        lines.push(format!("and {} more", sorted.len() - NAMES_SAID));
    }
    lines
}

/// The first of `paths` as far as a line holds them, and how many more there are.
fn listed(paths: &[String]) -> String {
    let said: Vec<&str> = paths.iter().take(3).map(String::as_str).collect();
    let mut line = said.join(", ");
    if paths.len() > said.len() {
        line.push_str(&format!(" and {} more", paths.len() - said.len()));
    }
    line
}

fn with_refused(mut line: String, refused: u32) -> String {
    if refused > 0 {
        line.push_str(&format!(", refused {}", times(refused)));
    }
    line
}

/// `n` times, as people say it.
fn times(n: u32) -> String {
    match n {
        1 => "once".to_owned(),
        2 => "twice".to_owned(),
        n => format!("{n} times"),
    }
}

fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// How many files, `n`.
fn count_of(n: u32) -> String {
    count(n as usize, "file", "files")
}

/// `seconds` long, in the largest unit that says it.
fn length(seconds: u64) -> String {
    match seconds {
        0..60 => count(seconds as usize, "second", "seconds"),
        60..3600 => count((seconds / 60) as usize, "minute", "minutes"),
        _ => count((seconds / 3600) as usize, "hour", "hours"),
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;

    #[test]
    fn what_a_program_did_is_said_a_section_per_capability() {
        let mut activity = Activity::default();
        Activity::add_session(&mut activity.camera, 300, false);
        Activity::add_session(&mut activity.camera, 420, false);
        Activity::add_named(
            &mut activity.sites,
            "example.com",
            Counts {
                allowed: 12,
                refused: 0,
            },
        );
        Activity::add_named(
            &mut activity.sites,
            "tracker.example",
            Counts {
                allowed: 0,
                refused: 2,
            },
        );
        activity.add_folder(Folder {
            path: "/Users/you/Documents".to_owned(),
            opened_to_read: 120,
            opened_to_write: 4,
            changes: 4,
            changed: vec!["notes.txt".to_owned()],
        });
        activity.clipboard_reads.allowed = 2;
        activity.add_command(
            "This Mac",
            "git",
            Counts {
                allowed: 14,
                refused: 0,
            },
        );
        activity.questions.push(Asked {
            question: "camera".to_owned(),
            at: 1_763_164_800,
            answer: Some("allow".to_owned()),
        });
        let said: Vec<(&str, Vec<String>)> = activity
            .sections()
            .into_iter()
            .map(|section| (section.kind.heading(), section.lines))
            .collect();
        assert_eq!(
            said,
            [
                ("Camera", vec!["twice, 12 minutes".to_owned()]),
                (
                    "Network",
                    vec![
                        "2 sites, refused 2".to_owned(),
                        "example.com: 12 times".to_owned(),
                        "tracker.example: 0 times, refused twice".to_owned(),
                    ]
                ),
                (
                    "Files",
                    vec![
                        "/Users/you/Documents: read 120 files, opened to write 4 files, 4 files changed: notes.txt"
                            .to_owned()
                    ]
                ),
                ("Clipboard", vec!["Read twice without your pasting".to_owned()]),
                ("Ran", vec!["git on This Mac, 14 times".to_owned()]),
                ("Asked", vec!["camera, allowed on 15 November 2025".to_owned()]),
            ]
        );
        assert_eq!(
            activity.marks(),
            [
                AccessKind::Camera,
                AccessKind::Network,
                AccessKind::Files,
                AccessKind::Clipboard,
                AccessKind::Machines
            ]
        );
        assert_eq!(activity.short()[0], "Camera: twice, 12 minutes");
    }

    #[test]
    fn a_program_that_used_nothing_says_nothing() {
        let activity = Activity::default();
        assert!(activity.is_empty() && activity.marks().is_empty() && activity.short().is_empty());
    }
}
