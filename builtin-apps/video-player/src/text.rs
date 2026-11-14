//! Text in Fluent's type ramp and the theme's colours, read from WinUI's resources where it is
//! built.

use waft_widgets::{Builder, IntoWidget, Text, TextAlign, TextOverflow, WidgetRef};
use waft_winui::{TextBlockStyle, ThemeResources};

/// How text stands out from what is around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tone {
    /// The theme's text colour.
    Primary,
    /// Its colour for what says less.
    Secondary,
}

/// `words` in `style` and `tone`, wrapping as they need, centred.
pub(crate) fn text(words: impl Into<String>, style: TextBlockStyle, tone: Tone) -> WidgetRef {
    styled(words.into(), style, tone, Lines::Wrapped)
}

/// `words` in `style` and `tone` on one line, cut short where they do not fit.
pub(crate) fn line(words: impl Into<String>, style: TextBlockStyle, tone: Tone) -> WidgetRef {
    styled(words.into(), style, tone, Lines::One)
}

#[derive(Clone, Copy)]
enum Lines {
    Wrapped,
    One,
}

fn styled(words: String, style: TextBlockStyle, tone: Tone, lines: Lines) -> WidgetRef {
    Builder::new(move |app, context| {
        let resources = ThemeResources::of(app, context);
        let color = match tone {
            Tone::Primary => resources.text_fill_color_primary,
            Tone::Secondary => resources.text_fill_color_secondary,
        };
        let text = Text::new(words.clone()).style(style.text_style(&resources, color));
        match lines {
            Lines::Wrapped => text.text_align(TextAlign::Center),
            Lines::One => text.max_lines(1).overflow(TextOverflow::Ellipsis),
        }
        .into_widget()
    })
    .into_widget()
}
