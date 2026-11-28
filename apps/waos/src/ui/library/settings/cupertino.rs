//! A settings page in Cupertino's widgets: the global settings, or a program's, as grouped
//! sections under a navigation bar with Back: on a program's page first the machines its user
//! picked for it, under a warning that it acts on them as the user; the network switch with its
//! rule of domains, the local network's and the two clipboard switches, the folders mounted, and
//! the answer each question gets; and on a program's page last its answers to opening links of
//! its own schemes, and the answers set for its pickers and file dialogs, each Ask or what WaOS
//! gives in place of asking, a picker's as a list whose chosen row has a check mark, as iOS's
//! Settings lists its choices. A setting that is on or off is a row as iOS's Settings has one,
//! its name at the leading side and a switch at the trailing side, and a tap anywhere on the row
//! flips it; a choice among named things is a segmented control. The switch shows the value in
//! force: on a program's page, a setting's section says beneath whether it follows the setting for
//! every program, and one that does not can be set back to it; and while
//! the system refuses the program its notifications, their section says so, with a button that
//! opens the system's settings at them. Where WaOS offers the choice, the global page first says
//! whether programs show inside WaOS, or, in a desktop's launcher, whether WaOS is kept in the Dock
//! or the taskbar, which are WaOS's own preferences rather than a program's settings.
//!
//! What the page keeps of its own is what is being typed: a domain, a folder and where it is
//! seen, whether it is writable, and a path for each file dialog.

use std::fmt::Debug;
use std::rc::Rc;

use waft::{
    AnyColor, App, BuildContext, Builder, Column, CrossAxisAlignment, EdgeInsetsGeometry, Entity,
    Handle, Icon, IntoWidget, ListView, Listener, Padding, Ref, SizedBox, State, StateData,
    StatefulWidget, Task, Text, TextEditingController, TextEditingControllerLeaf, ValueChanged,
    WidgetRef,
};
use waft_cupertino::{
    CupertinoButton, CupertinoColors, CupertinoIcons, CupertinoListSection, CupertinoListTile,
    CupertinoNavigationBar, CupertinoPageScaffold, CupertinoSwitch, CupertinoTextField,
    ObstructingPreferredSizeWidgetRef,
};
use waos::{FileDialog, SimulatedSound};
use waos_activity::Activity;
use waos_package::ProgramId;
use waos_services::permissions::{
    Answer, DomainRule, FileSystemGrant, NetworkGrant, Question, Role,
};

use super::cupertino_root::{
    ICON_SIDE, PERMISSION_ICON_SIDE, detail_icon, footnote, link_row, permission_icon,
};
use super::{
    AppAction, AppInfo, app_details, app_info, app_permissions, said_with, set_otherwise,
    simulated_detail,
};
use crate::logic::permissions::Permission;
use crate::logic::setting_changes::{
    DomainsKind, SettingChange, SettingsOf, Shown, ShownAnswerWith, ShownSettings, Which,
    domains_kind, simulatable,
};
use crate::state::library::{AppDetail, DeniedBySystem, Library, SettingsPage};
use crate::ui::library::cupertino::{back_to_page_beneath, under_bar};
use crate::ui::library::home::cupertino::resolved;
use crate::ui::library::program_icon::ProgramIcon;
use crate::ui::library::words;

/// The settings `of` shows, as a page of its own.
#[derive(Debug)]
pub(crate) struct CupertinoSettings {
    pub(crate) library: Entity<Library>,
    pub(crate) of: SettingsOf,
    /// The program's name, or none on the global page.
    pub(crate) program_name: Option<String>,
    /// The kind of permission whose settings alone the page shows, with the apps that set it
    /// otherwise: a kind's page, showing its default; none for an app's page, which shows all.
    pub(crate) only: Option<Permission>,
    /// What of the app's alone the page shows, on its page of it; none elsewhere.
    pub(crate) detail: Option<AppDetail>,
}

impl StatefulWidget for CupertinoSettings {
    type State = CupertinoSettingsState;

    fn create_state(&self) -> CupertinoSettingsState {
        CupertinoSettingsState {
            state: StateData::new(),
            typed: None,
            writable: false,
            simulating: Vec::new(),
        }
    }
}

pub(crate) struct CupertinoSettingsState {
    state: StateData<CupertinoSettings>,
    typed: Option<Typed>,
    /// Whether a folder added is writable.
    writable: bool,
    /// The file pickers the user chose to simulate and has set no path for yet.
    simulating: Vec<FileDialog>,
}

/// What is being typed on the page.
#[derive(Clone)]
struct Typed {
    domain: Ref<TextEditingController>,
    folder: Ref<TextEditingController>,
    folder_at: Ref<TextEditingController>,
    /// A path for each file dialog, in [`FileDialog::ALL`]'s order.
    paths: [Ref<TextEditingController>; 4],
}

impl Typed {
    fn path(&self, dialog: FileDialog) -> &Ref<TextEditingController> {
        let index = FileDialog::ALL.iter().position(|each| *each == dialog);
        &self.paths[index.expect("every dialog")]
    }
}

impl State for CupertinoSettingsState {
    type Widget = CupertinoSettings;
    waft_widgets::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let typed = Typed {
            domain: TextEditingController::new(app),
            folder: TextEditingController::new(app),
            folder_at: TextEditingController::new(app),
            paths: std::array::from_fn(|_| TextEditingController::new(app)),
        };
        app.get_mut(self).typed = Some(typed);
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        if let Some(typed) = app.get_mut(self).typed.take() {
            let fields = [typed.domain, typed.folder, typed.folder_at];
            for controller in fields.into_iter().chain(typed.paths) {
                controller.dispose(app);
            }
        }
    }

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        let widget = self.widget(app);
        let page = Page {
            library: widget.library.clone(),
            of: widget.of.clone(),
            only: widget.only,
            detail: widget.detail,
            defaults: widget.library.read(app).settings(&SettingsOf::Global).ok(),
            typed: app.get(self).typed.clone().expect("an initialized page"),
            writable: app.get(self).writable,
            simulating: app.get(self).simulating.clone(),
            state: self,
        };
        let (title, beneath) = match (page.only, &widget.program_name) {
            (_, Some(name)) if page.detail.is_some() => (
                words::app_detail_name(page.detail.expect("a detail's page")).to_owned(),
                name.clone(),
            ),
            (Some(only), Some(name)) => (words::permission_name(only).to_owned(), name.clone()),
            (Some(only), None) => (
                words::permission_name(only).to_owned(),
                words::PERMISSIONS.to_owned(),
            ),
            (None, Some(name)) => (name.clone(), words::SETTINGS.to_owned()),
            (None, None) => (
                words::GLOBAL_SETTINGS.to_owned(),
                words::SETTINGS.to_owned(),
            ),
        };
        let mut sections = Vec::new();
        sections.extend(page.switch_asked(app));
        let info = page.app_info(app);
        if let Some((id, info)) = &info {
            sections.extend(page.said_of_the_app(id, info));
        }
        if page.kind_page().is_some() {
            sections.push(said_over_the_lists(words::DEFAULT_DETAIL));
        }
        match page.library.read(app).settings(&page.of) {
            Ok(shown) => {
                let activity = page.library.read(app).activity(&page.of);
                let denied = (page.library.read(app)).denied_by_system(&page.of, &shown, app);
                sections.extend(page.sections(&shown, activity.as_ref(), denied));
            }
            Err(why) => sections.push(section(Some(words::SETTINGS), None, vec![wrapped(&why)])),
        }
        if let Some(only) = page.kind_page() {
            sections.push(page.set_otherwise(app, only));
        }
        if let Some((id, info)) = &info {
            sections.push(page.of_the_app_as_a_whole(id, info));
        }
        let bar = CupertinoNavigationBar::new()
            .leading(back_to_page_beneath(&page.library, &beneath))
            .middle(Text::new(title));
        CupertinoPageScaffold::new(under_bar(move |top| {
            ListView::new()
                .padding(EdgeInsetsGeometry::from_ltrb(0.0, top, 0.0, 24.0))
                .children(sections.clone())
                .into_widget()
        }))
        .navigation_bar(ObstructingPreferredSizeWidgetRef::new(bar))
        .background_color(CupertinoColors::SYSTEM_GROUPED_BACKGROUND)
        .into_widget()
    }
}

impl CupertinoSettingsState {
    fn set_writable(self: Handle<Self>, app: &mut App, writable: bool) {
        self.set_state(app, |state| state.writable = writable);
    }

    /// The user chose to simulate `dialog`, or not to: its path's field shows, or goes.
    fn simulate(self: Handle<Self>, app: &mut App, dialog: FileDialog, simulated: bool) {
        self.set_state(app, |state| {
            state.simulating.retain(|each| *each != dialog);
            if simulated {
                state.simulating.push(dialog);
            }
        });
    }
}

/// What one build of the page reads and writes.
struct Page {
    library: Entity<Library>,
    of: SettingsOf,
    only: Option<Permission>,
    detail: Option<AppDetail>,
    /// The defaults, which an app's choice among answers names as its first: "Ask (default)".
    defaults: Option<ShownSettings>,
    typed: Typed,
    writable: bool,
    simulating: Vec<FileDialog>,
    state: Handle<CupertinoSettingsState>,
}

impl Page {
    /// Whether the page shows the controls of `permission`: a kind's page, every app's or one
    /// app's, those of its kind alone.
    fn shows(&self, permission: Permission) -> bool {
        self.only == Some(permission)
    }

    /// The kind whose default the page shows; none on an app's pages.
    fn kind_page(&self) -> Option<Permission> {
        self.only.filter(|_| self.of == SettingsOf::Global)
    }

    /// On an app's page, each permission with what the app holds at the row's end, each leading
    /// to the app's page for it; and, in the same list, what is the app's alone.
    fn app_permissions(&self, shown: &ShownSettings) -> Option<WidgetRef> {
        let SettingsOf::Program(id) = &self.of else {
            return None;
        };
        let rows = app_permissions(shown)
            .into_iter()
            .map(|(permission, said)| {
                link_row(
                    &self.library,
                    words::permission_name(permission),
                    SettingsPage::AppPermission(id.clone(), permission),
                )
                .leading(permission_icon(permission))
                .leading_size(PERMISSION_ICON_SIDE)
                .additional_info(Text::new(said))
                .into_widget()
            });
        let rows = rows.chain(self.app_details(id, shown)).collect();
        Some(section_of_icons(Some(words::PERMISSIONS), None, rows))
    }

    /// On a kind's page, the apps that set `permission` otherwise than its default, each with
    /// what it set at the row's end and leading to its page; one line where every app follows
    /// the default.
    fn set_otherwise(&self, app: &App, permission: Permission) -> WidgetRef {
        let mut rows: Vec<WidgetRef> = set_otherwise(self.library.read(app), permission)
            .into_iter()
            .map(|(listed, value)| {
                let icon = ProgramIcon {
                    png: listed.icon,
                    side: ICON_SIDE,
                };
                link_row(&self.library, &listed.name, SettingsPage::App(listed.id))
                    .leading(icon)
                    .leading_size(ICON_SIDE)
                    .additional_info(Text::new(value))
                    .into_widget()
            })
            .collect();
        if rows.is_empty() {
            rows.push(wrapped(words::NONE_SET_OTHERWISE));
        }
        section_of_icons(
            Some(words::SET_OTHERWISE),
            Some(words::SET_OTHERWISE_DETAIL),
            rows,
        )
    }

    /// What is the app `id`'s alone, each with how many it has, each a row leading to its page.
    fn app_details(&self, id: &ProgramId, shown: &ShownSettings) -> Vec<WidgetRef> {
        app_details(shown)
            .into_iter()
            .map(|(detail, said)| {
                link_row(
                    &self.library,
                    words::app_detail_name(detail),
                    SettingsPage::AppDetail(id.clone(), detail),
                )
                .leading(detail_icon(detail))
                .leading_size(PERMISSION_ICON_SIDE)
                .additional_info(Text::new(said))
                .into_widget()
            })
            .collect()
    }

    /// On an app's own page, the app and what its page says of it; none on the pages under it,
    /// and on a permission's.
    fn app_info(&self, app: &App) -> Option<(ProgramId, AppInfo)> {
        let SettingsOf::Program(id) = &self.of else {
            return None;
        };
        if self.only.is_some() || self.detail.is_some() {
            return None;
        }
        let info = app_info(self.library.read(app), id)?;
        Some((id.clone(), info))
    }

    /// Over an app's settings, what the app is, in the middle of the page as iOS's App Store
    /// heads an app's page: its icon over its name, its version and what it is doing now; then
    /// a row for each thing to do with it now.
    fn said_of_the_app(&self, id: &ProgramId, info: &AppInfo) -> Vec<WidgetRef> {
        let icon = ProgramIcon {
            png: info.icon.clone(),
            side: APP_ICON_SIDE,
        };
        let name = Text::new(info.name.clone()).style(
            waft::TextStyle::new()
                .font_size(22.0)
                .font_weight(waft::FontWeight::W700),
        );
        let lines = [
            icon.into_widget(),
            SizedBox::new().height(10.0).into_widget(),
            name.into_widget(),
            footnote(&info.version),
        ]
        .into_iter()
        .chain(info.status.as_deref().map(footnote));
        let app = Padding::new(EdgeInsetsGeometry::from_ltrb(20.0, 12.0, 20.0, 4.0)).child(
            Column::new()
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .children(lines),
        );
        let actions = info
            .actions
            .iter()
            .filter(|action| !action.ends_the_page())
            .map(|action| self.action_row(id, action))
            .collect();
        vec![app.into_widget(), section(None, None, actions)]
    }

    /// Under an app's settings, what is done to the app as a whole: its permissions reset, where
    /// any is set otherwise, and the app removed.
    fn of_the_app_as_a_whole(&self, id: &ProgramId, info: &AppInfo) -> WidgetRef {
        let rows = info
            .actions
            .iter()
            .filter(|action| action.ends_the_page())
            .map(|action| self.action_row(id, action))
            .collect();
        section(None, None, rows)
    }

    /// A row that does `action` to the app `id`: in the tint colour, as iOS's rows that act are,
    /// and in red for the one that removes it.
    fn action_row(&self, id: &ProgramId, action: &AppAction) -> WidgetRef {
        let color = match action {
            AppAction::Remove => CupertinoColors::DESTRUCTIVE_RED,
            AppAction::Open
            | AppAction::Update(_)
            | AppAction::Close
            | AppAction::ResetPermissions => CupertinoColors::ACTIVE_BLUE,
        };
        let (library, id, does) = (self.library.clone(), id.clone(), action.clone());
        CupertinoListTile::new(colored(action.label(), color))
            .on_tap(Rc::new(move |app: &mut App| {
                does.carry_out(&library, &id, app);
                Task::ready(())
            }))
            .into_widget()
    }

    /// What a program asked for, when it asked for one of its switches here.
    fn switch_asked(&self, app: &App) -> Option<WidgetRef> {
        let asked = self.library.read(app).switch_asked()?;
        let message = words::switch_asked(&asked.name, asked.switch);
        Some(section(
            Some(words::SETTINGS),
            None,
            vec![wrapped(&message)],
        ))
    }

    fn sections(
        &self,
        shown: &ShownSettings,
        activity: Option<&Activity>,
        denied: DeniedBySystem,
    ) -> Vec<WidgetRef> {
        let mut sections = Vec::new();
        if self.only.is_none() && self.detail.is_none() {
            sections.extend(self.app_permissions(shown));
        }
        match self.detail {
            Some(AppDetail::Machines) => {
                sections.push(self.machines(&shown.machines));
                sections.push(self.machine_answer(&shown.answer_with, &shown.offered.machines));
            }
            Some(AppDetail::FilePickers) => {
                sections.extend(
                    FileDialog::ALL.map(|dialog| self.file_picker(dialog, &shown.answer_with)),
                );
            }
            Some(AppDetail::SoundOutput) => {
                sections.push(self.output_answer(&shown.answer_with, &shown.offered.outputs));
            }
            Some(AppDetail::Links) => sections.extend(self.link_schemes(&shown.link_schemes)),
            Some(AppDetail::Roles) => sections.extend(self.roles(&shown.roles)),
            None => {}
        }
        if self.shows(Permission::Network) {
            sections.extend(self.network(&shown.network));
        }
        if self.shows(Permission::LocalNetwork) {
            sections.push(self.switch(
                words::LOCAL_NETWORK,
                &shown.local_network,
                Which::LocalNetwork,
                SettingChange::LocalNetwork,
            ));
        }
        if self.shows(Permission::Clipboard) {
            sections.push(self.switch(
                words::CLIPBOARD_READ,
                &shown.clipboard_read,
                Which::ClipboardRead,
                SettingChange::ClipboardRead,
            ));
            sections.push(self.switch(
                words::CLIPBOARD_WRITE,
                &shown.clipboard_write,
                Which::ClipboardWrite,
                SettingChange::ClipboardWrite,
            ));
        }
        if self.shows(Permission::ActivityRecord) {
            sections.push(self.record(&shown.record));
            if let Some(activity) = activity {
                sections.push(last_days(activity, shown.record.value));
            }
        }
        if self.shows(Permission::Folders) {
            sections.push(self.folders(&shown.file_system));
            if denied.folders {
                sections.push(self.folders_denied_by_system());
            }
        }
        for question in Question::ALL {
            if !self.shows(Permission::of_question(question)) {
                continue;
            }
            let refused = question == Question::Notifications && denied.notifications;
            sections.extend(self.question(question, shown, refused));
        }
        sections
    }

    /// The program's answers to opening links of its own schemes, each with Remove; nothing
    /// before it asked to open any.
    fn link_schemes(&self, link_schemes: &[(String, Answer)]) -> Option<WidgetRef> {
        if link_schemes.is_empty() {
            return None;
        }
        let tiles = link_schemes
            .iter()
            .map(|(scheme, answer)| {
                let forget = SettingChange::LinkScheme(scheme.clone(), None);
                self.listed(&words::link_scheme(scheme, *answer), forget)
            })
            .collect();
        Some(section(
            Some(words::LINKS),
            Some(words::LINKS_DETAIL),
            tiles,
        ))
    }

    /// The program's answers to holding the roles it asked for, each with Remove; nothing before
    /// it asked for one.
    fn roles(&self, roles: &[(Role, Answer)]) -> Option<WidgetRef> {
        if roles.is_empty() {
            return None;
        }
        let tiles = roles
            .iter()
            .map(|(role, answer)| {
                let forget = SettingChange::Role(*role, None);
                self.listed(&words::role(*role, *answer), forget)
            })
            .collect();
        Some(section(
            Some(words::ROLES),
            Some(words::ROLES_DETAIL),
            tiles,
        ))
    }

    /// Ask, or one of the machines offered, the one set among them, by its id when it is not
    /// offered now.
    fn machine_answer(&self, set: &ShownAnswerWith, offered: &[waos::MachineChoice]) -> WidgetRef {
        let set_id = set.machine.as_ref().map(|machine| machine.id.as_str());
        let mut tiles = vec![self.checked(
            words::ASK,
            set_id.is_none(),
            SettingChange::MachineAnswer(None),
        )];
        for machine in offered {
            let change = SettingChange::MachineAnswer(Some(machine.id.clone()));
            tiles.push(self.checked(&machine.name, set_id == Some(machine.id.as_str()), change));
        }
        if let Some(machine) = &set.machine
            && !offered.iter().any(|each| each.id == machine.id)
        {
            let change = SettingChange::MachineAnswer(Some(machine.id.clone()));
            tiles.push(self.checked(&words::not_offered(&machine.name), true, change));
        }
        section(Some(words::WHICH_MACHINE), None, tiles)
    }

    /// Ask, or one of the outputs offered, the one set among them, by its id when it is not
    /// connected now.
    fn output_answer(&self, set: &ShownAnswerWith, offered: &[waos::AudioOutput]) -> WidgetRef {
        let set_id = set.audio_output.as_ref().map(|output| output.id.as_str());
        let mut tiles = vec![self.checked(
            words::ASK,
            set_id.is_none(),
            SettingChange::AudioOutputAnswer(None),
        )];
        for output in offered {
            let change = SettingChange::AudioOutputAnswer(Some(output.id.clone()));
            tiles.push(self.checked(&output.label, set_id == Some(output.id.as_str()), change));
        }
        if let Some(output) = &set.audio_output
            && !offered.iter().any(|each| each.id == output.id)
        {
            let change = SettingChange::AudioOutputAnswer(Some(output.id.clone()));
            tiles.push(self.checked(&words::not_offered(&output.label), true, change));
        }
        section(None, Some(words::SOUND_OUTPUT_DETAIL), tiles)
    }

    /// One of the app's file pickers: Ask, which shows the picker to the user, or Simulated,
    /// under which the path the app is given is set, as a simulated camera says what it shows.
    fn file_picker(&self, dialog: FileDialog, set: &ShownAnswerWith) -> WidgetRef {
        let paths = set.paths.file_dialog(dialog).unwrap_or_default();
        let simulated = !paths.is_empty() || self.simulating.contains(&dialog);
        let mut tiles = vec![
            self.picker_choice(words::ASK, !simulated, dialog, false),
            self.picker_choice(words::SIMULATED, simulated, dialog, true),
        ];
        if !simulated {
            return section(Some(words::file_dialog_name(dialog)), None, tiles);
        }
        let placeholder = words::file_dialog_placeholder(dialog);
        let typed = self.typed.path(dialog).clone();
        if dialog == FileDialog::OpenFiles {
            for (index, path) in paths.iter().enumerate() {
                let remove = SettingChange::RemoveFileToOpen(index);
                tiles.push(self.listed(&path.to_string_lossy(), remove));
            }
            tiles.push(self.adding(typed, placeholder, words::ADD, SettingChange::AddFileToOpen));
        } else {
            match paths.first() {
                Some(path) => {
                    // Another path is typed in the field, which shows once this one is let go.
                    let (state, ask) = (
                        self.state,
                        self.on(SettingChange::FileDialogAnswer(dialog, Vec::new())),
                    );
                    let change = Listener::new(move |app| {
                        state.simulate(app, dialog, true);
                        ask.call(app);
                    });
                    tiles.push(self.listed_with(&path.to_string_lossy(), words::CHANGE, change));
                }
                None => {
                    let answer =
                        move |typed: String| SettingChange::FileDialogAnswer(dialog, vec![typed]);
                    tiles.push(self.adding(typed, placeholder, words::SET, answer));
                }
            }
        }
        section(
            Some(words::file_dialog_name(dialog)),
            Some(words::SIMULATED_PICKER_DETAIL),
            tiles,
        )
    }

    /// A choice of a file picker, Ask or Simulated, checked where it is `chosen`: Simulated
    /// shows the path's field, Ask lets go of the path set.
    fn picker_choice(
        &self,
        label: &str,
        chosen: bool,
        dialog: FileDialog,
        simulate: bool,
    ) -> WidgetRef {
        let (state, ask) = (
            self.state,
            self.on(SettingChange::FileDialogAnswer(dialog, Vec::new())),
        );
        let choose = Rc::new(move |app: &mut App| {
            state.simulate(app, dialog, simulate);
            if !simulate {
                ask.call(app);
            }
            Task::ready(())
        });
        let tile = CupertinoListTile::new(Text::new(label)).on_tap(choose);
        if chosen {
            tile.trailing(checkmark()).into_widget()
        } else {
            tile.into_widget()
        }
    }

    /// A row naming one choice, a check mark at its trailing side when it is the one `chosen`; a
    /// tap on it makes `change`.
    fn checked(&self, label: &str, chosen: bool, change: SettingChange) -> WidgetRef {
        let choose = self.on(change);
        let tile = CupertinoListTile::new(spanning(label)).on_tap(Rc::new(move |app: &mut App| {
            choose.call(app);
            Task::ready(())
        }));
        if chosen {
            tile.trailing(checkmark()).into_widget()
        } else {
            tile.into_widget()
        }
    }

    /// The machines the user picked for the program, each with Remove, under the warning.
    fn machines(&self, machines: &[waos::MachineChoice]) -> WidgetRef {
        let mut tiles: Vec<WidgetRef> = machines
            .iter()
            .map(|machine| {
                let remove = SettingChange::RemoveMachine(machine.id.clone());
                self.listed(&machine.name, remove)
            })
            .collect();
        if tiles.is_empty() {
            tiles.push(wrapped(words::NO_MACHINES));
        }
        section(Some(words::MACHINES), Some(words::MACHINES_AS_YOU), tiles)
    }

    /// The network's switch, its rule of domains beneath it.
    /// The network's switch; and, while it is on, which domains, as a list of its own, with the
    /// domains named where the choice names some.
    fn network(&self, shown: &Shown<NetworkGrant>) -> Vec<WidgetRef> {
        let network = &shown.value;
        let switch = vec![self.switch_row(words::NETWORK, network.on, SettingChange::NetworkOn)];
        let mut sections =
            vec![self.setting(None, None, shown.follows_global, Which::Network, switch)];
        if !network.on {
            return sections;
        }
        let choices = [
            (DomainsKind::Any, words::ANY_DOMAIN),
            (DomainsKind::Only, words::ONLY_DOMAINS),
            (DomainsKind::AnyBut, words::ANY_DOMAIN_BUT),
        ];
        let mut tiles = self.checklist(
            &choices,
            domains_kind(&network.domains),
            SettingChange::Domains,
        );
        if let DomainRule::Only(domains) | DomainRule::AnyBut(domains) = &network.domains {
            for domain in domains {
                tiles.push(
                    self.listed(domain.as_str(), SettingChange::RemoveDomain(domain.clone())),
                );
            }
            tiles.push(self.adding(
                self.typed.domain.clone(),
                words::DOMAIN_PLACEHOLDER,
                words::ADD,
                SettingChange::AddDomain,
            ));
        }
        sections.push(section(Some(words::WHICH_DOMAINS), None, tiles));
        sections
    }

    /// A setting that is only on or off: a section of its switch's row alone, which names it.
    fn switch(
        &self,
        name: &str,
        shown: &Shown<bool>,
        which: Which,
        change: fn(bool) -> SettingChange,
    ) -> WidgetRef {
        let tiles = vec![self.switch_row(name, shown.value, change)];
        self.setting(None, None, shown.follows_global, which, tiles)
    }

    /// Whether a record is kept of what the program does, over what the record is.
    fn record(&self, shown: &Shown<bool>) -> WidgetRef {
        let (name, detail) = words::record(&self.of);
        let tiles = vec![self.switch_row(name, shown.value, SettingChange::Record)];
        self.setting(
            None,
            Some(detail),
            shown.follows_global,
            Which::Record,
            tiles,
        )
    }

    /// The folders mounted, each with Remove, and the rows that add one, over when a change
    /// applies.
    fn folders(&self, shown: &Shown<FileSystemGrant>) -> WidgetRef {
        let mut tiles = Vec::new();
        for (index, mount) in shown.value.mounts.iter().enumerate() {
            let said = words::mount(
                &mount.host.to_string_lossy(),
                &mount.at.to_string_lossy(),
                mount.writable,
            );
            tiles.push(self.listed(&said, SettingChange::RemoveMount(index)));
        }
        tiles.extend(self.adding_folder());
        self.setting(
            Some(words::FOLDERS),
            Some(words::FOLDERS_AT_ONCE),
            shown.follows_global,
            Which::FileSystem,
            tiles,
        )
    }

    /// The answer `question` gets, over what the device of WaOS's own is while it is simulated;
    /// under a simulated microphone, what it records; and, where the system `refused` the
    /// program it, the row that opens the system's settings at it.
    fn question(
        &self,
        question: Question,
        settings: &ShownSettings,
        refused: bool,
    ) -> Vec<WidgetRef> {
        let shown = settings.answers.get(question);
        // A choice is an answer, or, on an app's page, following the default: the first row,
        // named with the default's answer, as a browser's site settings name it.
        let global = self.of == SettingsOf::Global;
        let default = self.defaults.as_ref().map_or(words::ASK, |defaults| {
            words::answer_said(defaults.answers.get(question).value)
        });
        let defaulted = words::defaulted(default);
        let mut choices: Vec<(Option<Option<Answer>>, &str)> = Vec::new();
        if global {
            choices.push((Some(None), words::ASK));
        } else {
            choices.push((None, &defaulted));
        }
        choices.push((Some(Some(Answer::Granted)), words::ALLOW));
        choices.push((Some(Some(Answer::Denied)), words::DONT_ALLOW));
        if simulatable(question) {
            choices.push((Some(Some(Answer::Simulated)), words::SIMULATED));
        }
        let answer = shown.value;
        let chosen = if global || !shown.follows_global {
            Some(answer)
        } else {
            None
        };
        let tiles = self.checklist(&choices, chosen, move |choice| match choice {
            Some(answer) => SettingChange::Answer(question, answer),
            None => SettingChange::FollowGlobal(Which::Answer(question)),
        });
        let simulated = answer == Some(Answer::Simulated);
        let footer = simulated.then(|| simulated_detail(question)).flatten();
        let mut sections = vec![section(Some(words::question_name(question)), footer, tiles)];
        if simulated && question == Question::Microphone {
            sections.push(self.simulated_records(&settings.simulated_microphone));
        }
        if refused {
            sections.extend(self.refused_by_system());
        }
        sections
    }

    /// What a simulated microphone records, `sound` now, a list of its own; on an app's page
    /// that it follows the default, or the row that has it follow it again.
    fn simulated_records(&self, sound: &Shown<SimulatedSound>) -> WidgetRef {
        let choices = [
            (SimulatedSound::Tone, words::QUIET_TONE),
            (SimulatedSound::Silence, words::SILENCE),
        ];
        let tiles = self.checklist(&choices, sound.value, SettingChange::SimulatedMicrophone);
        self.setting(
            Some(words::SIMULATED_RECORDS),
            None,
            sound.follows_global,
            Which::SimulatedMicrophone,
            tiles,
        )
    }

    /// The row that opens the system's settings at the program's notifications, over that the
    /// system refuses it them.
    fn refused_by_system(&self) -> Option<WidgetRef> {
        let SettingsOf::Program(id) = &self.of else {
            return None;
        };
        let (library, id) = (self.library.clone(), id.clone());
        let open = CupertinoListTile::new(tinted(words::OPEN_SYSTEM_SETTINGS)).on_tap(Rc::new(
            move |app: &mut App| {
                library.update(app, |library, cx| {
                    library.open_notification_settings(&id, cx)
                });
                Task::ready(())
            },
        ));
        Some(section(
            None,
            Some(words::NOTIFICATIONS_REFUSED),
            vec![open.into_widget()],
        ))
    }

    /// The row that opens the system's settings where the user lets WaOS reach the phone's
    /// folders, over that the system keeps WaOS from them.
    fn folders_denied_by_system(&self) -> WidgetRef {
        let library = self.library.clone();
        let open = CupertinoListTile::new(tinted(words::OPEN_ANDROID_SETTINGS)).on_tap(Rc::new(
            move |app: &mut App| {
                library.update(app, |library, cx| library.open_phone_files_page(cx));
                Task::ready(())
            },
        ));
        section(None, Some(words::FOLDERS_DENIED), vec![open.into_widget()])
    }

    /// A setting's section, under `header` and over `footer` where given: on an app's page, its
    /// footer says too that it follows the default, or its last row has it follow it again.
    fn setting(
        &self,
        header: Option<&str>,
        footer: Option<&str>,
        follows_global: bool,
        which: Which,
        mut tiles: Vec<WidgetRef>,
    ) -> WidgetRef {
        if self.of == SettingsOf::Global {
            return section(header, footer, tiles);
        }
        if follows_global {
            let footer = said_with(footer, Some(words::FOLLOWS_GLOBAL));
            return section(header, footer.as_deref(), tiles);
        }
        tiles.push(self.use_the_default(which));
        section(header, footer, tiles)
    }

    /// The row that has the page's app follow the default for `which` again, in the tint
    /// colour, as iOS's rows that act are.
    fn use_the_default(&self, which: Which) -> WidgetRef {
        let follow = self.on(SettingChange::FollowGlobal(which));
        CupertinoListTile::new(tinted(words::USE_GLOBAL))
            .on_tap(Rc::new(move |app: &mut App| {
                follow.call(app);
                Task::ready(())
            }))
            .into_widget()
    }

    /// A row naming a setting that is `on` or off, its switch at the trailing side; a tap on the
    /// row flips the setting as a tap on the switch does.
    fn switch_row(&self, name: &str, on: bool, change: fn(bool) -> SettingChange) -> WidgetRef {
        let flip = self.changing(change);
        let row_flip = Rc::clone(&flip);
        CupertinoListTile::new(spanning(name))
            .trailing(CupertinoSwitch::new(on, Some(flip)))
            .on_tap(Rc::new(move |app: &mut App| {
                row_flip(app, !on);
                Task::ready(())
            }))
            .into_widget()
    }

    /// What makes `change` of a value to the page's settings.
    fn changing(&self, change: fn(bool) -> SettingChange) -> ValueChanged<bool> {
        let (library, of) = (self.library.clone(), self.of.clone());
        Rc::new(move |app: &mut App, value: bool| {
            library.update(app, |library, cx| {
                library.change_settings(&of, change(value), cx)
            });
        })
    }

    /// A tile choosing one of `choices`, `selected` the one chosen, or none of them when it is
    /// none of them, each choice changing the settings as `change` makes it.
    /// A row for each of `choices`, a checkmark on `selected`'s, as iOS's Settings lists a
    /// choice among named things; a tap on a row chooses it, the settings changing as `change`
    /// makes it.
    fn checklist<T: Copy + Eq + Debug + 'static>(
        &self,
        choices: &[(T, &str)],
        selected: T,
        change: impl Fn(T) -> SettingChange + 'static,
    ) -> Vec<WidgetRef> {
        let change = Rc::new(change);
        choices
            .iter()
            .map(|&(value, label)| {
                let (library, of, change) =
                    (self.library.clone(), self.of.clone(), Rc::clone(&change));
                let choose = Rc::new(move |app: &mut App| {
                    if value != selected {
                        library.update(app, |library, cx| {
                            library.change_settings(&of, change(value), cx)
                        });
                    }
                    Task::ready(())
                });
                let tile = CupertinoListTile::new(Text::new(label)).on_tap(choose);
                if value == selected {
                    tile.trailing(checkmark())
                } else {
                    tile
                }
                .into_widget()
            })
            .collect()
    }

    /// One of a rule's list, with Remove.
    fn listed(&self, text: &str, remove: SettingChange) -> WidgetRef {
        self.listed_with(text, words::REMOVE, self.on(remove))
    }

    /// `text`, with a button `label` doing `tap`.
    fn listed_with(&self, text: &str, label: &str, tap: Listener) -> WidgetRef {
        CupertinoListTile::new(Text::new(text))
            .trailing(CupertinoButton::new(
                Text::new(label).into_widget(),
                Some(tap),
            ))
            .into_widget()
    }

    /// A field to type one more of a list in, and a button `confirm`, which makes `change` of it
    /// and clears the field once it is made.
    fn adding(
        &self,
        typed: Ref<TextEditingController>,
        placeholder: &str,
        confirm: &str,
        change: impl Fn(String) -> SettingChange + 'static,
    ) -> WidgetRef {
        let (library, of) = (self.library.clone(), self.of.clone());
        let controller = typed.erase();
        let add = Listener::new(move |app| {
            let text = typed.text_value(app).to_owned();
            library.update(app, |library, cx| {
                library.change_settings(&of, change(text), cx)
            });
            if library.read(app).problem().is_none() {
                typed.clear(app);
            }
        });
        CupertinoListTile::new(typed_in_a_row(controller, placeholder))
            .trailing(CupertinoButton::new(
                Text::new(confirm).into_widget(),
                Some(add),
            ))
            .into_widget()
    }

    /// The rows that add a folder: its path, where the app sees it, whether it is writable, and
    /// the row that adds it.
    fn adding_folder(&self) -> Vec<WidgetRef> {
        let (library, of, typed, writable) = (
            self.library.clone(),
            self.of.clone(),
            self.typed.clone(),
            self.writable,
        );
        let (folder, folder_at) = (typed.folder.erase(), typed.folder_at.erase());
        let add = Listener::new(move |app| {
            let change = SettingChange::AddMount {
                host: typed.folder.text_value(app).to_owned(),
                at: typed.folder_at.text_value(app).to_owned(),
                writable,
            };
            library.update(app, |library, cx| library.change_settings(&of, change, cx));
            if library.read(app).problem().is_none() {
                typed.folder.clear(app);
                typed.folder_at.clear(app);
            }
        });
        let state = self.state;
        let set_writable: ValueChanged<bool> =
            Rc::new(move |app: &mut App, writable: bool| state.set_writable(app, writable));
        let flip = Rc::clone(&set_writable);
        let writable = self.writable;
        vec![
            CupertinoListTile::new(typed_in_a_row(folder, words::FOLDER_PLACEHOLDER)).into_widget(),
            CupertinoListTile::new(typed_in_a_row(folder_at, words::FOLDER_AT_PLACEHOLDER))
                .into_widget(),
            CupertinoListTile::new(spanning(words::WRITABLE))
                .trailing(CupertinoSwitch::new(writable, Some(set_writable)))
                .on_tap(Rc::new(move |app: &mut App| {
                    flip(app, !writable);
                    Task::ready(())
                }))
                .into_widget(),
            CupertinoListTile::new(tinted(words::ADD_FOLDER))
                .on_tap(Rc::new(move |app: &mut App| {
                    add.call(app);
                    Task::ready(())
                }))
                .into_widget(),
        ]
    }

    /// A tap that makes `change` to the page's settings.
    fn on(&self, change: SettingChange) -> Listener {
        let (library, of) = (self.library.clone(), self.of.clone());
        Listener::new(move |app| {
            let change = change.clone();
            library.update(app, |library, cx| library.change_settings(&of, change, cx));
        })
    }
}

/// The side of an app's icon over its settings.
const APP_ICON_SIDE: f64 = 64.0;

/// An inset-grouped section of rows that start with their words, titled `title` and with
/// `footer` under it where given.
fn section(title: Option<&str>, footer: Option<&str>, tiles: Vec<WidgetRef>) -> WidgetRef {
    grouped(title, footer, tiles, false)
}

/// [`section`], of rows that each start with an icon: the line between two rows starts where
/// their words do, as iOS's does.
fn section_of_icons(title: Option<&str>, footer: Option<&str>, tiles: Vec<WidgetRef>) -> WidgetRef {
    grouped(title, footer, tiles, true)
}

/// An inset-grouped section, whose rows start with an icon or not.
fn grouped(
    title: Option<&str>,
    footer: Option<&str>,
    tiles: Vec<WidgetRef>,
    has_leading: bool,
) -> WidgetRef {
    let section = CupertinoListSection::inset_grouped()
        .has_leading(has_leading)
        .children(tiles);
    let section = match title {
        Some(title) => section.header(footnote(title)),
        None => section,
    };
    match footer {
        Some(footer) => section.footer(footnote(footer)).into_widget(),
        None => section.into_widget(),
    }
}

/// The words of a row that acts, in the tint colour, as iOS's rows that act are.
fn tinted(text: &str) -> WidgetRef {
    colored(text, CupertinoColors::ACTIVE_BLUE)
}

/// `text` in `color`, as it is in the light or the dark.
fn colored(text: &str, color: AnyColor) -> WidgetRef {
    let text = text.to_owned();
    Builder::new(move |app, context| {
        let color = resolved(color.clone(), app, context);
        Text::new(text.clone())
            .style(waft::TextStyle::new().color(color))
            .into_widget()
    })
    .into_widget()
}

/// The checkmark on the row chosen, in the tint colour.
fn checkmark() -> WidgetRef {
    Builder::new(|app, context| {
        Icon::new(Some(CupertinoIcons::check_mark()))
            .color(resolved(CupertinoColors::ACTIVE_BLUE, app, context))
            .into_widget()
    })
    .into_widget()
}

/// `text` across the whole width it is given: a list tile cuts its title to one line, and under a
/// loose width Valo measures a text of three words or more too long for that line as nothing, so
/// such a title, as "This computer and the local network" on a narrow phone, would draw blank.
fn spanning(text: &str) -> WidgetRef {
    SizedBox::new()
        .width(f64::INFINITY)
        .child(Text::new(text))
        .into_widget()
}

/// What the program did over the days kept of it, a line per capability; that none is kept while
/// its record is off.
fn last_days(activity: &Activity, recorded: bool) -> WidgetRef {
    let lines = activity.short();
    let tiles = if !recorded {
        vec![wrapped(words::NOTHING_RECORDED)]
    } else if lines.is_empty() {
        vec![wrapped(words::NOTHING_USED)]
    } else {
        lines.iter().map(|line| wrapped(line)).collect()
    };
    section(Some(words::LAST_DAYS), None, tiles)
}

/// `text` in a list, on as many lines as it takes, where a row's title is cut to one: a file's
/// path, a site's address.
fn wrapped(text: &str) -> WidgetRef {
    Padding::new(EdgeInsetsGeometry::symmetric(11.0, 20.0))
        .child(SizedBox::new().width(f64::INFINITY).child(Text::new(text)))
        .into_widget()
}

/// `text` over the page's lists, as a list's footer says it, in from the page's side.
fn said_over_the_lists(text: &str) -> WidgetRef {
    Padding::new(EdgeInsetsGeometry::from_ltrb(20.0, 16.0, 20.0, 0.0))
        .child(footnote(text))
        .into_widget()
}

/// A field typed in as a row of a list, as iOS's Settings has one: no box of its own, its
/// words where a row's are.
fn typed_in_a_row(
    controller: waft::TextEditingControllerRef,
    placeholder: &str,
) -> CupertinoTextField {
    CupertinoTextField::borderless()
        .controller(controller)
        .placeholder(placeholder)
        .padding(EdgeInsetsGeometry::all(0.0))
}
