//! A program's settings as `cargo waos set` and `cargo waos settings` name them, one row each, as
//! its settings page shows them: a question's answer, a switch, the answer to opening links of a
//! scheme, and the answers set for its pickers and file dialogs, given in place of asking; and
//! what WaOS asks the program's user, which a run's terminal says while it waits.
//!
//! The invariant: a row's key is its name on the command line and in what `settings` prints, and
//! [`Row::key`] and [`Row::parse`] say the same names; a new kind of row adds its variant at the
//! end, as every message does (the crate's doc).

use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// What a program asks its user at its first use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Question {
    Notifications,
    Camera,
    Microphone,
}

/// What a program uses constantly, which its user turns on or off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Switch {
    Network,
    LocalNetwork,
    ClipboardRead,
    ClipboardWrite,
}

/// The user's answer to a question or a link scheme.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Answer {
    Allow,
    Deny,
    /// For the camera and the microphone alone: allowed, with a device of WaOS's own.
    Simulate,
}

/// What a simulated microphone records.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SimulatedSound {
    Tone,
    Silence,
}

impl SimulatedSound {
    pub fn key(self) -> &'static str {
        match self {
            SimulatedSound::Tone => "tone",
            SimulatedSound::Silence => "silence",
        }
    }
}

/// A kind of file dialog a program shows its user.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileDialog {
    OpenFile,
    OpenFiles,
    OpenFolder,
    SaveFile,
}

/// One row of a program's settings set to a value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Row {
    /// Allow or Don't allow; none to ask, which removes the program's own answer, so it follows
    /// the answer for every program, Ask unless its user set one.
    Question(Question, Option<Answer>),
    /// On or off; none to follow the setting for every program.
    Switch(Switch, Option<bool>),
    /// The answer to opening links of a scheme, any but `http`, `https` and `mailto`; none to ask
    /// again at its next link.
    LinkScheme(String, Option<Answer>),
    /// The machine, by its id, the picker of machines answers; none to ask.
    Machine(Option<String>),
    /// The sound output, by its id, the picker of outputs answers; none to ask.
    AudioOutput(Option<String>),
    /// The paths on the machine, whole, the dialog answers: one, or any number for the dialog that
    /// opens files; none to ask.
    FileDialog(FileDialog, Vec<PathBuf>),
    /// What a simulated microphone records; none to follow the setting for every program.
    SimulatedMicrophone(Option<SimulatedSound>),
}

/// One row of a program's settings as they are, as `cargo waos settings` prints it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingRow {
    /// The row's name, as `cargo waos set` takes it: `camera`, `link-scheme zoommtg`, `save-file`.
    pub key: String,
    /// What holds, as `cargo waos set` takes it: `allow`, `on`, a machine's id, paths; `ask`.
    pub values: Vec<String>,
    /// Whether it follows the setting for every program; never for a row that has none.
    pub follows_global: bool,
    /// What the row can be set to now, for the pickers' rows: the machines or the outputs offered.
    pub offered: Vec<Offered>,
}

/// A machine or an output a picker offers now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offered {
    pub id: String,
    /// As its user knows it.
    pub name: String,
}

/// What WaOS asks a program's user now, which a row on Ask lets it ask.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Asked {
    Question(Question),
    /// Whether it may open links of this scheme.
    LinkScheme(String),
    /// To turn a switch on, on the program's settings page.
    Switch(Switch),
    /// Which machine it may act on.
    Machine,
    /// Which sound output it plays on.
    AudioOutput,
    FileDialog(FileDialog),
}

impl Question {
    pub const ALL: [Question; 3] = [
        Question::Notifications,
        Question::Camera,
        Question::Microphone,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Question::Notifications => "notifications",
            Question::Camera => "camera",
            Question::Microphone => "microphone",
        }
    }
}

impl Switch {
    pub const ALL: [Switch; 4] = [
        Switch::Network,
        Switch::LocalNetwork,
        Switch::ClipboardRead,
        Switch::ClipboardWrite,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Switch::Network => "network",
            Switch::LocalNetwork => "local-network",
            Switch::ClipboardRead => "clipboard-read",
            Switch::ClipboardWrite => "clipboard-write",
        }
    }
}

impl Answer {
    pub fn key(self) -> &'static str {
        match self {
            Answer::Allow => "allow",
            Answer::Deny => "deny",
            Answer::Simulate => "simulated",
        }
    }
}

impl FileDialog {
    pub const ALL: [FileDialog; 4] = [
        FileDialog::OpenFile,
        FileDialog::OpenFiles,
        FileDialog::OpenFolder,
        FileDialog::SaveFile,
    ];

    pub fn key(self) -> &'static str {
        match self {
            FileDialog::OpenFile => "open-file",
            FileDialog::OpenFiles => "open-files",
            FileDialog::OpenFolder => "open-folder",
            FileDialog::SaveFile => "save-file",
        }
    }
}

/// The value a row on Ask holds.
pub const ASK: &str = "ask";
/// The value of a switch that follows the setting for every program, as `set` takes it.
pub const GLOBAL: &str = "global";
/// The key of a link scheme's row, before its scheme.
pub const LINK_SCHEME: &str = "link-scheme";
/// The key of the picker of machines' row.
pub const MACHINE: &str = "machine";
/// The key of the picker of outputs' row.
pub const AUDIO_OUTPUT: &str = "audio-output";
/// The key of what a simulated microphone records.
pub const SIMULATED_MICROPHONE: &str = "simulated-microphone";

/// Why the words of a row name no row, or no value of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowError(String);

impl fmt::Display for RowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RowError {}

impl Row {
    /// The row `words` name, its key first and its values after: `camera allow`,
    /// `link-scheme zoommtg deny`, `open-files /a.txt /b.txt`. Paths are taken as given; whoever
    /// reads them from a person makes them whole first.
    pub fn parse(words: &[String]) -> Result<Row, RowError> {
        let (key, values) = words
            .split_first()
            .ok_or_else(|| RowError(format!("name a row: {}", every_key())))?;
        if let Some(question) = Question::ALL.into_iter().find(|each| each.key() == key) {
            let answer = match question {
                Question::Camera | Question::Microphone => one_device_answer(key, values)?,
                Question::Notifications => one_answer(key, values)?,
            };
            return Ok(Row::Question(question, answer));
        }
        if let Some(switch) = Switch::ALL.into_iter().find(|each| each.key() == key) {
            return Ok(Row::Switch(switch, one_switch(key, values)?));
        }
        if let Some(dialog) = FileDialog::ALL.into_iter().find(|each| each.key() == key) {
            return Ok(Row::FileDialog(dialog, paths(dialog, values)?));
        }
        match key.as_str() {
            LINK_SCHEME => {
                let (scheme, answer) = values.split_first().ok_or_else(|| {
                    RowError("name the scheme: link-scheme zoommtg allow|deny|ask".to_owned())
                })?;
                Ok(Row::LinkScheme(
                    scheme.to_ascii_lowercase(),
                    one_answer(key, answer)?,
                ))
            }
            MACHINE => Ok(Row::Machine(one_id(key, values)?)),
            AUDIO_OUTPUT => Ok(Row::AudioOutput(one_id(key, values)?)),
            SIMULATED_MICROPHONE => Ok(Row::SimulatedMicrophone(one_sound(key, values)?)),
            _ => Err(RowError(format!(
                "no row is named {key:?}: {}",
                every_key()
            ))),
        }
    }
}

/// Every row's name, for a refusal to list.
fn every_key() -> String {
    let mut keys: Vec<&str> = Question::ALL.iter().map(|each| each.key()).collect();
    keys.extend(Switch::ALL.iter().map(|each| each.key()));
    keys.extend([LINK_SCHEME, MACHINE, AUDIO_OUTPUT, SIMULATED_MICROPHONE]);
    keys.extend(FileDialog::ALL.iter().map(|each| each.key()));
    keys.join(", ")
}

/// The one value of the row `key`.
fn one<'a>(key: &str, values: &'a [String], shape: &str) -> Result<&'a str, RowError> {
    match values {
        [value] => Ok(value),
        _ => Err(RowError(format!("{key} takes one value: {shape}"))),
    }
}

fn one_answer(key: &str, values: &[String]) -> Result<Option<Answer>, RowError> {
    match one(key, values, "allow, deny or ask")? {
        "allow" => Ok(Some(Answer::Allow)),
        "deny" => Ok(Some(Answer::Deny)),
        ASK => Ok(None),
        other => Err(RowError(format!(
            "{key} is allow, deny or ask, not {other:?}"
        ))),
    }
}

/// The camera's or the microphone's answer, which may be simulated too.
fn one_device_answer(key: &str, values: &[String]) -> Result<Option<Answer>, RowError> {
    match one(key, values, "allow, deny, simulated or ask")? {
        "simulated" => Ok(Some(Answer::Simulate)),
        "allow" => Ok(Some(Answer::Allow)),
        "deny" => Ok(Some(Answer::Deny)),
        ASK => Ok(None),
        other => Err(RowError(format!(
            "{key} is allow, deny, simulated or ask, not {other:?}"
        ))),
    }
}

fn one_sound(key: &str, values: &[String]) -> Result<Option<SimulatedSound>, RowError> {
    match one(key, values, "tone, silence or global")? {
        "tone" => Ok(Some(SimulatedSound::Tone)),
        "silence" => Ok(Some(SimulatedSound::Silence)),
        GLOBAL => Ok(None),
        other => Err(RowError(format!(
            "{key} is tone, silence or global, not {other:?}"
        ))),
    }
}

fn one_switch(key: &str, values: &[String]) -> Result<Option<bool>, RowError> {
    match one(key, values, "on, off or global")? {
        "on" => Ok(Some(true)),
        "off" => Ok(Some(false)),
        GLOBAL => Ok(None),
        other => Err(RowError(format!(
            "{key} is on, off or global, not {other:?}"
        ))),
    }
}

fn one_id(key: &str, values: &[String]) -> Result<Option<String>, RowError> {
    let id = one(key, values, "an id, or ask")?;
    Ok((id != ASK).then(|| id.to_owned()))
}

fn paths(dialog: FileDialog, values: &[String]) -> Result<Vec<PathBuf>, RowError> {
    if let [only] = values
        && only == ASK
    {
        return Ok(Vec::new());
    }
    let key = dialog.key();
    match (dialog, values.len()) {
        (_, 0) => Err(RowError(format!("{key} takes a path, or ask"))),
        (FileDialog::OpenFiles, _) | (_, 1) => Ok(values.iter().map(PathBuf::from).collect()),
        _ => Err(RowError(format!(
            "{key} takes one path; open-files takes several"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        text.split(' ').map(str::to_owned).collect()
    }

    #[test]
    fn each_row_is_named_by_its_key_with_its_values_after() {
        for (text, row) in [
            (
                "camera allow",
                Row::Question(Question::Camera, Some(Answer::Allow)),
            ),
            ("microphone ask", Row::Question(Question::Microphone, None)),
            (
                "camera simulated",
                Row::Question(Question::Camera, Some(Answer::Simulate)),
            ),
            (
                "simulated-microphone silence",
                Row::SimulatedMicrophone(Some(SimulatedSound::Silence)),
            ),
            (
                "simulated-microphone global",
                Row::SimulatedMicrophone(None),
            ),
            (
                "clipboard-read off",
                Row::Switch(Switch::ClipboardRead, Some(false)),
            ),
            ("network global", Row::Switch(Switch::Network, None)),
            (
                "link-scheme ZoomMtg deny",
                Row::LinkScheme("zoommtg".into(), Some(Answer::Deny)),
            ),
            (
                "machine this-computer",
                Row::Machine(Some("this-computer".into())),
            ),
            ("audio-output ask", Row::AudioOutput(None)),
            (
                "save-file /tmp/out.pdf",
                Row::FileDialog(FileDialog::SaveFile, vec!["/tmp/out.pdf".into()]),
            ),
            (
                "open-files /a /b",
                Row::FileDialog(FileDialog::OpenFiles, vec!["/a".into(), "/b".into()]),
            ),
            (
                "open-file ask",
                Row::FileDialog(FileDialog::OpenFile, Vec::new()),
            ),
        ] {
            assert_eq!(Row::parse(&words(text)).unwrap(), row, "{text}");
        }
    }

    #[test]
    fn words_naming_no_row_or_no_value_say_what_would() {
        for (text, said) in [
            ("camera yes", "camera is allow, deny, simulated or ask"),
            (
                "notifications simulated",
                "notifications is allow, deny or ask",
            ),
            (
                "simulated-microphone loud",
                "simulated-microphone is tone, silence or global",
            ),
            ("network maybe", "network is on, off or global"),
            ("save-file /a /b", "save-file takes one path"),
            ("open-file", "open-file takes a path"),
            ("webcam allow", "no row is named \"webcam\""),
            ("link-scheme", "name the scheme"),
        ] {
            let error = Row::parse(&words(text)).unwrap_err().to_string();
            assert!(error.starts_with(said), "{text}: {error}");
        }
    }
}
