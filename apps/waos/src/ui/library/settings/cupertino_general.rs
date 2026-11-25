//! WaOS's own settings at a phone's size, in Cupertino's widgets: Back to the settings' first
//! page, then an inset list for each setting this system offers, under its name and over what
//! it does: whether apps show inside WaOS, whether WaOS is kept in the Dock or the taskbar, and
//! whether WaOS reaches the phone's files, with the button that opens the system's page for it,
//! and whether computers on the network may reach WaOS's dev server.
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

#[cfg(not(any(target_os = "android", target_os = "ios")))]
use super::developer_access_said;
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
        let sections: Vec<WidgetRef> = [
            self.where_programs_open(app),
            self.where_waos_stays(app),
            self.phone_files(app),
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            self.developer_access(app),
        ]
        .into_iter()
        .flatten()
        .collect();
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
        let row = CupertinoListTile::new(Text::new(""))
            .trailing(CupertinoButton::new(
                Text::new(words::OPEN_ANDROID_SETTINGS).into_widget(),
                Some(open),
            ))
            .into_widget();
        Some(section(words::PHONE_FILES, said, row))
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    /// Where WaOS offers it, whether computers on the network may reach WaOS's dev server: a row
    /// whose switch, or a tap anywhere on it, changes it, over what that does and whether it
    /// listens.
    fn developer_access(&self, app: &App) -> Option<WidgetRef> {
        let library = self.library.read(app);
        let on = library.developer_access_on()?;
        let said = developer_access_said(library.developer_listening());
        let library = self.library.clone();
        let row = switch_row(
            words::DEVELOPER_ACCESS,
            on,
            Rc::new(move |app: &mut App, on: bool| {
                library.update(app, |library, cx| library.turn_developer_access(on, cx));
            }),
        );
        Some(section(words::DEVELOPER_ACCESS, &said, row))
    }
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
