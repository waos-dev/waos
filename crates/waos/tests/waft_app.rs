//! A Waft app, `builtin-apps/notes` over `wasdk-adapter-waft`, under the test shell, given this
//! computer's fonts as an app gives them: given no view, it asks for its window, which the shell
//! answers with a view at once; it presents frames to the view, its text drawn with glyphs the
//! engine draws, each glyph call recorded under `waos::targets::GLYPHS`; it presents another when
//! the view is resized; and closing its one window quits it, its `main` returning.
//!
//! Its own binary, so the glyph calls the engine records are the app's alone. It is a debug build
//! of Waft, large to compile and slow to start, so the test waits the test shell's longest and
//! counts few frames. Hardware buffers are made on Apple's systems, and elsewhere the test says so
//! and passes; so does it on a machine with no fonts installed.

// Proving the test shell crosses threads walks from a program's handle into wgpu-core's
// registries, deeper than the default limit allows; an app embedding the engine needs the same.
#![recursion_limit = "256"]

#[path = "all/shell/mod.rs"]
mod shell;

#[cfg(not(target_vendor = "apple"))]
#[test]
fn a_waft_app_is_not_drawn_on_this_system() {
    eprintln!(
        "the engine makes hardware buffers on Apple's systems; a Waft app's test does not run here"
    );
}

#[cfg(target_vendor = "apple")]
mod waft_app {
    use std::time::Instant;

    use waos::ProgramEnd;
    use waos_local::LocalCapabilities;

    use super::shell::{self, VIEW_HEIGHT, VIEW_WIDTH, WITHIN, wait_for_end, wait_for_presents};

    /// The title Notes asks its window to have.
    const TITLE: &str = "Notes";

    #[test]
    fn a_waft_app_draws_its_text_with_the_engines_glyphs_follows_its_view_and_quits_when_it_closes()
    {
        let environment = LocalCapabilities::new().environment();
        let installed = environment
            .fonts
            .as_ref()
            .map_or(0, |fonts| fonts.lock().unwrap().face_count());
        if installed == 0 {
            eprintln!("SKIP: no fonts installed on this machine");
            return;
        }
        let glyph_calls = shell::glyph_calls();
        let output = shell::output();
        let mark = output.mark();
        let printed = || output.lines_of(mark, "notes");
        let (engine, shell, arrivals) = shell::engine();
        let loading = Instant::now();
        let app = shell::load(&engine, waos_test_programs::NOTES);
        let loaded_in = loading.elapsed();
        let starting = Instant::now();
        let running = engine.run(&app, environment).unwrap();

        let view = arrivals
            .recv_timeout(WITHIN)
            .expect("the app asks for its window");
        let asked_in = starting.elapsed();
        assert_eq!(shell.requested_titles(), [TITLE]);
        let shell_view = shell.placed_views().pop().expect("the shell placed a view");
        wait_for_presents(&running, &shell_view, 1, printed);
        let first_frame_in = starting.elapsed();
        glyph_calls.wait_for_drawn(1);
        eprintln!(
            "notes: loaded in {loaded_in:?}; asked for its window {asked_in:?} after its start and presented its first frame {first_frame_in:?} after it; {} glyphs drawn by the engine",
            glyph_calls.drawn()
        );

        let presented = shell_view.presents();
        view.metrics_changed(shell::metrics(VIEW_WIDTH / 2, VIEW_HEIGHT / 2));
        wait_for_presents(&running, &shell_view, presented + 1, printed);

        view.close_requested();
        let end = wait_for_end(&running);
        assert!(
            matches!(end, ProgramEnd::Returned(Ok(()))),
            "closing its one window quits the app, not {end:?}; it printed {:#?}",
            printed()
        );
    }
}
