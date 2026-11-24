//! The library's settings pages on a desktop of the tests' own
//! (`state::library::test_desktop`), at a desktop's size in the launcher and at a phone's in its
//! window: the settings for every program, changed and kept in the store; a program's page, whose
//! settings follow those for every program until changed there and can be set back to follow
//! them, and which shows the machines its user picked for it under the warning that it acts on
//! them as the user, each removed there, no page offering a switch of commands, since machines
//! are picked; a program's answers set for its pickers and file dialogs, shown as they are,
//! asked again and set there, and its answers to its link schemes, forgotten there, in both
//! layouts; a program asking for its network switch, which opens its page with what it asks, and
//! hears the user is done once the user leaves the page; a phone's page, where a setting that is
//! on or off is a switch on a row naming it, a tap on the switch or the row changing it, and a
//! program's switch that follows the setting for every program says so; and a program whose
//! notifications the system refuses, whose page says so, its answer kept, and opens System
//! Settings at them, at a desktop's width in a window tall enough to show it and at a phone's; and,
//! where WaOS reaches the phone's files only as the user allows, the global page saying whether it
//! may, as the system says once WaOS is back in front, and opening Android's page for it, in both
//! layouts.

use std::sync::Arc;

use waft::ui::{Rect, View as _, ViewId};
use waft::{Icon, IntoWidget};
use waft_cupertino::{CupertinoIcons, CupertinoNavigationBar, CupertinoSwitch};
use waft_winui::{FluentSymbol, FontIcon};
use waos::FileDialog;
use waos_package::ProgramId;
use waos_store::{Answer, DomainRule, GlobalSettings, NetworkGrant, Setting};

use super::home::fluent::FluentHome;
use super::test_steps::{in_launcher, in_launcher_of, installs, on_phone, on_phone_of};
use super::{LaidOutLibrary, words};
use crate::logic::permissions::Permission;
use crate::logic::phone_files::TestPhone;
use crate::logic::setting_changes::{DomainsKind, SettingChange, SettingsOf};
use crate::state::library::test_desktop::{DesktopKind, LibraryDesktop};
use crate::state::library::{LibraryPage, SettingsPage};
use crate::ui::launcher::home::LauncherHome;

const TRIANGLE: &str = "dev.example.triangle";
const PERMISSIONS: &str = "dev.example.permissions";

fn triangle() -> ProgramId {
    TRIANGLE.parse().unwrap()
}

fn global_network(desktop: &LibraryDesktop) -> NetworkGrant {
    desktop.store.global_settings().unwrap().network
}

/// Opens the page of `permission`, which shows its default: the settings, Permissions, then the
/// kind.
fn open_permission(desktop: &mut LibraryDesktop, view: ViewId, permission: Permission) {
    open_settings(desktop, view);
    desktop.tap(view, words::PERMISSIONS);
    desktop.tap(view, words::permission_name(permission));
}

/// From a kind's page, opens the page of `permission` instead, as the user goes back to
/// Permissions and on.
fn go_to_permission(desktop: &mut LibraryDesktop, view: ViewId, permission: Permission) {
    back_once(desktop, view);
    desktop.tap(view, words::permission_name(permission));
}

/// Opens WaOS's own settings: the settings, then General.
fn open_waos_settings(desktop: &mut LibraryDesktop, view: ViewId) {
    open_settings(desktop, view);
    desktop.tap(view, words::WAOS_SETTINGS);
}

/// Opens the settings' first page from the launcher's "…", or the home's gear, in either layout.
fn open_settings(desktop: &mut LibraryDesktop, view: ViewId) {
    if desktop.has::<LauncherHome>() {
        let more = FluentSymbol::More.glyph().to_string();
        desktop.tap_widget::<FontIcon>(view, |icon| icon.glyph == more);
        desktop.tap(view, words::SETTINGS);
    } else if desktop.has::<FluentHome>() {
        let gear = FluentSymbol::Settings.glyph().to_string();
        desktop.tap_widget::<FontIcon>(view, |icon| icon.glyph == gear);
    } else {
        let gear = CupertinoIcons::gear_alt().code_point;
        desktop.tap_widget::<Icon>(view, |icon| {
            icon.icon
                .as_ref()
                .is_some_and(|icon| icon.code_point == gear)
        });
    }
}

/// Opens the settings of the program `name` from its icon's actions: a right click in the
/// launcher, a long press on the phone.
fn open_settings_from_icon(desktop: &mut LibraryDesktop, view: ViewId, name: &str) {
    if desktop.has::<LauncherHome>() || desktop.has::<FluentHome>() {
        desktop.right_click(view, name);
    } else {
        desktop.long_press(view, name);
    }
    desktop.tap(view, words::SETTINGS);
}

/// Goes back to the home from a settings page, in either layout: to the settings' first page,
/// and from there to the home.
fn back(desktop: &mut LibraryDesktop, view: ViewId) {
    while desktop.read(|library| *library.page() != LibraryPage::Home) {
        back_once(desktop, view);
    }
}

/// Goes back from a settings page to the page it was opened over, in either layout.
fn back_once(desktop: &mut LibraryDesktop, view: ViewId) {
    if desktop.has::<CupertinoNavigationBar>() {
        let page = desktop.read(|library| library.page().clone());
        let beneath = match page {
            LibraryPage::Settings(SettingsPage::Root) => words::HOME_TITLE,
            LibraryPage::Settings(SettingsPage::Permission(_)) => words::PERMISSIONS,
            _ => words::SETTINGS,
        };
        desktop.tap(view, beneath);
    } else {
        let back = FluentSymbol::Back.glyph().to_string();
        desktop.tap_widget::<FontIcon>(view, |icon| icon.glyph == back);
    }
}

#[test]
fn the_settings_first_page_leads_to_each_apps_settings_and_back_in_either_layout() {
    let layouts: [fn() -> (LibraryDesktop, ViewId); 2] = [in_launcher, on_phone];
    for layout in layouts {
        let (mut desktop, view) = layout();
        installs(&mut desktop, TRIANGLE, "Triangle", b"\0asm");
        let page = |desktop: &LibraryDesktop| desktop.read(|library| library.page().clone());
        open_settings(&mut desktop, view);
        assert_eq!(page(&desktop), LibraryPage::Settings(SettingsPage::Root));
        assert!(
            desktop.shows(words::PERMISSIONS) && desktop.shows(words::APPS),
            "{:?}",
            desktop.labels()
        );
        desktop.tap(view, "Triangle");
        assert_eq!(
            page(&desktop),
            LibraryPage::Settings(SettingsPage::App(triangle()))
        );
        assert!(
            desktop.shows(&words::settings_of("Triangle")),
            "{:?}",
            desktop.labels()
        );
        back_once(&mut desktop, view);
        assert_eq!(page(&desktop), LibraryPage::Settings(SettingsPage::Root));
        back_once(&mut desktop, view);
        assert_eq!(page(&desktop), LibraryPage::Home);
    }
}

#[test]
fn a_permissions_default_changes_on_its_page_in_the_launcher_and_is_kept() {
    let (mut desktop, view) = in_launcher();
    open_permission(&mut desktop, view, Permission::Network);
    assert!(
        desktop.shows(words::DEFAULT) && desktop.shows(words::ANY_DOMAIN),
        "{:?}",
        desktop.labels()
    );
    assert!(!desktop.shows(words::LOCAL_NETWORK), "one kind's page");
    desktop.tap(view, words::ONLY_DOMAINS);
    assert_eq!(
        global_network(&desktop).domains,
        DomainRule::Only(Vec::new())
    );
    desktop.update(|library, cx| {
        library.change_settings(
            &SettingsOf::Global,
            SettingChange::AddDomain("example.com".into()),
            cx,
        );
    });
    assert!(desktop.shows("example.com"), "{:?}", desktop.labels());
    desktop.tap(view, words::REMOVE);
    assert_eq!(
        global_network(&desktop).domains,
        DomainRule::Only(Vec::new())
    );
    go_to_permission(&mut desktop, view, Permission::Folders);
    assert!(desktop.shows(words::NO_FOLDERS), "{:?}", desktop.labels());
}

#[test]
fn a_permissions_page_lists_the_apps_set_otherwise_each_leading_to_its_page() {
    let layouts: [fn() -> (LibraryDesktop, ViewId); 2] = [in_launcher, on_phone];
    for layout in layouts {
        let (mut desktop, view) = layout();
        installs(&mut desktop, TRIANGLE, "Triangle", b"\0asm");
        open_permission(&mut desktop, view, Permission::Network);
        assert!(
            desktop.shows(words::NONE_SET_OTHERWISE) && !desktop.shows("Triangle"),
            "{:?}",
            desktop.labels()
        );
        desktop.update(|library, cx| {
            let of = SettingsOf::Program(triangle());
            library.change_settings(&of, SettingChange::Domains(DomainsKind::Only), cx);
        });
        assert!(
            desktop.shows("Triangle") && desktop.shows("Only some domains"),
            "{:?}",
            desktop.labels()
        );
        desktop.tap(view, "Triangle");
        assert_eq!(
            desktop.read(|library| library.page().clone()),
            LibraryPage::Settings(SettingsPage::App(triangle()))
        );
        desktop.tap(view, words::USE_GLOBAL);
        back(&mut desktop, view);
        open_permission(&mut desktop, view, Permission::Network);
        assert!(
            !desktop.shows("Triangle"),
            "it follows the default again: {:?}",
            desktop.labels()
        );
    }
}

#[test]
fn a_programs_settings_follow_those_for_every_program_until_changed_in_the_launcher() {
    let (mut desktop, view) = in_launcher();
    installs(&mut desktop, TRIANGLE, "Triangle", b"\0asm");
    let settings_of_triangle = || LibraryPage::Settings(SettingsPage::App(triangle()));
    desktop.update(|library, cx| library.show_page(settings_of_triangle(), cx));
    assert!(
        desktop.shows("Triangle's settings"),
        "{:?}",
        desktop.labels()
    );
    assert!(desktop.shows(words::FOLLOWS_GLOBAL));
    assert!(!desktop.shows(words::USE_GLOBAL));
    desktop.tap(view, words::ONLY_DOMAINS);
    let own = desktop.store.program_settings(&triangle()).unwrap();
    assert_eq!(
        own.network,
        Setting::Own(NetworkGrant {
            on: true,
            domains: DomainRule::Only(Vec::new())
        }),
        "starts from the setting for every program"
    );
    assert_eq!(
        global_network(&desktop).domains,
        DomainRule::Any,
        "only this program's"
    );
    desktop.tap(view, words::USE_GLOBAL);
    let own = desktop.store.program_settings(&triangle()).unwrap();
    assert_eq!(own.network, Setting::Global);
    back(&mut desktop, view);
    assert_eq!(
        desktop.read(|library| library.page().clone()),
        LibraryPage::Home
    );
}

#[test]
fn a_programs_machines_are_shown_under_the_warning_and_taken_back_in_the_launcher() {
    let (mut desktop, view) = in_launcher();
    installs(&mut desktop, TRIANGLE, "Triangle", b"\0asm");
    desktop
        .store
        .grants(&triangle())
        .keep_machine("this-computer");
    open_settings_from_icon(&mut desktop, view, "Triangle");
    for shown in [words::MACHINES, words::MACHINES_AS_YOU, "This computer"] {
        assert!(desktop.shows(shown), "{shown}: {:?}", desktop.labels());
    }
    desktop.tap(view, words::REMOVE);
    let own = desktop.store.program_settings(&triangle()).unwrap();
    assert!(own.machines.is_empty(), "taken back");
    assert!(desktop.shows(words::NO_MACHINES), "{:?}", desktop.labels());
}

/// Sets Triangle's machine, its output and its dialogs but saving to answers of their own, the
/// output one not connected, and gives it an answer to its `zoommtg` links; answers where its save
/// is to go.
fn triangle_answered(desktop: &LibraryDesktop, folder: &std::path::Path) -> std::path::PathBuf {
    let model = folder.join("model.glb");
    std::fs::write(&model, "glTF").unwrap();
    let changed = desktop.store.change_program_settings(&triangle(), |own| {
        own.answer_with.machine = Some("this-computer".to_owned());
        own.answer_with.audio_output = Some("unplugged speakers".to_owned());
        own.answer_with.open_file = Some(model.clone());
        own.answer_with.open_files = Some(vec![model.clone()]);
        own.answer_with.open_folder = Some(folder.to_path_buf());
        own.link_schemes
            .insert("zoommtg".to_owned(), Answer::Granted);
        own.roles
            .insert(waos_store::Role::Inspector, Answer::Denied);
    });
    changed.unwrap();
    folder.join("out.pdf")
}

/// How far the tests' wheel scrolls a settings page at a time.
const SCROLL_STEP: f64 = 200.0;

#[test]
fn a_programs_set_answers_show_as_they_are_and_change_in_the_launcher() {
    let (mut desktop, view) = in_launcher();
    installs(&mut desktop, TRIANGLE, "Triangle", b"\0asm");
    let scratch = tempfile::tempdir().unwrap();
    let save = triangle_answered(&desktop, scratch.path());
    open_settings_from_icon(&mut desktop, view, "Triangle");
    let placeholder = words::file_dialog_placeholder(FileDialog::SaveFile);
    desktop.scroll_until_visible(view, placeholder, words::NETWORK, SCROLL_STEP);
    desktop.tap(view, placeholder);
    let saved = save.display().to_string();
    desktop.type_text(view, &saved);
    desktop.tap(view, words::ANSWER_WITH);
    let own = desktop.store.program_settings(&triangle()).unwrap();
    assert_eq!(
        own.answer_with.save_file.as_ref(),
        Some(&save),
        "{:?}",
        desktop.labels()
    );

    let not_connected = words::not_offered("unplugged speakers");
    desktop.scroll_until_visible(view, &not_connected, &saved, SCROLL_STEP);
    for shown in [&not_connected, "Speakers", "Headphones", "This computer"] {
        assert!(desktop.shows(shown), "{shown}: {:?}", desktop.labels());
    }
    desktop.tap(view, words::ASK);
    let own = desktop.store.program_settings(&triangle()).unwrap();
    assert_eq!(
        own.answer_with.audio_output, None,
        "the output is asked again"
    );
    assert_eq!(own.answer_with.machine.as_deref(), Some("this-computer"));
}

#[test]
fn a_path_no_dialog_could_choose_is_refused_and_kept_out_of_the_settings() {
    let (mut desktop, view) = in_launcher();
    installs(&mut desktop, TRIANGLE, "Triangle", b"\0asm");
    let scratch = tempfile::tempdir().unwrap();
    triangle_answered(&desktop, scratch.path());
    open_settings_from_icon(&mut desktop, view, "Triangle");
    let placeholder = words::file_dialog_placeholder(FileDialog::SaveFile);
    desktop.scroll_until_visible(view, placeholder, words::NETWORK, SCROLL_STEP);
    desktop.tap(view, placeholder);
    let nowhere = scratch.path().join("missing").join("out.pdf");
    desktop.type_text(view, &nowhere.display().to_string());
    desktop.tap(view, words::ANSWER_WITH);
    let own = desktop.store.program_settings(&triangle()).unwrap();
    assert_eq!(own.answer_with.save_file, None);
    let problem = desktop.read(|library| library.problem().map(str::to_owned));
    assert!(
        problem.is_some_and(|why| why.ends_with("is in a folder that does not exist")),
        "said why"
    );
}

#[test]
fn a_programs_set_answers_change_on_the_phone() {
    let (mut desktop, view) = on_phone();
    installs(&mut desktop, TRIANGLE, "Triangle", b"\0asm");
    let scratch = tempfile::tempdir().unwrap();
    triangle_answered(&desktop, scratch.path());
    open_settings_from_icon(&mut desktop, view, "Triangle");
    let not_connected = words::not_offered("unplugged speakers");
    desktop.scroll_until_visible(view, &not_connected, words::NETWORK, SCROLL_STEP);
    let check = CupertinoIcons::check_mark().code_point;
    let is_check = |icon: &Icon| {
        icon.icon
            .as_ref()
            .is_some_and(|icon| icon.code_point == check)
    };
    let mark = desktop.bounds_of::<Icon>(&is_check).unwrap();
    let output = label_bounds(&desktop, &not_connected);
    let middle = (output.top + output.bottom) / 2.0;
    assert!(
        mark.top <= middle && middle <= mark.bottom,
        "the output set is checked on its row: {mark:?}, {output:?}"
    );
    desktop.tap(view, words::ASK);
    let own = desktop.store.program_settings(&triangle()).unwrap();
    assert_eq!(
        own.answer_with.audio_output, None,
        "the output is asked again"
    );
    desktop.tap(view, "Headphones");
    let own = desktop.store.program_settings(&triangle()).unwrap();
    assert_eq!(own.answer_with.audio_output.as_deref(), Some("headphones"));
}

#[test]
fn a_programs_answer_to_a_link_scheme_shows_and_is_forgotten_in_the_launcher() {
    let (mut desktop, view) = in_launcher();
    installs(&mut desktop, TRIANGLE, "Triangle", b"\0asm");
    let scratch = tempfile::tempdir().unwrap();
    triangle_answered(&desktop, scratch.path());
    open_settings_from_icon(&mut desktop, view, "Triangle");
    let link = words::link_scheme("zoommtg", Answer::Granted);
    desktop.scroll_until_visible(view, &link, words::NETWORK, SCROLL_STEP);
    desktop.tap_beside::<waft_winui::Button>(view, &link);
    let own = desktop.store.program_settings(&triangle()).unwrap();
    assert!(own.link_schemes.is_empty(), "asked again at its next link");
}

#[test]
fn a_programs_answer_to_a_role_shows_and_is_forgotten_in_the_launcher() {
    let (mut desktop, view) = in_launcher();
    installs(&mut desktop, TRIANGLE, "Triangle", b"\0asm");
    let scratch = tempfile::tempdir().unwrap();
    triangle_answered(&desktop, scratch.path());
    open_settings_from_icon(&mut desktop, view, "Triangle");
    let role = words::role(waos_store::Role::Inspector, Answer::Denied);
    desktop.scroll_until_visible(view, &role, words::NETWORK, SCROLL_STEP);
    desktop.tap_beside::<waft_winui::Button>(view, &role);
    let own = desktop.store.program_settings(&triangle()).unwrap();
    assert!(own.roles.is_empty(), "asked again at its next start");
}

#[test]
fn the_camera_is_simulated_for_every_program_in_the_launcher() {
    let (mut desktop, view) = in_launcher();
    open_permission(&mut desktop, view, Permission::Camera);
    desktop.tap(view, words::SIMULATED);
    let global = desktop.store.global_settings().unwrap();
    assert_eq!(global.answers.camera, Some(Answer::Simulated));
    let detail = if crate::state::SIMULATED_CAMERA {
        words::SIMULATED_CAMERA_DETAIL
    } else {
        words::NO_SIMULATED_CAMERA
    };
    assert!(desktop.shows(detail), "{:?}", desktop.labels());
}

#[test]
fn a_programs_simulated_microphone_records_what_its_page_picks_in_the_launcher_and_on_the_phone() {
    for (mut desktop, view) in [in_launcher(), on_phone()] {
        installs(&mut desktop, TRIANGLE, "Triangle", b"\0asm");
        let simulated = desktop.store.change_program_settings(&triangle(), |own| {
            own.answers.set(
                waos_store::Question::Microphone,
                Setting::Own(Answer::Simulated),
            );
        });
        simulated.unwrap();
        open_settings_from_icon(&mut desktop, view, "Triangle");
        desktop.scroll_until_visible(view, words::SILENCE, words::NETWORK, SCROLL_STEP);
        assert!(
            desktop.shows(words::SIMULATED_MICROPHONE_DETAIL) && desktop.shows(words::QUIET_TONE),
            "{:?}",
            desktop.labels()
        );
        desktop.tap(view, words::SILENCE);
        let own = desktop.store.program_settings(&triangle()).unwrap();
        assert_eq!(
            own.simulated_microphone,
            Setting::Own(waos::SimulatedSound::Silence)
        );
    }
}

#[test]
fn no_page_offers_commands_in_the_launcher_or_on_the_phone() {
    let (mut desktop, view) = in_launcher();
    installs(&mut desktop, TRIANGLE, "Triangle", b"\0asm");
    open_permission(&mut desktop, view, Permission::Network);
    assert!(desktop.shows(words::NETWORK), "{:?}", desktop.labels());
    assert!(!desktop.shows("Commands") && !desktop.shows("Any command"));
    back(&mut desktop, view);
    open_settings_from_icon(&mut desktop, view, "Triangle");
    assert!(desktop.shows(words::MACHINES), "{:?}", desktop.labels());
    assert!(!desktop.shows("Commands") && !desktop.shows("Any command"));

    let (mut desktop, view) = on_phone();
    open_permission(&mut desktop, view, Permission::Network);
    assert!(desktop.shows(words::NETWORK), "{:?}", desktop.labels());
    assert!(!desktop.shows("Commands") && !desktop.shows("Any command"));
}

#[test]
fn a_programs_settings_open_from_its_icon_in_the_launcher() {
    let (mut desktop, view) = in_launcher();
    installs(&mut desktop, TRIANGLE, "Triangle", b"\0asm");
    open_settings_from_icon(&mut desktop, view, "Triangle");
    assert!(
        desktop.shows("Triangle's settings"),
        "{:?}",
        desktop.labels()
    );
}

#[test]
fn a_program_asking_for_its_network_opens_its_page_and_hears_when_the_user_leaves_it() {
    let (mut desktop, view) = in_launcher();
    desktop.press_escape();
    let component = std::fs::read(waos_test_programs::PERMISSIONS).unwrap();
    installs(&mut desktop, PERMISSIONS, "Permissions", &component);
    desktop.update(|library, cx| {
        library.change_settings(
            &SettingsOf::Global,
            SettingChange::Answer(
                waos_store::Question::Notifications,
                Some(waos_store::Answer::Granted),
            ),
            cx,
        );
    });
    let id: ProgramId = PERMISSIONS.parse().unwrap();
    desktop.update(|library, cx| library.open(&id, cx));
    let asked = words::switch_asked("Permissions", waos::Switch::Network);
    desktop.settle_until("the program's page", |desktop| desktop.shows(&asked));
    assert!(
        desktop.shows("Permissions's settings"),
        "{:?}",
        desktop.labels()
    );
    assert!(desktop.windows()[0].is_shown(), "the launcher shows for it");

    back(&mut desktop, view);
    assert!(!desktop.shows(&asked));
    // Past its network request, the program waits for its clipboard-write switch.
    desktop.update(|library, cx| {
        library.change_settings(
            &SettingsOf::Global,
            SettingChange::ClipboardWrite(false),
            cx,
        );
    });
    let runs = Arc::clone(&desktop.runs);
    desktop.settle_until("the program ends", move |_| {
        futures::executor::block_on(runs.names()).is_empty()
    });
}

#[test]
fn a_permissions_default_changes_on_its_page_on_the_phone() {
    let (mut desktop, view) = on_phone();
    open_permission(&mut desktop, view, Permission::Network);
    assert!(desktop.shows(words::NETWORK), "{:?}", desktop.labels());
    desktop.tap(view, words::ONLY_DOMAINS);
    assert_eq!(
        global_network(&desktop).domains,
        DomainRule::Only(Vec::new())
    );
    back(&mut desktop, view);
    assert!(desktop.shows(words::HOME_TITLE), "{:?}", desktop.labels());
    assert!(!desktop.shows(words::ONLY_DOMAINS));
}

#[test]
fn a_programs_settings_change_on_the_phone_and_follow_again() {
    let (mut desktop, view) = on_phone();
    installs(&mut desktop, TRIANGLE, "Triangle", b"\0asm");
    open_settings_from_icon(&mut desktop, view, "Triangle");
    assert!(
        desktop.shows("Triangle's settings"),
        "{:?}",
        desktop.labels()
    );
    desktop.tap(view, words::ANY_DOMAIN_BUT);
    let own = desktop.store.program_settings(&triangle()).unwrap();
    assert_eq!(
        own.network,
        Setting::Own(NetworkGrant {
            on: true,
            domains: DomainRule::AnyBut(Vec::new())
        })
    );
    desktop.tap(view, words::USE_GLOBAL);
    let own = desktop.store.program_settings(&triangle()).unwrap();
    assert_eq!(own.network, Setting::Global);
}

/// What reads one setting that is on or off from the settings for every program.
type GlobalValue = fn(&GlobalSettings) -> bool;

/// Each setting that is on or off, by the kind whose page shows it, the name its row shows, and
/// its value for every program.
fn on_off_settings() -> [(Permission, &'static str, GlobalValue); 5] {
    [
        (Permission::Network, words::NETWORK, |settings| {
            settings.network.on
        }),
        (Permission::LocalNetwork, words::LOCAL_NETWORK, |settings| {
            settings.local_network
        }),
        (Permission::Clipboard, words::CLIPBOARD_READ, |settings| {
            settings.clipboard_read
        }),
        (Permission::Clipboard, words::CLIPBOARD_WRITE, |settings| {
            settings.clipboard_write
        }),
        (
            Permission::ActivityRecord,
            words::RECORD_EVERY,
            |settings| settings.record,
        ),
    ]
}

/// The value the switch on the row named `name` shows.
fn switch_shows(desktop: &LibraryDesktop, name: &str) -> bool {
    desktop
        .widget_beside::<CupertinoSwitch>(name)
        .unwrap_or_else(|| panic!("no switch beside {name:?}: {:?}", desktop.labels()))
        .value
}

/// Where the label `text` is drawn.
fn label_bounds(desktop: &LibraryDesktop, text: &str) -> Rect {
    desktop
        .bounds_of::<waft::Text>(&|label| label.data.as_deref() == Some(text))
        .unwrap_or_else(|| panic!("no label {text:?}: {:?}", desktop.labels()))
}

#[test]
fn each_setting_that_is_on_or_off_is_a_switch_on_a_row_naming_it_on_the_phone() {
    let (mut desktop, view) = on_phone();
    open_permission(&mut desktop, view, Permission::Network);
    assert!(!desktop.shows(words::ON) && !desktop.shows(words::OFF));
    assert!(
        desktop.shows(words::ANY_DOMAIN),
        "a choice among named things is a list, its choice checked"
    );
    for (permission, name, value) in on_off_settings() {
        go_to_permission(&mut desktop, view, permission);
        let global = desktop.store.global_settings().unwrap();
        assert_eq!(switch_shows(&desktop, name), value(&global), "{name}");
        let switch = desktop.bounds_beside::<CupertinoSwitch>(name).unwrap();
        assert!(
            label_bounds(&desktop, name).right >= switch.left,
            "{name} spans its row up to the switch, so a name too long for it is cut with an ellipsis, not measured to nothing"
        );
    }
    go_to_permission(&mut desktop, view, Permission::Notifications);
    assert!(desktop.shows(words::ALLOW), "the notifications' choice too");
}

#[test]
fn a_tap_on_a_switch_or_on_its_row_changes_the_setting_on_the_phone() {
    let (mut desktop, view) = on_phone();
    open_permission(&mut desktop, view, Permission::LocalNetwork);
    let local_network =
        |desktop: &LibraryDesktop| desktop.store.global_settings().unwrap().local_network;
    let before = local_network(&desktop);
    desktop.tap_beside::<CupertinoSwitch>(view, words::LOCAL_NETWORK);
    assert_eq!(local_network(&desktop), !before, "the switch's tap");
    assert_eq!(switch_shows(&desktop, words::LOCAL_NETWORK), !before);
    desktop.tap(view, words::LOCAL_NETWORK);
    assert_eq!(local_network(&desktop), before, "the row's tap");
    assert_eq!(switch_shows(&desktop, words::LOCAL_NETWORK), before);
}

/// A day of Triangle's kept, on which it posted a notification.
fn triangle_posted_once(desktop: &LibraryDesktop) {
    use waos::AuditLog;
    let mut day = waos::AuditDay::starting(std::time::SystemTime::UNIX_EPOCH);
    day.notifications.allowed = 1;
    desktop.store.keep(triangle().as_str(), &day);
}

#[test]
fn a_programs_last_seven_days_are_said_on_its_page_on_the_phone() {
    let (mut desktop, view) = on_phone();
    installs(&mut desktop, TRIANGLE, "Triangle", b"\0asm");
    triangle_posted_once(&desktop);
    open_settings_from_icon(&mut desktop, view, "Triangle");
    desktop.scroll_until_visible(view, words::LAST_DAYS, words::NETWORK, SCROLL_STEP);
    assert!(
        desktop.shows("Notifications: Posted once"),
        "{:?}",
        desktop.labels()
    );
}

#[test]
fn a_programs_last_seven_days_are_said_on_its_page_in_the_launcher() {
    let (mut desktop, view) = in_launcher();
    installs(&mut desktop, TRIANGLE, "Triangle", b"\0asm");
    triangle_posted_once(&desktop);
    open_settings_from_icon(&mut desktop, view, "Triangle");
    desktop.scroll_until_visible(
        view,
        "Notifications: Posted once",
        words::NETWORK,
        SCROLL_STEP,
    );
}

#[test]
fn a_programs_record_turned_off_removes_what_was_kept_of_it_on_the_phone() {
    use waos::AuditLog;
    let (mut desktop, view) = on_phone();
    installs(&mut desktop, TRIANGLE, "Triangle", b"\0asm");
    let day = waos::AuditDay::starting(std::time::SystemTime::UNIX_EPOCH);
    desktop.store.keep(triangle().as_str(), &day);
    assert_eq!(desktop.store.days(triangle().as_str()).len(), 1);
    open_settings_from_icon(&mut desktop, view, "Triangle");
    desktop.scroll_until_visible(view, words::RECORD, words::NETWORK, SCROLL_STEP);
    assert!(
        switch_shows(&desktop, words::RECORD),
        "on, as for every program"
    );
    desktop.tap_beside::<CupertinoSwitch>(view, words::RECORD);
    let own = desktop.store.program_settings(&triangle()).unwrap();
    assert_eq!(own.record, Setting::Own(false));
    assert!(
        desktop.store.days(triangle().as_str()).is_empty(),
        "what was kept is removed"
    );
}

#[test]
fn a_programs_switch_says_it_follows_the_setting_for_every_program_on_the_phone() {
    let (mut desktop, view) = on_phone();
    installs(&mut desktop, TRIANGLE, "Triangle", b"\0asm");
    open_settings_from_icon(&mut desktop, view, "Triangle");
    let global = desktop.store.global_settings().unwrap();
    assert_eq!(switch_shows(&desktop, words::NETWORK), global.network.on);
    assert!(
        desktop.shows(words::FOLLOWS_GLOBAL),
        "{:?}",
        desktop.labels()
    );
    assert!(!desktop.shows(words::USE_GLOBAL));

    desktop.tap(view, words::NETWORK);
    let own = desktop.store.program_settings(&triangle()).unwrap();
    assert_eq!(
        own.network,
        Setting::Own(NetworkGrant {
            on: !global.network.on,
            ..global.network.clone()
        }),
        "the program's own, from the setting for every program"
    );
    assert_eq!(switch_shows(&desktop, words::NETWORK), !global.network.on);
    assert!(desktop.shows(words::USE_GLOBAL), "{:?}", desktop.labels());
    assert_eq!(
        desktop.store.global_settings().unwrap().network.on,
        global.network.on,
        "only this program's"
    );

    desktop.tap(view, words::USE_GLOBAL);
    let own = desktop.store.program_settings(&triangle()).unwrap();
    assert_eq!(own.network, Setting::Global);
    assert_eq!(switch_shows(&desktop, words::NETWORK), global.network.on);
}

/// Only on macOS are a program's notifications its own app's, which the system can refuse and its
/// settings open at; elsewhere they are WaOS's, and a program's page says nothing of the system.
#[cfg(target_os = "macos")]
mod system_refusals {
    use std::rc::Rc;

    use waft::WindowConfig;
    use waft_test::Opened;

    use super::*;
    use crate::ui::every_window::LibraryWindow;

    /// The desktop with the library in a window of a desktop's width, tall enough to show a program's
    /// whole page.
    fn in_tall_window() -> (LibraryDesktop, ViewId) {
        let (desktop, ()) = LibraryDesktop::new(|app, library, recordings, _view_windows| {
            let config = WindowConfig {
                title: "WaOS".to_owned(),
                size: [800.0, 1600.0],
                ..WindowConfig::default()
            };
            let page = LaidOutLibrary {
                library: library.clone(),
                recordings: recordings.clone(),
                questions_here: true,
            };
            let _asked = app.platform().request_view(config, None);
            let window = LibraryWindow {
                content: page.into_widget(),
                close_requested: Rc::new(|_app, _view| {}),
                initial_data: None,
            };
            (Some(window), ())
        });
        let view = desktop.windows()[0].test_view().id();
        (desktop, view)
    }

    /// The library heard macOS refuse Triangle its notifications, as Triangle's app told it.
    fn macos_refuses_triangle(desktop: &mut LibraryDesktop) {
        desktop.update(|library, cx| library.system_permission_said(&triangle(), true, cx));
    }

    /// Taps Open System Settings, and answers what the app opened.
    fn opens_system_settings(desktop: &mut LibraryDesktop, view: ViewId) -> Vec<Opened> {
        desktop.tap(view, words::OPEN_SYSTEM_SETTINGS);
        desktop.opened()
    }

    #[test]
    fn a_programs_page_says_the_system_refuses_its_notifications_and_opens_system_settings_at_a_desktops_width()
     {
        let (mut desktop, view) = in_tall_window();
        installs(&mut desktop, TRIANGLE, "Triangle", b"\0asm");
        open_settings_from_icon(&mut desktop, view, "Triangle");
        assert!(desktop.shows(words::ALLOW), "{:?}", desktop.labels());
        assert!(
            !desktop.shows(words::NOTIFICATIONS_REFUSED),
            "not until macOS says so"
        );
        macos_refuses_triangle(&mut desktop);
        assert!(
            desktop.shows(words::NOTIFICATIONS_REFUSED),
            "{:?}",
            desktop.labels()
        );
        let link = format!(
            "x-apple.systempreferences:com.apple.Notifications-Settings.extension?id=dev.waos.program.{TRIANGLE}"
        );
        assert_eq!(
            opens_system_settings(&mut desktop, view),
            [Opened::Link(link)]
        );
        let own = desktop.store.program_settings(&triangle()).unwrap();
        assert_eq!(
            own.answers.get(waos_store::Question::Notifications),
            &Setting::Global,
            "WaOS's own answer is as the user left it"
        );

        desktop.update(|library, cx| library.system_permission_said(&triangle(), false, cx));
        assert!(
            !desktop.shows(words::NOTIFICATIONS_REFUSED),
            "macOS lets them again"
        );
    }

    #[test]
    fn a_programs_page_says_the_system_refuses_its_notifications_on_the_phone() {
        let (mut desktop, view) = on_phone();
        installs(&mut desktop, TRIANGLE, "Triangle", b"\0asm");
        macos_refuses_triangle(&mut desktop);
        open_settings_from_icon(&mut desktop, view, "Triangle");
        desktop.scroll_until_visible(
            view,
            words::OPEN_SYSTEM_SETTINGS,
            words::NETWORK,
            SCROLL_STEP,
        );
        assert!(
            desktop.shows(words::NOTIFICATIONS_REFUSED),
            "{:?}",
            desktop.labels()
        );
        assert_eq!(opens_system_settings(&mut desktop, view).len(), 1);
        back(&mut desktop, view);
        open_permission(&mut desktop, view, Permission::Notifications);
        assert!(
            !desktop.shows(words::NOTIFICATIONS_REFUSED),
            "a program's alone"
        );
    }
}

#[test]
fn the_global_page_says_whether_waos_reaches_the_phone_s_files_and_opens_android_s_page() {
    let layouts: [fn(DesktopKind) -> (LibraryDesktop, ViewId); 2] = [in_launcher_of, on_phone_of];
    for layout in layouts {
        let phone = std::rc::Rc::new(TestPhone::reachable(false));
        let kind = DesktopKind {
            phone_files: Some(std::rc::Rc::clone(&phone) as _),
            ..DesktopKind::default()
        };
        let (mut desktop, view) = layout(kind);
        open_waos_settings(&mut desktop, view);
        assert!(
            desktop.shows(words::PHONE_FILES_OFF),
            "{:?}",
            desktop.labels()
        );
        desktop.tap(view, words::OPEN_ANDROID_SETTINGS);
        assert_eq!(phone.pages_opened(), 1);

        phone.reachable.set(true);
        desktop.leave_and_come_back();
        assert!(
            desktop.shows(words::PHONE_FILES_ON),
            "read again back in front: {:?}",
            desktop.labels()
        );
    }
}

#[test]
fn the_global_page_says_nothing_of_the_phone_s_files_where_waos_reaches_what_its_user_does() {
    let (mut desktop, view) = in_launcher();
    open_settings(&mut desktop, view);
    assert!(desktop.shows(words::PERMISSIONS), "{:?}", desktop.labels());
    assert!(
        !desktop.shows(words::WAOS_SETTINGS),
        "no page of WaOS's own settings where it has none: {:?}",
        desktop.labels()
    );
}
