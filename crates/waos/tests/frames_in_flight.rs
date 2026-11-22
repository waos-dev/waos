//! `examples/two-windows` under the test shell, both windows on its one screen, with the first
//! window's presents held back, as GPU work that does not finish holds a frame: the first window is
//! served no more than two frames meanwhile, its two frames in flight, while every refresh serves
//! the second alone; let through, the first is served again.
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
fn frames_in_flight_are_not_drawn_on_this_system() {
    eprintln!("the engine makes hardware buffers on Apple's systems; this test does not run here");
}

#[cfg(target_vendor = "apple")]
mod frames_in_flight {
    use waos::{ProgramEnd, ProgramEnvironment, ProgramView};

    use super::shell::{self, WITHIN, wait_for_end, wait_for_presents};

    /// How many of a view's frames may be in flight at once, as Flutter's pipeline allows.
    const MOST_FRAMES_IN_FLIGHT: usize = 2;

    /// How many frames the second window presents while the first's are held back.
    const WATCHED_FRAMES: usize = 20;

    #[test]
    fn a_view_whose_frames_do_not_land_is_served_two_while_the_other_on_its_screen_goes_on() {
        let output = shell::output();
        let mark = output.mark();
        let printed = || output.lines_of(mark, "two-windows");
        let frames = shell::frames_handed_over();
        let (engine, shell, arrivals) = shell::engine();
        let program = shell::load(&engine, waos_test_programs::TWO_WINDOWS);
        let running = engine.run(&program, ProgramEnvironment::default()).unwrap();
        let views: Vec<ProgramView> = (0..2)
            .map(|_| {
                arrivals
                    .recv_timeout(WITHIN)
                    .expect("two-windows asks for its windows")
            })
            .collect();
        let placed = shell.placed_views();
        let (held, going_on) = (&placed[0], &placed[1]);
        wait_for_presents(&running, held, 1, printed);
        wait_for_presents(&running, going_on, 1, printed);

        held.hold_presents();
        let held_from = frames.mark();
        wait_for_presents(
            &running,
            going_on,
            going_on.presents() + WATCHED_FRAMES,
            printed,
        );
        let while_held = frames.since(held_from);
        let served_held = while_held
            .iter()
            .filter(|frame| frame.view == views[0].id())
            .count();
        eprintln!(
            "two-windows: {served_held} frames served to the held window while the other presented {WATCHED_FRAMES}"
        );
        assert!(
            served_held <= MOST_FRAMES_IN_FLIGHT,
            "the held window was served {served_held} frames, more than can be in flight"
        );
        assert!(
            while_held
                .iter()
                .any(|frame| frame.view == views[1].id() && frame.views == [views[1].id()]),
            "refreshes serve the other window alone meanwhile"
        );

        let let_through_from = frames.mark();
        held.let_presents_through();
        frames.wait_for(let_through_from, |frame| frame.view == views[0].id());
        wait_for_presents(&running, held, held.presents() + 1, printed);

        views[0].close_requested();
        let end = wait_for_end(&running);
        assert!(
            matches!(end, ProgramEnd::Returned(Ok(()))),
            "asking to close a window quits two-windows, not {end:?}; it printed {:#?}",
            printed()
        );
    }
}
