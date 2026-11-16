//! The side the document is written on: its markup in a borderless Fluent text box that fills
//! the pane, in a fixed-width font, each stretch in the colour of what it is, edited with the
//! system's input methods; and beneath it the problems Typst found, while there are any.

use waft::{
    App, BuildContext, Color, ColoredBox, Column, CrossAxisAlignment, Expanded, IntoWidget,
    StatelessWidget, WidgetRef,
};
use waft_winui::{TextBox, TextWrapping, Theme, ThemeResources, ThemeScope};

use crate::documents_app::DocumentsApp;
use crate::editor_font::EDITOR_FONT;
use crate::indenting::with_tab_indenting;
use crate::markup::Palette;
use crate::problem_list::ProblemList;

/// The size of the markup's letters, in logical points.
const FONT_SIZE: f64 = 13.0;

/// The room around the markup, left, top, right and bottom, in logical points.
const PADDING: [f64; 4] = [16.0, 12.0, 16.0, 12.0];

#[derive(Debug)]
pub(crate) struct EditorPane {
    pub(crate) documents: DocumentsApp,
}

impl StatelessWidget for EditorPane {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let palette = match resources.theme {
            Theme::Light => Palette::LIGHT,
            Theme::Dark => Palette::DARK,
        };
        self.documents.editor.text.set_palette(app, palette);
        let parts = [
            Expanded::new(self.text_box()).into_widget(),
            ProblemList {
                documents: self.documents.clone(),
            }
            .into_widget(),
        ];
        ColoredBox::new(resources.solid_background_fill_color_tertiary)
            .child(
                Column::new()
                    .cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .children(parts),
            )
            .into_widget()
    }
}

impl EditorPane {
    /// The markup, wrapped at the pane's width, scrolling past its height.
    fn text_box(&self) -> WidgetRef {
        let editor = &self.documents.editor;
        let text_box = TextBox::new(editor.text.erase())
            .focus_node(editor.focus.erase())
            .accepts_return(true)
            .text_wrapping(TextWrapping::Wrap)
            .font_size(FONT_SIZE)
            .padding(PADDING)
            .placeholder_text("Write in Typst's markup");
        let text_box = ThemeScope::overriding(text_box).any(as_source);
        with_tab_indenting(editor.indenting.clone(), text_box.into_widget())
    }
}

/// A text box's resources for source text: the editor's font, and no background or border in any
/// state, as XAML's `<TextBox.Resources>` would set them.
fn as_source(resources: &mut ThemeResources) {
    resources.content_control_theme_font_family = EDITOR_FONT;
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
