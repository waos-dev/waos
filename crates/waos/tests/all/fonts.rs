//! `examples/fonts` under the test shell: given this computer's fonts, as an app gives them, it
//! finds the face covering a letter, its family and the bold among them, and the system font,
//! reads the system font's character map from the table the engine serves, opens the system
//! font's file and finds the same glyph in it, and gets the coverage
//! and the outline of the letter's glyph, drawn by the engine; given none, every answer is
//! empty, a glyph of a font it names anyway among them.
//!
//! Every test here reads what the program printed in the log, and two runs at once would print
//! alike, so each takes the program's turn (`shell::turns`). On a machine with no fonts installed
//! the first says so and passes.

use std::sync::Arc;

use futures::executor::block_on;
use waos::{ProgramEnd, ProgramEnvironment, WaOS};
use waos_capabilities::valo_fontmgr::Style;
use waos_local::LocalCapabilities;

use crate::shell;
use crate::shell::output;

#[test]
fn a_program_given_this_computers_fonts_draws_a_glyph_of_the_system_font() {
    let _fonts = shell::turns::take("fonts");
    let environment = LocalCapabilities::new().environment();
    let installed = environment
        .fonts
        .as_ref()
        .map_or(0, |fonts| fonts.lock().unwrap().face_count());
    if installed == 0 {
        eprintln!("SKIP: no fonts installed on this machine");
        return;
    }
    // A system whose font manager names no face as its own, as Linux's does not, has none for
    // the program to draw, and every answer about it is empty: say so and pass.
    let names_a_system_font = environment.fonts.as_ref().is_some_and(|fonts| {
        fonts
            .lock()
            .unwrap()
            .system_font(13.0, Style::default())
            .is_some()
    });
    if !names_a_system_font {
        eprintln!("SKIP: this system names no system font");
        return;
    }
    let printed = run_fonts(environment);

    let families: usize = number_after(&printed, "families: ");
    assert!(families > 0, "{printed:#?}");
    let covering = line_after(&printed, "covering 'a': ");
    assert_ne!(covering, "none", "{printed:#?}");
    let family = covering.split(", ").next().unwrap();
    assert!(
        number_after::<usize>(&printed, &format!("{family}: ")) > 0,
        "{printed:#?}"
    );
    assert!(
        line_after(&printed, "its bold: ").contains("weight 7"),
        "{printed:#?}"
    );
    let system_font = line_after(&printed, "system font: ");
    assert_ne!(system_font, "none", "{printed:#?}");
    assert!(
        number_after::<usize>(&printed, "cmap: ") > 0,
        "{printed:#?}"
    );
    let glyph: u32 = number_after(&printed, "glyph of 'a': ");
    assert!(glyph > 0, "{printed:#?}");
    let file = line_after(&printed, "file: ");
    assert!(
        number_after::<usize>(&printed, "file: ") > 0,
        "{printed:#?}"
    );
    assert!(
        file.ends_with(&format!("glyph of 'a' {glyph}")),
        "the file's own character map agrees: {printed:#?}"
    );
    let coverage = line_after(&printed, "coverage: ");
    let inked: usize = coverage
        .split(", ")
        .nth(1)
        .and_then(|inked| inked.strip_suffix(" pixels inked"))
        .and_then(|count| count.parse().ok())
        .unwrap_or_else(|| panic!("{printed:#?}"));
    assert!(inked > 0, "the glyph has ink: {printed:#?}");
    let outline = line_after(&printed, "outline: ");
    let drawing: usize = outline
        .split(", ")
        .nth(1)
        .and_then(|drawing| drawing.strip_suffix(" drawing"))
        .and_then(|count| count.parse().ok())
        .unwrap_or_else(|| panic!("{printed:#?}"));
    assert!(drawing > 0, "the outline has curves or lines: {printed:#?}");
}

#[test]
fn a_program_given_no_fonts_gets_empty_answers() {
    let _fonts = shell::turns::take("fonts");
    let printed = run_fonts(ProgramEnvironment::default());
    assert_eq!(
        printed,
        [
            "families: 0",
            "covering 'a': none",
            "system font: none",
            "coverage: none",
            "outline: none",
        ],
    );
}

/// What `examples/fonts` printed, run in `environment` to its end.
fn run_fonts(environment: ProgramEnvironment) -> Vec<String> {
    let output = output();
    let mark = output.mark();
    let (engine, _shell, _arrivals): (Arc<WaOS>, _, _) = shell::engine();
    let fonts = shell::load(&engine, waos_test_programs::FONTS);
    let running = engine.run(&fonts, environment).unwrap();
    let end = block_on(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
    output.lines_of(mark, "fonts")
}

/// What follows `start` on the line that begins with it.
fn line_after<'a>(printed: &'a [String], start: &str) -> &'a str {
    printed
        .iter()
        .find_map(|line| line.strip_prefix(start))
        .unwrap_or_else(|| panic!("no line {start:?}…: {printed:#?}"))
}

/// The number that begins what follows `start`.
fn number_after<T: std::str::FromStr>(printed: &[String], start: &str) -> T {
    let rest = line_after(printed, start);
    let number = rest.split(' ').next().unwrap_or(rest);
    number
        .parse()
        .unwrap_or_else(|_| panic!("{rest:?} after {start:?}: {printed:#?}"))
}
