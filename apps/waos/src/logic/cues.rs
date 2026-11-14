//! A player's cues in Waft's terms, as the program reads them: Media3's `Cue` on both sides, so
//! field for field, a colour as the `0xAARRGGBB` of Waft's `Color`; the user's caption settings
//! are in them already, as Waft's host merged them.

use waft::ui;
use waos::bindings::wasdk::media::media_players::{
    Cue, CueAnchor, CueContent, CueImage, CueLine, CueTextAlignment, CueTextEdge, CueTextRun,
    CueVertical,
};

/// Waft's `cue`, as the program reads it.
pub(crate) fn program_cue(cue: ui::Cue) -> Cue {
    Cue {
        content: program_content(cue.content),
        line: cue.line.map(program_line),
        line_anchor: program_anchor(cue.line_anchor),
        position: cue.position,
        position_anchor: program_anchor(cue.position_anchor),
        size: cue.size,
        text_alignment: program_alignment(cue.text_alignment),
        vertical: cue.vertical.map(program_vertical),
        window_color: cue.window_color.map(|color| color.to_argb32()),
    }
}

fn program_content(content: ui::CueContent) -> CueContent {
    match content {
        ui::CueContent::Text(runs) => CueContent::Text(runs.into_iter().map(program_run).collect()),
        ui::CueContent::Image(image) => CueContent::Image(CueImage {
            width: image.width,
            height: image.height,
            pixels: image.pixels,
            display_height: image.display_height,
        }),
    }
}

fn program_run(run: ui::CueTextRun) -> CueTextRun {
    CueTextRun {
        text: run.text,
        bold: run.bold,
        italic: run.italic,
        underline: run.underline,
        color: run.color.map(|color| color.to_argb32()),
        background: run.background.map(|color| color.to_argb32()),
        font_size: run.font_size,
        font_family: run.font_family,
        edge: run.edge.map(program_edge),
        edge_color: run.edge_color.map(|color| color.to_argb32()),
    }
}

fn program_line(line: ui::CueLine) -> CueLine {
    match line {
        ui::CueLine::Fraction(fraction) => CueLine::Fraction(fraction),
        ui::CueLine::Number(number) => CueLine::Number(number),
    }
}

fn program_anchor(anchor: ui::CueAnchor) -> CueAnchor {
    match anchor {
        ui::CueAnchor::Start => CueAnchor::Start,
        ui::CueAnchor::Middle => CueAnchor::Middle,
        ui::CueAnchor::End => CueAnchor::End,
    }
}

fn program_alignment(alignment: ui::CueTextAlignment) -> CueTextAlignment {
    match alignment {
        ui::CueTextAlignment::Start => CueTextAlignment::Start,
        ui::CueTextAlignment::Center => CueTextAlignment::Center,
        ui::CueTextAlignment::End => CueTextAlignment::End,
    }
}

fn program_vertical(vertical: ui::CueVertical) -> CueVertical {
    match vertical {
        ui::CueVertical::RightToLeft => CueVertical::RightToLeft,
        ui::CueVertical::LeftToRight => CueVertical::LeftToRight,
    }
}

pub(crate) fn program_edge(edge: ui::CueTextEdge) -> CueTextEdge {
    match edge {
        ui::CueTextEdge::None => CueTextEdge::None,
        ui::CueTextEdge::Outline => CueTextEdge::Outline,
        ui::CueTextEdge::DropShadow => CueTextEdge::DropShadow,
        ui::CueTextEdge::Raised => CueTextEdge::Raised,
        ui::CueTextEdge::Depressed => CueTextEdge::Depressed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cue_is_the_programs_field_for_field() {
        let mut run = ui::CueTextRun::new("Hello");
        run.italic = true;
        run.background = Some(ui::Color::new(0x8000_0000));
        run.edge = Some(ui::CueTextEdge::Outline);
        let mut cue = ui::Cue::new(ui::CueContent::Text(vec![run]));
        cue.line = Some(ui::CueLine::Fraction(0.85));
        cue.position = Some(0.5);
        cue.position_anchor = ui::CueAnchor::Middle;
        cue.vertical = Some(ui::CueVertical::LeftToRight);
        cue.window_color = Some(ui::Color::new(0x4000_0000));
        let cue = program_cue(cue);
        let CueContent::Text(runs) = &cue.content else {
            panic!("a text cue");
        };
        assert_eq!(runs[0].text, "Hello");
        assert!(runs[0].italic && !runs[0].bold);
        assert_eq!(runs[0].color, None, "the media says no colour");
        assert_eq!(runs[0].background, Some(0x8000_0000));
        assert_eq!(runs[0].edge, Some(CueTextEdge::Outline));
        assert!(matches!(cue.line, Some(CueLine::Fraction(line)) if line == 0.85));
        assert_eq!(cue.position, Some(0.5));
        assert_eq!(cue.position_anchor, CueAnchor::Middle);
        assert_eq!(cue.text_alignment, CueTextAlignment::Center);
        assert_eq!(cue.vertical, Some(CueVertical::LeftToRight));
        assert_eq!(cue.window_color, Some(0x4000_0000));
    }

    #[test]
    fn a_picture_subtitle_keeps_its_pixels() {
        let cue = program_cue(ui::Cue::new(ui::CueContent::Image(ui::CueImage {
            width: 1,
            height: 1,
            pixels: vec![1, 2, 3, 4],
            display_height: 0.1,
        })));
        let CueContent::Image(image) = cue.content else {
            panic!("a picture subtitle");
        };
        assert_eq!(image.pixels, [1, 2, 3, 4]);
        assert_eq!(image.display_height, 0.1);
    }
}
