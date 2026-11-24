//! A settings page at a desktop's width, in Fluent's controls, made of the settings cards
//! Windows's settings pages are made of: Back and the page's title over what scrolls. It is one
//! of: a permission's page, its default over the apps set otherwise; an app's page, its
//! permissions each with what it holds, then what is the app's alone, each leading to a page; an
//! app's page for one permission, which follows the default or holds its own; and a page of what
//! is the app's alone: its machines, its file pickers, its sound output, its links or its roles.
//! A choice among named things is a drop-down at its card's end; a list, such as a rule's
//! domains, is the rows under its card, which the card's chevron folds away, with the row that
//! adds one. While the system refuses an app its notifications, a warning says so, with a
//! button that opens the system's settings.
//!
//! What the page keeps of its own is what is being typed: a domain, a folder and where it is
//! seen, whether it is writable, and a path for each file picker; which file pickers the user
//! chose to simulate and has set no path for yet; and which cards' rows the user folded away.

use std::rc::Rc;

use waft::{
    App, BoxConstraints, BuildContext, Column, ConstrainedBox, CrossAxisAlignment,
    EdgeInsetsGeometry, Entity, Expanded, Handle, IntoWidget, ListView, Listener, MainAxisSize,
    Padding, Ref, Row, SizedBox, State, StateData, StatefulWidget, Text, TextEditingController,
    TextEditingControllerLeaf, WidgetRef,
};
use waft_winui::{
    Button, CheckBox, ComboBox, ComboBoxItem, ContentAlignment, FontIcon, HorizontalAlignment,
    InfoBar, InfoBarSeverity, SETTINGS_CARD_CONTENT_MIN_WIDTH, SettingsCard, SettingsExpander,
    TextBox, ThemeResources,
};
use waos::{FileDialog, SimulatedSound};
use waos_activity::Activity;
use waos_store::{Answer, DomainRule, Question, Role};

use super::fluent_root::{
    app_icon, card_switch, heading, link_card, note, permission_symbol, setting_card, spaced,
    title_bar,
};
use super::{
    any_set_otherwise, app_details, app_permissions, reset_permissions, said_with, set_otherwise,
    simulated_detail,
};
use crate::logic::permissions::Permission;
use crate::logic::setting_changes::{
    DomainsKind, SettingChange, SettingsOf, Shown, ShownAnswerWith, ShownSettings, Which,
    domains_kind, simulatable,
};
use crate::state::library::{AppDetail, Library, SettingsPage};
use crate::ui::library::home::fluent::MARGIN as PAGE_MARGIN;
use crate::ui::library::words;

const GAP: f64 = 8.0;

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
            folded: Vec::new(),
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
    /// The cards whose rows the user folded away; every other card shows its rows.
    folded: Vec<Folding>,
}

/// A card of the page that has rows under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Folding {
    Network,
    Folders,
    Microphone,
    FilePicker(FileDialog),
    LastDays,
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
            folded: app.get(self).folded.clone(),
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

    /// The user folded the rows of `card` away, or has them show again.
    fn fold(self: Handle<Self>, app: &mut App, card: Folding, shown: bool) {
        self.set_state(app, |state| {
            state.folded.retain(|each| *each != card);
            if !shown {
                state.folded.push(card);
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
    folded: Vec<Folding>,
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
                Some(FontIcon::symbol(permission_symbol(permission)).into_widget()),
                words::permission_name(permission),
                Some(&said),
                SettingsPage::AppPermission(id.clone(), permission),
            ));
        }
        if any_set_otherwise(shown) {
            let reset = Button::text(words::RESET, self.resetting()).into_widget();
            rows.push(spaced(setting_card(
                None,
                words::RESET_PERMISSIONS,
                Some(words::RESET_PERMISSIONS_DETAIL),
                Some(reset),
            )));
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
            rows.push(link_card(
                &self.library,
                Some(app_icon(listed.icon)),
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

    /// The program's answers to opening links of its own schemes, each a card with Remove;
    /// nothing before it asked to open any.
    fn link_schemes(&self, link_schemes: &[(String, Answer)]) -> Vec<WidgetRef> {
        if link_schemes.is_empty() {
            return Vec::new();
        }
        let cards = link_schemes.iter().map(|(scheme, answer)| {
            let forget = SettingChange::LinkScheme(scheme.clone(), None);
            spaced(self.listed(&words::link_scheme(scheme, *answer), forget))
        });
        [self.said(words::LINKS_DETAIL)]
            .into_iter()
            .chain(cards)
            .collect()
    }

    /// The program's answers to holding the roles it asked for, each a card with Remove; nothing
    /// before it asked for one.
    fn roles(&self, roles: &[(Role, Answer)]) -> Vec<WidgetRef> {
        if roles.is_empty() {
            return Vec::new();
        }
        let cards = roles.iter().map(|(role, answer)| {
            let forget = SettingChange::Role(*role, None);
            spaced(self.listed(&words::role(*role, *answer), forget))
        });
        [self.said(words::ROLES_DETAIL)]
            .into_iter()
            .chain(cards)
            .collect()
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
        vec![spaced(setting_card(
            None,
            words::WHICH_MACHINE,
            None,
            Some(self.drop_down(choices)),
        ))]
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
        vec![spaced(setting_card(
            None,
            words::SOUND_OUTPUT,
            Some(words::SOUND_OUTPUT_DETAIL),
            Some(self.drop_down(choices)),
        ))]
    }

    /// One of the app's file pickers, a card: Ask, which shows the picker to the user, or
    /// Simulated, under which the path the app is given is set, as a simulated microphone says
    /// what it records.
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
        let pick = self.drop_down(vec![
            (words::ASK.to_owned(), !simulated, choice(false)),
            (words::SIMULATED.to_owned(), simulated, choice(true)),
        ]);
        let name = words::file_dialog_name(dialog);
        if !simulated {
            return vec![spaced(setting_card(None, name, None, Some(pick)))];
        }
        vec![self.with_rows(
            Folding::FilePicker(dialog),
            None,
            name,
            Some(words::SIMULATED_PICKER_DETAIL),
            Some(pick),
            self.simulated_paths(dialog, &paths),
        )]
    }

    /// Under a simulated file picker, the rows that set what the app is given: for the picker of
    /// several files each path with Remove and the field that adds one; for another, its path
    /// with Change, or the field that sets it.
    fn simulated_paths(&self, dialog: FileDialog, paths: &[std::path::PathBuf]) -> Vec<WidgetRef> {
        let placeholder = words::file_dialog_placeholder(dialog);
        let typed = self.typed.path(dialog).clone();
        if dialog == FileDialog::OpenFiles {
            let mut rows: Vec<WidgetRef> = paths
                .iter()
                .enumerate()
                .map(|(index, path)| {
                    let remove = SettingChange::RemoveFileToOpen(index);
                    self.listed(&path.to_string_lossy(), remove)
                })
                .collect();
            rows.push(self.adding(typed, placeholder, words::ADD, SettingChange::AddFileToOpen));
            return rows;
        }
        let Some(path) = paths.first() else {
            let answer = move |typed: String| SettingChange::FileDialogAnswer(dialog, vec![typed]);
            return vec![self.adding(typed, placeholder, words::SET, answer)];
        };
        // Another path is typed in the field, which shows once this one is let go.
        let (state, ask) = (
            self.state,
            self.on(SettingChange::FileDialogAnswer(dialog, Vec::new())),
        );
        let change = Listener::new(move |app| {
            state.simulate(app, dialog, true);
            ask.call(app);
        });
        vec![self.listed_with(&path.to_string_lossy(), words::CHANGE, change)]
    }

    /// The machines the user picked for the program, each a card with Remove, under the warning.
    fn machines(&self, machines: &[waos::MachineChoice]) -> Vec<WidgetRef> {
        let mut rows = vec![self.said(words::MACHINES_AS_YOU)];
        if machines.is_empty() {
            rows.push(self.said(words::NO_MACHINES));
        }
        rows.extend(machines.iter().map(|machine| {
            let remove = SettingChange::RemoveMachine(machine.id.clone());
            spaced(self.listed(&machine.name, remove))
        }));
        rows
    }

    /// The network's card: its switch; and while it is on, under it, which domains, and the
    /// domains named where the choice names some, with the field that adds one.
    fn network(&self, shown: &Shown<waos_store::NetworkGrant>) -> Vec<WidgetRef> {
        let network = &shown.value;
        let icon = Some(FontIcon::symbol(permission_symbol(Permission::Network)).into_widget());
        let (detail, control) = self.switched(
            None,
            shown.follows_global,
            Which::Network,
            network.on,
            SettingChange::NetworkOn,
        );
        if !network.on {
            let card = setting_card(icon, words::NETWORK, detail.as_deref(), Some(control));
            return vec![spaced(card)];
        }
        let kind = domains_kind(&network.domains);
        let choice = |label: &str, of: DomainsKind| {
            (
                label.to_owned(),
                kind == of,
                self.on(SettingChange::Domains(of)),
            )
        };
        let which_domains = self.drop_down(vec![
            choice(words::ANY_DOMAIN, DomainsKind::Any),
            choice(words::ONLY_DOMAINS, DomainsKind::Only),
            choice(words::ANY_DOMAIN_BUT, DomainsKind::AnyBut),
        ]);
        let mut rows =
            vec![setting_card(None, words::WHICH_DOMAINS, None, Some(which_domains)).into_widget()];
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
        vec![self.with_rows(
            Folding::Network,
            icon,
            words::NETWORK,
            detail.as_deref(),
            Some(control),
            rows,
        )]
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
        let icon = FontIcon::symbol(permission_symbol(permission)).into_widget();
        let (detail, control) =
            self.switched(None, shown.follows_global, which, shown.value, change);
        let card = setting_card(Some(icon), name, detail.as_deref(), Some(control));
        vec![spaced(card)]
    }

    /// Whether a record is kept of what the program does, with what the record is.
    fn record(&self, shown: &Shown<bool>) -> Vec<WidgetRef> {
        let (name, detail) = words::record(&self.of);
        let icon = FontIcon::symbol(permission_symbol(Permission::ActivityRecord)).into_widget();
        let (detail, control) = self.switched(
            Some(detail),
            shown.follows_global,
            Which::Record,
            shown.value,
            SettingChange::Record,
        );
        let card = setting_card(Some(icon), name, detail.as_deref(), Some(control));
        vec![spaced(card)]
    }

    /// What the program did over the days kept of it, a row per capability under their card;
    /// or, on the card, that none is kept while its record is off, or that it used nothing.
    fn last_days(&self, activity: &Activity, recorded: bool) -> Vec<WidgetRef> {
        let lines = activity.short();
        let nothing = if !recorded {
            Some(words::NOTHING_RECORDED)
        } else if lines.is_empty() {
            Some(words::NOTHING_USED)
        } else {
            None
        };
        if nothing.is_some() {
            return vec![spaced(setting_card(None, words::LAST_DAYS, nothing, None))];
        }
        let rows = lines
            .iter()
            .map(|line| setting_card(None, line, None, None).into_widget())
            .collect();
        vec![self.with_rows(Folding::LastDays, None, words::LAST_DAYS, None, None, rows)]
    }

    /// The folders mounted, a card: on an app's page whether they follow the default; under it
    /// each folder with Remove, and the fields that add one.
    fn folders(&self, shown: &Shown<waos_store::FileSystemGrant>) -> Vec<WidgetRef> {
        let (follows, use_default) = if self.of == SettingsOf::Global {
            (None, None)
        } else {
            self.following(shown.follows_global, Which::FileSystem)
        };
        let detail = said_with(Some(words::FOLDERS_AT_ONCE), follows);
        let mut rows = Vec::new();
        if shown.value.mounts.is_empty() {
            rows.push(
                SettingsCard::new()
                    .description(Text::new(words::NO_FOLDERS))
                    .into_widget(),
            );
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
        vec![self.with_rows(
            Folding::Folders,
            Some(FontIcon::symbol(permission_symbol(Permission::Folders)).into_widget()),
            words::FOLDERS,
            detail.as_deref(),
            use_default,
            rows,
        )]
    }

    /// The answer `question` gets, a card: its choice, over what the device of WaOS's own is
    /// while it is simulated; under a simulated microphone, what it records; and where the
    /// system `refused` the program it, the warning that it does.
    fn question(
        &self,
        question: Question,
        settings: &ShownSettings,
        refused: bool,
    ) -> Vec<WidgetRef> {
        let shown = settings.answers.get(question);
        let icon = FontIcon::symbol(permission_symbol(Permission::of_question(question)));
        let icon = Some(icon.into_widget());
        let name = words::question_name(question);
        let pick = Some(self.drop_down(self.answers(question, settings)));
        let simulated = shown.value == Some(Answer::Simulated);
        let detail = simulated.then(|| simulated_detail(question)).flatten();
        let card = if simulated && question == Question::Microphone {
            let records = self.simulated_records(&settings.simulated_microphone);
            self.with_rows(Folding::Microphone, icon, name, detail, pick, vec![records])
        } else {
            spaced(setting_card(icon, name, detail, pick))
        };
        let mut cards = vec![card];
        if refused {
            cards.extend(self.refused_by_system());
        }
        cards
    }

    /// The answers `question` can get, each with whether it is the one set. On an app's page,
    /// following the default is the first, named with the default's answer, as a browser's site
    /// settings name it.
    fn answers(
        &self,
        question: Question,
        settings: &ShownSettings,
    ) -> Vec<(String, bool, Listener)> {
        let shown = settings.answers.get(question);
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
        choices
    }

    /// Under a simulated microphone, what it records: a quiet beep or silence; on an app's page,
    /// that it follows the default, or the button that has it follow it again.
    fn simulated_records(&self, sound: &Shown<SimulatedSound>) -> WidgetRef {
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
        setting_card(
            None,
            words::SIMULATED_RECORDS,
            detail,
            Some(control.into_widget()),
        )
        .into_widget()
    }

    /// The warning that the system refuses the program its notifications, with a button that
    /// opens the system's settings at them.
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
        let warning = InfoBar::new(true, |_, _| {})
            .severity(InfoBarSeverity::Warning)
            .message(words::NOTIFICATIONS_REFUSED)
            .is_closable(false)
            .action_button(Button::text(words::OPEN_SYSTEM_SETTINGS, open));
        vec![spaced(warning)]
    }

    /// What a card with a switch says and ends with: `detail`, and on an app's page that it
    /// follows the default; and the switch, `on` or off, on an app's page after the button that
    /// has it follow the default again.
    fn switched(
        &self,
        detail: Option<&str>,
        follows_global: bool,
        which: Which,
        on: bool,
        change: fn(bool) -> SettingChange,
    ) -> (Option<String>, WidgetRef) {
        let (follows, use_default) = if self.of == SettingsOf::Global {
            (None, None)
        } else {
            self.following(follows_global, which)
        };
        let control = Row::new()
            .main_axis_size(MainAxisSize::Min)
            .spacing(GAP)
            .children(use_default.into_iter().chain([self.toggle(on, change)]));
        (said_with(detail, follows), control.into_widget())
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

    /// A settings card with `rows` under it, which show until the user folds them away: `icon`
    /// where it has one, `name` over `detail` where that is said, and `control` at its end where
    /// it has one.
    fn with_rows(
        &self,
        card: Folding,
        icon: Option<WidgetRef>,
        name: &str,
        detail: Option<&str>,
        control: Option<WidgetRef>,
        rows: Vec<WidgetRef>,
    ) -> WidgetRef {
        let state = self.state;
        let shown = !self.folded.contains(&card);
        let mut expander =
            SettingsExpander::new(shown, move |app, shown| state.fold(app, card, shown))
                .header(Text::new(name))
                .items(rows);
        expander.header_icon = icon;
        expander.description = detail.map(|detail| Text::new(detail).into_widget());
        expander.content = control;
        spaced(expander)
    }

    /// A card's switch that is `on` or off, changing the page's settings as `change` makes it.
    fn toggle(&self, on: bool, change: fn(bool) -> SettingChange) -> WidgetRef {
        let (library, of) = (self.library.clone(), self.of.clone());
        card_switch(on, move |app, on| {
            library.update(app, |library, cx| {
                library.change_settings(&of, change(on), cx)
            });
        })
    }

    /// A drop-down of `choices`, each its name, whether it is the one chosen, and what choosing
    /// it does, no narrower than a settings card has the drop-down at its end.
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
        ConstrainedBox::new(BoxConstraints::new().min_width(SETTINGS_CARD_CONTENT_MIN_WIDTH))
            .child(pick)
            .into_widget()
    }

    /// One of a list, a card: `text`, with Remove.
    fn listed(&self, text: &str, remove: SettingChange) -> WidgetRef {
        self.listed_with(text, words::REMOVE, self.on(remove))
    }

    /// A card: `text`, with a button `label` doing `click`.
    fn listed_with(&self, text: &str, label: &str, click: Listener) -> WidgetRef {
        let button = Button::text(label, click).into_widget();
        setting_card(None, text, None, Some(button)).into_widget()
    }

    /// A row under a card with a field to type one more of a list in, and a button `confirm`,
    /// which adds it as `change` makes it and clears the field once it is added.
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
        across_its_card(Row::new().spacing(GAP).children([
            Expanded::new(TextBox::new(controller).placeholder_text(placeholder)).into_widget(),
            Button::text(confirm, add).into_widget(),
        ]))
    }

    /// A row under the folders' card with the fields of a folder to add, and Add.
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
        across_its_card(
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
                ]),
        )
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

/// A settings card that is all `content`, from one side to the other: a row under a card that
/// holds fields in place of a name and a control.
fn across_its_card<K>(content: impl IntoWidget<K>) -> WidgetRef {
    SettingsCard::new()
        .content_alignment(ContentAlignment::Vertical)
        .horizontal_content_alignment(HorizontalAlignment::Stretch)
        .content(content)
        .into_widget()
}
