//! The settings' first page at a phone's size, in Cupertino's widgets, as iOS's Settings leads
//! to its pages: Back to the home, then inset lists of rows with a chevron: WaOS's own
//! settings, where this system has any; the settings every app follows, over what they are;
//! and, under Apps, each installed app with its icon.
//!
//! What it shows is the library's, read as it builds; it keeps nothing of its own.

use std::rc::Rc;

use waft::{
    App, BuildContext, EdgeInsetsGeometry, Entity, IntoWidget, ListView, StatelessWidget, Task,
    Text, WidgetRef,
};
use waft_cupertino::{
    CupertinoColors, CupertinoListSection, CupertinoListTile, CupertinoListTileChevron,
    CupertinoNavigationBar, CupertinoPageScaffold, ObstructingPreferredSizeWidgetRef,
};

use super::{listed_apps, waos_has_settings};
use crate::state::library::{Library, LibraryPage, SettingsPage};
use crate::ui::library::cupertino::{back_to_home, under_bar};
use crate::ui::library::program_icon::ProgramIcon;
use crate::ui::library::words;

/// The side of an app's icon in its row, as iOS's Settings shows its apps.
const ICON_SIDE: f64 = 30.0;

/// The settings' first page, in Cupertino's widgets.
#[derive(Debug)]
pub(crate) struct CupertinoSettingsRoot {
    pub(crate) library: Entity<Library>,
}

impl StatelessWidget for CupertinoSettingsRoot {
    fn build(&self, app: &mut App, _context: BuildContext) -> WidgetRef {
        let library = self.library.read(app);
        let mut sections = Vec::new();
        if waos_has_settings(library, app) {
            let waos = self.row(Text::new(words::WAOS_SETTINGS), SettingsPage::Waos);
            sections.push(
                CupertinoListSection::inset_grouped()
                    .children(vec![waos.into_widget()])
                    .into_widget(),
            );
        }
        let every_app = self.row(Text::new(words::EVERY_APP), SettingsPage::EveryApp);
        sections.push(
            CupertinoListSection::inset_grouped()
                .children(vec![every_app.into_widget()])
                .footer(Text::new(words::GLOBAL_DETAIL))
                .into_widget(),
        );
        let apps: Vec<WidgetRef> = listed_apps(library)
            .into_iter()
            .map(|listed| {
                let icon = ProgramIcon {
                    png: listed.icon,
                    side: ICON_SIDE,
                };
                self.row(Text::new(listed.name), SettingsPage::App(listed.id))
                    .leading(icon)
                    .leading_size(ICON_SIDE)
                    .into_widget()
            })
            .collect();
        if !apps.is_empty() {
            sections.push(
                CupertinoListSection::inset_grouped()
                    .header(Text::new(words::APPS))
                    .children(apps)
                    .into_widget(),
            );
        }
        let bar = CupertinoNavigationBar::new()
            .leading(back_to_home(&self.library))
            .middle(Text::new(words::SETTINGS));
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

impl CupertinoSettingsRoot {
    /// A row that leads to `page`: `name`, and a chevron; a tap on it opens the page.
    fn row(&self, name: Text, page: SettingsPage) -> CupertinoListTile {
        let library = self.library.clone();
        let open = Rc::new(move |app: &mut App| {
            let page = LibraryPage::Settings(page.clone());
            library.update(app, |library, cx| library.show_page(page, cx));
            Task::ready(())
        });
        CupertinoListTile::new(name)
            .trailing(CupertinoListTileChevron::new())
            .on_tap(open)
    }
}
