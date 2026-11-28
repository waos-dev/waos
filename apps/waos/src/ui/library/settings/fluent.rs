//! A settings page at a desktop's width, in Fluent's controls, laid out as `fluent_page` lays
//! every desktop settings page out. It is one of: a permission's page, its default over the apps
//! set otherwise; an app's page, its permissions each with what it holds, then what is the app's
//! alone, each leading to a page; an app's page for one permission; and a page of what is the
//! app's alone: its machines, its file pickers, its sound output, its links or its roles.
//!
//! Each card has one control. A default that is on or off is a switch; on an app's page it is a
//! drop-down whose first choice follows the default, named with what the default is, as every
//! choice among named things is a drop-down. A list is a block: a card saying what it lists,
//! with Add, over a row for each listed, with Remove. Add asks for what to add in a dialog, so
//! no field stands on the page. While the system refuses an app its notifications, a warning
//! says so, with a button that opens the system's settings.
//!
//! What the page keeps of its own is what is being typed in the dialog: a domain, a folder and
//! where it is seen, whether it is writable, and a path for each file picker; and which file
//! pickers the user chose to simulate and has set no path for yet.

use waft::{
    App, BuildContext, Column, CrossAxisAlignment, Handle, IntoWidget, Listener, MainAxisSize,
    OptionRetained, Ref, SizedBox, State, StateData, StatefulWidget, Text, TextEditingController,
    TextEditingControllerLeaf, WidgetRef,
};
use waft_winui::{
    CheckBox, ContentDialog, ContentDialogButton, ContentDialogResult, InfoBar, InfoBarSeverity,
    TextBox, ThemeResources,
};
use waos::{FileDialog, SimulatedSound};
use waos_activity::Activity;
use waos_package::ProgramId;
use waos_permissions::{Answer, DomainRule, Question, Role};

use super::fluent_page::{
    ListRow, Start, app_endings, app_header, app_page, card_button, card_switch, detail_glyph,
    drop_down, heading, link_card, list_block, note, on_and_off, page, permission_symbol,
    setting_card, spaced,
};
use super::{
    AppInfo, app_details, app_info, app_permissions, said_with, set_otherwise, simulated_detail,
};
use crate::logic::permissions::Permission;
use crate::logic::setting_changes::{
    DomainsKind, SettingChange, SettingsOf, Shown, ShownAnswerWith, ShownSettings, Which,
    domains_kind, simulatable,
};
use crate::state::library::{AppDetail, DeniedBySystem, Library, SettingsPage};
use crate::ui::library::words;
use std::rc::Rc;
use waft::Entity;

const GAP: f64 = 8.0;

/// How wide the dialog's fields are.
const DIALOG_FIELD_WIDTH: f64 = 400.0;

/// The settings `of` shows.
#[derive(Debug)]
pub(crate) struct FluentSettings {
    pub(crate) library: Entity<Library>,
    pub(crate) of: SettingsOf,
    /// The app's name, on an app's pages; none on a permission's page.
    pub(crate) program_name: Option<String>,
    /// The permission whose settings alone the page shows: on its own page with the apps that
    /// set it otherwise, or on an app's page for it; none for an app's page, which lists all.
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
            dialog: None,
            writable: false,
            simulating: Vec::new(),
            endings_shown: false,
        }
    }
}

pub(crate) struct FluentSettingsState {
    state: StateData<FluentSettings>,
    typed: Option<Typed>,
    /// The dialog that asks for what to add, set to what it asks for each time it shows.
    dialog: Option<Ref<ContentDialog>>,
    /// Whether a folder added is writable.
    writable: bool,
    /// The file pickers the user chose to simulate and has set no path for yet.
    simulating: Vec<FileDialog>,
    /// Whether the card that ends an app's page shows its rows; folded away until asked for.
    endings_shown: bool,
}

/// What is being typed in the dialog.
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
        let dialog = ContentDialog::new(app);
        dialog.close_button_text(app, words::CANCEL);
        dialog.default_button(app, ContentDialogButton::Primary);
        let state = app.get_mut(self);
        state.typed = Some(typed);
        state.dialog = Some(dialog);
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        if let Some(dialog) = app.get_mut(self).dialog.take() {
            dialog.dispose(app);
        }
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
        let shown = Page {
            library: widget.library.clone(),
            of: widget.of.clone(),
            only: widget.only,
            detail: widget.detail,
            defaults: widget.library.read(app).settings(&SettingsOf::Global).ok(),
            resources,
            typed: app.get(self).typed.clone().expect("an initialized page"),
            simulating: app.get(self).simulating.clone(),
            state: self,
        };
        let trail = shown.trail(widget.program_name.as_deref());
        let mut rows: Vec<WidgetRef> = shown.switch_asked(app).into_iter().collect();
        rows.extend(shown.over_the_default());
        let set_otherwise = shown.kind_page().map(|only| shown.set_otherwise(app, only));
        match shown.library.read(app).settings(&shown.of) {
            Ok(settings) => {
                let activity = shown.library.read(app).activity(&shown.of);
                let denied = (shown.library.read(app)).denied_by_system(&shown.of, &settings, app);
                rows.extend(shown.sections(&settings, activity.as_ref(), denied));
            }
            Err(why) => rows.push(note(&shown.resources, &why)),
        }
        rows.extend(set_otherwise.into_iter().flatten());
        match shown.app_info(app) {
            Some((id, info)) => {
                let header = app_header(&shown.library, &shown.resources, &id, &info);
                let endings_shown = app.get(self).endings_shown;
                rows.extend(app_endings(
                    &shown.library,
                    &id,
                    &info,
                    endings_shown,
                    move |app, shown| self.set_state(app, |state| state.endings_shown = shown),
                ));
                app_page(&shown.library, header, rows)
            }
            None => page(&shown.library, &shown.resources, &trail, rows),
        }
    }
}

impl FluentSettingsState {
    fn set_writable(self: Handle<Self>, app: &mut App, writable: bool) {
        self.set_state(app, |state| state.writable = writable);
    }

    /// The user chose to simulate `dialog`, or not to: the row that sets its path shows, or
    /// goes.
    fn simulate(self: Handle<Self>, app: &mut App, dialog: FileDialog, simulated: bool) {
        self.set_state(app, |state| {
            state.simulating.retain(|each| *each != dialog);
            if simulated {
                state.simulating.push(dialog);
            }
        });
    }

    /// Asks for what to add in the dialog, titled `title` over `fields`, its button saying
    /// `confirm`; `confirmed` runs once the dialog has closed on that button, so what it says
    /// in a dialog of its own finds none open.
    fn ask(
        self: Handle<Self>,
        app: &mut App,
        title: &str,
        fields: WidgetRef,
        confirm: &str,
        confirmed: Listener,
    ) {
        let Some(dialog) = app.get(self).dialog.handle() else {
            return;
        };
        dialog.title(app, Text::new(title));
        dialog.content(app, fields);
        dialog.primary_button_text(app, confirm);
        let context = self.context(app);
        match dialog.show_async(app, context) {
            Ok(closed) => closed.then(app, move |app, result| {
                if result == ContentDialogResult::Primary {
                    confirmed.call(app);
                }
            }),
            Err(error) => tracing::warn!(?error, "the settings' dialog could not be shown"),
        }
    }
}

/// What one build of the page reads and writes.
struct Page {
    library: Entity<Library>,
    of: SettingsOf,
    only: Option<Permission>,
    detail: Option<AppDetail>,
    /// The defaults, which an app's choice names as its first: "Ask (default)".
    defaults: Option<ShownSettings>,
    resources: Rc<ThemeResources>,
    typed: Typed,
    simulating: Vec<FileDialog>,
    state: Handle<FluentSettingsState>,
}

impl Page {
    /// The names the page's title says, the page's own last: an app's page under the app's
    /// name, a permission's own page under Permissions.
    fn trail<'a>(&self, program_name: Option<&'a str>) -> Vec<&'a str> {
        let own = match (self.detail, self.only) {
            (Some(detail), _) => Some(words::app_detail_name(detail)),
            (None, Some(only)) => Some(words::permission_name(only)),
            (None, None) => None,
        };
        let over = program_name.unwrap_or(words::PERMISSIONS);
        [Some(over), own].into_iter().flatten().collect()
    }

    /// On an app's own page, the app and what its page says of it, which heads the page in
    /// place of a title; none on the pages under it, and on a permission's.
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

    /// On a permission's page, what is over its default: that it is the default, and what a
    /// default is.
    fn over_the_default(&self) -> Vec<WidgetRef> {
        if self.kind_page().is_none() {
            return Vec::new();
        }
        vec![
            heading(words::DEFAULT, &self.resources),
            note(&self.resources, words::DEFAULT_DETAIL),
        ]
    }

    /// Whether the page shows the controls of `permission`.
    fn shows(&self, permission: Permission) -> bool {
        self.only == Some(permission)
    }

    /// The permission whose default the page shows; none on an app's pages.
    fn kind_page(&self) -> Option<Permission> {
        self.only.filter(|_| self.is_the_default())
    }

    /// Whether the page shows the defaults, and not an app's own.
    fn is_the_default(&self) -> bool {
        self.of == SettingsOf::Global
    }

    /// On an app's page, each permission with what the app holds, each leading to the app's
    /// page for it.
    fn app_permissions(&self, shown: &ShownSettings) -> Vec<WidgetRef> {
        let SettingsOf::Program(id) = &self.of else {
            return Vec::new();
        };
        let mut rows = vec![heading(words::PERMISSIONS, &self.resources)];
        for (permission, said) in app_permissions(shown) {
            rows.push(link_card(
                &self.library,
                Start::symbol(permission_symbol(permission)),
                words::permission_name(permission),
                Some(&said),
                SettingsPage::AppPermission(id.clone(), permission),
            ));
        }
        rows
    }

    /// On an app's page, what is the app's alone, each with how many it has, each leading to its
    /// page: more of the same list as its permissions.
    fn app_details(&self, shown: &ShownSettings) -> Vec<WidgetRef> {
        let SettingsOf::Program(id) = &self.of else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        for (detail, said) in app_details(shown) {
            rows.push(link_card(
                &self.library,
                Start::glyph(detail_glyph(detail)),
                words::app_detail_name(detail),
                Some(&said),
                SettingsPage::AppDetail(id.clone(), detail),
            ));
        }
        rows
    }

    /// On a permission's page, the apps that set it otherwise than its default, each with what
    /// it set and leading to its page; one line where every app follows the default.
    fn set_otherwise(&self, app: &App, permission: Permission) -> Vec<WidgetRef> {
        let mut rows = vec![
            heading(words::SET_OTHERWISE, &self.resources),
            note(&self.resources, words::SET_OTHERWISE_DETAIL),
        ];
        let apps = set_otherwise(self.library.read(app), permission);
        if apps.is_empty() {
            rows.push(note(&self.resources, words::NONE_SET_OTHERWISE));
        }
        for (listed, value) in apps {
            rows.push(link_card(
                &self.library,
                Start::app_icon(listed.icon),
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
        Some(spaced(bar))
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
            sections.extend(self.app_details(shown));
        }
        match self.detail {
            Some(AppDetail::Machines) => {
                sections.extend(self.machines(&shown.machines));
                sections.push(self.machine_answer(&shown.answer_with, &shown.offered.machines));
            }
            Some(AppDetail::FilePickers) => {
                for dialog in FileDialog::ALL {
                    sections.push(self.file_picker(dialog, &shown.answer_with));
                }
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
            sections.push(self.on_or_off_card(
                Permission::LocalNetwork,
                words::LOCAL_NETWORK,
                None,
                &shown.local_network,
                Which::LocalNetwork,
                SettingChange::LocalNetwork,
            ));
        }
        if self.shows(Permission::Clipboard) {
            sections.push(self.on_or_off_card(
                Permission::Clipboard,
                words::CLIPBOARD_READ,
                None,
                &shown.clipboard_read,
                Which::ClipboardRead,
                SettingChange::ClipboardRead,
            ));
            sections.push(self.on_or_off_card(
                Permission::Clipboard,
                words::CLIPBOARD_WRITE,
                None,
                &shown.clipboard_write,
                Which::ClipboardWrite,
                SettingChange::ClipboardWrite,
            ));
        }
        if self.shows(Permission::ActivityRecord) {
            let (name, detail) = words::record(&self.of);
            sections.push(self.on_or_off_card(
                Permission::ActivityRecord,
                name,
                Some(detail),
                &shown.record,
                Which::Record,
                SettingChange::Record,
            ));
            if let Some(activity) = activity {
                sections.push(self.last_days(activity, shown.record.value));
            }
        }
        if self.shows(Permission::Folders) {
            sections.extend(self.folders(&shown.file_system));
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

    /// The program's answers to opening links of its own schemes, each a card with Remove;
    /// nothing before it asked to open any.
    fn link_schemes(&self, link_schemes: &[(String, Answer)]) -> Vec<WidgetRef> {
        if link_schemes.is_empty() {
            return Vec::new();
        }
        let cards = link_schemes.iter().map(|(scheme, answer)| {
            let forget = SettingChange::LinkScheme(scheme.clone(), None);
            self.listed(&words::link_scheme(scheme, *answer), forget)
        });
        [note(&self.resources, words::LINKS_DETAIL)]
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
            self.listed(&words::role(*role, *answer), forget)
        });
        [note(&self.resources, words::ROLES_DETAIL)]
            .into_iter()
            .chain(cards)
            .collect()
    }

    /// The machines the user picked for the program, each a card with Remove, under the warning.
    fn machines(&self, machines: &[waos::MachineChoice]) -> Vec<WidgetRef> {
        let mut rows = vec![note(&self.resources, words::MACHINES_AS_YOU)];
        if machines.is_empty() {
            rows.push(note(&self.resources, words::NO_MACHINES));
        }
        rows.extend(machines.iter().map(|machine| {
            let remove = SettingChange::RemoveMachine(machine.id.clone());
            self.listed(&machine.name, remove)
        }));
        rows
    }

    /// Which computer the app is given when it asks for one: Ask, or one of the machines
    /// offered, the one set among them, by its id when it is not offered now.
    fn machine_answer(&self, set: &ShownAnswerWith, offered: &[waos::MachineChoice]) -> WidgetRef {
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
        setting_card(
            Start::Edge,
            words::WHICH_MACHINE,
            None,
            Some(drop_down(choices)),
        )
    }

    /// Which sound output the app plays to: Ask, or one of the outputs offered, the one set
    /// among them, by its id when it is not connected now.
    fn output_answer(&self, set: &ShownAnswerWith, offered: &[waos::AudioOutput]) -> WidgetRef {
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
        setting_card(
            Start::Edge,
            words::SOUND_OUTPUT,
            Some(words::SOUND_OUTPUT_DETAIL),
            Some(drop_down(choices)),
        )
    }

    /// One of the app's file pickers: Ask, which shows the picker to the user, or Simulated,
    /// under which are the rows that set the path the app is given.
    fn file_picker(&self, dialog: FileDialog, set: &ShownAnswerWith) -> WidgetRef {
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
        let pick = drop_down(vec![
            (words::ASK.to_owned(), !simulated, choice(false)),
            (words::SIMULATED.to_owned(), simulated, choice(true)),
        ]);
        let name = words::file_dialog_name(dialog);
        if !simulated {
            return setting_card(Start::Edge, name, None, Some(pick));
        }
        list_block(
            &self.resources,
            Start::Edge,
            name,
            Some(words::SIMULATED_PICKER_DETAIL),
            Some(pick),
            self.simulated_paths(dialog, &paths),
        )
    }

    /// Under a simulated file picker, the rows that set what the app is given: for the picker of
    /// several files each path with Remove and the row that adds one; for another, its path
    /// with Change, or the row that sets it.
    fn simulated_paths(&self, dialog: FileDialog, paths: &[std::path::PathBuf]) -> Vec<ListRow> {
        if dialog == FileDialog::OpenFiles {
            let mut rows: Vec<ListRow> = paths
                .iter()
                .enumerate()
                .map(|(index, path)| {
                    let remove = self.on(SettingChange::RemoveFileToOpen(index));
                    ListRow {
                        text: path.to_string_lossy().into_owned(),
                        control: Some(card_button(words::REMOVE, remove)),
                    }
                })
                .collect();
            let add = self.asking_path(dialog, words::ADD, SettingChange::AddFileToOpen);
            rows.push(ListRow {
                text: words::ADD_A_FILE.to_owned(),
                control: Some(card_button(words::ADD, add)),
            });
            return rows;
        }
        let answer = move |typed: String| SettingChange::FileDialogAnswer(dialog, vec![typed]);
        let row = match paths.first() {
            Some(path) => ListRow {
                text: path.to_string_lossy().into_owned(),
                control: Some(card_button(
                    words::CHANGE,
                    self.asking_path(dialog, words::CHANGE, answer),
                )),
            },
            None => ListRow {
                text: words::NO_PATH.to_owned(),
                control: Some(card_button(
                    words::SET,
                    self.asking_path(dialog, words::SET, answer),
                )),
            },
        };
        vec![row]
    }

    /// The network's cards: whether apps reach it; while they do, which domains; and where the
    /// choice names some, the block that lists them.
    fn network(&self, shown: &Shown<waos_permissions::NetworkGrant>) -> Vec<WidgetRef> {
        let network = &shown.value;
        let grant = Shown {
            value: network.on,
            follows_global: shown.follows_global,
        };
        let mut cards = vec![self.on_or_off_card(
            Permission::Network,
            words::NETWORK,
            None,
            &grant,
            Which::Network,
            SettingChange::NetworkOn,
        )];
        if !network.on {
            return cards;
        }
        let kind = domains_kind(&network.domains);
        let choice = |label: &str, of: DomainsKind| {
            (
                label.to_owned(),
                kind == of,
                self.on(SettingChange::Domains(of)),
            )
        };
        let which_domains = drop_down(vec![
            choice(words::ANY_DOMAIN, DomainsKind::Any),
            choice(words::ONLY_DOMAINS, DomainsKind::Only),
            choice(words::ANY_DOMAIN_BUT, DomainsKind::AnyBut),
        ]);
        cards.push(setting_card(
            Start::Room,
            words::WHICH_DOMAINS,
            None,
            Some(which_domains),
        ));
        if let DomainRule::Only(domains) | DomainRule::AnyBut(domains) = &network.domains {
            let rows = domains
                .iter()
                .map(|domain| ListRow {
                    text: domain.as_str().to_owned(),
                    control: Some(card_button(
                        words::REMOVE,
                        self.on(SettingChange::RemoveDomain(domain.clone())),
                    )),
                })
                .collect();
            let add = self.asking(
                words::ADD_A_DOMAIN,
                self.typed.domain.clone(),
                words::DOMAIN_PLACEHOLDER,
                words::ADD,
                SettingChange::AddDomain,
            );
            cards.push(list_block(
                &self.resources,
                Start::Room,
                words::THESE_DOMAINS,
                Some(&words::how_many(domains.len(), "domain", "domains")),
                Some(card_button(words::ADD, add)),
                rows,
            ));
        }
        cards
    }

    /// A setting that is on or off, a card: on a default's page its switch; on an app's page a
    /// drop-down whose first choice follows the default, named with what the default is.
    fn on_or_off_card(
        &self,
        permission: Permission,
        name: &str,
        detail: Option<&str>,
        shown: &Shown<bool>,
        which: Which,
        change: fn(bool) -> SettingChange,
    ) -> WidgetRef {
        let control = if self.is_the_default() {
            self.toggle(shown.value, change)
        } else {
            let (on, off) = on_and_off(permission);
            let said = |is_on: bool| if is_on { on } else { off };
            let default = self
                .defaults
                .as_ref()
                .is_some_and(|defaults| is_on(defaults, which));
            let own = !shown.follows_global;
            drop_down(vec![
                (
                    words::defaulted(said(default)),
                    shown.follows_global,
                    self.on(SettingChange::FollowGlobal(which)),
                ),
                (on.to_owned(), own && shown.value, self.on(change(true))),
                (off.to_owned(), own && !shown.value, self.on(change(false))),
            ])
        };
        setting_card(
            Start::symbol(permission_symbol(permission)),
            name,
            detail,
            Some(control),
        )
    }

    /// What the program did over the days kept of it, a row per capability under their card;
    /// or, on the card, that none is kept while its record is off, or that it used nothing.
    fn last_days(&self, activity: &Activity, recorded: bool) -> WidgetRef {
        let lines = activity.short();
        let nothing = if !recorded {
            Some(words::NOTHING_RECORDED)
        } else if lines.is_empty() {
            Some(words::NOTHING_USED)
        } else {
            None
        };
        if nothing.is_some() {
            return setting_card(Start::Room, words::LAST_DAYS, nothing, None);
        }
        let rows = lines
            .into_iter()
            .map(|text| ListRow {
                text,
                control: None,
            })
            .collect();
        list_block(
            &self.resources,
            Start::Room,
            words::LAST_DAYS,
            None,
            None,
            rows,
        )
    }

    /// The folders mounted, a block: its card with Add, a row for each folder with Remove; and,
    /// on an app's page whose folders are its own, the card that has them follow the default
    /// again.
    fn folders(&self, shown: &Shown<waos_permissions::FileSystemGrant>) -> Vec<WidgetRef> {
        let follows = !self.is_the_default() && shown.follows_global;
        let detail = said_with(
            Some(words::FOLDERS_AT_ONCE),
            follows.then_some(words::FOLLOWS_GLOBAL),
        );
        let mut rows: Vec<ListRow> = shown
            .value
            .mounts
            .iter()
            .enumerate()
            .map(|(index, mount)| ListRow {
                text: words::mount(
                    &mount.host.to_string_lossy(),
                    &mount.at.to_string_lossy(),
                    mount.writable,
                ),
                control: Some(card_button(
                    words::REMOVE,
                    self.on(SettingChange::RemoveMount(index)),
                )),
            })
            .collect();
        if rows.is_empty() {
            rows.push(ListRow {
                text: words::NO_FOLDERS.to_owned(),
                control: None,
            });
        }
        let mut cards = vec![list_block(
            &self.resources,
            Start::symbol(permission_symbol(Permission::Folders)),
            words::FOLDERS,
            detail.as_deref(),
            Some(card_button(words::ADD, self.asking_folder())),
            rows,
        )];
        if !self.is_the_default() && !shown.follows_global {
            let follow = self.on(SettingChange::FollowGlobal(Which::FileSystem));
            cards.push(setting_card(
                Start::Room,
                words::USE_GLOBAL,
                None,
                Some(card_button(words::RESET, follow)),
            ));
        }
        cards
    }

    /// The answer `question` gets, a card: its choice, over what the device of WaOS's own is
    /// while it is simulated; under a simulated microphone, the card of what it records; and
    /// where the system `refused` the program it, the warning that it does.
    fn question(
        &self,
        question: Question,
        settings: &ShownSettings,
        refused: bool,
    ) -> Vec<WidgetRef> {
        let shown = settings.answers.get(question);
        let simulated = shown.value == Some(Answer::Simulated);
        let detail = simulated.then(|| simulated_detail(question)).flatten();
        let mut cards = vec![setting_card(
            Start::symbol(permission_symbol(Permission::of_question(question))),
            words::question_name(question),
            detail,
            Some(drop_down(self.answers(question, settings))),
        )];
        if simulated && question == Question::Microphone {
            cards.push(self.simulated_records(&settings.simulated_microphone));
        }
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
        let global = self.is_the_default();
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

    /// Under a simulated microphone, the card of what it records: a quiet beep or silence; on
    /// an app's page the first choice follows the default, named with what the default records.
    fn simulated_records(&self, sound: &Shown<SimulatedSound>) -> WidgetRef {
        let said = |kind| match kind {
            SimulatedSound::Tone => words::QUIET_TONE,
            SimulatedSound::Silence => words::SILENCE,
        };
        let own = self.is_the_default() || !sound.follows_global;
        let mut choices = Vec::new();
        if !self.is_the_default() {
            let default = self
                .defaults
                .as_ref()
                .map_or(SimulatedSound::Tone, |defaults| {
                    defaults.simulated_microphone.value
                });
            choices.push((
                words::defaulted(said(default)),
                sound.follows_global,
                self.on(SettingChange::FollowGlobal(Which::SimulatedMicrophone)),
            ));
        }
        for kind in [SimulatedSound::Tone, SimulatedSound::Silence] {
            choices.push((
                said(kind).to_owned(),
                own && sound.value == kind,
                self.on(SettingChange::SimulatedMicrophone(kind)),
            ));
        }
        setting_card(
            Start::Room,
            words::SIMULATED_RECORDS,
            None,
            Some(drop_down(choices)),
        )
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
            .action_button(card_button(words::OPEN_SYSTEM_SETTINGS, open));
        vec![spaced(warning)]
    }

    /// The warning that the system keeps WaOS from the folders on the phone, with a button that
    /// opens the system's settings where the user lets it reach them.
    fn folders_denied_by_system(&self) -> WidgetRef {
        let library = self.library.clone();
        let open = Listener::new(move |app| {
            library.update(app, |library, cx| library.open_phone_files_page(cx));
        });
        let warning = InfoBar::new(true, |_, _| {})
            .severity(InfoBarSeverity::Warning)
            .message(words::FOLDERS_DENIED)
            .is_closable(false)
            .action_button(card_button(words::OPEN_ANDROID_SETTINGS, open));
        spaced(warning)
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

    /// One of a list that has no card of its own over it, a card: `text`, with Remove.
    fn listed(&self, text: &str, remove: SettingChange) -> WidgetRef {
        let remove = card_button(words::REMOVE, self.on(remove));
        setting_card(Start::Edge, text, None, Some(remove))
    }

    /// A click that asks for a path of `dialog`'s in the dialog, its button saying `confirm`,
    /// then makes `change` of it.
    fn asking_path(
        &self,
        dialog: FileDialog,
        confirm: &'static str,
        change: impl Fn(String) -> SettingChange + 'static,
    ) -> Listener {
        self.asking(
            words::file_dialog_name(dialog),
            self.typed.path(dialog).clone(),
            words::file_dialog_placeholder(dialog),
            confirm,
            change,
        )
    }

    /// A click that asks, in the dialog titled `title`, for what is typed in one field, its
    /// button saying `confirm`; confirmed, it makes `change` of what was typed and clears the
    /// field once that is made, so what was refused is there to mend.
    fn asking(
        &self,
        title: &'static str,
        typed: Ref<TextEditingController>,
        placeholder: &'static str,
        confirm: &'static str,
        change: impl Fn(String) -> SettingChange + 'static,
    ) -> Listener {
        let (library, of, state) = (self.library.clone(), self.of.clone(), self.state);
        let change = Rc::new(change);
        Listener::new(move |app| {
            let field = TextBox::new(typed.erase()).placeholder_text(placeholder);
            let (library, of, typed, change) = (
                library.clone(),
                of.clone(),
                typed.clone(),
                Rc::clone(&change),
            );
            let confirmed = Listener::new(move |app| {
                let text = typed.text_value(app).to_owned();
                library.update(app, |library, cx| {
                    library.change_settings(&of, change(text), cx)
                });
                if library.read(app).problem().is_none() {
                    typed.clear(app);
                }
            });
            state.ask(app, title, in_the_dialog(field), confirm, confirmed);
        })
    }

    /// A click that asks, in the dialog, for a folder to add: its path, where the app sees it,
    /// and whether it is writable; confirmed, it adds the folder and clears the fields once it
    /// is added.
    fn asking_folder(&self) -> Listener {
        let (library, of, typed, state) = (
            self.library.clone(),
            self.of.clone(),
            self.typed.clone(),
            self.state,
        );
        Listener::new(move |app| {
            let fields = Column::new()
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .main_axis_size(MainAxisSize::Min)
                .spacing(GAP)
                .children([
                    TextBox::new(typed.folder.erase())
                        .placeholder_text(words::FOLDER_PLACEHOLDER)
                        .into_widget(),
                    TextBox::new(typed.folder_at.erase())
                        .placeholder_text(words::FOLDER_AT_PLACEHOLDER)
                        .into_widget(),
                    WritableBox {
                        page: state,
                        initially: app.get(state).writable,
                    }
                    .into_widget(),
                ]);
            let (library, of, typed) = (library.clone(), of.clone(), typed.clone());
            let confirmed = Listener::new(move |app| {
                let change = SettingChange::AddMount {
                    host: typed.folder.text_value(app).to_owned(),
                    at: typed.folder_at.text_value(app).to_owned(),
                    writable: app.get(state).writable,
                };
                library.update(app, |library, cx| library.change_settings(&of, change, cx));
                if library.read(app).problem().is_none() {
                    typed.folder.clear(app);
                    typed.folder_at.clear(app);
                }
            });
            state.ask(
                app,
                words::ADD_A_FOLDER,
                in_the_dialog(fields),
                words::ADD,
                confirmed,
            );
        })
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

/// Whether the setting `which` names is on in `settings`; no for one that is not on or off.
fn is_on(settings: &ShownSettings, which: Which) -> bool {
    match which {
        Which::Network => settings.network.value.on,
        Which::LocalNetwork => settings.local_network.value,
        Which::ClipboardRead => settings.clipboard_read.value,
        Which::ClipboardWrite => settings.clipboard_write.value,
        Which::Record => settings.record.value,
        Which::FileSystem | Which::Answer(_) | Which::SimulatedMicrophone => false,
    }
}

/// `fields` at the width the dialog shows its fields.
fn in_the_dialog<K>(fields: impl IntoWidget<K>) -> WidgetRef {
    SizedBox::new()
        .width(DIALOG_FIELD_WIDTH)
        .child(fields)
        .into_widget()
}

/// Whether a folder added is writable, a check box in the dialog that adds it. The dialog's
/// content is built once as it shows, so the box keeps what is checked itself, and tells the
/// page, which adds the folder so.
#[derive(Debug)]
struct WritableBox {
    page: Handle<FluentSettingsState>,
    initially: bool,
}

impl StatefulWidget for WritableBox {
    type State = WritableBoxState;

    fn create_state(&self) -> WritableBoxState {
        WritableBoxState {
            state: StateData::new(),
            writable: self.initially,
        }
    }
}

struct WritableBoxState {
    state: StateData<WritableBox>,
    writable: bool,
}

impl State for WritableBoxState {
    type Widget = WritableBox;
    waft_widgets::state_accessors!();

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        let page = self.widget(app).page;
        CheckBox::new(Some(app.get(self).writable), move |app, checked| {
            let writable = checked == Some(true);
            self.set_state(app, |state| state.writable = writable);
            page.set_writable(app, writable);
        })
        .content(Text::new(words::WRITABLE))
        .into_widget()
    }
}
