//! What the library says, in every layout.

use waft::ui::TargetPlatform;
use waos::{RecordingDevice, Switch};
use waos_package::Category;

use crate::logic::setting_changes::SettingsOf;
use crate::state::library::Question;

/// The home's title.
pub(crate) const HOME_TITLE: &str = "WaOS";
/// The page of sources and what they list, and the buttons that open it.
pub(crate) const GET_PROGRAMS: &str = "Get apps";
pub(crate) const ADD_SOURCE: &str = "Add a source";
/// Beside adding a source in Fluent's layout, which opens the system's file picker for a package:
/// the ellipsis Windows ends a command with that asks for more before it acts.
pub(crate) const INSTALL_FROM_FILE: &str = "Install from file…";
/// The same in Cupertino's, without the ellipsis, as iOS's list rows have none.
pub(crate) const INSTALL_FROM_FILE_ROW: &str = "Install from file";
pub(crate) const REMOVE: &str = "Remove";
/// On a program's icon, which asks the launcher to put the program on the home screen.
pub(crate) const ADD_TO_HOME_SCREEN: &str = "Add to Home Screen";
/// On a running program's icon, which stops it.
pub(crate) const CLOSE: &str = "Close";
pub(crate) const CANCEL: &str = "Cancel";
/// Remove, on a program, which asks first.
pub(crate) const ASK_TO_REMOVE: &str = "Remove…";
pub(crate) const ADD: &str = "Add";
pub(crate) const REMOVE_SOURCE: &str = "Remove source";
pub(crate) const OK: &str = "OK";
pub(crate) const PROBLEM: &str = "Something went wrong";
pub(crate) const INSTALL: &str = "Install";
/// On a program a source lists, which installs it.
pub(crate) const GET: &str = "Get";
/// On a program a source lists newer than installed, which updates it.
pub(crate) const UPDATE: &str = "Update";
pub(crate) const INSTALLED: &str = "Installed";
pub(crate) const INSTALLING: &str = "Installing…";
pub(crate) const READING: &str = "Reading…";
pub(crate) const LISTS_NOTHING: &str = "Lists no apps.";
pub(crate) const NO_PROGRAMS: &str = "No Apps";
/// Under the title of a home with nothing installed: what will appear there.
pub(crate) const NO_PROGRAMS_SAID: &str = "Apps you get appear here.";
/// In the field the home is searched from, the deck's pill or the launcher's row, while nothing is
/// typed in it.
pub(crate) const SEARCH_PROGRAMS: &str = "Search apps";
/// On the home, while the user searches for a program and none installed is it.
pub(crate) const NO_PROGRAMS_FOUND: &str = "No apps found.";
pub(crate) const NO_SOURCES: &str = "No sources added.";
pub(crate) const SOURCE_PLACEHOLDER: &str = "github.com/owner/repo, or an index.json's URL";

/// A question as the user is asked it: its title, what it says, and its two answers.
pub(crate) struct Asked {
    pub(crate) title: String,
    pub(crate) message: String,
    pub(crate) yes: &'static str,
    pub(crate) no: &'static str,
}

/// `question` in words.
pub(crate) fn asked(question: &Question) -> Asked {
    match question {
        Question::Install(package) => {
            let manifest = package.manifest();
            let file = package
                .file()
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            Asked {
                title: format!("Install {}?", manifest.name()),
                message: format!(
                    "{} {} from {file}, signed by publisher {}.",
                    manifest.id(),
                    manifest.version(),
                    package.publisher().fingerprint()
                ),
                yes: INSTALL,
                no: "Don't install",
            }
        }
        Question::Remove { name, .. } => Asked {
            title: format!("Remove {name}?"),
            message: "Its files are deleted.".to_owned(),
            yes: REMOVE,
            no: "Keep",
        },
    }
}

/// What the library says of the programs recording, `using` naming those using each device, in
/// the order they began; none while none does.
pub(crate) fn recording(using: &[(RecordingDevice, Vec<String>)]) -> Option<String> {
    let said: Vec<String> = using
        .iter()
        .filter_map(|(device, names)| self::using(names, device_said(*device)))
        .collect();
    (!said.is_empty()).then(|| said.join(" "))
}

/// A device as the library's sentence of who records names it: one of WaOS's own said simulated,
/// so nobody takes it for the machine's.
fn device_said(device: RecordingDevice) -> &'static str {
    match device {
        RecordingDevice::Microphone => "the microphone",
        RecordingDevice::Camera => "the camera",
        RecordingDevice::SimulatedMicrophone => "the microphone (simulated)",
        RecordingDevice::SimulatedCamera => "the camera (simulated)",
    }
}

/// What the library says of the programs `names` using `device`; none for none.
fn using(names: &[String], device: &str) -> Option<String> {
    match names {
        [] => None,
        [one] => Some(format!("{one} is using {device}.")),
        [first @ .., last] => Some(format!(
            "{} and {last} are using {device}.",
            first.join(", ")
        )),
    }
}

/// A program's version, under its name where it is offered.
pub(crate) fn version(version: &str) -> String {
    format!("Version {version}")
}

/// A program a source lists at `version`, installed at `installed`.
pub(crate) fn update_detail(version: &str, installed: &str) -> String {
    format!("{version}, installed {installed}")
}

/// What a source that could not be read says.
pub(crate) fn unreadable(why: &str) -> String {
    format!("Could not be read: {why}")
}

pub(crate) const SETTINGS: &str = "Settings";
pub(crate) const GLOBAL_SETTINGS: &str = "Every app's settings";
pub(crate) const GLOBAL_DETAIL: &str = "What every app may use, until you set it for one app.";
pub(crate) const FOLLOWS_GLOBAL: &str = "Follows the setting for every app.";
pub(crate) const WHERE_PROGRAMS_OPEN: &str = "Where apps open";
pub(crate) const PROGRAMS_INSIDE_WAOS: &str = "Show apps inside WaOS";
pub(crate) const PROGRAMS_INSIDE_WAOS_DETAIL: &str =
    "Apps you open from now on show in WaOS's tabs, rather than each on its own.";
/// On the global page of a desktop's launcher, the choice of how WaOS stays on the desktop.
pub(crate) const WHERE_WAOS_STAYS: &str = "Where WaOS stays";
/// The global settings' line on the phone's files, where WaOS reaches them only as the user
/// allows, and what it says while WaOS may reach them or not.
pub(crate) const PHONE_FILES: &str = "This phone's files";
pub(crate) const PHONE_FILES_ON: &str =
    "All files access is on: an app working on This computer sees the files on this phone.";
pub(crate) const PHONE_FILES_OFF: &str = "All files access is off: an app working on This computer sees only WaOS's own folder. Turn it on for WaOS in Android's settings.";
/// The button that opens Android's page for WaOS's All files access.
pub(crate) const OPEN_ANDROID_SETTINGS: &str = "Open Android's settings";

/// The choice whether WaOS is kept in the Dock or the taskbar, named as the desktop of `platform`
/// names the place: macOS's Dock, Windows' and Linux's taskbar.
pub(crate) fn keep_in_dock(platform: TargetPlatform) -> &'static str {
    match platform {
        TargetPlatform::MacOS => "Keep WaOS in the Dock",
        _ => "Keep WaOS in the taskbar",
    }
}

/// What turning [`keep_in_dock`] off does on the desktop of `platform`.
pub(crate) fn keep_in_dock_detail(platform: TargetPlatform) -> &'static str {
    match platform {
        TargetPlatform::MacOS => "Off, WaOS is only an icon in the menu bar, which opens it.",
        _ => "Off, WaOS is only an icon in the tray, which opens it.",
    }
}

pub(crate) const USE_GLOBAL: &str = "Use the setting for every app";
pub(crate) const ON: &str = "On";
pub(crate) const OFF: &str = "Off";
pub(crate) const NETWORK: &str = "Network";
pub(crate) const LOCAL_NETWORK: &str = "This computer and the local network";
pub(crate) const ANY_DOMAIN: &str = "Any domain";
pub(crate) const ONLY_DOMAINS: &str = "Only these domains";
pub(crate) const ANY_DOMAIN_BUT: &str = "All domains but these";
pub(crate) const DOMAIN_PLACEHOLDER: &str = "example.com";
pub(crate) const CLIPBOARD_READ: &str = "Reading the clipboard";
pub(crate) const CLIPBOARD_WRITE: &str = "Writing the clipboard";
pub(crate) const RECORD: &str = "Keep a record of what it does";
pub(crate) const RECORD_DETAIL: &str = "The sites it reaches, the files it opens and changes, when it uses the camera and the microphone, and what it runs, over the last seven days, on this device alone. Turned off, what was kept is removed.";
pub(crate) const RECORD_EVERY: &str = "Keep a record of what apps do";
pub(crate) const LAST_DAYS: &str = "Last seven days";
pub(crate) const NOTHING_USED: &str = "It has used nothing that needs your permission.";
pub(crate) const NOTHING_RECORDED: &str = "No record is kept of what it does.";
pub(crate) const RECORD_EVERY_DETAIL: &str = "The sites each reaches, the files it opens and changes, when it uses the camera and the microphone, and what it runs, over the last seven days, on this device alone. Turned off, what was kept of every app that follows this is removed.";

/// The record's name and what it is, on the page of `of`.
pub(crate) fn record(of: &SettingsOf) -> (&'static str, &'static str) {
    match of {
        SettingsOf::Global => (RECORD_EVERY, RECORD_EVERY_DETAIL),
        SettingsOf::Program(_) => (RECORD, RECORD_DETAIL),
    }
}
pub(crate) const FOLDERS: &str = "Folders";
pub(crate) const FOLDER_PLACEHOLDER: &str = "/Users/you/Documents";
pub(crate) const FOLDER_AT_PLACEHOLDER: &str = "Where the app sees it: the same path when empty";
pub(crate) const WRITABLE: &str = "Writable";
pub(crate) const READ_ONLY: &str = "Read-only";
pub(crate) const FOLDERS_AT_ONCE: &str =
    "A folder added or removed applies at once, to an app running too.";
pub(crate) const NO_FOLDERS: &str = "No folders.";
pub(crate) const MACHINES: &str = "Machines";
/// The warning a program's machines are shown under.
pub(crate) const MACHINES_AS_YOU: &str = "This app acts on these machines as you: it can run any command there and read and change any file you can. Remove a machine to take it back.";
pub(crate) const NO_MACHINES: &str = "No machines.";
pub(crate) const ASK: &str = "Ask";
pub(crate) const ALLOW: &str = "Allow";
pub(crate) const DONT_ALLOW: &str = "Don't allow";
pub(crate) const SIMULATED: &str = "Simulated";
pub(crate) const SIMULATED_CAMERA_DETAIL: &str = "Simulated: a test pattern from WaOS, not your camera. Nothing asks the system, and the app is shown using a simulated camera.";
pub(crate) const NO_SIMULATED_CAMERA: &str = "WaOS has no simulated camera on this system yet, so an app answered Simulated sees no camera at all.";
pub(crate) const SIMULATED_MICROPHONE_DETAIL: &str = "Simulated: a microphone of WaOS's own, not yours. Nothing asks the system, and the app is shown using a simulated microphone.";
pub(crate) const SIMULATED_RECORDS: &str = "What it records";
pub(crate) const QUIET_TONE: &str = "A quiet beep";
pub(crate) const SILENCE: &str = "Silence";
/// What a program's page says while the system refuses the program its notifications.
pub(crate) const NOTIFICATIONS_REFUSED: &str = "macOS does not let this app show notifications, whatever you allow here. macOS asks only once: turn them on for it in System Settings.";
/// The button that opens the system's settings at a program's notifications.
pub(crate) const OPEN_SYSTEM_SETTINGS: &str = "Open System Settings";

pub(crate) const LINKS: &str = "Links";
pub(crate) const LINKS_DETAIL: &str = "What this app was answered when it asked to open links of these kinds. Remove one to be asked again.";
pub(crate) const ROLES: &str = "Roles";
pub(crate) const ROLES_DETAIL: &str = "What this app was answered when it asked to be more than an app. Remove one to be asked again at its next start.";
pub(crate) const ANSWERED_WITHOUT_ASKING: &str = "Answered without asking";
pub(crate) const ANSWERED_WITHOUT_ASKING_DETAIL: &str =
    "When this app asks for one of these, WaOS gives it what is set here instead of asking you.";
pub(crate) const WHICH_MACHINE: &str = "Which computer";
pub(crate) const WHICH_OUTPUT: &str = "Which sound output";
pub(crate) const ANSWER_WITH: &str = "Answer with";
pub(crate) const ASK_INSTEAD: &str = "Ask instead";

/// A program's settings page's title, `name` being the program's.
pub(crate) fn settings_of(name: &str) -> String {
    format!("{name}'s settings")
}

/// What a program's question to its user is about, as its setting names it.
pub(crate) fn question_name(question: waos_store::Question) -> &'static str {
    match question {
        waos_store::Question::Notifications => "Notifications",
        waos_store::Question::Camera => "Camera",
        waos_store::Question::Microphone => "Microphone",
    }
}

/// What a switch is, as its setting names it.
pub(crate) fn switch_name(switch: Switch) -> &'static str {
    match switch {
        Switch::Network => NETWORK,
        Switch::LocalNetwork => LOCAL_NETWORK,
        Switch::ClipboardRead => CLIPBOARD_READ,
        Switch::ClipboardWrite => CLIPBOARD_WRITE,
    }
}

/// What a page says of the switch `name`, a program, asked its user to change.
pub(crate) fn switch_asked(name: &str, switch: Switch) -> String {
    format!(
        "{name} asks for this: {}. Turn it on below if you allow it.",
        switch_name(switch)
    )
}

/// A file dialog's answer row, as its setting names it.
pub(crate) fn file_dialog_name(dialog: waos::FileDialog) -> &'static str {
    match dialog {
        waos::FileDialog::OpenFile => "Opening a file",
        waos::FileDialog::OpenFiles => "Opening files",
        waos::FileDialog::OpenFolder => "Opening a folder",
        waos::FileDialog::SaveFile => "Saving a file",
    }
}

/// What a file dialog's answer row's field shows while empty.
pub(crate) fn file_dialog_placeholder(dialog: waos::FileDialog) -> &'static str {
    match dialog {
        waos::FileDialog::OpenFile => "/Users/you/Documents/file.txt",
        waos::FileDialog::OpenFiles => "/Users/you/Documents/one-of-them.txt",
        waos::FileDialog::OpenFolder => "/Users/you/Documents",
        waos::FileDialog::SaveFile => "/Users/you/Documents/saved.txt",
    }
}

/// A machine or an output set as a picker's answer that is not offered now, by `name`.
pub(crate) fn not_offered(name: &str) -> String {
    format!("{name} (not offered now)")
}

/// The answer a program was given to opening links of `scheme`.
pub(crate) fn link_scheme(scheme: &str, answer: waos_store::Answer) -> String {
    let given = match answer {
        waos_store::Answer::Granted => ALLOW,
        waos_store::Answer::Denied => DONT_ALLOW,
        waos_store::Answer::Simulated => SIMULATED,
    };
    format!("{scheme} links: {given}")
}

/// The answer a program was given to holding `role`.
pub(crate) fn role(role: waos_store::Role, answer: waos_store::Answer) -> String {
    let what = match role {
        waos_store::Role::Inspector => "See what other apps do",
    };
    let given = match answer {
        waos_store::Answer::Granted => ALLOW,
        waos_store::Answer::Denied | waos_store::Answer::Simulated => DONT_ALLOW,
    };
    format!("{what}: {given}")
}

/// A folder mounted: where it is, where the program sees it, and whether it may write there.
pub(crate) fn mount(host: &str, at: &str, writable: bool) -> String {
    let may = if writable { "writable" } else { "read-only" };
    if host == at {
        format!("{host}, {may}")
    } else {
        format!("{host} at {at}, {may}")
    }
}

/// What a category is called on the chip that shows its apps alone, as the App Store names it.
pub(crate) fn category(category: Category) -> &'static str {
    match category {
        Category::Business => "Business",
        Category::DeveloperTools => "Developer Tools",
        Category::Education => "Education",
        Category::Entertainment => "Entertainment",
        Category::Finance => "Finance",
        Category::Games => "Games",
        Category::GraphicsDesign => "Graphics & Design",
        Category::HealthcareFitness => "Health & Fitness",
        Category::Lifestyle => "Lifestyle",
        Category::Medical => "Medical",
        Category::Music => "Music",
        Category::News => "News",
        Category::Photography => "Photography",
        Category::Productivity => "Productivity",
        Category::Reference => "Reference",
        Category::SocialNetworking => "Social Networking",
        Category::Sports => "Sports",
        Category::Travel => "Travel",
        Category::Utilities => "Utilities",
        Category::Video => "Video",
        Category::Weather => "Weather",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_programs_recording_are_named_in_a_sentence_for_each_device() {
        let names = |names: &[&str]| {
            names
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>()
        };
        let microphone = |list: &[&str]| vec![(RecordingDevice::Microphone, names(list))];
        assert_eq!(recording(&[]), None);
        assert_eq!(recording(&microphone(&[])), None);
        assert_eq!(
            recording(&microphone(&["Memo"])).as_deref(),
            Some("Memo is using the microphone.")
        );
        assert_eq!(
            recording(&microphone(&["Memo", "Call"])).as_deref(),
            Some("Memo and Call are using the microphone.")
        );
        assert_eq!(
            recording(&microphone(&["Memo", "Call", "Tuner"])).as_deref(),
            Some("Memo, Call and Tuner are using the microphone.")
        );
        let both = [
            (RecordingDevice::Microphone, names(&["Call"])),
            (RecordingDevice::Camera, names(&["Call", "Scanner"])),
        ];
        assert_eq!(
            recording(&both).as_deref(),
            Some("Call is using the microphone. Call and Scanner are using the camera.")
        );
    }

    #[test]
    fn a_simulated_device_is_said_simulated_beside_the_machines() {
        let names = |name: &str| vec![name.to_owned()];
        let using = [
            (RecordingDevice::Camera, names("Scanner")),
            (RecordingDevice::SimulatedCamera, names("Camera")),
            (RecordingDevice::SimulatedMicrophone, names("Memo")),
        ];
        assert_eq!(
            recording(&using).as_deref(),
            Some(
                "Scanner is using the camera. Camera is using the camera (simulated). Memo is using the microphone (simulated)."
            )
        );
    }
}
