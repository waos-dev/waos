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
use std::hash::Hash;
use std::rc::Rc;

use indexmap::IndexMap;
use waft::{
    App, BuildContext, EdgeInsetsGeometry, Entity, Handle, Icon, IntoWidget, ListView, Listener,
    Padding, SizedBox, State, StateData, StatefulWidget, Task, Text, TextEditingController,
    TextEditingControllerLeaf, ValueChanged, WidgetRef,
};
use waft_cupertino::{
    CupertinoButton, CupertinoColors, CupertinoIcons, CupertinoListSection, CupertinoListTile,
    CupertinoNavigationBar, CupertinoPageScaffold, CupertinoSegmentedControl, CupertinoSwitch,
    CupertinoTextField, ObstructingPreferredSizeWidgetRef,
};
use waos::{FileDialog, SimulatedSound};
use waos_store::{Answer, DomainRule, FileSystemGrant, NetworkGrant, Question, Role};

use crate::logic::setting_changes::{
    DomainsKind, SettingChange, SettingsOf, Shown, ShownAnswerWith, ShownSettings, Which,
    domains_kind, simulatable,
};
use crate::state::SIMULATED_CAMERA;
use crate::state::library::Library;
use crate::ui::library::cupertino::{back_to_home, under_bar};
use crate::ui::library::words;

/// The settings `of` shows, as a page of its own.
#[derive(Debug)]
pub(crate) struct CupertinoSettings {
    pub(crate) library: Entity<Library>,
    pub(crate) of: SettingsOf,
    /// The program's name, or none on the global page.
    pub(crate) program_name: Option<String>,
}

impl StatefulWidget for CupertinoSettings {
    type State = CupertinoSettingsState;

    fn create_state(&self) -> CupertinoSettingsState {
        CupertinoSettingsState {
            state: StateData::new(),
            typed: None,
            writable: false,
        }
    }
}

pub(crate) struct CupertinoSettingsState {
    state: StateData<CupertinoSettings>,
    typed: Option<Typed>,
    /// Whether a folder added is writable.
    writable: bool,
}

/// What is being typed on the page.
#[derive(Clone, Copy)]
struct Typed {
    domain: Handle<TextEditingController>,
    folder: Handle<TextEditingController>,
    folder_at: Handle<TextEditingController>,
    /// A path for each file dialog, in [`FileDialog::ALL`]'s order.
    paths: [Handle<TextEditingController>; 4],
}

impl Typed {
    fn path(&self, dialog: FileDialog) -> Handle<TextEditingController> {
        let index = FileDialog::ALL.iter().position(|each| *each == dialog);
        self.paths[index.expect("every dialog")]
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
                app.get_mut(controller).dispose();
                app.destroy(controller);
            }
        }
    }

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        let widget = self.widget(app);
        let page = Page {
            library: widget.library.clone(),
            of: widget.of.clone(),
            typed: app.get(self).typed.expect("an initialized page"),
            writable: app.get(self).writable,
            state: self,
        };
        let title = match &widget.program_name {
            Some(name) => words::settings_of(name),
            None => words::GLOBAL_SETTINGS.to_owned(),
        };
        let mut sections = Vec::new();
        sections.extend(page.switch_asked(app));
        sections.extend(page.where_programs_open(app));
        sections.extend(page.where_waos_stays(app));
        match page.library.read(app).settings(&page.of) {
            Ok(shown) => sections.extend(page.sections(&shown, page.notifications_refused(app))),
            Err(why) => sections.push(section(Some(words::SETTINGS), None, vec![note(&why)])),
        }
        let bar = CupertinoNavigationBar::new()
            .leading(back_to_home(&page.library))
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
}

/// What one build of the page reads and writes.
struct Page {
    library: Entity<Library>,
    of: SettingsOf,
    typed: Typed,
    writable: bool,
    state: Handle<CupertinoSettingsState>,
}

impl Page {
    /// What a program asked for, when it asked for one of its switches here.
    fn switch_asked(&self, app: &App) -> Option<WidgetRef> {
        let asked = self.library.read(app).switch_asked()?;
        let message = words::switch_asked(&asked.name, asked.switch);
        Some(section(Some(words::SETTINGS), None, vec![note(&message)]))
    }

    /// On the global page, where WaOS offers the choice, whether programs show inside WaOS: a row
    /// whose switch, or a tap anywhere on it, changes it.
    fn where_programs_open(&self, app: &App) -> Option<WidgetRef> {
        if self.of != SettingsOf::Global {
            return None;
        }
        let inside = self.library.read(app).programs_inside_waos()?;
        let library = self.library.clone();
        let flip: ValueChanged<bool> = Rc::new(move |app: &mut App, inside: bool| {
            library.update(app, |library, cx| {
                library.show_programs_inside_waos(inside, cx)
            });
        });
        let row_flip = Rc::clone(&flip);
        let row = CupertinoListTile::new(spanning(words::PROGRAMS_INSIDE_WAOS))
            .trailing(CupertinoSwitch::new(inside, Some(flip)))
            .on_tap(Rc::new(move |app: &mut App| {
                row_flip(app, !inside);
                Task::ready(())
            }))
            .into_widget();
        Some(section(
            Some(words::WHERE_PROGRAMS_OPEN),
            Some(words::PROGRAMS_INSIDE_WAOS_DETAIL),
            vec![row],
        ))
    }

    /// On the global page, in a desktop's launcher, whether WaOS is kept in the Dock or the
    /// taskbar: a row whose switch, or a tap anywhere on it, changes it.
    fn where_waos_stays(&self, app: &App) -> Option<WidgetRef> {
        if self.of != SettingsOf::Global {
            return None;
        }
        let kept = self.library.read(app).kept_in_dock()?;
        let platform = app.platform().target_platform();
        let library = self.library.clone();
        let flip: ValueChanged<bool> = Rc::new(move |app: &mut App, kept: bool| {
            library.update(app, |library, cx| library.keep_in_dock(kept, cx));
        });
        let row_flip = Rc::clone(&flip);
        let row = CupertinoListTile::new(spanning(words::keep_in_dock(platform)))
            .trailing(CupertinoSwitch::new(kept, Some(flip)))
            .on_tap(Rc::new(move |app: &mut App| {
                row_flip(app, !kept);
                Task::ready(())
            }))
            .into_widget();
        Some(section(
            Some(words::WHERE_WAOS_STAYS),
            Some(words::keep_in_dock_detail(platform)),
            vec![row],
        ))
    }

    /// Whether the system refuses the page's program its notifications; never on the global
    /// page.
    fn notifications_refused(&self, app: &App) -> bool {
        match &self.of {
            SettingsOf::Program(id) => self.library.read(app).notifications_refused(id),
            SettingsOf::Global => false,
        }
    }

    fn sections(&self, shown: &ShownSettings, notifications_refused: bool) -> Vec<WidgetRef> {
        let mut sections = Vec::new();
        if self.of != SettingsOf::Global {
            sections.push(self.machines(&shown.machines));
        }
        sections.extend([
            self.network(&shown.network),
            self.switch(
                words::LOCAL_NETWORK,
                &shown.local_network,
                Which::LocalNetwork,
                SettingChange::LocalNetwork,
            ),
            self.switch(
                words::CLIPBOARD_READ,
                &shown.clipboard_read,
                Which::ClipboardRead,
                SettingChange::ClipboardRead,
            ),
            self.switch(
                words::CLIPBOARD_WRITE,
                &shown.clipboard_write,
                Which::ClipboardWrite,
                SettingChange::ClipboardWrite,
            ),
            self.folders(&shown.file_system),
        ]);
        for question in Question::ALL {
            let refused = question == Question::Notifications && notifications_refused;
            sections.push(self.question(question, shown, refused));
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

    /// What the program's pickers and file dialogs are answered with in place of asking: a
    /// section each, the first saying what they are.
    fn answered_without_asking(&self, shown: &ShownSettings) -> Vec<WidgetRef> {
        let set = &shown.answer_with;
        let mut sections = vec![section(
            Some(words::ANSWERED_WITHOUT_ASKING),
            None,
            vec![note(words::ANSWERED_WITHOUT_ASKING_DETAIL)],
        )];
        for dialog in FileDialog::ALL {
            sections.push(self.file_dialog_answer(dialog, set));
        }
        sections.push(self.output_answer(set, &shown.offered.outputs));
        sections.push(self.machine_answer(set, &shown.offered.machines));
        sections
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
        section(Some(words::WHICH_OUTPUT), None, tiles)
    }

    /// The paths `dialog` is answered with, or a field to set one: for the dialog that opens
    /// files, each path with Remove and a field to add one; for the others, the path with Ask
    /// instead.
    fn file_dialog_answer(&self, dialog: FileDialog, set: &ShownAnswerWith) -> WidgetRef {
        let paths = set.paths.file_dialog(dialog).unwrap_or_default();
        let placeholder = words::file_dialog_placeholder(dialog);
        let typed = self.typed.path(dialog);
        let mut tiles = Vec::new();
        if dialog == FileDialog::OpenFiles {
            for (index, path) in paths.iter().enumerate() {
                let remove = SettingChange::RemoveFileToOpen(index);
                tiles.push(self.listed(&path.to_string_lossy(), remove));
            }
            tiles.push(self.adding(typed, placeholder, words::ADD, SettingChange::AddFileToOpen));
        } else {
            match paths.first() {
                Some(path) => {
                    let ask = self.on(SettingChange::FileDialogAnswer(dialog, Vec::new()));
                    tiles.push(self.listed_with(&path.to_string_lossy(), words::ASK_INSTEAD, ask));
                }
                None => {
                    let answer =
                        move |typed: String| SettingChange::FileDialogAnswer(dialog, vec![typed]);
                    tiles.push(self.adding(typed, placeholder, words::ANSWER_WITH, answer));
                }
            }
        }
        section(Some(words::file_dialog_name(dialog)), None, tiles)
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
            tile.trailing(Icon::new(Some(CupertinoIcons::check_mark())))
                .into_widget()
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
            tiles.push(note(words::NO_MACHINES));
        }
        section(Some(words::MACHINES), Some(words::MACHINES_AS_YOU), tiles)
    }

    /// The network's switch, its rule of domains beneath it.
    fn network(&self, shown: &Shown<NetworkGrant>) -> WidgetRef {
        let network = &shown.value;
        let mut tiles = vec![self.switch_row(words::NETWORK, network.on, SettingChange::NetworkOn)];
        let choices = [
            (DomainsKind::Any, words::ANY_DOMAIN),
            (DomainsKind::Only, words::ONLY_DOMAINS),
            (DomainsKind::AnyBut, words::ANY_DOMAIN_BUT),
        ];
        tiles.push(self.choice(
            &choices,
            domains_kind(&network.domains),
            SettingChange::Domains,
        ));
        if let DomainRule::Only(domains) | DomainRule::AnyBut(domains) = &network.domains {
            for domain in domains {
                tiles.push(
                    self.listed(domain.as_str(), SettingChange::RemoveDomain(domain.clone())),
                );
            }
            tiles.push(self.adding(
                self.typed.domain,
                words::DOMAIN_PLACEHOLDER,
                words::ADD,
                SettingChange::AddDomain,
            ));
        }
        self.setting(None, shown.follows_global, Which::Network, tiles)
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
        self.setting(None, shown.follows_global, which, tiles)
    }

    fn folders(&self, shown: &Shown<FileSystemGrant>) -> WidgetRef {
        let mut tiles = vec![note(words::FOLDERS_AT_ONCE)];
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
            shown.follows_global,
            Which::FileSystem,
            tiles,
        )
    }

    /// The answer `question` gets; under it, where the system `refused` the program it, that it
    /// does, with a button that opens the system's settings at it.
    fn question(&self, question: Question, settings: &ShownSettings, refused: bool) -> WidgetRef {
        let shown = settings.answers.get(question);
        let mut choices = Vec::new();
        if self.of == SettingsOf::Global {
            choices.push((None, words::ASK));
        }
        choices.push((Some(Answer::Granted), words::ALLOW));
        choices.push((Some(Answer::Denied), words::DONT_ALLOW));
        if simulatable(question) {
            choices.push((Some(Answer::Simulated), words::SIMULATED));
        }
        let answer = shown.value;
        let mut tiles = vec![self.choice(&choices, answer, move |answer| {
            SettingChange::Answer(question, answer)
        })];
        if answer == Some(Answer::Simulated) {
            tiles.extend(self.simulated(question, &settings.simulated_microphone));
        }
        if refused {
            tiles.extend(self.refused_by_system());
        }
        let name = words::question_name(question);
        let which = Which::Answer(question);
        self.setting(Some(name), shown.follows_global, which, tiles)
    }

    /// What `question`'s device of WaOS's own is, and for the microphone what it records, `sound`
    /// now, which on a program's page that does not follow every program's can follow it again;
    /// for the camera, where this system has none, that it does not.
    fn simulated(&self, question: Question, sound: &Shown<SimulatedSound>) -> Vec<WidgetRef> {
        match question {
            Question::Camera if !SIMULATED_CAMERA => vec![note(words::NO_SIMULATED_CAMERA)],
            Question::Camera => vec![note(words::SIMULATED_CAMERA_DETAIL)],
            Question::Microphone => {
                let choices = [
                    (SimulatedSound::Tone, words::QUIET_TONE),
                    (SimulatedSound::Silence, words::SILENCE),
                ];
                let mut tiles = vec![
                    note(words::SIMULATED_MICROPHONE_DETAIL),
                    self.choice(&choices, sound.value, SettingChange::SimulatedMicrophone),
                ];
                if self.of != SettingsOf::Global && !sound.follows_global {
                    let follow = self.on(SettingChange::FollowGlobal(Which::SimulatedMicrophone));
                    tiles.push(
                        CupertinoListTile::new(Text::new(""))
                            .trailing(CupertinoButton::new(
                                Text::new(words::USE_GLOBAL).into_widget(),
                                Some(follow),
                            ))
                            .into_widget(),
                    );
                }
                tiles
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
            note(words::NOTIFICATIONS_REFUSED),
            CupertinoListTile::new(Text::new(""))
                .trailing(CupertinoButton::new(
                    Text::new(words::OPEN_SYSTEM_SETTINGS).into_widget(),
                    Some(open),
                ))
                .into_widget(),
        ]
    }

    /// A setting's section, under `header` where given: on a program's page, it says whether it
    /// follows the setting for every program, and offers to follow it again when it does not.
    fn setting(
        &self,
        header: Option<&str>,
        follows_global: bool,
        which: Which,
        mut tiles: Vec<WidgetRef>,
    ) -> WidgetRef {
        if self.of == SettingsOf::Global {
            return section(header, None, tiles);
        }
        if follows_global {
            return section(header, Some(words::FOLLOWS_GLOBAL), tiles);
        }
        let follow = self.on(SettingChange::FollowGlobal(which));
        tiles.push(
            CupertinoListTile::new(Text::new(""))
                .trailing(CupertinoButton::new(
                    Text::new(words::USE_GLOBAL).into_widget(),
                    Some(follow),
                ))
                .into_widget(),
        );
        section(header, None, tiles)
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
    fn choice<T: Copy + Eq + Hash + Debug + 'static>(
        &self,
        choices: &[(T, &str)],
        selected: T,
        change: impl Fn(T) -> SettingChange + 'static,
    ) -> WidgetRef {
        let segments: IndexMap<T, WidgetRef> = choices
            .iter()
            .map(|(value, label)| {
                let label = Padding::new(EdgeInsetsGeometry::symmetric(6.0, 10.0))
                    .child(Text::new(*label))
                    .into_widget();
                (*value, label)
            })
            .collect();
        let (library, of) = (self.library.clone(), self.of.clone());
        let chosen = Rc::new(move |app: &mut App, value: T| {
            if value != selected {
                library.update(app, |library, cx| {
                    library.change_settings(&of, change(value), cx)
                });
            }
        });
        let mut control = CupertinoSegmentedControl::new(segments, chosen);
        if choices.iter().any(|(value, _)| *value == selected) {
            control = control.group_value(selected);
        }
        Padding::new(EdgeInsetsGeometry::symmetric(8.0, 16.0))
            .child(control)
            .into_widget()
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
        typed: Handle<TextEditingController>,
        placeholder: &str,
        confirm: &str,
        change: impl Fn(String) -> SettingChange + 'static,
    ) -> WidgetRef {
        let (library, of) = (self.library.clone(), self.of.clone());
        let add = Listener::new(move |app| {
            let text = typed.text_value(app).to_owned();
            library.update(app, |library, cx| {
                library.change_settings(&of, change(text), cx)
            });
            if library.read(app).problem().is_none() {
                typed.clear(app);
            }
        });
        let field = CupertinoTextField::new()
            .controller(typed.as_controller())
            .placeholder(placeholder);
        CupertinoListTile::new(field)
            .trailing(CupertinoButton::new(
                Text::new(confirm).into_widget(),
                Some(add),
            ))
            .into_widget()
    }

    /// The fields of a folder to add, whether it is writable, and Add.
    fn adding_folder(&self) -> Vec<WidgetRef> {
        let (library, of, typed, writable) = (
            self.library.clone(),
            self.of.clone(),
            self.typed,
            self.writable,
        );
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
        let segments: IndexMap<bool, WidgetRef> =
            [(false, words::READ_ONLY), (true, words::WRITABLE)]
                .into_iter()
                .map(|(value, label)| {
                    let label = Padding::new(EdgeInsetsGeometry::symmetric(6.0, 10.0))
                        .child(Text::new(label))
                        .into_widget();
                    (value, label)
                })
                .collect();
        let writable_choice = CupertinoSegmentedControl::new(
            segments,
            Rc::new(move |app: &mut App, writable: bool| state.set_writable(app, writable)),
        )
        .group_value(self.writable);
        vec![
            CupertinoListTile::new(
                CupertinoTextField::new()
                    .controller(typed.folder.as_controller())
                    .placeholder(words::FOLDER_PLACEHOLDER),
            )
            .into_widget(),
            CupertinoListTile::new(
                CupertinoTextField::new()
                    .controller(typed.folder_at.as_controller())
                    .placeholder(words::FOLDER_AT_PLACEHOLDER),
            )
            .into_widget(),
            CupertinoListTile::new(writable_choice)
                .trailing(CupertinoButton::new(
                    Text::new(words::ADD).into_widget(),
                    Some(add),
                ))
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

/// An inset-grouped section, titled `title` and with `footer` under it where given.
fn section(title: Option<&str>, footer: Option<&str>, tiles: Vec<WidgetRef>) -> WidgetRef {
    let section = CupertinoListSection::inset_grouped().children(tiles);
    let section = match title {
        Some(title) => section.header(Text::new(title)),
        None => section,
    };
    match footer {
        Some(footer) => section.footer(Text::new(footer)).into_widget(),
        None => section.into_widget(),
    }
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

/// A tile that only says `text`.
fn note(text: &str) -> WidgetRef {
    CupertinoListTile::new(Text::new(text)).into_widget()
}
