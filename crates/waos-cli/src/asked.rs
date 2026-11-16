//! What `cargo waos` says WaOS asks a program's user: the question in plain words, and the row of
//! the program's settings that answers it ahead, so the next run does not wait for a person.
//!
//! The invariant: the row named is the one `cargo waos set` takes, by `waos-dev-protocol`'s keys.

use waos_dev_protocol::{Asked, FileDialog, Question, Role};

/// What `asked` asks the user of the program `name`, and the row that answers it ahead.
pub fn said(asked: &Asked, name: &str) -> String {
    format!("{}; {}", question(asked, name), answered_ahead(asked))
}

/// What `asked` asks, as its user reads it.
fn question(asked: &Asked, name: &str) -> String {
    match asked {
        Asked::Question(Question::Camera) => format!("may {name} use the camera?"),
        Asked::Question(Question::Microphone) => format!("may {name} use the microphone?"),
        Asked::Question(Question::Notifications) => format!("may {name} show notifications?"),
        Asked::LinkScheme(scheme) => format!("may {name} open {scheme} links?"),
        Asked::Switch(switch) => format!(
            "to turn {} on for {name}, on its settings page",
            switch.key()
        ),
        Asked::Machine => format!("which computer may {name} act on?"),
        Asked::AudioOutput => format!("which sound output does {name} play on?"),
        Asked::FileDialog(FileDialog::OpenFile) => format!("which file does {name} open?"),
        Asked::FileDialog(FileDialog::OpenFiles) => format!("which files does {name} open?"),
        Asked::FileDialog(FileDialog::OpenFolder) => format!("which folder does {name} open?"),
        Asked::FileDialog(FileDialog::SaveFile) => format!("where does {name} save the file?"),
        Asked::Role(Role::Inspector) => format!("may {name} see what your other programs do?"),
    }
}

/// The command that answers `asked` ahead, from the next time it is asked.
fn answered_ahead(asked: &Asked) -> String {
    let command = match asked {
        Asked::Question(question @ (Question::Camera | Question::Microphone)) => {
            format!("cargo waos set {} allow|deny|simulated", question.key())
        }
        Asked::Question(question) => format!("cargo waos set {} allow|deny", question.key()),
        Asked::LinkScheme(scheme) => format!("cargo waos set link-scheme {scheme} allow|deny"),
        Asked::Switch(switch) => format!("cargo waos set {} on|off", switch.key()),
        Asked::Machine => {
            "cargo waos set machine <id>, the ids as `cargo waos settings` lists them".to_owned()
        }
        Asked::AudioOutput => {
            "cargo waos set audio-output <id>, the ids as `cargo waos settings` lists them"
                .to_owned()
        }
        Asked::FileDialog(FileDialog::OpenFiles) => {
            "cargo waos set open-files <path>...".to_owned()
        }
        Asked::FileDialog(dialog) => format!("cargo waos set {} <path>", dialog.key()),
        Asked::Role(role) => format!("cargo waos set {} allow|deny", role.key()),
    };
    format!("`{command}` answers it without asking from then on")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_question_is_said_as_its_user_reads_it_with_the_row_that_answers_it_ahead() {
        assert_eq!(
            said(&Asked::Question(Question::Camera), "Camera"),
            "may Camera use the camera?; `cargo waos set camera allow|deny|simulated` answers it without asking from then on"
        );
        let saving = said(&Asked::FileDialog(FileDialog::SaveFile), "Reader");
        assert!(
            saving.starts_with("where does Reader save the file?"),
            "{saving}"
        );
        assert!(
            saving.contains("cargo waos set save-file <path>"),
            "{saving}"
        );
        assert_eq!(
            said(&Asked::Role(Role::Inspector), "Inspector"),
            "may Inspector see what your other programs do?; `cargo waos set role.inspector allow|deny` answers it without asking from then on"
        );
    }
}
