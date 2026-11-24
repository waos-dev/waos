//! WaOS's own settings at a desktop's width, in Fluent's controls: Back to the settings' first
//! page and the page's title, then each setting this system offers under its name and what it
//! does: whether apps show inside WaOS, whether WaOS is kept in the Dock or the taskbar, and
//! whether WaOS reaches the phone's files, with the button that opens the system's page for it.
//!
//! What it shows is the library's, read as it builds; it keeps nothing of its own.

use std::rc::Rc;

use waft::{
    App, BuildContext, EdgeInsetsGeometry, Entity, IntoWidget, ListView, Listener, Padding, Row,
    StatelessWidget, Text, WidgetRef,
};
use waft_winui::{
    Button, ButtonStyle, FluentSymbol, FontIcon, TextBlockStyle, ThemeResources, ToggleSwitch,
};

use crate::state::library::Library;
use crate::ui::library::home::fluent::MARGIN;
use crate::ui::library::words;

/// The room under a setting's name.
const NAME_GAP: f64 = 4.0;

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
        let mut rows = vec![shown.title()];
        rows.extend(shown.where_programs_open(app));
        rows.extend(shown.where_waos_stays(app));
        rows.extend(shown.phone_files(app));
        ListView::new()
            .padding(EdgeInsetsGeometry::all(MARGIN))
            .children(rows)
            .into_widget()
    }
}

/// What one build of the page reads and writes.
struct Shown {
    library: Entity<Library>,
    resources: Rc<ThemeResources>,
}

impl Shown {
    /// The page's title, after Back to the settings' first page.
    fn title(&self) -> WidgetRef {
        let library = self.library.clone();
        let back = Listener::new(move |app| {
            library.update(app, |library, cx| library.go_back(cx));
        });
        let primary = self.resources.text_fill_color_primary;
        Row::new()
            .spacing(4.0)
            .children([
                Button::new(FontIcon::symbol(FluentSymbol::Back).font_size(16.0), back)
                    .style(ButtonStyle::Subtle)
                    .padding([8.0, 8.0, 8.0, 8.0])
                    .into_widget(),
                Text::new(words::WAOS_SETTINGS)
                    .style(TextBlockStyle::Subtitle.text_style(&self.resources, primary))
                    .into_widget(),
            ])
            .into_widget()
    }

    /// Where WaOS offers the choice, whether apps show inside WaOS, under what the choice does.
    fn where_programs_open(&self, app: &App) -> Vec<WidgetRef> {
        let Some(inside) = self.library.read(app).programs_inside_waos() else {
            return Vec::new();
        };
        let library = self.library.clone();
        vec![
            self.setting_name(words::PROGRAMS_INSIDE_WAOS),
            self.detail(words::PROGRAMS_INSIDE_WAOS_DETAIL),
            self.toggle(inside, move |app, inside| {
                library.update(app, |library, cx| {
                    library.show_programs_inside_waos(inside, cx)
                });
            }),
        ]
    }

    /// In a desktop's launcher, whether WaOS is kept in the Dock or the taskbar, under what
    /// turning it off does.
    fn where_waos_stays(&self, app: &App) -> Vec<WidgetRef> {
        let Some(kept) = self.library.read(app).kept_in_dock() else {
            return Vec::new();
        };
        let platform = app.platform().target_platform();
        let library = self.library.clone();
        vec![
            self.setting_name(words::keep_in_dock(platform)),
            self.detail(words::keep_in_dock_detail(platform)),
            self.toggle(kept, move |app, kept| {
                library.update(app, |library, cx| library.keep_in_dock(kept, cx));
            }),
        ]
    }

    /// Where WaOS reaches the phone's files only as the user allows, whether it may, and a
    /// button that opens the system's page for it.
    fn phone_files(&self, app: &App) -> Vec<WidgetRef> {
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
            self.detail(said),
            Row::new()
                .children([Button::text(words::OPEN_ANDROID_SETTINGS, open).into_widget()])
                .into_widget(),
        ]
    }

    /// A setting's name, with the room over it that sets it apart from the one before.
    fn setting_name(&self, name: &str) -> WidgetRef {
        let primary = self.resources.text_fill_color_primary;
        Padding::new(EdgeInsetsGeometry::only(0.0, MARGIN, 0.0, NAME_GAP))
            .child(
                Text::new(name)
                    .style(TextBlockStyle::BodyStrong.text_style(&self.resources, primary)),
            )
            .into_widget()
    }

    /// What a setting does, under its name.
    fn detail(&self, text: &str) -> WidgetRef {
        let secondary = self.resources.text_fill_color_secondary;
        Text::new(text)
            .style(TextBlockStyle::Caption.text_style(&self.resources, secondary))
            .into_widget()
    }

    /// A switch that is `on` or off, telling `changed` when the user flips it.
    fn toggle(&self, on: bool, changed: impl Fn(&mut App, bool) + 'static) -> WidgetRef {
        ToggleSwitch::new(on, changed)
            .on_content(Text::new(words::ON))
            .off_content(Text::new(words::OFF))
            .into_widget()
    }
}
