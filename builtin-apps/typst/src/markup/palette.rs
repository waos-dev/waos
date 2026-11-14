//! The colours the markup is drawn in, for a light and a dark theme: the ones of Visual Studio
//! Code's default light and dark themes, which people who write markup know.

use typst::syntax::Tag;
use waft::{Color, FontWeight, TextDecoration, TextDecorationStyle, TextStyle};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Palette {
    comment: Color,
    keyword: Color,
    function: Color,
    string: Color,
    number: Color,
    math: Color,
    reference: Color,
    raw: Color,
    escape: Color,
    punctuation: Color,
    problem: Color,
}

impl Palette {
    pub(crate) const LIGHT: Palette = Palette {
        comment: Color::from_argb(255, 0, 128, 0),
        keyword: Color::from_argb(255, 175, 0, 219),
        function: Color::from_argb(255, 0, 92, 197),
        string: Color::from_argb(255, 163, 21, 21),
        number: Color::from_argb(255, 9, 134, 88),
        math: Color::from_argb(255, 38, 127, 153),
        reference: Color::from_argb(255, 0, 112, 193),
        raw: Color::from_argb(255, 129, 95, 3),
        escape: Color::from_argb(255, 238, 0, 0),
        punctuation: Color::from_argb(255, 110, 110, 110),
        problem: Color::from_argb(255, 196, 43, 28),
    };

    pub(crate) const DARK: Palette = Palette {
        comment: Color::from_argb(255, 106, 153, 85),
        keyword: Color::from_argb(255, 197, 134, 192),
        function: Color::from_argb(255, 121, 184, 255),
        string: Color::from_argb(255, 206, 145, 120),
        number: Color::from_argb(255, 181, 206, 168),
        math: Color::from_argb(255, 78, 201, 176),
        reference: Color::from_argb(255, 79, 193, 255),
        raw: Color::from_argb(255, 215, 186, 125),
        escape: Color::from_argb(255, 215, 186, 125),
        punctuation: Color::from_argb(255, 150, 150, 150),
        problem: Color::from_argb(255, 255, 153, 164),
    };

    /// How a run tagged `tag` differs from plain text; none where it does not.
    pub(crate) fn style_of(&self, tag: Tag) -> Option<TextStyle> {
        let style = TextStyle::new();
        Some(match tag {
            Tag::Heading | Tag::Strong => style.font_weight(FontWeight::BOLD),
            Tag::Emph => return None,
            Tag::Comment => style.color(self.comment),
            Tag::Keyword => style.color(self.keyword),
            Tag::Function | Tag::Interpolated => style.color(self.function),
            Tag::String => style.color(self.string),
            Tag::Number => style.color(self.number),
            Tag::MathDelimiter | Tag::MathOperator | Tag::MathGroupingParens => {
                style.color(self.math)
            }
            Tag::Label | Tag::Ref => style.color(self.reference),
            Tag::Link => style
                .color(self.reference)
                .decoration(TextDecoration::UNDERLINE),
            Tag::Raw => style.color(self.raw),
            Tag::Escape => style.color(self.escape),
            Tag::Punctuation | Tag::Operator | Tag::ListMarker | Tag::ListTerm => {
                style.color(self.punctuation)
            }
            Tag::Error => self.problem_style(),
        })
    }

    /// The wavy line under a problem.
    pub(crate) fn problem_style(&self) -> TextStyle {
        TextStyle::new()
            .decoration(TextDecoration::UNDERLINE)
            .decoration_style(TextDecorationStyle::Wavy)
            .decoration_color(self.problem)
    }
}
