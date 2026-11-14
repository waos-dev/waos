//! A player's cues as the WIT carries them, in Waft's terms: Media3's `Cue` on both sides, so
//! field for field, a colour's `0xAARRGGBB` as Waft's `Color` holds it.

use waft_embedder::{
    Color, Cue, CueAnchor, CueContent, CueImage, CueLine, CueTextAlignment, CueTextEdge,
    CueTextRun, CueVertical,
};
use wasdk::media_players as platform;

/// `cue` in Waft's terms.
pub(crate) fn framework_cue(cue: platform::Cue) -> Cue {
    Cue {
        content: framework_content(cue.content),
        line: cue.line.map(framework_line),
        line_anchor: framework_anchor(cue.line_anchor),
        position: cue.position,
        position_anchor: framework_anchor(cue.position_anchor),
        size: cue.size,
        text_alignment: framework_alignment(cue.text_alignment),
        vertical: cue.vertical.map(framework_vertical),
        window_color: cue.window_color.map(Color::new),
    }
}

fn framework_content(content: platform::CueContent) -> CueContent {
    match content {
        platform::CueContent::Text(runs) => {
            CueContent::Text(runs.into_iter().map(framework_run).collect())
        }
        platform::CueContent::Image(image) => CueContent::Image(CueImage {
            width: image.width,
            height: image.height,
            pixels: image.pixels,
            display_height: image.display_height,
        }),
    }
}

fn framework_run(run: platform::CueTextRun) -> CueTextRun {
    CueTextRun {
        text: run.text,
        bold: run.bold,
        italic: run.italic,
        underline: run.underline,
        color: run.color.map(Color::new),
        background: run.background.map(Color::new),
        font_size: run.font_size,
        font_family: run.font_family,
        edge: run.edge.map(framework_edge),
        edge_color: run.edge_color.map(Color::new),
    }
}

fn framework_line(line: platform::CueLine) -> CueLine {
    match line {
        platform::CueLine::Fraction(fraction) => CueLine::Fraction(fraction),
        platform::CueLine::Number(number) => CueLine::Number(number),
    }
}

fn framework_anchor(anchor: platform::CueAnchor) -> CueAnchor {
    match anchor {
        platform::CueAnchor::Start => CueAnchor::Start,
        platform::CueAnchor::Middle => CueAnchor::Middle,
        platform::CueAnchor::End => CueAnchor::End,
    }
}

fn framework_alignment(alignment: platform::CueTextAlignment) -> CueTextAlignment {
    match alignment {
        platform::CueTextAlignment::Start => CueTextAlignment::Start,
        platform::CueTextAlignment::Center => CueTextAlignment::Center,
        platform::CueTextAlignment::End => CueTextAlignment::End,
    }
}

fn framework_vertical(vertical: platform::CueVertical) -> CueVertical {
    match vertical {
        platform::CueVertical::RightToLeft => CueVertical::RightToLeft,
        platform::CueVertical::LeftToRight => CueVertical::LeftToRight,
    }
}

pub(crate) fn framework_edge(edge: platform::CueTextEdge) -> CueTextEdge {
    match edge {
        platform::CueTextEdge::None => CueTextEdge::None,
        platform::CueTextEdge::Outline => CueTextEdge::Outline,
        platform::CueTextEdge::DropShadow => CueTextEdge::DropShadow,
        platform::CueTextEdge::Raised => CueTextEdge::Raised,
        platform::CueTextEdge::Depressed => CueTextEdge::Depressed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cue_is_wafts_field_for_field() {
        let cue = framework_cue(platform::Cue {
            content: platform::CueContent::Text(vec![platform::CueTextRun {
                text: "Hello".to_owned(),
                bold: true,
                italic: false,
                underline: true,
                color: Some(0xFFFF_FF00),
                background: Some(0x8000_0000),
                font_size: Some(0.05),
                font_family: Some("Menlo".to_owned()),
                edge: Some(platform::CueTextEdge::DropShadow),
                edge_color: None,
            }]),
            line: Some(platform::CueLine::Number(-1)),
            line_anchor: platform::CueAnchor::End,
            position: Some(0.5),
            position_anchor: platform::CueAnchor::Middle,
            size: Some(0.8),
            text_alignment: platform::CueTextAlignment::Start,
            vertical: Some(platform::CueVertical::RightToLeft),
            window_color: Some(0x4000_0000),
        });
        let mut run = CueTextRun::new("Hello");
        run.bold = true;
        run.underline = true;
        run.color = Some(Color::new(0xFFFF_FF00));
        run.background = Some(Color::new(0x8000_0000));
        run.font_size = Some(0.05);
        run.font_family = Some("Menlo".to_owned());
        run.edge = Some(CueTextEdge::DropShadow);
        let mut expected = Cue::new(CueContent::Text(vec![run]));
        expected.line = Some(CueLine::Number(-1));
        expected.line_anchor = CueAnchor::End;
        expected.position = Some(0.5);
        expected.position_anchor = CueAnchor::Middle;
        expected.size = Some(0.8);
        expected.text_alignment = CueTextAlignment::Start;
        expected.vertical = Some(CueVertical::RightToLeft);
        expected.window_color = Some(Color::new(0x4000_0000));
        assert_eq!(cue, expected);
    }

    #[test]
    fn a_picture_subtitle_keeps_its_pixels() {
        let cue = framework_cue(platform::Cue {
            content: platform::CueContent::Image(platform::CueImage {
                width: 2,
                height: 1,
                pixels: vec![255, 0, 0, 255, 0, 255, 0, 128],
                display_height: 0.1,
            }),
            line: Some(platform::CueLine::Fraction(0.9)),
            line_anchor: platform::CueAnchor::Start,
            position: None,
            position_anchor: platform::CueAnchor::Start,
            size: None,
            text_alignment: platform::CueTextAlignment::Center,
            vertical: None,
            window_color: None,
        });
        let CueContent::Image(image) = cue.content else {
            panic!("a picture subtitle");
        };
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(image.pixels, [255, 0, 0, 255, 0, 255, 0, 128]);
        assert_eq!(cue.line, Some(CueLine::Fraction(0.9)));
    }
}
