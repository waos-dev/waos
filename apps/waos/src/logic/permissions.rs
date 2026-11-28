//! The kinds of permission the settings show a page of each: what each covers of an app's
//! settings, so a kind's page shows its default and the apps that set their own.
//!
//! The invariant: every setting an app follows the default for is covered by exactly one kind,
//! so an app that set its own shows on exactly that kind's page.

use waos::Switch;
use waos_services::permissions::Question;

use super::setting_changes::{ShownSettings, Which};

/// A kind of permission, as Settings lists them and shows a page of each.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Permission {
    Network,
    LocalNetwork,
    Clipboard,
    Folders,
    Notifications,
    Camera,
    Microphone,
    ActivityRecord,
}

impl Permission {
    /// Every kind, in the order Settings lists them: what an app reaches, then what it is asked
    /// for, then the record of what it did.
    pub(crate) const ALL: [Permission; 8] = [
        Permission::Network,
        Permission::LocalNetwork,
        Permission::Clipboard,
        Permission::Folders,
        Permission::Notifications,
        Permission::Camera,
        Permission::Microphone,
        Permission::ActivityRecord,
    ];

    /// The kind an app's switch is.
    pub(crate) fn of_switch(switch: Switch) -> Permission {
        match switch {
            Switch::Network => Permission::Network,
            Switch::LocalNetwork => Permission::LocalNetwork,
            Switch::ClipboardRead | Switch::ClipboardWrite => Permission::Clipboard,
        }
    }

    /// The kind a question is.
    pub(crate) fn of_question(question: Question) -> Permission {
        match question {
            Question::Notifications => Permission::Notifications,
            Question::Camera => Permission::Camera,
            Question::Microphone => Permission::Microphone,
        }
    }

    /// The settings of an app's page the kind covers.
    pub(crate) fn covers(self) -> &'static [Which] {
        match self {
            Permission::Network => &[Which::Network],
            Permission::LocalNetwork => &[Which::LocalNetwork],
            Permission::Clipboard => &[Which::ClipboardRead, Which::ClipboardWrite],
            Permission::Folders => &[Which::FileSystem],
            Permission::Notifications => &[Which::Answer(Question::Notifications)],
            Permission::Camera => &[Which::Answer(Question::Camera)],
            Permission::Microphone => &[
                Which::Answer(Question::Microphone),
                Which::SimulatedMicrophone,
            ],
            Permission::ActivityRecord => &[Which::Record],
        }
    }

    /// Whether an app whose settings are `shown` set its own for this kind, rather than
    /// following the default.
    pub(crate) fn set_its_own(self, shown: &ShownSettings) -> bool {
        self.covers()
            .iter()
            .any(|which| !follows_global(*which, shown))
    }
}

/// Whether the setting `which` of `shown` follows the default.
fn follows_global(which: Which, shown: &ShownSettings) -> bool {
    match which {
        Which::Network => shown.network.follows_global,
        Which::LocalNetwork => shown.local_network.follows_global,
        Which::ClipboardRead => shown.clipboard_read.follows_global,
        Which::ClipboardWrite => shown.clipboard_write.follows_global,
        Which::Record => shown.record.follows_global,
        Which::FileSystem => shown.file_system.follows_global,
        Which::Answer(question) => shown.answers.get(question).follows_global,
        Which::SimulatedMicrophone => shown.simulated_microphone.follows_global,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every setting a page shows with a default is covered by exactly one kind.
    #[test]
    fn every_setting_with_a_default_is_one_kinds() {
        let mut every = vec![
            Which::Network,
            Which::LocalNetwork,
            Which::ClipboardRead,
            Which::ClipboardWrite,
            Which::Record,
            Which::FileSystem,
            Which::SimulatedMicrophone,
        ];
        every.extend(Question::ALL.map(Which::Answer));
        for which in every {
            let kinds = Permission::ALL
                .iter()
                .filter(|kind| kind.covers().contains(&which))
                .count();
            assert_eq!(kinds, 1, "{which:?}");
        }
    }
}
