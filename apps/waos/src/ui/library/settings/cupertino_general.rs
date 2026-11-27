//! WaOS's own settings at a phone's size, in Cupertino's widgets: Back to the settings' first
//! page, then an inset list for each setting this system offers, under its name and over what
//! it does: whether apps show inside WaOS, whether WaOS is kept in the Dock or the taskbar, and
//! whether WaOS reaches the phone's files, with the button that opens the system's page for it,
//! and whether computers on the network may reach WaOS's dev server, with, while they may, the
//! button that shows a code to pair one with and the computers paired, each with a button that
//! removes it.
//!
//! What it shows is the library's, read as it builds; it keeps nothing of its own.

use std::rc::Rc;

use waft::{
    App, BuildContext, EdgeInsetsGeometry, Entity, IntoWidget, ListView, Listener, SizedBox,
    StatelessWidget, Task, Text, ValueChanged, WidgetRef,
};
use waft_cupertino::{
    CupertinoButton, CupertinoColors, CupertinoListSection, CupertinoListTile,
    CupertinoNavigationBar, CupertinoPageScaffold, CupertinoSwitch,
    ObstructingPreferredSizeWidgetRef,
};

#[cfg(desktop)]
use super::developer_access_said;
#[cfg(desktop)]
use crate::logic::developer_access::PairedComputer;
use crate::state::library::Library;
use crate::ui::library::cupertino::{back_to_page_beneath, under_bar};
use crate::ui::library::words;

/// WaOS's own settings, in Cupertino's widgets.
#[derive(Debug)]
pub(crate) struct CupertinoGeneralSettings {
    pub(crate) library: Entity<Library>,
}

impl StatelessWidget for CupertinoGeneralSettings {
    fn build(&self, app: &mut App, _context: BuildContext) -> WidgetRef {
        let sections = [
            self.where_programs_open(app),
            self.where_waos_stays(app),
            self.phone_files(app),
        ]
        .into_iter()
        .flatten();
        #[cfg(desktop)]
        let sections = sections.chain(self.developer_access(app));
        let sections: Vec<WidgetRef> = sections.collect();
        let bar = CupertinoNavigationBar::new()
            .leading(back_to_page_beneath(&self.library, words::SETTINGS))
            .middle(Text::new(words::WAOS_SETTINGS));
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

impl CupertinoGeneralSettings {
    /// Where WaOS offers the choice, whether apps show inside WaOS: a row whose switch, or a
    /// tap anywhere on it, changes it.
    fn where_programs_open(&self, app: &App) -> Option<WidgetRef> {
        let inside = self.library.read(app).programs_inside_waos()?;
        let library = self.library.clone();
        let row = switch_row(
            words::PROGRAMS_INSIDE_WAOS,
            inside,
            Rc::new(move |app: &mut App, inside: bool| {
                library.update(app, |library, cx| {
                    library.show_programs_inside_waos(inside, cx)
                });
            }),
        );
        Some(section(
            words::WHERE_PROGRAMS_OPEN,
            words::PROGRAMS_INSIDE_WAOS_DETAIL,
            row,
        ))
    }

    /// In a desktop's launcher, whether WaOS is kept in the Dock or the taskbar: a row whose
    /// switch, or a tap anywhere on it, changes it.
    fn where_waos_stays(&self, app: &App) -> Option<WidgetRef> {
        let kept = self.library.read(app).kept_in_dock()?;
        let platform = app.platform().target_platform();
        let library = self.library.clone();
        let row = switch_row(
            words::keep_in_dock(platform),
            kept,
            Rc::new(move |app: &mut App, kept: bool| {
                library.update(app, |library, cx| library.keep_in_dock(kept, cx));
            }),
        );
        Some(section(
            words::WHERE_WAOS_STAYS,
            words::keep_in_dock_detail(platform),
            row,
        ))
    }

    /// Where WaOS reaches the phone's files only as the user allows, whether it may, and a
    /// button that opens the system's page for it.
    fn phone_files(&self, app: &App) -> Option<WidgetRef> {
        let reachable = self.library.read(app).phone_files_reachable(app)?;
        let library = self.library.clone();
        let open = Listener::new(move |app| {
            library.update(app, |library, cx| library.open_phone_files_page(cx));
        });
        let said = if reachable {
            words::PHONE_FILES_ON
        } else {
            words::PHONE_FILES_OFF
        };
        let row = button_row(words::OPEN_ANDROID_SETTINGS, open);
        Some(section(words::PHONE_FILES, said, row))
    }

    #[cfg(desktop)]
    /// Where WaOS offers it, whether computers on the network may reach WaOS's dev server: a row
    /// whose switch, or a tap anywhere on it, changes it, over what that does and whether it
    /// listens.
    fn developer_access(&self, app: &App) -> Vec<WidgetRef> {
        let library = self.library.read(app);
        let Some(on) = library.developer_access_on() else {
            return Vec::new();
        };
        let said = developer_access_said(library.developer_listening());
        let entity = self.library.clone();
        let row = switch_row(
            words::DEVELOPER_ACCESS,
            on,
            Rc::new(move |app: &mut App, on: bool| {
                entity.update(app, |library, cx| library.turn_developer_access(on, cx));
            }),
        );
        let switch = section(words::DEVELOPER_ACCESS, &said, row);
        if !on {
            return vec![switch];
        }
        let paired = library.paired_computers().unwrap_or_default();
        let pairing = self.pairing(library.pairing_code());
        vec![switch, pairing, self.paired_computers(paired)]
    }

    #[cfg(desktop)]
    /// The code shown for a computer to pair with, or the button that shows one.
    fn pairing(&self, code: Option<waos_dev_protocol::PairingCode>) -> WidgetRef {
        let library = self.library.clone();
        let show = Listener::new(move |app| {
            library.update(app, |library, cx| library.show_pairing_code(cx));
        });
        let said = match &code {
            Some(code) => words::pairing_code(code.as_str()),
            None => words::PAIR_A_COMPUTER_DETAIL.to_owned(),
        };
        let row = button_row(words::SHOW_A_CODE, show);
        section(words::PAIR_A_COMPUTER, &said, row)
    }

    #[cfg(desktop)]
    /// The computers `paired`, each with the button that removes it; or that there are none.
    fn paired_computers(&self, paired: Vec<PairedComputer>) -> WidgetRef {
        let rows: Vec<WidgetRef> = paired
            .into_iter()
            .map(|computer| {
                let library = self.library.clone();
                let name = computer.name.clone();
                let remove = Listener::new(move |app| {
                    library.update(app, |library, cx| library.remove_paired_computer(&name, cx));
                });
                CupertinoListTile::new(Text::new(computer.name))
                    .trailing(CupertinoButton::new(
                        Text::new(words::REMOVE).into_widget(),
                        Some(remove),
                    ))
                    .into_widget()
            })
            .collect();
        let section =
            CupertinoListSection::inset_grouped().header(Text::new(words::PAIRED_COMPUTERS));
        if rows.is_empty() {
            let none = CupertinoListTile::new(Text::new(words::NO_PAIRED_COMPUTERS)).into_widget();
            return section.children(vec![none]).into_widget();
        }
        section.children(rows).into_widget()
    }
}

/// A row with one button at its trailing side, saying `label` and doing `click`.
fn button_row(label: &str, click: Listener) -> WidgetRef {
    CupertinoListTile::new(Text::new(""))
        .trailing(CupertinoButton::new(
            Text::new(label).into_widget(),
            Some(click),
        ))
        .into_widget()
}

/// A row naming a setting that is `on` or off, its switch at the trailing side; a tap on the row
/// flips the setting as a tap on the switch does.
fn switch_row(name: &str, on: bool, flip: ValueChanged<bool>) -> WidgetRef {
    let row_flip = Rc::clone(&flip);
    // A list tile cuts its title to one line; a name across the whole width wraps instead.
    let name = SizedBox::new().width(f64::INFINITY).child(Text::new(name));
    CupertinoListTile::new(name)
        .trailing(CupertinoSwitch::new(on, Some(flip)))
        .on_tap(Rc::new(move |app: &mut App| {
            row_flip(app, !on);
            Task::ready(())
        }))
        .into_widget()
}

/// An inset list of one `row`, under `title` and over `said`, what the setting does.
fn section(title: &str, said: &str, row: WidgetRef) -> WidgetRef {
    CupertinoListSection::inset_grouped()
        .header(Text::new(title))
        .footer(Text::new(said))
        .children(vec![row])
        .into_widget()
}
