//! What the user lets one program use of what is guarded, read live: [`ProgramGrants`], which
//! whoever keeps the user's settings implements (the store, for an installed program), and the
//! values it answers. A switch is a value the user sets, with a rule beneath it where it has
//! one; a [`Question`] is put to the user at a program's first use, and its [`Answer`] kept, as is
//! the user's answer to a program opening links of a scheme of its own, asked once per scheme; and
//! the machines the user picked for a program are kept by their ids, as the files picked are. A
//! picker or a file dialog can have an answer the user set instead of asking: a machine, an
//! output, or paths, given in place of showing it. The camera and the microphone can be answered
//! simulated: allowed, with devices of WaOS's own.
//!
//! Every setting follows a global default until the user sets it for one program, as Chrome's
//! site settings do: a program's own value or the global one is a [`Setting`]. The trait answers
//! what holds for the program, whichever that is.
//!
//! The invariant: a grant is read at each check, never copied into the program's state, so a
//! change the user makes holds from the next check on.

use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::str::FromStr;

/// What the user lets one program use. Each check asks again, from whichever thread the check is
/// made on.
pub trait ProgramGrants: Send + Sync {
    /// The network switch and the domains beneath it: the program's own, or the global one when
    /// it has none. The network is the internet: every address but this computer's and the local
    /// network's.
    fn network(&self) -> NetworkGrant;

    /// Whether the program may reach this computer and the local network: loopback addresses,
    /// the private ranges, link-local and multicast addresses, which a domain rule cannot guard,
    /// as a connection to one needs no name.
    fn local_network(&self) -> bool;

    /// Whether the program may read the clipboard without the user's paste.
    fn clipboard_read(&self) -> bool;

    /// Whether the program may write the clipboard.
    fn clipboard_write(&self) -> bool;

    /// The folders of the machine the program sees, and where.
    fn file_system(&self) -> FileSystemGrant;

    /// The answer to `question` that holds for the program: the user's to it, or the global
    /// one; none while neither is given, and the question is to be put to the user.
    fn answer(&self, question: Question) -> Option<Answer>;

    /// Keeps `answer`, the user's to `question` for this program.
    fn set_answer(&self, question: Question, answer: Answer);

    /// The user's answer to the program opening links of `scheme`, one other than `http`, `https`
    /// and `mailto`, which open freely, in lower case (`zoommtg`); none while the user has not
    /// answered, and the question is to be put to the user at the program's next link of it.
    fn link_scheme_answer(&self, scheme: &str) -> Option<Answer>;

    /// Keeps `answer`, the user's to the program opening links of `scheme`.
    fn set_link_scheme_answer(&self, scheme: &str, answer: Answer);

    /// The machines the user picked for the program, by their ids, which it acts on as each
    /// machine's user (the processes proposal's decisions 2 and 3); none where nothing keeps them.
    fn machines(&self) -> Vec<String> {
        Vec::new()
    }

    /// Keeps `id`, a machine the user picked for the program, among its machines until the user
    /// removes it; nothing where nothing keeps them, and the pick is the program's for its run
    /// alone.
    fn keep_machine(&self, id: &str) {
        let _ = id;
    }

    /// Keeps `mount`, a file or a folder the user picked for the program in a dialog, among its
    /// mounts until the user removes it; nothing where nothing keeps them, and the pick is the
    /// program's for its run alone.
    fn keep_picked(&self, mount: Mount) {
        let _ = mount;
    }

    /// The machine, by its id, the user set as the answer to the program's picker of machines,
    /// given in place of asking; none to ask.
    fn machine_answer(&self) -> Option<String> {
        None
    }

    /// The sound output, by its id, the user set as the answer to the program's picker of
    /// outputs, given in place of asking; none to ask.
    fn audio_output_answer(&self) -> Option<String> {
        None
    }

    /// The paths on the machine the user set as the answer to the program's `dialog`, given in
    /// place of showing it: one for every dialog but [`FileDialog::OpenFiles`]; none to ask.
    fn file_dialog_answer(&self, dialog: FileDialog) -> Option<Vec<PathBuf>> {
        let _ = dialog;
        None
    }

    /// What the program's microphone records while its answer is [`Answer::Simulated`]; the tone
    /// where nothing keeps a choice.
    fn simulated_microphone(&self) -> SimulatedSound {
        SimulatedSound::default()
    }

    /// Resolves at the next change the user makes to what holds for the program, its own
    /// settings or the global ones; a change is heard by every future asked for before it.
    fn changed(&self) -> ChangedFuture;
}

/// What [`ProgramGrants::changed`] answers.
pub type ChangedFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

/// The network switch, and the domains the program may reach while it is on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkGrant {
    pub on: bool,
    pub domains: DomainRule,
}

/// Which domains a program may reach. A domain stands for itself and every name under it:
/// `example.com` for `api.example.com` too.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DomainRule {
    Any,
    Only(Vec<Domain>),
    AnyBut(Vec<Domain>),
}

impl DomainRule {
    /// Whether the rule lets a program reach `name`, a host name as a lookup gives it: in lower
    /// case, its labels in ASCII.
    pub fn allows(&self, name: &str) -> bool {
        match self {
            DomainRule::Any => true,
            DomainRule::Only(domains) => domains.iter().any(|domain| domain.covers(name)),
            DomainRule::AnyBut(domains) => !domains.iter().any(|domain| domain.covers(name)),
        }
    }
}

/// A domain name, as a rule names it: `example.com`, its labels in lower case.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Domain(String);

impl Domain {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the domain stands for `name`: the name itself, or a name under it, with or
    /// without the root's trailing dot.
    pub fn covers(&self, name: &str) -> bool {
        let name = name.strip_suffix('.').unwrap_or(name);
        name == self.0
            || name
                .strip_suffix(self.0.as_str())
                .is_some_and(|above| above.ends_with('.'))
    }
}

impl fmt::Display for Domain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Domain {
    type Err = DomainError;

    /// A name of dot-separated labels, each of ASCII letters, digits and inner hyphens; any
    /// case, and a trailing dot, are taken; at least two labels, as a rule never names a whole
    /// top-level domain.
    fn from_str(text: &str) -> Result<Domain, DomainError> {
        let name = text.trim().trim_end_matches('.').to_ascii_lowercase();
        let labels: Vec<&str> = name.split('.').collect();
        let valid = labels.len() >= 2 && labels.iter().all(|label| is_label(label));
        if valid {
            Ok(Domain(name))
        } else {
            Err(DomainError(text.to_owned()))
        }
    }
}

/// A label of a domain name: 1 to 63 letters, digits and hyphens, no hyphen at either end.
fn is_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

/// Text that names no domain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainError(String);

impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?} is not a domain: write a name such as example.com",
            self.0
        )
    }
}

impl std::error::Error for DomainError {}

/// The folders of the machine a program sees, beyond its own `/data` and `/cache`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileSystemGrant {
    pub mounts: Vec<Mount>,
}

/// A folder of the machine at a path inside the program, as Docker's bind mounts are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mount {
    /// The folder on the machine.
    pub host: PathBuf,
    /// Where the program sees it.
    pub at: PathBuf,
    pub writable: bool,
}

/// What a program asks the user for at its first use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Question {
    Notifications,
    Camera,
    Microphone,
}

impl Question {
    pub const ALL: [Question; 3] = [
        Question::Notifications,
        Question::Camera,
        Question::Microphone,
    ];
}

/// A kind of file dialog a program shows its user, whose answer the user can set instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FileDialog {
    OpenFile,
    OpenFiles,
    OpenFolder,
    SaveFile,
}

impl FileDialog {
    pub const ALL: [FileDialog; 4] = [
        FileDialog::OpenFile,
        FileDialog::OpenFiles,
        FileDialog::OpenFolder,
        FileDialog::SaveFile,
    ];
}

/// What a program uses constantly, which the user turns on or off in its settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Switch {
    Network,
    LocalNetwork,
    ClipboardRead,
    ClipboardWrite,
}

impl Switch {
    pub const ALL: [Switch; 4] = [
        Switch::Network,
        Switch::LocalNetwork,
        Switch::ClipboardRead,
        Switch::ClipboardWrite,
    ];
}

/// The user's answer to a question.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Answer {
    Granted,
    Denied,
    /// For the camera and the microphone alone: allowed, with a device of WaOS's own in place of
    /// the machine's, a test pattern for the camera and what [`SimulatedSound`] says for the
    /// microphone, so nothing of the machine is touched and the system asks nothing.
    Simulated,
}

impl Answer {
    /// Whether the answer lets the program use what it asked for: granted, or simulated.
    pub fn allows(self) -> bool {
        matches!(self, Answer::Granted | Answer::Simulated)
    }
}

/// What a simulated microphone records. One kind of sound now; a later kind, such as a file
/// played, is another case.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SimulatedSound {
    /// A quiet beep, so a recording can be told from a broken microphone.
    #[default]
    Tone,
    Silence,
}

/// One of a program's settings: the global default, which it follows until the user sets it,
/// or its own value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Setting<T> {
    #[default]
    Global,
    Own(T),
}

impl<T> Setting<T> {
    /// The value that holds: the program's own, or `global`.
    pub fn or<'a>(&'a self, global: &'a T) -> &'a T {
        match self {
            Setting::Global => global,
            Setting::Own(own) => own,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_domain_is_its_labels_in_lower_case() {
        let domain: Domain = " Example.COM. ".parse().unwrap();
        assert_eq!(domain.as_str(), "example.com");
        assert_eq!(
            "api.example.co.uk".parse::<Domain>().unwrap().to_string(),
            "api.example.co.uk"
        );
    }

    #[test]
    fn text_that_names_no_domain_is_refused_with_why() {
        for text in [
            "",
            "com",
            "example..com",
            "-example.com",
            "exa mple.com",
            "https://example.com",
        ] {
            let error = text.parse::<Domain>().unwrap_err();
            assert!(
                error.to_string().contains("is not a domain"),
                "{text}: {error}"
            );
        }
    }

    #[test]
    fn a_domain_stands_for_itself_and_every_name_under_it() {
        let domain: Domain = "example.com".parse().unwrap();
        for name in [
            "example.com",
            "api.example.com",
            "a.b.example.com",
            "example.com.",
        ] {
            assert!(domain.covers(name), "{name}");
        }
        for name in ["badexample.com", "example.com.evil", "com", "example.org"] {
            assert!(!domain.covers(name), "{name}");
        }
    }

    #[test]
    fn a_rule_allows_any_name_only_those_named_or_all_but_those_named() {
        let named = vec!["example.com".parse().unwrap()];
        assert!(DomainRule::Any.allows("tracker.org"));
        assert!(DomainRule::Only(named.clone()).allows("api.example.com"));
        assert!(!DomainRule::Only(named.clone()).allows("tracker.org"));
        assert!(!DomainRule::AnyBut(named.clone()).allows("api.example.com"));
        assert!(DomainRule::AnyBut(named).allows("tracker.org"));
    }

    #[test]
    fn a_setting_holds_its_own_value_or_the_global_one() {
        assert!(!*Setting::Own(false).or(&true));
        assert!(*Setting::<bool>::Global.or(&true));
    }
}
