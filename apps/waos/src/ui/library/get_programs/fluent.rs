//! Getting programs in the desktop's panel, in Fluent's controls: Back and the page's title, the
//! field that adds a source with Add and, where the system has a file picker, Install from file…
//! beside it, then each source as a header with Remove at its end, over a row for each program it
//! lists, its icon, name and version, and Get or Update as a button, or Installing… or Installed
//! as a caption; or what the source says while it is read or could not be.
//!
//! What the page keeps of its own is the text of a source being added; what it shows is the
//! library's.

use waft::{
    App, BuildContext, Column, CrossAxisAlignment, EdgeInsetsGeometry, Entity, Expanded, Handle,
    IntoWidget, ListView, Listener, Padding, Ref, Row, State, StateData, StatefulWidget, Text,
    TextEditingController, TextEditingControllerLeaf, TextOverflow, WidgetRef,
};
use waft_winui::{
    Button, ButtonStyle, FluentSymbol, FontIcon, TextBlockStyle, TextBox, ThemeResources,
};
use waos_store::Source;

use super::{ListedRow, PackagePicker, RowAction, listed_rows};
use crate::state::library::{Library, LibraryPage};
use crate::ui::library::home::fluent::MARGIN;
use crate::ui::library::program_icon::ProgramIcon;
use crate::ui::library::words;

const GAP: f64 = 8.0;

/// The side of a listed program's icon.
const ICON_SIDE: f64 = 32.0;

/// Getting programs, in Fluent's controls.
#[derive(Debug)]
pub(crate) struct FluentGetPrograms {
    pub(crate) library: Entity<Library>,
}

impl StatefulWidget for FluentGetPrograms {
    type State = FluentGetProgramsState;

    fn create_state(&self) -> FluentGetProgramsState {
        FluentGetProgramsState {
            state: StateData::new(),
            source_text: None,
        }
    }
}

pub(crate) struct FluentGetProgramsState {
    state: StateData<FluentGetPrograms>,
    /// What is typed as a source to add.
    source_text: Option<Ref<TextEditingController>>,
}

impl State for FluentGetProgramsState {
    type Widget = FluentGetPrograms;
    waft_widgets::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let source_text = TextEditingController::new(app);
        app.get_mut(self).source_text = Some(source_text);
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        if let Some(source_text) = app.get_mut(self).source_text.take() {
            source_text.dispose(app);
        }
    }

    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let library = self.widget(app).library.clone();
        let page = Page {
            package_picker: PackagePicker::of(&library, app, context),
            library,
            source_text: app
                .get(self)
                .source_text
                .clone()
                .expect("an initialized page"),
            resources: ThemeResources::of(app, context),
        };
        let mut rows = Vec::new();
        let library = page.library.read(app);
        for listing in library.sources() {
            rows.push(page.source_header(&listing.source));
            rows.extend(page.listed(&listing.source, listed_rows(listing, library)));
        }
        if rows.is_empty() {
            rows.push(page.caption(words::NO_SOURCES));
        }
        Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children([
                page.title_bar(),
                page.add_row(),
                Expanded::new(
                    ListView::new()
                        .padding(EdgeInsetsGeometry::from_ltrb(MARGIN, 0.0, MARGIN, MARGIN))
                        .children(rows),
                )
                .into_widget(),
            ])
            .into_widget()
    }
}

/// What one build of the page reads and writes.
struct Page {
    library: Entity<Library>,
    source_text: Ref<TextEditingController>,
    /// What Install from file… shows; none where the system has no file picker.
    package_picker: Option<PackagePicker>,
    resources: std::rc::Rc<ThemeResources>,
}

impl Page {
    /// Back to the home, and the page's title.
    fn title_bar(&self) -> WidgetRef {
        let library = self.library.clone();
        let back = Listener::new(move |app| {
            library.update(app, |library, cx| library.show_page(LibraryPage::Home, cx));
        });
        let resources = &self.resources;
        let title = Text::new(words::GET_PROGRAMS).style(
            TextBlockStyle::Subtitle.text_style(resources, resources.text_fill_color_primary),
        );
        Padding::new(EdgeInsetsGeometry::from_ltrb(
            MARGIN - 8.0,
            16.0,
            MARGIN,
            12.0,
        ))
        .child(
            Row::new().spacing(4.0).children([
                Button::new(FontIcon::symbol(FluentSymbol::Back).font_size(16.0), back)
                    .style(ButtonStyle::Subtle)
                    .padding([8.0, 8.0, 8.0, 8.0])
                    .into_widget(),
                title.into_widget(),
            ]),
        )
        .into_widget()
    }

    /// The field a source is typed in, Add, and Install from file… where the system has a file
    /// picker.
    fn add_row(&self) -> WidgetRef {
        let (library, text) = (self.library.clone(), self.source_text.clone());
        let add = Listener::new(move |app| {
            let typed = text.text_value(app).to_owned();
            library.update(app, |library, cx| library.add_source(&typed, cx));
            if library.read(app).problem().is_none() {
                text.clear(app);
            }
        });
        let field =
            TextBox::new(self.source_text.erase()).placeholder_text(words::SOURCE_PLACEHOLDER);
        let mut controls = vec![
            Expanded::new(field).into_widget(),
            Button::text(words::ADD, add).into_widget(),
        ];
        controls.extend(self.install_from_file());
        Padding::new(EdgeInsetsGeometry::from_ltrb(MARGIN, 0.0, MARGIN, 8.0))
            .child(Row::new().spacing(GAP).children(controls))
            .into_widget()
    }

    /// Install from file…, which shows the system's file picker for a package; none where the
    /// system has no file picker.
    fn install_from_file(&self) -> Option<WidgetRef> {
        let picker = self.package_picker.clone()?;
        let pick = Listener::new(move |app| picker.show(app));
        Some(Button::text(words::INSTALL_FROM_FILE, pick).into_widget())
    }

    /// The source, and Remove.
    fn source_header(&self, source: &Source) -> WidgetRef {
        let (library, removed) = (self.library.clone(), source.clone());
        let remove = Listener::new(move |app| {
            library.update(app, |library, cx| library.remove_source(&removed, cx))
        });
        let resources = &self.resources;
        let title = Text::new(source.to_string())
            .max_lines(1)
            .overflow(TextOverflow::Ellipsis)
            .style(
                TextBlockStyle::BodyStrong.text_style(resources, resources.text_fill_color_primary),
            );
        Padding::new(EdgeInsetsGeometry::only(0.0, 16.0, 0.0, 4.0))
            .child(
                Row::new().spacing(GAP).children([
                    Expanded::new(title).into_widget(),
                    Button::text(words::REMOVE_SOURCE, remove)
                        .style(ButtonStyle::Subtle)
                        .into_widget(),
                ]),
            )
            .into_widget()
    }

    /// The rows of what `source` lists, or what it says.
    fn listed(&self, source: &Source, rows: Result<Vec<ListedRow>, String>) -> Vec<WidgetRef> {
        match rows {
            Ok(rows) if rows.is_empty() => vec![self.caption(words::LISTS_NOTHING)],
            Ok(rows) => rows
                .into_iter()
                .map(|row| self.listed_row(source, row))
                .collect(),
            Err(said) => vec![self.caption(&said)],
        }
    }

    /// A program the source lists: its icon, name and version, and what can be done with it.
    fn listed_row(&self, source: &Source, row: ListedRow) -> WidgetRef {
        let resources = &self.resources;
        let icon = ProgramIcon {
            png: row.icon.clone(),
            side: ICON_SIDE,
            badge: None,
        };
        let text = Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children([
                Text::new(row.program.name.clone())
                    .max_lines(1)
                    .overflow(TextOverflow::Ellipsis)
                    .style(
                        TextBlockStyle::Body
                            .text_style(resources, resources.text_fill_color_primary),
                    )
                    .into_widget(),
                self.caption(&row.detail),
            ]);
        let action = match row.action {
            RowAction::Installing => self.caption(words::INSTALLING),
            RowAction::Installed => self.caption(words::INSTALLED),
            RowAction::Get | RowAction::Update => {
                let label = if row.action == RowAction::Get {
                    words::GET
                } else {
                    words::UPDATE
                };
                let (library, source, listed) =
                    (self.library.clone(), source.clone(), row.program.clone());
                let install = Listener::new(move |app| {
                    library.update(app, |library, cx| {
                        library.install_from(&source, &listed, cx)
                    })
                });
                Button::text(label, install)
                    .style(ButtonStyle::Accent)
                    .into_widget()
            }
        };
        Padding::new(EdgeInsetsGeometry::symmetric(6.0, 0.0))
            .child(Row::new().spacing(12.0).children([
                icon.into_widget(),
                Expanded::new(text).into_widget(),
                action,
            ]))
            .into_widget()
    }

    fn caption(&self, text: &str) -> WidgetRef {
        let resources = &self.resources;
        Text::new(text)
            .style(
                TextBlockStyle::Caption.text_style(resources, resources.text_fill_color_secondary),
            )
            .into_widget()
    }
}
