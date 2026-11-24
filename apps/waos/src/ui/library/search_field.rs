//! What the desktop's library is searched in, in Fluent's controls: a field with no box, as a
//! launcher's is, and before what is typed a token for each thing that narrows what is searched,
//! a category or a source, as macOS's Apps shows a category chosen. The token is taken back as
//! a character is: by Backspace, where nothing is typed after it.

use waft::{
    BorderRadiusGeometry, BoxDecoration, Color, DecoratedBox, EdgeInsetsGeometry, IntoWidget,
    Padding, Text, WidgetRef,
};
use waft_services::{KeyEvent, LogicalKeyboardKey};
use waft_winui::{TextBlockStyle, TextBox, ThemeResources, ThemeScope};

/// How much of the text's colour fills a token, how round its corners are, and the room inside
/// it around its name: across, and up and down.
const TOKEN_FILL: f64 = 0.1;
const TOKEN_CORNER_RADIUS: f64 = 6.0;
const TOKEN_PADDING: [f64; 2] = [8.0, 2.0];

/// `text_box` with no box around it and no accent line under it, focused or not: a launcher's
/// field is the launcher's own surface to type on, as Spotlight's and PowerToys Run's are. Done as
/// WinUI restyles one control, by overriding the resources its states read.
pub(crate) fn with_no_box(text_box: TextBox) -> ThemeScope {
    const NONE: Color = Color::from_argb(0, 0, 0, 0);
    ThemeScope::overriding(text_box).any(|resources| {
        resources.text_control_background = NONE;
        resources.text_control_background_pointer_over = NONE;
        resources.text_control_background_focused = NONE;
        resources.text_control_border_brush = [(0.0, NONE), (1.0, NONE)];
        resources.text_control_border_brush_pointer_over = [(0.0, NONE), (1.0, NONE)];
        resources.text_control_border_brush_focused = [(0.0, NONE), (1.0, NONE)];
        resources.text_control_border_theme_thickness_focused =
            resources.text_control_border_theme_thickness;
    })
}

/// The token of what narrows the search to `name`, its text `font_size` high as the field's is:
/// the name on a rounded fill of the text's own colour, thinned, so it reads over the card
/// whether the card is solid or a tint over the system's blur.
pub(crate) fn search_token(name: &str, font_size: f64, resources: &ThemeResources) -> WidgetRef {
    let fill =
        resources
            .text_fill_color_primary
            .with_values(Some(TOKEN_FILL), None, None, None, None);
    let surface = BoxDecoration {
        color: Some(fill.into()),
        border_radius: Some(BorderRadiusGeometry::circular(TOKEN_CORNER_RADIUS)),
        ..BoxDecoration::default()
    };
    let style = TextBlockStyle::Body
        .text_style(resources, resources.text_fill_color_primary)
        .font_size(font_size);
    DecoratedBox::new(surface)
        .child(
            Padding::new(EdgeInsetsGeometry::symmetric(
                TOKEN_PADDING[1],
                TOKEN_PADDING[0],
            ))
            .child(Text::new(name).style(style)),
        )
        .into_widget()
}

/// Whether `event` takes back the last token of a field holding `typed`: Backspace, pressed or
/// held, where nothing is typed.
pub(crate) fn takes_back_a_token(event: &KeyEvent, typed: &str) -> bool {
    matches!(event, KeyEvent::Down(_) | KeyEvent::Repeat(_))
        && event.logical_key() == LogicalKeyboardKey::BACKSPACE
        && typed.is_empty()
}
