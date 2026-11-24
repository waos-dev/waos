//! WaOS's own settings at a desktop's width, in Fluent's controls: Back to the settings' first
//! page and the page's title, then a card for each setting this system offers, its name over
//! what it does and its control at the trailing side: whether apps show inside WaOS, whether
//! WaOS is kept in the Dock or the taskbar, and whether WaOS reaches the phone's files, with
//! the button that opens the system's page for it.
//!
//! What it shows is the library's, read as it builds; it keeps nothing of its own.

use std::rc::Rc;

use waft::{
    App, BuildContext, Column, CrossAxisAlignment, EdgeInsetsGeometry, Entity, Expanded,
    IntoWidget, ListView, Listener, StatelessWidget, WidgetRef,
};
use waft_winui::{Button, ThemeResources};

use super::fluent_cards::{card, card_row, card_switch};
use super::fluent_root::title_bar;
use crate::state::library::Library;
use crate::ui::library::home::fluent::MARGIN;
use crate::ui::library::words;

/// WaOS's own settings, in Fluent's controls.
#[derive(Debug)]
pub(crate) struct FluentGeneralSettings {
    pub(crate) library: Entity<Library>,
}

impl StatelessWidget for FluentGeneralSettings {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let shown = Shown {
            library: self.library.clone(),
            resources: ThemeResources::of(app, context),
        };
        let cards: Vec<WidgetRef> = [
            shown.where_programs_open(app),
            shown.where_waos_stays(app),
            shown.phone_files(app),
        ]
        .into_iter()
        .flatten()
        .collect();
        let list = ListView::new()
            .padding(EdgeInsetsGeometry::from_ltrb(MARGIN, 8.0, MARGIN, MARGIN))
            .children(cards);
        Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children([
                title_bar(&self.library, &shown.resources, words::WAOS_SETTINGS),
                Expanded::new(list).into_widget(),
            ])
            .into_widget()
    }
}

/// What one build of the page reads and writes.
struct Shown {
    library: Entity<Library>,
    resources: Rc<ThemeResources>,
}

impl Shown {
    /// Where WaOS offers the choice, whether apps show inside WaOS, over what the choice does.
    fn where_programs_open(&self, app: &App) -> Option<WidgetRef> {
        let inside = self.library.read(app).programs_inside_waos()?;
        let library = self.library.clone();
        let toggle = card_switch(&self.resources, inside, move |app, inside| {
            library.update(app, |library, cx| {
                library.show_programs_inside_waos(inside, cx)
            });
        });
        Some(self.setting(
            words::PROGRAMS_INSIDE_WAOS,
            words::PROGRAMS_INSIDE_WAOS_DETAIL,
            toggle,
        ))
    }

    /// In a desktop's launcher, whether WaOS is kept in the Dock or the taskbar, over what
    /// turning it off does.
    fn where_waos_stays(&self, app: &App) -> Option<WidgetRef> {
        let kept = self.library.read(app).kept_in_dock()?;
        let platform = app.platform().target_platform();
        let library = self.library.clone();
        let toggle = card_switch(&self.resources, kept, move |app, kept| {
            library.update(app, |library, cx| library.keep_in_dock(kept, cx));
        });
        Some(self.setting(
            words::keep_in_dock(platform),
            words::keep_in_dock_detail(platform),
            toggle,
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
        let button = Button::text(words::OPEN_ANDROID_SETTINGS, open).into_widget();
        Some(self.setting(words::PHONE_FILES, said, button))
    }

    /// A setting's card: its `name` over what it does, and its `control`.
    fn setting(&self, name: &str, detail: &str, control: WidgetRef) -> WidgetRef {
        let row = card_row(&self.resources, None, name, Some(detail), Some(control));
        card(&self.resources, vec![row])
    }
}
