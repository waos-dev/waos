//! A settings page in Fluent's controls: the global settings, or a program's, each setting with
//! its controls, one section after another: on a program's page first the machines its user
//! picked for it, under a warning that it acts on them as the user; the network switch with its
//! rule of domains, the two clipboard switches, the folders mounted, and the answer each question
//! gets; and on a program's page last its answers to opening links of its own schemes, and the
//! answers set for its pickers and file dialogs, each Ask or what WaOS gives in place of asking. On a program's page, each setting says whether it follows the setting for every program,
//! and one that does not can be set back to it; and while the system refuses the program its
//! notifications, they say so, with a button that opens the system's settings at them. Where WaOS
//! offers the choice, the global page first says whether programs show inside WaOS, or, in a
//! desktop's launcher, whether WaOS is kept in the Dock or the taskbar, which are WaOS's own
//! preferences rather than a program's settings.
//!
//! What the page keeps of its own is what is being typed: a domain, a folder and where it is
//! seen, whether it is writable, and a path for each file dialog.

use std::rc::Rc;

use waft::{
    App, BuildContext, Column, CrossAxisAlignment, EdgeInsetsGeometry, Entity, Expanded, Handle,
    IntoWidget, ListView, Listener, Padding, Ref, Row, State, StateData, StatefulWidget, Text,
    TextEditingController, TextEditingControllerLeaf, WidgetRef,
};
use waft_winui::{
    Button, ButtonStyle, CheckBox, FluentSymbol, FontIcon, InfoBar, InfoBarSeverity, RadioButton,
    TextBlockStyle, TextBox, ThemeResources, ToggleSwitch,
};
use waos::{FileDialog, SimulatedSound};
use waos_activity::Activity;
use waos_store::{Answer, DomainRule, Question, Role};

use crate::logic::setting_changes::{
    DomainsKind, SettingChange, SettingsOf, Shown, ShownAnswerWith, ShownSettings, Which,
    domains_kind, simulatable,
};
use crate::state::SIMULATED_CAMERA;
use crate::state::library::{Library, LibraryPage};
use crate::ui::library::words;

const MARGIN: f64 = 16.0;
const GAP: f64 = 8.0;

/// The settings `of` shows.
#[derive(Debug)]
pub(crate) struct FluentSettings {
    pub(crate) library: Entity<Library>,
    pub(crate) of: SettingsOf,
    /// The page's title: the program's name, or none on the global page.
    pub(crate) program_name: Option<String>,
}

impl StatefulWidget for FluentSettings {
    type State = FluentSettingsState;

    fn create_state(&self) -> FluentSettingsState {
        FluentSettingsState {
            state: StateData::new(),
            typed: None,
            writable: false,
        }
    }
}

pub(crate) struct FluentSettingsState {
    state: StateData<FluentSettings>,
    typed: Option<Typed>,
    /// Whether a folder added is writable.
    writable: bool,
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
            resources,
            typed: app.get(self).typed.clone().expect("an initialized page"),
            writable: app.get(self).writable,
            state: self,
        };
        let mut rows = vec![page.title(widget.program_name.as_deref())];
        rows.extend(page.switch_asked(app));
        rows.extend(page.where_programs_open(app));
        rows.extend(page.where_waos_stays(app));
        rows.extend(page.phone_files(app));
        match page.library.read(app).settings(&page.of) {
            Ok(shown) => {
                let activity = page.library.read(app).activity(&page.of);
                let refused = page.notifications_refused(app);
                rows.extend(page.sections(&shown, activity.as_ref(), refused));
            }
            Err(why) => rows.push(caption(&why, &page.resources)),
        }
        ListView::new()
            .padding(EdgeInsetsGeometry::all(MARGIN))
            .children(rows)
            .into_widget()
    }
}

impl FluentSettingsState {
    fn set_writable(self: Handle<Self>, app: &mut App, writable: bool) {
        self.set_state(app, |state| state.writable = writable);
    }
}

/// What one build of the page reads and writes.
struct Page {
    library: Entity<Library>,
    of: SettingsOf,
    resources: Rc<ThemeResources>,
    typed: Typed,
    writable: bool,
    state: Handle<FluentSettingsState>,
}

impl Page {
    /// The page's title, after Back to the home, over what the global page is for.
    fn title(&self, program_name: Option<&str>) -> WidgetRef {
        let primary = self.resources.text_fill_color_primary;
        let library = self.library.clone();
        let back = Listener::new(move |app| {
            library.update(app, |library, cx| library.show_page(LibraryPage::Home, cx));
        });
        let title = match program_name {
            Some(name) => words::settings_of(name),
            None => words::GLOBAL_SETTINGS.to_owned(),
        };
        let bar = Row::new().spacing(4.0).children([
            Button::new(FontIcon::symbol(FluentSymbol::Back).font_size(16.0), back)
                .style(ButtonStyle::Subtle)
                .padding([8.0, 8.0, 8.0, 8.0])
                .into_widget(),
            Text::new(title)
                .style(TextBlockStyle::Subtitle.text_style(&self.resources, primary))
                .into_widget(),
        ]);
        if program_name.is_some() {
            return bar.into_widget();
        }
        Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .children([
                bar.into_widget(),
                caption(words::GLOBAL_DETAIL, &self.resources),
            ])
            .into_widget()
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
        if self.of != SettingsOf::Global {
            sections.extend(self.machines(&shown.machines));
        }
        sections.extend(self.network(&shown.network));
        sections.extend(self.switch(
            words::LOCAL_NETWORK,
            &shown.local_network,
            Which::LocalNetwork,
            SettingChange::LocalNetwork,
        ));
        sections.extend(self.switch(
            words::CLIPBOARD_READ,
            &shown.clipboard_read,
            Which::ClipboardRead,
            SettingChange::ClipboardRead,
        ));
        sections.extend(self.switch(
            words::CLIPBOARD_WRITE,
            &shown.clipboard_write,
            Which::ClipboardWrite,
            SettingChange::ClipboardWrite,
        ));
        sections.extend(self.record(&shown.record));
        if let Some(activity) = activity {
            sections.extend(self.last_days(activity, shown.record.value));
        }
        sections.extend(self.folders(&shown.file_system));
        for question in Question::ALL {
            let refused = question == Question::Notifications && notifications_refused;
            sections.extend(self.question(question, shown.answers.get(question), refused));
            sections.extend(self.simulated(question, shown));
        }
        if self.of != SettingsOf::Global {
            sections.extend(self.link_schemes(&shown.link_schemes));
            sections.extend(self.roles(&shown.roles));
            sections.extend(self.answered_without_asking(shown));
        }
        sections
    }

    /// The program's answers to opening links of its own schemes, each with Remove; nothing
    /// before it asked to open any.
    fn link_schemes(&self, link_schemes: &[(String, Answer)]) -> Vec<WidgetRef> {
        if link_schemes.is_empty() {
            return Vec::new();
        }
        let mut rows = vec![
            self.setting_name(words::LINKS),
            caption(words::LINKS_DETAIL, &self.resources),
        ];
        for (scheme, answer) in link_schemes {
            let forget = SettingChange::LinkScheme(scheme.clone(), None);
            rows.push(self.listed(&words::link_scheme(scheme, *answer), forget));
        }
        rows
    }

    /// The program's answers to holding the roles it asked for, each with Remove; nothing before
    /// it asked for one.
    fn roles(&self, roles: &[(Role, Answer)]) -> Vec<WidgetRef> {
        if roles.is_empty() {
            return Vec::new();
        }
        let mut rows = vec![
            self.setting_name(words::ROLES),
            caption(words::ROLES_DETAIL, &self.resources),
        ];
        for (role, answer) in roles {
            let forget = SettingChange::Role(*role, None);
            rows.push(self.listed(&words::role(*role, *answer), forget));
        }
        rows
    }

    /// What the program's pickers and file dialogs are answered with in place of asking.
    fn answered_without_asking(&self, shown: &ShownSettings) -> Vec<WidgetRef> {
        let set = &shown.answer_with;
        let mut rows = vec![
            self.setting_name(words::ANSWERED_WITHOUT_ASKING),
            caption(words::ANSWERED_WITHOUT_ASKING_DETAIL, &self.resources),
        ];
        for dialog in FileDialog::ALL {
            rows.extend(self.file_dialog_answer(dialog, set));
        }
        rows.extend(self.output_answer(set, &shown.offered.outputs));
        rows.extend(self.machine_answer(set, &shown.offered.machines));
        rows
    }

    /// Ask, or one of the machines offered, the one set among them, by its id when it is not
    /// offered now.
    fn machine_answer(
        &self,
        set: &ShownAnswerWith,
        offered: &[waos::MachineChoice],
    ) -> Vec<WidgetRef> {
        let set_id = set.machine.as_ref().map(|machine| machine.id.as_str());
        let mut choices = vec![(
            words::ASK.to_owned(),
            set_id.is_none(),
            SettingChange::MachineAnswer(None),
        )];
        for machine in offered {
            let change = SettingChange::MachineAnswer(Some(machine.id.clone()));
            choices.push((
                machine.name.clone(),
                set_id == Some(machine.id.as_str()),
                change,
            ));
        }
        if let Some(machine) = &set.machine
            && !offered.iter().any(|each| each.id == machine.id)
        {
            let change = SettingChange::MachineAnswer(Some(machine.id.clone()));
            choices.push((words::not_offered(&machine.name), true, change));
        }
        vec![
            self.subsetting_name(words::WHICH_MACHINE),
            self.radio_column(choices),
        ]
    }

    /// Ask, or one of the outputs offered, the one set among them, by its id when it is not
    /// connected now.
    fn output_answer(
        &self,
        set: &ShownAnswerWith,
        offered: &[waos::AudioOutput],
    ) -> Vec<WidgetRef> {
        let set_id = set.audio_output.as_ref().map(|output| output.id.as_str());
        let mut choices = vec![(
            words::ASK.to_owned(),
            set_id.is_none(),
            SettingChange::AudioOutputAnswer(None),
        )];
        for output in offered {
            let change = SettingChange::AudioOutputAnswer(Some(output.id.clone()));
            choices.push((
                output.label.clone(),
                set_id == Some(output.id.as_str()),
                change,
            ));
        }
        if let Some(output) = &set.audio_output
            && !offered.iter().any(|each| each.id == output.id)
        {
            let change = SettingChange::AudioOutputAnswer(Some(output.id.clone()));
            choices.push((words::not_offered(&output.label), true, change));
        }
        vec![
            self.subsetting_name(words::WHICH_OUTPUT),
            self.radio_column(choices),
        ]
    }

    /// The paths `dialog` is answered with, or a field to set one: for the dialog that opens
    /// files, each path with Remove and a field to add one; for the others, the path with Ask
    /// instead.
    fn file_dialog_answer(&self, dialog: FileDialog, set: &ShownAnswerWith) -> Vec<WidgetRef> {
        let mut rows = vec![self.subsetting_name(words::file_dialog_name(dialog))];
        let paths = set.paths.file_dialog(dialog).unwrap_or_default();
        let placeholder = words::file_dialog_placeholder(dialog);
        let typed = self.typed.path(dialog).clone();
        if dialog == FileDialog::OpenFiles {
            for (index, path) in paths.iter().enumerate() {
                let remove = SettingChange::RemoveFileToOpen(index);
                rows.push(self.listed(&path.to_string_lossy(), remove));
            }
            let add = self.adding(typed, placeholder, words::ADD, SettingChange::AddFileToOpen);
            rows.push(add);
            return rows;
        }
        match paths.first() {
            Some(path) => {
                let ask = self.on(SettingChange::FileDialogAnswer(dialog, Vec::new()));
                rows.push(self.listed_with(&path.to_string_lossy(), words::ASK_INSTEAD, ask));
            }
            None => {
                let answer =
                    move |typed: String| SettingChange::FileDialogAnswer(dialog, vec![typed]);
                rows.push(self.adding(typed, placeholder, words::ANSWER_WITH, answer));
            }
        }
        rows
    }

    /// The machines the user picked for the program, each with Remove, under the warning.
    fn machines(&self, machines: &[waos::MachineChoice]) -> Vec<WidgetRef> {
        let primary = self.resources.text_fill_color_primary;
        let title = Text::new(words::MACHINES)
            .style(TextBlockStyle::BodyStrong.text_style(&self.resources, primary))
            .into_widget();
        let mut rows = vec![
            Padding::new(EdgeInsetsGeometry::only(0.0, MARGIN, 0.0, GAP / 2.0))
                .child(title)
                .into_widget(),
            caption(words::MACHINES_AS_YOU, &self.resources),
        ];
        if machines.is_empty() {
            rows.push(caption(words::NO_MACHINES, &self.resources));
        }
        for machine in machines {
            let remove = SettingChange::RemoveMachine(machine.id.clone());
            rows.push(self.listed(&machine.name, remove));
        }
        rows
    }

    fn network(&self, shown: &Shown<waos_store::NetworkGrant>) -> Vec<WidgetRef> {
        let network = &shown.value;
        let mut rows = self.header(words::NETWORK, shown.follows_global, Which::Network);
        rows.push(self.toggle(network.on, SettingChange::NetworkOn));
        let kind = domains_kind(&network.domains);
        rows.push(self.radios(&[
            (
                words::ANY_DOMAIN,
                kind == DomainsKind::Any,
                SettingChange::Domains(DomainsKind::Any),
            ),
            (
                words::ONLY_DOMAINS,
                kind == DomainsKind::Only,
                SettingChange::Domains(DomainsKind::Only),
            ),
            (
                words::ANY_DOMAIN_BUT,
                kind == DomainsKind::AnyBut,
                SettingChange::Domains(DomainsKind::AnyBut),
            ),
        ]));
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
        rows
    }

    /// A switch of its own: the clipboard's.
    fn switch(
        &self,
        name: &str,
        shown: &Shown<bool>,
        which: Which,
        change: fn(bool) -> SettingChange,
    ) -> Vec<WidgetRef> {
        let mut rows = self.header(name, shown.follows_global, which);
        rows.push(self.toggle(shown.value, change));
        rows
    }

    /// Whether a record is kept of what the program does, with what the record is.
    fn record(&self, shown: &Shown<bool>) -> Vec<WidgetRef> {
        let (name, detail) = words::record(&self.of);
        let mut rows = self.header(name, shown.follows_global, Which::Record);
        rows.push(caption(detail, &self.resources));
        rows.push(self.toggle(shown.value, SettingChange::Record));
        rows
    }

    /// What the program did over the days kept of it, a line per capability; that none is kept
    /// while its record is off.
    fn last_days(&self, activity: &Activity, recorded: bool) -> Vec<WidgetRef> {
        let mut rows = vec![self.setting_name(words::LAST_DAYS)];
        let lines = activity.short();
        if !recorded {
            rows.push(caption(words::NOTHING_RECORDED, &self.resources));
        } else if lines.is_empty() {
            rows.push(caption(words::NOTHING_USED, &self.resources));
        }
        rows.extend(lines.iter().map(|line| caption(line, &self.resources)));
        rows
    }

    fn folders(&self, shown: &Shown<waos_store::FileSystemGrant>) -> Vec<WidgetRef> {
        let mut rows = self.header(words::FOLDERS, shown.follows_global, Which::FileSystem);
        rows.push(caption(words::FOLDERS_AT_ONCE, &self.resources));
        if shown.value.mounts.is_empty() {
            rows.push(caption(words::NO_FOLDERS, &self.resources));
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
        rows
    }

    /// The answer `question` gets; under it, where the system `refused` the program it, that it
    /// does, with a button that opens the system's settings at it.
    fn question(
        &self,
        question: Question,
        shown: &Shown<Option<Answer>>,
        refused: bool,
    ) -> Vec<WidgetRef> {
        let name = words::question_name(question);
        let mut rows = self.header(name, shown.follows_global, Which::Answer(question));
        let answer = shown.value;
        let mut choices = Vec::new();
        if self.of == SettingsOf::Global {
            choices.push((
                words::ASK,
                answer.is_none(),
                SettingChange::Answer(question, None),
            ));
        }
        choices.push((
            words::ALLOW,
            answer == Some(Answer::Granted),
            SettingChange::Answer(question, Some(Answer::Granted)),
        ));
        choices.push((
            words::DONT_ALLOW,
            answer == Some(Answer::Denied),
            SettingChange::Answer(question, Some(Answer::Denied)),
        ));
        if simulatable(question) {
            choices.push((
                words::SIMULATED,
                answer == Some(Answer::Simulated),
                SettingChange::Answer(question, Some(Answer::Simulated)),
            ));
        }
        rows.push(self.radios(&choices));
        if refused {
            rows.extend(self.refused_by_system());
        }
        rows
    }

    /// While `question`'s answer is simulated, what the device of WaOS's own is, and for the
    /// microphone what it records; for the camera, where this system has none, that it does not.
    fn simulated(&self, question: Question, shown: &ShownSettings) -> Vec<WidgetRef> {
        if shown.answers.get(question).value != Some(Answer::Simulated) {
            return Vec::new();
        }
        match question {
            Question::Camera if !SIMULATED_CAMERA => {
                vec![caption(words::NO_SIMULATED_CAMERA, &self.resources)]
            }
            Question::Camera => vec![caption(words::SIMULATED_CAMERA_DETAIL, &self.resources)],
            Question::Microphone => {
                let sound = shown.simulated_microphone.value;
                let choice = |label, kind| {
                    (
                        label,
                        sound == kind,
                        SettingChange::SimulatedMicrophone(kind),
                    )
                };
                let mut rows = vec![caption(words::SIMULATED_MICROPHONE_DETAIL, &self.resources)];
                rows.extend(self.header(
                    words::SIMULATED_RECORDS,
                    shown.simulated_microphone.follows_global,
                    Which::SimulatedMicrophone,
                ));
                rows.push(self.radios(&[
                    choice(words::QUIET_TONE, SimulatedSound::Tone),
                    choice(words::SILENCE, SimulatedSound::Silence),
                ]));
                rows
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
        vec![
            caption(words::NOTIFICATIONS_REFUSED, &self.resources),
            Row::new()
                .children([Button::text(words::OPEN_SYSTEM_SETTINGS, open).into_widget()])
                .into_widget(),
        ]
    }

    /// A setting's name, and on a program's page whether it follows the setting for every
    /// program, or a button that makes it follow it again.
    fn header(&self, name: &str, follows_global: bool, which: Which) -> Vec<WidgetRef> {
        let mut rows = vec![self.setting_name(name)];
        if self.of == SettingsOf::Global {
            return rows;
        }
        if follows_global {
            rows.push(caption(words::FOLLOWS_GLOBAL, &self.resources));
        } else {
            let follow = self.on(SettingChange::FollowGlobal(which));
            rows.push(
                Row::new()
                    .children([Button::text(words::USE_GLOBAL, follow).into_widget()])
                    .into_widget(),
            );
        }
        rows
    }

    /// The name of one of a setting's parts, over its controls.
    fn subsetting_name(&self, name: &str) -> WidgetRef {
        let primary = self.resources.text_fill_color_primary;
        let title = Text::new(name)
            .style(TextBlockStyle::Body.text_style(&self.resources, primary))
            .into_widget();
        Padding::new(EdgeInsetsGeometry::only(0.0, GAP, 0.0, GAP / 2.0))
            .child(title)
            .into_widget()
    }

    /// A setting's name, over its controls.
    fn setting_name(&self, name: &str) -> WidgetRef {
        let primary = self.resources.text_fill_color_primary;
        let title = Text::new(name)
            .style(TextBlockStyle::BodyStrong.text_style(&self.resources, primary))
            .into_widget();
        Padding::new(EdgeInsetsGeometry::only(0.0, MARGIN, 0.0, GAP / 2.0))
            .child(title)
            .into_widget()
    }

    /// On the global page, where WaOS offers the choice, whether programs show inside WaOS, under
    /// what the choice does.
    fn where_programs_open(&self, app: &App) -> Vec<WidgetRef> {
        if self.of != SettingsOf::Global {
            return Vec::new();
        }
        let Some(inside) = self.library.read(app).programs_inside_waos() else {
            return Vec::new();
        };
        let mut rows = vec![self.setting_name(words::PROGRAMS_INSIDE_WAOS)];
        rows.push(caption(words::PROGRAMS_INSIDE_WAOS_DETAIL, &self.resources));
        let library = self.library.clone();
        rows.push(
            ToggleSwitch::new(inside, move |app, inside| {
                library.update(app, |library, cx| {
                    library.show_programs_inside_waos(inside, cx)
                });
            })
            .on_content(Text::new(words::ON))
            .off_content(Text::new(words::OFF))
            .into_widget(),
        );
        rows
    }

    /// On the global page, in a desktop's launcher, whether WaOS is kept in the Dock or the
    /// taskbar, under what turning it off does.
    fn where_waos_stays(&self, app: &App) -> Vec<WidgetRef> {
        if self.of != SettingsOf::Global {
            return Vec::new();
        }
        let Some(kept) = self.library.read(app).kept_in_dock() else {
            return Vec::new();
        };
        let platform = app.platform().target_platform();
        let library = self.library.clone();
        vec![
            self.setting_name(words::keep_in_dock(platform)),
            caption(words::keep_in_dock_detail(platform), &self.resources),
            ToggleSwitch::new(kept, move |app, kept| {
                library.update(app, |library, cx| library.keep_in_dock(kept, cx));
            })
            .on_content(Text::new(words::ON))
            .off_content(Text::new(words::OFF))
            .into_widget(),
        ]
    }

    /// On the global page, where WaOS reaches the phone's files only as the user allows, whether
    /// it may, and a button that opens the system's page for it.
    fn phone_files(&self, app: &App) -> Vec<WidgetRef> {
        if self.of != SettingsOf::Global {
            return Vec::new();
        }
        let Some(reachable) = self.library.read(app).phone_files_reachable(app) else {
            return Vec::new();
        };
        let library = self.library.clone();
        let open = Listener::new(move |app| {
            library.update(app, |library, cx| library.open_phone_files_page(cx));
        });
        let said = if reachable {
            words::PHONE_FILES_ON
        } else {
            words::PHONE_FILES_OFF
        };
        vec![
            self.setting_name(words::PHONE_FILES),
            caption(said, &self.resources),
            Row::new()
                .children([Button::text(words::OPEN_ANDROID_SETTINGS, open).into_widget()])
                .into_widget(),
        ]
    }

    fn toggle(&self, on: bool, change: fn(bool) -> SettingChange) -> WidgetRef {
        let (library, of) = (self.library.clone(), self.of.clone());
        ToggleSwitch::new(on, move |app, on| {
            library.update(app, |library, cx| {
                library.change_settings(&of, change(on), cx)
            });
        })
        .on_content(Text::new(words::ON))
        .off_content(Text::new(words::OFF))
        .into_widget()
    }

    fn radios(&self, choices: &[(&str, bool, SettingChange)]) -> WidgetRef {
        Row::new()
            .spacing(MARGIN)
            .children(choices.iter().map(|(label, checked, change)| {
                let listener = self.on(change.clone());
                RadioButton::new(*checked, move |app| listener.call(app))
                    .content(Text::new(*label))
                    .into_widget()
            }))
            .into_widget()
    }

    /// Choices one under another, one of them `checked`, each making its change.
    fn radio_column(&self, choices: Vec<(String, bool, SettingChange)>) -> WidgetRef {
        Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .spacing(GAP / 2.0)
            .children(choices.into_iter().map(|(label, checked, change)| {
                let listener = self.on(change);
                RadioButton::new(checked, move |app| listener.call(app))
                    .content(Text::new(label))
                    .into_widget()
            }))
            .into_widget()
    }

    /// One of a rule's list, with Remove.
    fn listed(&self, text: &str, remove: SettingChange) -> WidgetRef {
        self.listed_with(text, words::REMOVE, self.on(remove))
    }

    /// `text`, with a button `label` doing `click`.
    fn listed_with(&self, text: &str, label: &str, click: Listener) -> WidgetRef {
        Row::new()
            .spacing(GAP)
            .children([
                Expanded::new(Text::new(text)).into_widget(),
                Button::text(label, click).into_widget(),
            ])
            .into_widget()
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
                        writable_box.into_widget(),
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

fn caption(text: &str, resources: &ThemeResources) -> WidgetRef {
    Text::new(text)
        .style(TextBlockStyle::Caption.text_style(resources, resources.text_fill_color_secondary))
        .into_widget()
}
