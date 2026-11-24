//! A settings page at a desktop's width, in Fluent's controls, made of cards as Windows's
//! Settings is (`fluent_cards`): Back and the page's title over what scrolls. It is one of:
//! a permission's page, its default over the apps set otherwise; an app's page, its permissions
//! each with what it holds, then what is the app's alone, each leading to a page; an app's page
//! for one permission, which follows the default or holds its own; and a page of what is the
//! app's alone: its machines, its file pickers, its sound output, its links or its roles. A
//! choice among named things is a drop-down in its card; a list, such as a rule's domains, is
//! rows of the card under it, with the field that adds one. While the system refuses an app
//! its notifications, their card says so, with a button that opens the system's settings.
//!
//! What the page keeps of its own is what is being typed: a domain, a folder and where it is
//! seen, whether it is writable, and a path for each file picker; and which file pickers the
//! user chose to simulate and has set no path for yet.

use std::rc::Rc;

use waft::{
    App, BuildContext, Column, CrossAxisAlignment, EdgeInsetsGeometry, Entity, Expanded, Handle,
    IntoWidget, ListView, Listener, MainAxisSize, Padding, Ref, Row, State, StateData,
    StatefulWidget, Text, TextEditingController, TextEditingControllerLeaf, WidgetRef,
};
use waft_winui::{
    Button, CheckBox, ComboBox, ComboBoxItem, InfoBar, InfoBarSeverity, TextBox, ThemeResources,
};
use waos::{FileDialog, SimulatedSound};
use waos_activity::Activity;
use waos_store::{Answer, DomainRule, Question, Role};

use super::fluent_cards::{card, card_row, card_switch, note};
use super::fluent_root::{MARK_SIDE, heading, link_card, permission_symbol, symbol, title_bar};
use super::{any_set_otherwise, app_details, app_permissions, reset_permissions, set_otherwise};
use crate::logic::permissions::Permission;
use crate::logic::setting_changes::{
    DomainsKind, SettingChange, SettingsOf, Shown, ShownAnswerWith, ShownSettings, Which,
    domains_kind, simulatable,
};
use crate::state::SIMULATED_CAMERA;
use crate::state::library::{AppDetail, Library, SettingsPage};
use crate::ui::library::home::fluent::MARGIN as PAGE_MARGIN;
use crate::ui::library::program_icon::ProgramIcon;
use crate::ui::library::words;
use waft::SizedBox;

const GAP: f64 = 8.0;

/// How wide a card's drop-down is, so the drop-downs of a page line up.
const DROP_DOWN_WIDTH: f64 = 200.0;

/// The settings `of` shows.
#[derive(Debug)]
pub(crate) struct FluentSettings {
    pub(crate) library: Entity<Library>,
    pub(crate) of: SettingsOf,
    /// The page's title: the program's name, or none on the global page.
    pub(crate) program_name: Option<String>,
    /// The kind of permission whose settings alone the page shows, with the apps that set it
    /// otherwise: a kind's page, showing its default; none for an app's page, which shows all.
    pub(crate) only: Option<Permission>,
    /// What of the app's alone the page shows, on its page of it; none elsewhere.
    pub(crate) detail: Option<AppDetail>,
}

impl StatefulWidget for FluentSettings {
    type State = FluentSettingsState;

    fn create_state(&self) -> FluentSettingsState {
        FluentSettingsState {
            state: StateData::new(),
            typed: None,
            writable: false,
            simulating: Vec::new(),
        }
    }
}

pub(crate) struct FluentSettingsState {
    state: StateData<FluentSettings>,
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

impl State for FluentSettingsState {
    type Widget = FluentSettings;
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

    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let widget = self.widget(app);
        let page = Page {
            library: widget.library.clone(),
            of: widget.of.clone(),
            only: widget.only,
            detail: widget.detail,
            defaults: widget.library.read(app).settings(&SettingsOf::Global).ok(),
            resources,
            typed: app.get(self).typed.clone().expect("an initialized page"),
            writable: app.get(self).writable,
            simulating: app.get(self).simulating.clone(),
            state: self,
        };
        let title = page.title(widget.program_name.as_deref());
        let mut rows = page.over_the_default();
        let set_otherwise = page.kind_page().map(|only| page.set_otherwise(app, only));
        rows.extend(page.switch_asked(app));
        match page.library.read(app).settings(&page.of) {
            Ok(shown) => {
                let activity = page.library.read(app).activity(&page.of);
                let refused = page.notifications_refused(app);
                rows.extend(page.sections(&shown, activity.as_ref(), refused));
            }
            Err(why) => rows.push(note(&page.resources, &why)),
        }
        rows.extend(set_otherwise.into_iter().flatten());
        let list = ListView::new()
            .padding(EdgeInsetsGeometry::from_ltrb(
                PAGE_MARGIN,
                8.0,
                PAGE_MARGIN,
                PAGE_MARGIN,
            ))
            .children(rows);
        Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children([
                title_bar(&page.library, &page.resources, &title),
                Expanded::new(list).into_widget(),
            ])
            .into_widget()
    }
}

impl FluentSettingsState {
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
    resources: Rc<ThemeResources>,
    typed: Typed,
    writable: bool,
    simulating: Vec<FileDialog>,
    state: Handle<FluentSettingsState>,
}

impl Page {
    /// The page's title: the permission's, what of the app's alone it shows, or the app's name.
    fn title(&self, program_name: Option<&str>) -> String {
        match (self.detail, self.only, program_name) {
            (Some(detail), ..) => words::app_detail_name(detail).to_owned(),
            (None, Some(only), _) => words::permission_name(only).to_owned(),
            (None, None, Some(name)) => name.to_owned(),
            (None, None, None) => words::PERMISSIONS.to_owned(),
        }
    }

    /// On a permission's page, what is over its default: that it is the default, and what a
    /// default is.
    fn over_the_default(&self) -> Vec<WidgetRef> {
        if self.kind_page().is_none() {
            return Vec::new();
        }
        vec![
            heading(words::DEFAULT, &self.resources),
            self.said(words::DEFAULT_DETAIL),
        ]
    }

    /// Whether the page shows the controls of `permission`: a kind's page, every app's or one
    /// app's, those of its kind alone.
    fn shows(&self, permission: Permission) -> bool {
        self.only == Some(permission)
    }

    /// The kind whose default the page shows; none on an app's pages.
    fn kind_page(&self) -> Option<Permission> {
        self.only.filter(|_| self.of == SettingsOf::Global)
    }

    /// On an app's page, each kind of permission with what the app holds, each leading to the
    /// app's page for that kind; and, where any is set otherwise, what resets them all.
    fn app_permissions(&self, shown: &ShownSettings) -> Vec<WidgetRef> {
        let SettingsOf::Program(id) = &self.of else {
            return Vec::new();
        };
        let mut rows = vec![heading(words::PERMISSIONS, &self.resources)];
        for (permission, said) in app_permissions(shown) {
            rows.push(link_card(
                &self.library,
                &self.resources,
                Some(symbol(permission_symbol(permission), &self.resources)),
                words::permission_name(permission),
                Some(&said),
                SettingsPage::AppPermission(id.clone(), permission),
            ));
        }
        if any_set_otherwise(shown) {
            rows.push(SizedBox::new().height(GAP).into_widget());
            rows.push(
                Row::new()
                    .children([
                        Button::text(words::RESET_PERMISSIONS, self.resetting()).into_widget()
                    ])
                    .into_widget(),
            );
            rows.push(note(&self.resources, words::RESET_PERMISSIONS_DETAIL));
        }
        rows
    }

    /// On an app's page, what is the app's alone, each with how many it has, each leading to its
    /// page.
    fn app_details(&self, shown: &ShownSettings) -> Vec<WidgetRef> {
        let SettingsOf::Program(id) = &self.of else {
            return Vec::new();
        };
        let mut rows = vec![SizedBox::new().height(PAGE_MARGIN).into_widget()];
        for (detail, said) in app_details(shown) {
            rows.push(link_card(
                &self.library,
                &self.resources,
                None,
                words::app_detail_name(detail),
                Some(&said),
                SettingsPage::AppDetail(id.clone(), detail),
            ));
        }
        rows
    }

    /// What puts every kind of permission of the page's app back on its default.
    fn resetting(&self) -> Listener {
        let (library, of) = (self.library.clone(), self.of.clone());
        Listener::new(move |app| {
            library.update(app, |library, cx| {
                for change in reset_permissions() {
                    library.change_settings(&of, change, cx);
                }
            });
        })
    }

    /// On a kind's page, the apps that set `permission` otherwise than its default, each with
    /// what it set and leading to its page; one line where every app follows the default.
    fn set_otherwise(&self, app: &App, permission: Permission) -> Vec<WidgetRef> {
        let mut rows = vec![
            heading(words::SET_OTHERWISE, &self.resources),
            note(&self.resources, words::SET_OTHERWISE_DETAIL),
        ];
        let apps = set_otherwise(self.library.read(app), permission);
        if apps.is_empty() {
            rows.push(note(&self.resources, words::NONE_SET_OTHERWISE));
        }
        rows.push(SizedBox::new().height(GAP).into_widget());
        for (listed, value) in apps {
            let icon = ProgramIcon {
                png: listed.icon,
                side: MARK_SIDE,
            };
            rows.push(link_card(
                &self.library,
                &self.resources,
                Some(icon.into_widget()),
                &listed.name,
                Some(&value),
                SettingsPage::App(listed.id),
            ));
        }
        rows
    }

    /// What a program asked for, when it asked for one of its switches here.
    fn switch_asked(&self, app: &App) -> Option<WidgetRef> {
        let asked = self.library.read(app).switch_asked()?;
        let message = words::switch_asked(&asked.name, asked.switch);
        let bar = InfoBar::new(true, |_, _| {})
            .severity(InfoBarSeverity::Informational)
            .message(message)
            .is_closable(false);
        Some(
            Padding::new(EdgeInsetsGeometry::symmetric(GAP, 0.0))
                .child(bar)
                .into_widget(),
        )
    }

    /// Whether the system refuses the page's program its notifications; never on the global
    /// page.
    fn notifications_refused(&self, app: &App) -> bool {
        match &self.of {
            SettingsOf::Program(id) => self.library.read(app).notifications_refused(id),
            SettingsOf::Global => false,
        }
    }

    fn sections(
        &self,
        shown: &ShownSettings,
        activity: Option<&Activity>,
        notifications_refused: bool,
    ) -> Vec<WidgetRef> {
        let mut sections = Vec::new();
        if self.only.is_none() && self.detail.is_none() {
            sections.extend(self.app_permissions(shown));
            sections.extend(self.app_details(shown));
        }
        match self.detail {
            Some(AppDetail::Machines) => {
                sections.extend(self.machines(&shown.machines));
                sections.extend(self.machine_answer(&shown.answer_with, &shown.offered.machines));
            }
            Some(AppDetail::FilePickers) => {
                for dialog in FileDialog::ALL {
                    sections.extend(self.file_picker(dialog, &shown.answer_with));
                }
            }
            Some(AppDetail::SoundOutput) => {
                sections.extend(self.output_answer(&shown.answer_with, &shown.offered.outputs));
            }
            Some(AppDetail::Links) => sections.extend(self.link_schemes(&shown.link_schemes)),
            Some(AppDetail::Roles) => sections.extend(self.roles(&shown.roles)),
            None => {}
        }
        if self.shows(Permission::Network) {
            sections.extend(self.network(&shown.network));
        }
        if self.shows(Permission::LocalNetwork) {
            sections.extend(self.switch(
                Permission::LocalNetwork,
                words::LOCAL_NETWORK,
                &shown.local_network,
                Which::LocalNetwork,
                SettingChange::LocalNetwork,
            ));
        }
        if self.shows(Permission::Clipboard) {
            sections.extend(self.switch(
                Permission::Clipboard,
                words::CLIPBOARD_READ,
                &shown.clipboard_read,
                Which::ClipboardRead,
                SettingChange::ClipboardRead,
            ));
            sections.extend(self.switch(
                Permission::Clipboard,
                words::CLIPBOARD_WRITE,
                &shown.clipboard_write,
                Which::ClipboardWrite,
                SettingChange::ClipboardWrite,
            ));
        }
        if self.shows(Permission::ActivityRecord) {
            sections.extend(self.record(&shown.record));
            if let Some(activity) = activity {
                sections.extend(self.last_days(activity, shown.record.value));
            }
        }
        if self.shows(Permission::Folders) {
            sections.extend(self.folders(&shown.file_system));
        }
        for question in Question::ALL {
            if !self.shows(Permission::of_question(question)) {
                continue;
            }
            let refused = question == Question::Notifications && notifications_refused;
            sections.extend(self.question(question, shown, refused));
        }
        sections
    }

    /// The program's answers to opening links of its own schemes, each with Remove; nothing
    /// before it asked to open any.
    fn link_schemes(&self, link_schemes: &[(String, Answer)]) -> Vec<WidgetRef> {
        if link_schemes.is_empty() {
            return Vec::new();
        }
        let rows = link_schemes
            .iter()
            .map(|(scheme, answer)| {
                let forget = SettingChange::LinkScheme(scheme.clone(), None);
                self.listed(&words::link_scheme(scheme, *answer), forget)
            })
            .collect();
        vec![self.said(words::LINKS_DETAIL), card(&self.resources, rows)]
    }

    /// The program's answers to holding the roles it asked for, each with Remove; nothing before
    /// it asked for one.
    fn roles(&self, roles: &[(Role, Answer)]) -> Vec<WidgetRef> {
        if roles.is_empty() {
            return Vec::new();
        }
        let rows = roles
            .iter()
            .map(|(role, answer)| {
                let forget = SettingChange::Role(*role, None);
                self.listed(&words::role(*role, *answer), forget)
            })
            .collect();
        vec![self.said(words::ROLES_DETAIL), card(&self.resources, rows)]
    }

    /// Which computer the app is given when it asks for one: Ask, or one of the machines
    /// offered, the one set among them, by its id when it is not offered now.
    fn machine_answer(
        &self,
        set: &ShownAnswerWith,
        offered: &[waos::MachineChoice],
    ) -> Vec<WidgetRef> {
        let set_id = set.machine.as_ref().map(|machine| machine.id.as_str());
        let mut choices = vec![(
            words::ASK.to_owned(),
            set_id.is_none(),
            self.on(SettingChange::MachineAnswer(None)),
        )];
        for machine in offered {
            let change = SettingChange::MachineAnswer(Some(machine.id.clone()));
            choices.push((
                machine.name.clone(),
                set_id == Some(machine.id.as_str()),
                self.on(change),
            ));
        }
        if let Some(machine) = &set.machine
            && !offered.iter().any(|each| each.id == machine.id)
        {
            let change = SettingChange::MachineAnswer(Some(machine.id.clone()));
            choices.push((words::not_offered(&machine.name), true, self.on(change)));
        }
        let row = card_row(
            &self.resources,
            None,
            words::WHICH_MACHINE,
            None,
            Some(self.drop_down(choices)),
        );
        vec![card(&self.resources, vec![row])]
    }

    /// Which sound output the app plays to: Ask, or one of the outputs offered, the one set
    /// among them, by its id when it is not connected now.
    fn output_answer(
        &self,
        set: &ShownAnswerWith,
        offered: &[waos::AudioOutput],
    ) -> Vec<WidgetRef> {
        let set_id = set.audio_output.as_ref().map(|output| output.id.as_str());
        let mut choices = vec![(
            words::ASK.to_owned(),
            set_id.is_none(),
            self.on(SettingChange::AudioOutputAnswer(None)),
        )];
        for output in offered {
            let change = SettingChange::AudioOutputAnswer(Some(output.id.clone()));
            choices.push((
                output.label.clone(),
                set_id == Some(output.id.as_str()),
                self.on(change),
            ));
        }
        if let Some(output) = &set.audio_output
            && !offered.iter().any(|each| each.id == output.id)
        {
            let change = SettingChange::AudioOutputAnswer(Some(output.id.clone()));
            choices.push((words::not_offered(&output.label), true, self.on(change)));
        }
        let row = card_row(
            &self.resources,
            None,
            words::SOUND_OUTPUT,
            Some(words::SOUND_OUTPUT_DETAIL),
            Some(self.drop_down(choices)),
        );
        vec![card(&self.resources, vec![row])]
    }

    /// One of the app's file pickers, a card: Ask, which shows the picker to the user, or
    /// Simulated, under which the path the app is given is set, as a simulated camera says
    /// what it shows.
    fn file_picker(&self, dialog: FileDialog, set: &ShownAnswerWith) -> Vec<WidgetRef> {
        let paths = set.paths.file_dialog(dialog).unwrap_or_default();
        let simulated = !paths.is_empty() || self.simulating.contains(&dialog);
        let choice = |simulate: bool| {
            let (state, ask) = (
                self.state,
                self.on(SettingChange::FileDialogAnswer(dialog, Vec::new())),
            );
            Listener::new(move |app| {
                state.simulate(app, dialog, simulate);
                if !simulate {
                    ask.call(app);
                }
            })
        };
        let choices = vec![
            (words::ASK.to_owned(), !simulated, choice(false)),
            (words::SIMULATED.to_owned(), simulated, choice(true)),
        ];
        let mut rows = vec![card_row(
            &self.resources,
            None,
            words::file_dialog_name(dialog),
            simulated.then_some(words::SIMULATED_PICKER_DETAIL),
            Some(self.drop_down(choices)),
        )];
        if !simulated {
            return vec![card(&self.resources, rows)];
        }
        let placeholder = words::file_dialog_placeholder(dialog);
        let typed = self.typed.path(dialog).clone();
        if dialog == FileDialog::OpenFiles {
            for (index, path) in paths.iter().enumerate() {
                let remove = SettingChange::RemoveFileToOpen(index);
                rows.push(self.listed(&path.to_string_lossy(), remove));
            }
            rows.push(self.adding(typed, placeholder, words::ADD, SettingChange::AddFileToOpen));
            return vec![card(&self.resources, rows)];
        }
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
                rows.push(self.listed_with(&path.to_string_lossy(), words::CHANGE, change));
            }
            None => {
                let answer =
                    move |typed: String| SettingChange::FileDialogAnswer(dialog, vec![typed]);
                rows.push(self.adding(typed, placeholder, words::SET, answer));
            }
        }
        vec![card(&self.resources, rows)]
    }

    /// The machines the user picked for the program, each with Remove, under the warning.
    fn machines(&self, machines: &[waos::MachineChoice]) -> Vec<WidgetRef> {
        let rows: Vec<WidgetRef> = if machines.is_empty() {
            vec![note(&self.resources, words::NO_MACHINES)]
        } else {
            machines
                .iter()
                .map(|machine| {
                    let remove = SettingChange::RemoveMachine(machine.id.clone());
                    self.listed(&machine.name, remove)
                })
                .collect()
        };
        vec![
            self.said(words::MACHINES_AS_YOU),
            card(&self.resources, rows),
        ]
    }

    /// The network's card: its switch; while it is on, which domains; and the domains named
    /// where the choice names some, with the field that adds one.
    fn network(&self, shown: &Shown<waos_store::NetworkGrant>) -> Vec<WidgetRef> {
        let network = &shown.value;
        let mut rows = vec![self.switch_row(
            Permission::Network,
            words::NETWORK,
            None,
            shown.follows_global,
            Which::Network,
            network.on,
            SettingChange::NetworkOn,
        )];
        if !network.on {
            return vec![card(&self.resources, rows)];
        }
        let kind = domains_kind(&network.domains);
        let choice = |label: &str, of: DomainsKind| {
            (
                label.to_owned(),
                kind == of,
                self.on(SettingChange::Domains(of)),
            )
        };
        rows.push(card_row(
            &self.resources,
            None,
            words::WHICH_DOMAINS,
            None,
            Some(self.drop_down(vec![
                choice(words::ANY_DOMAIN, DomainsKind::Any),
                choice(words::ONLY_DOMAINS, DomainsKind::Only),
                choice(words::ANY_DOMAIN_BUT, DomainsKind::AnyBut),
            ])),
        ));
        if let DomainRule::Only(domains) | DomainRule::AnyBut(domains) = &network.domains {
            for domain in domains {
                let remove = SettingChange::RemoveDomain(domain.clone());
                rows.push(self.listed(domain.as_str(), remove));
            }
            rows.push(self.adding(
                self.typed.domain.clone(),
                words::DOMAIN_PLACEHOLDER,
                words::ADD,
                SettingChange::AddDomain,
            ));
        }
        vec![card(&self.resources, rows)]
    }

    /// A switch of its own, a card: the local network's, the clipboard's.
    fn switch(
        &self,
        permission: Permission,
        name: &str,
        shown: &Shown<bool>,
        which: Which,
        change: fn(bool) -> SettingChange,
    ) -> Vec<WidgetRef> {
        let row = self.switch_row(
            permission,
            name,
            None,
            shown.follows_global,
            which,
            shown.value,
            change,
        );
        vec![card(&self.resources, vec![row])]
    }

    /// Whether a record is kept of what the program does, with what the record is.
    fn record(&self, shown: &Shown<bool>) -> Vec<WidgetRef> {
        let (name, detail) = words::record(&self.of);
        let row = self.switch_row(
            Permission::ActivityRecord,
            name,
            Some(detail),
            shown.follows_global,
            Which::Record,
            shown.value,
            SettingChange::Record,
        );
        vec![card(&self.resources, vec![row])]
    }

    /// What the program did over the days kept of it, a line per capability; that none is kept
    /// while its record is off.
    fn last_days(&self, activity: &Activity, recorded: bool) -> Vec<WidgetRef> {
        let lines = activity.short();
        let rows: Vec<WidgetRef> = if !recorded {
            vec![note(&self.resources, words::NOTHING_RECORDED)]
        } else if lines.is_empty() {
            vec![note(&self.resources, words::NOTHING_USED)]
        } else {
            lines
                .iter()
                .map(|line| card_row(&self.resources, None, line, None, None))
                .collect()
        };
        vec![
            heading(words::LAST_DAYS, &self.resources),
            card(&self.resources, rows),
        ]
    }

    /// The folders mounted, a card: on an app's page whether they follow the default, each
    /// folder with Remove, and the fields that add one.
    fn folders(&self, shown: &Shown<waos_store::FileSystemGrant>) -> Vec<WidgetRef> {
        let mut rows = Vec::new();
        if self.of != SettingsOf::Global {
            let (detail, control) = self.following(shown.follows_global, Which::FileSystem);
            rows.push(card_row(
                &self.resources,
                Some(symbol(
                    permission_symbol(Permission::Folders),
                    &self.resources,
                )),
                words::FOLDERS,
                detail,
                control,
            ));
        }
        if shown.value.mounts.is_empty() {
            rows.push(note(&self.resources, words::NO_FOLDERS));
        }
        for (index, mount) in shown.value.mounts.iter().enumerate() {
            let said = words::mount(
                &mount.host.to_string_lossy(),
                &mount.at.to_string_lossy(),
                mount.writable,
            );
            rows.push(self.listed(&said, SettingChange::RemoveMount(index)));
        }
        rows.push(self.adding_folder());
        vec![
            self.said(words::FOLDERS_AT_ONCE),
            card(&self.resources, rows),
        ]
    }

    /// The answer `question` gets, a card: its choice; while it is simulated, what the device
    /// of WaOS's own is; and where the system `refused` the program it, that it does, with a
    /// button that opens the system's settings at it.
    fn question(
        &self,
        question: Question,
        settings: &ShownSettings,
        refused: bool,
    ) -> Vec<WidgetRef> {
        let shown = settings.answers.get(question);
        // On an app's page, following the default is the first choice, named with the default's
        // answer, as a browser's site settings name it.
        let global = self.of == SettingsOf::Global;
        let mut choices: Vec<(String, bool, Listener)> = Vec::new();
        if global {
            choices.push((
                words::ASK.to_owned(),
                shown.value.is_none(),
                self.on(SettingChange::Answer(question, None)),
            ));
        } else {
            let default = self.defaults.as_ref().map_or(words::ASK, |defaults| {
                words::answer_said(defaults.answers.get(question).value)
            });
            choices.push((
                words::defaulted(default),
                shown.follows_global,
                self.on(SettingChange::FollowGlobal(Which::Answer(question))),
            ));
        }
        let mut answers = vec![
            (words::ALLOW, Answer::Granted),
            (words::DONT_ALLOW, Answer::Denied),
        ];
        if simulatable(question) {
            answers.push((words::SIMULATED, Answer::Simulated));
        }
        for (label, answer) in answers {
            let chosen = shown.value == Some(answer) && (global || !shown.follows_global);
            choices.push((
                label.to_owned(),
                chosen,
                self.on(SettingChange::Answer(question, Some(answer))),
            ));
        }
        let permission = Permission::of_question(question);
        let mut rows = vec![card_row(
            &self.resources,
            Some(symbol(permission_symbol(permission), &self.resources)),
            words::question_name(question),
            None,
            Some(self.drop_down(choices)),
        )];
        rows.extend(self.simulated(question, settings));
        if refused {
            rows.extend(self.refused_by_system());
        }
        vec![card(&self.resources, rows)]
    }

    /// While `question`'s answer is simulated, what the device of WaOS's own is, and for the
    /// microphone what it records; for the camera, where this system has none, that it does not.
    fn simulated(&self, question: Question, shown: &ShownSettings) -> Vec<WidgetRef> {
        if shown.answers.get(question).value != Some(Answer::Simulated) {
            return Vec::new();
        }
        match question {
            Question::Camera if !SIMULATED_CAMERA => {
                vec![note(&self.resources, words::NO_SIMULATED_CAMERA)]
            }
            Question::Camera => vec![note(&self.resources, words::SIMULATED_CAMERA_DETAIL)],
            Question::Microphone => {
                let sound = &shown.simulated_microphone;
                let choice = |label: &str, kind| {
                    (
                        label.to_owned(),
                        sound.value == kind,
                        self.on(SettingChange::SimulatedMicrophone(kind)),
                    )
                };
                let records = self.drop_down(vec![
                    choice(words::QUIET_TONE, SimulatedSound::Tone),
                    choice(words::SILENCE, SimulatedSound::Silence),
                ]);
                let (detail, use_default) = if self.of == SettingsOf::Global {
                    (None, None)
                } else {
                    self.following(sound.follows_global, Which::SimulatedMicrophone)
                };
                let control = Row::new()
                    .main_axis_size(MainAxisSize::Min)
                    .spacing(GAP)
                    .children(use_default.into_iter().chain([records]));
                vec![
                    note(&self.resources, words::SIMULATED_MICROPHONE_DETAIL),
                    card_row(
                        &self.resources,
                        None,
                        words::SIMULATED_RECORDS,
                        detail,
                        Some(control.into_widget()),
                    ),
                ]
            }
            Question::Notifications => Vec::new(),
        }
    }

    /// That the system refuses the program its notifications, and a button that opens the
    /// system's settings at them.
    fn refused_by_system(&self) -> Vec<WidgetRef> {
        let SettingsOf::Program(id) = &self.of else {
            return Vec::new();
        };
        let (library, id) = (self.library.clone(), id.clone());
        let open = Listener::new(move |app| {
            library.update(app, |library, cx| {
                library.open_notification_settings(&id, cx)
            });
        });
        vec![card_row(
            &self.resources,
            None,
            words::NOTIFICATIONS_REFUSED,
            None,
            Some(Button::text(words::OPEN_SYSTEM_SETTINGS, open).into_widget()),
        )]
    }

    /// A row with a switch: the mark of `permission`, `name` over `detail`, and at its end the
    /// switch, `on` or off; on an app's page, that it follows the default, or the button that
    /// has it follow it again.
    #[expect(
        clippy::too_many_arguments,
        reason = "one row's parts, each named at its call"
    )]
    fn switch_row(
        &self,
        permission: Permission,
        name: &str,
        detail: Option<&str>,
        follows_global: bool,
        which: Which,
        on: bool,
        change: fn(bool) -> SettingChange,
    ) -> WidgetRef {
        let (follows, use_default) = if self.of == SettingsOf::Global {
            (None, None)
        } else {
            self.following(follows_global, which)
        };
        let detail = match (detail, follows) {
            (Some(detail), Some(follows)) => Some(format!("{detail} {follows}")),
            (Some(said), None) | (None, Some(said)) => Some(said.to_owned()),
            (None, None) => None,
        };
        let control = Row::new()
            .main_axis_size(MainAxisSize::Min)
            .spacing(GAP)
            .children(use_default.into_iter().chain([self.toggle(on, change)]));
        card_row(
            &self.resources,
            Some(symbol(permission_symbol(permission), &self.resources)),
            name,
            detail.as_deref(),
            Some(control.into_widget()),
        )
    }

    /// On an app's page, what a setting says of the default: that it follows it, or the button
    /// that has it follow it again.
    fn following(
        &self,
        follows_global: bool,
        which: Which,
    ) -> (Option<&'static str>, Option<WidgetRef>) {
        if follows_global {
            return (Some(words::FOLLOWS_GLOBAL), None);
        }
        let follow = self.on(SettingChange::FollowGlobal(which));
        (
            None,
            Some(Button::text(words::USE_GLOBAL, follow).into_widget()),
        )
    }

    /// What a page says over its cards: caption-sized, in the secondary colour, with the room
    /// under it that sets it apart from them.
    fn said(&self, text: &str) -> WidgetRef {
        Padding::new(EdgeInsetsGeometry::only(0.0, 0.0, 0.0, GAP))
            .child(note(&self.resources, text))
            .into_widget()
    }

    /// A card's switch that is `on` or off, changing the page's settings as `change` makes it.
    fn toggle(&self, on: bool, change: fn(bool) -> SettingChange) -> WidgetRef {
        let (library, of) = (self.library.clone(), self.of.clone());
        card_switch(&self.resources, on, move |app, on| {
            library.update(app, |library, cx| {
                library.change_settings(&of, change(on), cx)
            });
        })
    }

    /// A drop-down of `choices`, each its name, whether it is the one chosen, and what choosing
    /// it does: as Windows's Settings offers a choice among named things in a card.
    fn drop_down(&self, choices: Vec<(String, bool, Listener)>) -> WidgetRef {
        let chosen = choices.iter().position(|(_, chosen, _)| *chosen);
        let items = choices
            .iter()
            .enumerate()
            .map(|(index, (name, ..))| ComboBoxItem::text(index.to_string(), name.clone()))
            .collect();
        let chooses: Vec<Listener> = choices.into_iter().map(|(.., choose)| choose).collect();
        let pick = ComboBox::new(items, chosen, move |app, picked| {
            let picked = picked.selected_index();
            if picked != chosen
                && let Some(choose) = picked.and_then(|index| chooses.get(index))
            {
                choose.call(app);
            }
        });
        SizedBox::new()
            .width(DROP_DOWN_WIDTH)
            .child(pick)
            .into_widget()
    }

    /// One of a list, a card's row: `text`, with Remove.
    fn listed(&self, text: &str, remove: SettingChange) -> WidgetRef {
        self.listed_with(text, words::REMOVE, self.on(remove))
    }

    /// A card's row: `text`, with a button `label` doing `click`.
    fn listed_with(&self, text: &str, label: &str, click: Listener) -> WidgetRef {
        card_row(
            &self.resources,
            None,
            text,
            None,
            Some(Button::text(label, click).into_widget()),
        )
    }

    /// A field to type one more of a list in, and a button `confirm`, which adds it as `change`
    /// makes it and clears the field once it is added.
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
        Row::new()
            .spacing(GAP)
            .children([
                Expanded::new(TextBox::new(controller).placeholder_text(placeholder)).into_widget(),
                Button::text(confirm, add).into_widget(),
            ])
            .into_widget()
    }

    /// The fields of a folder to add, and Add.
    fn adding_folder(&self) -> WidgetRef {
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
        let writable_box = CheckBox::new(Some(self.writable), move |app, checked| {
            state.set_writable(app, checked == Some(true));
        })
        .content(Text::new(words::WRITABLE));
        Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .spacing(GAP)
            .children([
                TextBox::new(folder)
                    .placeholder_text(words::FOLDER_PLACEHOLDER)
                    .into_widget(),
                TextBox::new(folder_at)
                    .placeholder_text(words::FOLDER_AT_PLACEHOLDER)
                    .into_widget(),
                Row::new()
                    .spacing(GAP)
                    .children([
                        Expanded::new(writable_box).into_widget(),
                        Button::text(words::ADD, add).into_widget(),
                    ])
                    .into_widget(),
            ])
            .into_widget()
    }

    /// A click that makes `change` to the page's settings.
    fn on(&self, change: SettingChange) -> Listener {
        let (library, of) = (self.library.clone(), self.of.clone());
        Listener::new(move |app| {
            let change = change.clone();
            library.update(app, |library, cx| library.change_settings(&of, change, cx));
        })
    }
}
