//! What a setting made while a prompt waits answers it with, in the prompt's terms: the question
//! the change names, as `waos-dev-protocol` says it is asked, and the reply it gives; or the file
//! dialog a change answers and the paths it gives.
//!
//! The invariant: only a change that gives an answer answers a waiting prompt; a setting back to
//! ask leaves the prompt waiting, as a person choosing ask in the page would.

use std::path::PathBuf;

use waos_dev_protocol::{
    Asked, FileDialog as ProtocolDialog, Question as ProtocolQuestion, Role as ProtocolRole,
};
use waos_store::{FileDialog, Question, Role};

use super::prompts::Reply;
use super::setting_changes::SettingChange;

/// The question `change` answers, and the reply it gives, for a prompt waiting on it; none for a
/// change that answers no prompt, or sets it back to ask.
pub(crate) fn question(change: &SettingChange) -> Option<(Asked, Reply)> {
    Some(match change {
        SettingChange::Answer(question, Some(answer)) => (
            Asked::Question(protocol_question(*question)),
            Reply::Answer(*answer),
        ),
        SettingChange::LinkScheme(scheme, Some(answer)) => {
            (Asked::LinkScheme(scheme.clone()), Reply::Answer(*answer))
        }
        SettingChange::Role(role, Some(answer)) => {
            (Asked::Role(protocol_role(*role)), Reply::Answer(*answer))
        }
        SettingChange::MachineAnswer(Some(id)) => (Asked::Machine, Reply::Machine(id.clone())),
        SettingChange::AudioOutputAnswer(Some(id)) => {
            (Asked::AudioOutput, Reply::Output(id.clone()))
        }
        _ => return None,
    })
}

/// The file dialog `change` answers, and the paths it gives, for a dialog waiting on it; none for
/// another change, or one set back to ask.
pub(crate) fn file_dialog(change: &SettingChange) -> Option<(ProtocolDialog, Vec<PathBuf>)> {
    let SettingChange::FileDialogAnswer(dialog, typed) = change else {
        return None;
    };
    if typed.is_empty() {
        return None;
    }
    Some((
        protocol_dialog(*dialog),
        typed.iter().map(PathBuf::from).collect(),
    ))
}

fn protocol_question(question: Question) -> ProtocolQuestion {
    match question {
        Question::Notifications => ProtocolQuestion::Notifications,
        Question::Camera => ProtocolQuestion::Camera,
        Question::Microphone => ProtocolQuestion::Microphone,
    }
}

fn protocol_role(role: Role) -> ProtocolRole {
    match role {
        Role::Inspector => ProtocolRole::Inspector,
    }
}

fn protocol_dialog(dialog: FileDialog) -> ProtocolDialog {
    match dialog {
        FileDialog::OpenFile => ProtocolDialog::OpenFile,
        FileDialog::OpenFiles => ProtocolDialog::OpenFiles,
        FileDialog::OpenFolder => ProtocolDialog::OpenFolder,
        FileDialog::SaveFile => ProtocolDialog::SaveFile,
    }
}

#[cfg(test)]
mod tests {
    use waos_dev_protocol::{FileDialog, Question, Role};
    use waos_store::{Answer as StoreAnswer, Question as StoreQuestion};

    use super::super::setting_changes::Which;
    use super::*;

    #[test]
    fn an_answer_answers_the_question_it_names() {
        let change = SettingChange::Answer(StoreQuestion::Camera, Some(StoreAnswer::Granted));
        assert_eq!(
            question(&change),
            Some((
                Asked::Question(Question::Camera),
                Reply::Answer(waos::Answer::Granted)
            ))
        );
        let scheme = SettingChange::LinkScheme("zoommtg".into(), Some(StoreAnswer::Denied));
        assert_eq!(
            question(&scheme),
            Some((
                Asked::LinkScheme("zoommtg".into()),
                Reply::Answer(waos::Answer::Denied)
            ))
        );
        let role = SettingChange::Role(waos_store::Role::Inspector, Some(StoreAnswer::Granted));
        assert_eq!(
            question(&role),
            Some((
                Asked::Role(Role::Inspector),
                Reply::Answer(waos::Answer::Granted)
            ))
        );
        let machine = SettingChange::MachineAnswer(Some("this-computer".into()));
        assert_eq!(
            question(&machine),
            Some((Asked::Machine, Reply::Machine("this-computer".into())))
        );
    }

    #[test]
    fn a_setting_back_to_ask_or_a_change_that_asks_nothing_answers_no_prompt() {
        let ask = SettingChange::Answer(StoreQuestion::Camera, None);
        assert_eq!(question(&ask), None);
        let follow = SettingChange::FollowGlobal(Which::Answer(StoreQuestion::Camera));
        assert_eq!(question(&follow), None);
        let network = SettingChange::NetworkOn(true);
        assert_eq!(question(&network), None);
    }

    #[test]
    fn a_file_dialog_answer_names_its_dialog_and_paths_and_ask_answers_none() {
        let answered = SettingChange::FileDialogAnswer(
            waos_store::FileDialog::SaveFile,
            vec!["/tmp/out.pdf".into()],
        );
        assert_eq!(
            file_dialog(&answered),
            Some((FileDialog::SaveFile, vec![PathBuf::from("/tmp/out.pdf")]))
        );
        let ask = SettingChange::FileDialogAnswer(waos_store::FileDialog::SaveFile, Vec::new());
        assert_eq!(file_dialog(&ask), None);
        assert_eq!(file_dialog(&SettingChange::NetworkOn(true)), None);
    }
}
