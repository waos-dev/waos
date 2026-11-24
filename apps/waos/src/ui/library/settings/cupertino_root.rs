//! The settings' pages that lead to others, at a phone's size, in Cupertino's widgets, as iOS's
//! Settings leads to its pages: inset lists of rows with a chevron. The first page leads to
//! General, where this system has settings of WaOS's own, to Permissions, and, under Apps, to
//! each installed app with its icon; Permissions leads to each kind, its default at the row's
//! end.
//!
//! What they show is the library's, read as they build; they keep nothing of their own.

use std::rc::Rc;

use waft::{
    App, BuildContext, EdgeInsetsGeometry, Entity, IntoWidget, ListView, StatelessWidget, Task,
    Text, WidgetRef,
};
use waft_cupertino::{
    CupertinoColors, CupertinoListSection, CupertinoListTile, CupertinoListTileChevron,
    CupertinoNavigationBar, CupertinoPageScaffold, ObstructingPreferredSizeWidgetRef,
};

use super::{listed_apps, permission_defaults, waos_has_settings};
use crate::state::library::{Library, LibraryPage, SettingsPage};
use crate::ui::library::cupertino::{back_to_home, back_to_page_beneath, under_bar};
use crate::ui::library::program_icon::ProgramIcon;
use crate::ui::library::words;

/// The side of an app's icon in its row, as iOS's Settings shows its apps.
pub(crate) const ICON_SIDE: f64 = 30.0;

/// The settings' first page, in Cupertino's widgets.
#[derive(Debug)]
pub(crate) struct CupertinoSettingsRoot {
    pub(crate) library: Entity<Library>,
}

impl StatelessWidget for CupertinoSettingsRoot {
    fn build(&self, app: &mut App, _context: BuildContext) -> WidgetRef {
        let library = self.library.read(app);
        let mut first = Vec::new();
        if waos_has_settings(library, app) {
            first.push(
                link_row(&self.library, words::WAOS_SETTINGS, SettingsPage::General).into_widget(),
            );
        }
        first.push(
            link_row(&self.library, words::PERMISSIONS, SettingsPage::Permissions).into_widget(),
        );
        let mut sections = vec![
            CupertinoListSection::inset_grouped()
                .children(first)
                .into_widget(),
        ];
        let apps: Vec<WidgetRef> = listed_apps(library)
            .into_iter()
            .map(|listed| {
                let icon = ProgramIcon {
                    png: listed.icon,
                    side: ICON_SIDE,
                };
                link_row(&self.library, &listed.name, SettingsPage::App(listed.id))
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
        page(back_to_home(&self.library), words::SETTINGS, sections)
    }
}

/// The list of permissions, each leading to its page with its default at the row's end, in
/// Cupertino's widgets.
#[derive(Debug)]
pub(crate) struct CupertinoPermissions {
    pub(crate) library: Entity<Library>,
}

impl StatelessWidget for CupertinoPermissions {
    fn build(&self, app: &mut App, _context: BuildContext) -> WidgetRef {
        let rows: Vec<WidgetRef> = permission_defaults(self.library.read(app))
            .into_iter()
            .map(|(permission, default)| {
                link_row(
                    &self.library,
                    words::permission_name(permission),
                    SettingsPage::Permission(permission),
                )
                .additional_info(Text::new(default))
                .into_widget()
            })
            .collect();
        let sections = vec![
            CupertinoListSection::inset_grouped()
                .children(rows)
                .footer(Text::new(words::PERMISSIONS_DETAIL))
                .into_widget(),
        ];
        let back = back_to_page_beneath(&self.library, words::SETTINGS);
        page(back, words::PERMISSIONS, sections)
    }
}

/// A page of `sections` on the grouped background, under a bar with `back` and `title`.
fn page(back: WidgetRef, title: &str, sections: Vec<WidgetRef>) -> WidgetRef {
    let bar = CupertinoNavigationBar::new()
        .leading(back)
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

/// A row that leads to `page`: `name`, and a chevron; a tap on it opens the page.
pub(crate) fn link_row(
    library: &Entity<Library>,
    name: &str,
    page: SettingsPage,
) -> CupertinoListTile {
    let library = library.clone();
    let open = Rc::new(move |app: &mut App| {
        let page = LibraryPage::Settings(page.clone());
        library.update(app, |library, cx| library.show_page(page, cx));
        Task::ready(())
    });
    CupertinoListTile::new(Text::new(name))
        .trailing(CupertinoListTileChevron::new())
        .on_tap(open)
}
