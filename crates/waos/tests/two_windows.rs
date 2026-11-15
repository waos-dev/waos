//! `examples/two-windows`, an Waft app with two windows over `wasdk-adapter-waft`, under the
//! test shell, both on its one screen: a refresh serves both windows, each frame naming both.
//! While the shell has the second window hidden, reporting its view off screen, the first keeps
//! presenting frames, each refresh serving it alone, and the second presents none; shown again,
//! the second presents again. Asking to close a window quits the program, its `main` returning.
//!
//! Its own binary, so the frames the engine records handing over are this program's alone. The
//! program is a debug build of Waft, slow to start, so the test waits the test shell's longest.
//! Hardware buffers are made on Apple's systems, and elsewhere the test says so and passes.

// Proving the test shell crosses threads walks from a program's handle into wgpu-core's
// registries, deeper than the default limit allows; an app embedding the engine needs the same.
#![recursion_limit = "256"]

#[path = "all/shell/mod.rs"]
mod shell;

#[cfg(not(target_vendor = "apple"))]
#[test]
fn two_windows_are_not_drawn_on_this_system() {
    eprintln!(
        "the engine makes hardware buffers on Apple's systems; two-windows' test does not run here"
    );
}

#[cfg(target_vendor = "apple")]
mod two_windows {
    use std::time::Instant;

    use waos::{ProgramEnd, ProgramEnvironment, ProgramView};

    use super::shell::{self, WITHIN, wait_for_end, wait_for_presents};

    /// The titles two-windows asks its windows to have, in order.
    const TITLES: [&str; 2] = ["first", "second"];

    /// How many frames the shown window presents after the hidden one's last could have reached
    /// the shell: a frame handed over just before the hide is presented by then.
    const SETTLING_FRAMES: usize = 3;

    /// How many frames the shown window presents while the hidden one is watched.
    const WATCHED_FRAMES: usize = 20;

    #[test]
    fn a_hidden_window_gets_no_frames_while_the_shown_one_keeps_drawing_and_draws_again_once_shown()
    {
        let output = shell::output();
        let mark = output.mark();
        let printed = || output.lines_of(mark, "two-windows");
        let (engine, shell, arrivals) = shell::engine();
        let program = shell::load(&engine, waos_test_programs::TWO_WINDOWS);
        let (running, given) = engine
            .run(&program, ProgramEnvironment::default(), Vec::new())
            .unwrap();
        assert!(given.is_empty(), "the shell gave two-windows no view");
        let views: Vec<ProgramView> = TITLES
            .iter()
            .map(|_| {
                arrivals
                    .recv_timeout(WITHIN)
                    .expect("two-windows asks for its windows")
            })
            .collect();
        assert_eq!(shell.requested_titles(), TITLES);
        let placed = shell.placed_views();
        let (shown, hidden) = (&placed[0], &placed[1]);
        wait_for_presents(&running, shown, 1, printed);
        wait_for_presents(&running, hidden, 1, printed);

        let frames = shell::frames_handed_over();
        let mark = frames.mark();
        let both = vec![views[0].id(), views[1].id()];
        for view in &views {
            let frame = frames.wait_for(mark, |frame| {
                frame.view == view.id() && frame.views.len() > 1
            });
            assert_eq!(
                frame.views, both,
                "a refresh serves both windows on the one screen, each frame naming both"
            );
        }

        shell::hide(&views[1]);
        let hidden_at = Instant::now();
        wait_for_presents(&running, shown, shown.presents() + SETTLING_FRAMES, printed);
        let hidden_presents = hidden.presents();
        let shown_presents = shown.presents();
        let mark = frames.mark();
        wait_for_presents(&running, shown, shown_presents + WATCHED_FRAMES, printed);
        eprintln!(
            "two-windows: {WATCHED_FRAMES} frames of the shown window, and none of the hidden one, in {:?} after the hide",
            hidden_at.elapsed()
        );
        assert_eq!(
            hidden.presents(),
            hidden_presents,
            "the hidden window presents nothing while the shown one presents {WATCHED_FRAMES} frames"
        );
        let shown_alone = frames.wait_for(mark, |frame| frame.view == views[0].id());
        assert_eq!(
            shown_alone.views,
            [views[0].id()],
            "a refresh serves the shown window alone"
        );

        shell::show(&views[1]);
        wait_for_presents(&running, hidden, hidden_presents + 1, printed);
        wait_for_presents(&running, shown, shown.presents() + 1, printed);

        views[0].close_requested();
        let end = wait_for_end(&running);
        assert!(
            matches!(end, ProgramEnd::Returned(Ok(()))),
            "asking to close a window quits two-windows, not {end:?}; it printed {:#?}",
            printed()
        );
    }
}
