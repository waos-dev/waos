//! The note or the file shown, as the content of the window's `NavigationView`: in its header, the
//! note's title, with Delete, or the file's name, with Close File; beneath, what the window says
//! (`notices`), when the note last changed or where the file is, then its text in a borderless
//! Fluent text box filling the rest, edited with the system's input methods; a deleted note's text
//! cannot be edited. With nothing shown, the header names the folder and the rest is blank.
//!
//! The text box is built anew for each note and file, so its undo history is the shown's alone,
//! over the editor's one text and focus.

use waft_embedder::Color;
use waft_foundation::{App, ValueKey};
use waft_widgets::{
    BuildContext, Column, CrossAxisAlignment, EdgeInsetsGeometry, Expanded, IntoWidget, Padding,
    Row, StatelessWidget, Text, TextOverflow, WidgetRef,
};
use waft_winui::{TextBox, TextWrapping, ThemeResources, ThemeScope};

use crate::bar_button::icon_button;
use crate::browsing::Shown;
use crate::dates;
use crate::editing::delete_shown;
use crate::icons;
use crate::notes_app::NotesApp;
use crate::notices::Notices;
use crate::opening::close_shown_file;
use crate::secondary_text::secondary_text;
use crate::window::shown_title;

/// The space between the line above the text and the text, and below the text, in logical points.
const GAP: f64 = 8.0;

/// What the pane shows, read from the library or the files open as it is built.
struct Showing {
    shown: Shown,
    /// When the note last changed, or where the file is.
    line: String,
    read_only: bool,
}

impl Showing {
    fn of(app: &App, notes: &NotesApp) -> Option<Showing> {
        let shown = notes.browsing.read(app).shown()?.clone();
        let (line, read_only) = match &shown {
            Shown::Note(id) => {
                let note = notes.library.read(app).note(*id)?;
                (
                    dates::in_full(note.changed, dates::users_offset),
                    note.is_deleted(),
                )
            }
            Shown::File(path) => {
                notes.files.read(app).file(path)?;
                (path.display().to_string(), false)
            }
        };
        Some(Showing {
            shown,
            line,
            read_only,
        })
    }
}

/// The header above the note or the file: its title, with Delete or Close File; the folder's name
/// while nothing is shown.
#[derive(Debug)]
pub(crate) struct EditorHeader {
    pub(crate) notes: NotesApp,
}

impl StatelessWidget for EditorHeader {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let inset = content_inset(app, context, &self.notes);
        let title = shown_title(app, &self.notes)
            .unwrap_or_else(|| self.notes.browsing.read(app).folder().name().to_owned());
        let mut parts = vec![
            Expanded::new(
                Text::new(title)
                    .max_lines(1)
                    .overflow(TextOverflow::Ellipsis),
            )
            .into_widget(),
        ];
        parts.extend(self.command(app));
        Padding::new(EdgeInsetsGeometry::from_ltrb(0.0, 0.0, inset, 0.0))
            .child(Row::new().children(parts))
            .into_widget()
    }
}

impl EditorHeader {
    /// Delete for a note, Close File for a file.
    fn command(&self, app: &App) -> Option<WidgetRef> {
        let button = match self.notes.browsing.read(app).shown()? {
            Shown::Note(_) => icon_button(icons::delete(), "Delete", &self.notes, delete_shown),
            Shown::File(_) => {
                icon_button(icons::close(), "Close File", &self.notes, close_shown_file)
            }
        };
        Some(button)
    }
}

/// What the window says, and the note or the file shown, beneath the header.
#[derive(Debug)]
pub(crate) struct EditorPane {
    pub(crate) notes: NotesApp,
}

impl StatelessWidget for EditorPane {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let inset = content_inset(app, context, &self.notes);
        let mut parts = vec![
            Notices {
                notes: self.notes.clone(),
            }
            .into_widget(),
        ];
        if let Some(showing) = Showing::of(app, &self.notes) {
            parts.push(line_above(app, context, &showing.line));
            parts.push(Expanded::new(self.text_box(showing)).into_widget());
        }
        Padding::new(EdgeInsetsGeometry::from_ltrb(inset, 0.0, inset, GAP))
            .child(
                Column::new()
                    .cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .children(parts),
            )
            .into_widget()
    }
}

impl EditorPane {
    /// The text, wrapped, as many lines as the pane holds, scrolling past them; read-only for a
    /// deleted note.
    fn text_box(&self, showing: Showing) -> WidgetRef {
        let text_box = TextBox::new(self.notes.editor.text.clone())
            .key(ValueKey::new(showing.shown).into())
            .focus_node(self.notes.editor.focus.erase())
            .is_read_only(showing.read_only)
            .accepts_return(true)
            .text_wrapping(TextWrapping::Wrap)
            .padding([0.0; 4]);
        ThemeScope::overriding(text_box)
            .any(borderless)
            .into_widget()
    }
}

/// When the note last changed, or where the file is, above its text.
fn line_above(app: &mut App, context: BuildContext, line: &str) -> WidgetRef {
    let said = secondary_text(app, context, line);
    Padding::new(EdgeInsetsGeometry::from_ltrb(0.0, 0.0, 0.0, GAP))
        .child(said)
        .into_widget()
}

/// How far the note is from the content's edges: in line with the header's title, past the pane's
/// toggle where the list is a pane over it, as WinUI's gallery lays out its pages.
fn content_inset(app: &mut App, context: BuildContext, notes: &NotesApp) -> f64 {
    let resources = ThemeResources::of(app, context);
    if notes.browsing.read(app).narrow() {
        resources.pane_toggle_button_width + resources.navigation_view_minimal_header_margin[0]
    } else {
        resources.navigation_view_header_margin[0]
    }
}

/// A text box's resources with no background or border in any state, as a document's editor is
/// drawn, as XAML's `<TextBox.Resources>` would set them.
fn borderless(resources: &mut ThemeResources) {
    let none = Color::new(0);
    for background in [
        &mut resources.text_control_background,
        &mut resources.text_control_background_pointer_over,
        &mut resources.text_control_background_focused,
        &mut resources.text_control_background_disabled,
        &mut resources.text_control_border_brush_disabled,
    ] {
        *background = none;
    }
    for border in [
        &mut resources.text_control_border_brush,
        &mut resources.text_control_border_brush_pointer_over,
        &mut resources.text_control_border_brush_focused,
        &mut resources.text_control_elevation_border_brush,
        &mut resources.text_control_elevation_border_focused_brush,
    ] {
        for stop in border.iter_mut() {
            stop.1 = none;
        }
    }
    resources.text_control_border_theme_thickness = [0.0; 4];
    resources.text_control_border_theme_thickness_focused = [0.0; 4];
}
