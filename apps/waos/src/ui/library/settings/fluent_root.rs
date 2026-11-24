//! The settings' pages that lead to others, at a desktop's width, in Fluent's controls, as
//! Windows's Settings leads to its pages: a settings card to click for each place, its icon, its
//! name and what it holds. The first page leads to General, where this system has settings of
//! WaOS's own, to Permissions, and, under Apps, to each installed app; Permissions leads to
//! each permission, saying what its default is.
//!
//! What they show is the library's, read as they build; they keep nothing of their own.

use waft::{App, BuildContext, Entity, StatelessWidget, WidgetRef};
use waft_winui::{FluentSymbol, ThemeResources};

use super::fluent_page::{Start, heading, link_card, page, permission_symbol};
use super::{listed_apps, permission_defaults, waos_has_settings};
use crate::state::library::{Library, SettingsPage};
use crate::ui::library::words;

/// The settings' first page, in Fluent's controls.
#[derive(Debug)]
pub(crate) struct FluentSettingsRoot {
    pub(crate) library: Entity<Library>,
}

impl StatelessWidget for FluentSettingsRoot {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let library = self.library.read(app);
        let mut rows = Vec::new();
        if waos_has_settings(library, app) {
            rows.push(link_card(
                &self.library,
                Start::symbol(FluentSymbol::Settings),
                words::WAOS_SETTINGS,
                Some(words::WAOS_SETTINGS_DETAIL),
                SettingsPage::General,
            ));
        }
        rows.push(link_card(
            &self.library,
            Start::symbol(FluentSymbol::Shield),
            words::PERMISSIONS,
            Some(words::PERMISSIONS_DETAIL),
            SettingsPage::Permissions,
        ));
        let apps = listed_apps(library);
        if !apps.is_empty() {
            rows.push(heading(words::APPS, &resources));
        }
        for listed in apps {
            rows.push(link_card(
                &self.library,
                Start::app_icon(listed.icon),
                &listed.name,
                None,
                SettingsPage::App(listed.id),
            ));
        }
        page(&self.library, &resources, &[words::SETTINGS], rows)
    }
}

/// The list of permissions, each leading to its page, in Fluent's controls.
#[derive(Debug)]
pub(crate) struct FluentPermissions {
    pub(crate) library: Entity<Library>,
}

impl StatelessWidget for FluentPermissions {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let rows = permission_defaults(self.library.read(app))
            .into_iter()
            .map(|(permission, default)| {
                link_card(
                    &self.library,
                    Start::symbol(permission_symbol(permission)),
                    words::permission_name(permission),
                    Some(&default),
                    SettingsPage::Permission(permission),
                )
            })
            .collect();
        page(&self.library, &resources, &[words::PERMISSIONS], rows)
    }
}
