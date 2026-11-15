//! A program's capture sessions: each the shell's from its start while the program holds its
//! handle, stopped for the program when it drops it or ends, as its store's table drops it, shown
//! by the shell's indicator of who records meanwhile, and keeping the program from freezing, as
//! the freeze rule has it. The shell reports a session's state and its rotation angles into its
//! [`CaptureSessionReports`], as it reports a player's; the engine keeps the newest of each, which
//! `state` and `video-rotation-angles` read and every stream of their changes watches.
//!
//! The invariant: once the program lets a session go, its shell's session is dropped, nothing more
//! reaches its state, every stream of its state ends, the indicator hears it stopped, and the
//! freeze rule applies to the program again.

use std::sync::{Arc, Weak};

use futures::stream::BoxStream;
use tokio::sync::watch;
use wasmtime::bail;

use crate::access::Held;
use crate::bindings::wasdk::media::cameras::{CameraFormat, CaptureState, VideoRotationAngles};
use crate::lifecycle::Capturing;
use crate::newest::changes_of;
use crate::shell::{PhotoFuture, ShellCaptureSession, StartedSession};

/// `cameras.capture-session`: a camera running for the program, with the format it runs in.
pub struct CaptureSession {
    shell: Box<dyn ShellCaptureSession>,
    format: CameraFormat,
    state: SessionState,
    /// The camera's use, held while the session runs, which the shell's indicator shows.
    _recording: Held,
    _capturing: Capturing,
}

/// A session's state from before the shell starts it, where the shell reports it from its start
/// on, and whether photos are among its outputs.
pub(super) struct SessionState {
    reported: Arc<Reported>,
    takes_photos: bool,
}

/// What the shell reported of a session last.
struct Reported {
    /// Its state: `running` until the shell reports.
    state: watch::Sender<CaptureState>,
    /// Its rotation angles: 0 until the shell reports.
    angles: watch::Sender<VideoRotationAngles>,
}

impl SessionState {
    /// A session's state, running and unturned until the shell reports otherwise, which takes
    /// photos where `takes_photos`.
    pub(super) fn new(takes_photos: bool) -> SessionState {
        let unturned = VideoRotationAngles {
            for_horizon_level_preview: 0,
            for_horizon_level_capture: 0,
        };
        SessionState {
            reported: Arc::new(Reported {
                state: watch::Sender::new(CaptureState::Running),
                angles: watch::Sender::new(unturned),
            }),
            takes_photos,
        }
    }

    /// Where the shell reports the session's state and its rotation angles.
    pub(super) fn reports(&self) -> CaptureSessionReports {
        CaptureSessionReports(Arc::downgrade(&self.reported))
    }
}

impl CaptureSession {
    /// `started`, the session the shell started, whose state is `state`, recording as `recording`
    /// holds and keeping the program thawed as `capturing` does.
    pub(super) fn new(
        started: StartedSession,
        state: SessionState,
        recording: Held,
        capturing: Capturing,
    ) -> CaptureSession {
        CaptureSession {
            shell: started.session,
            format: started.format,
            state,
            _recording: recording,
            _capturing: capturing,
        }
    }

    pub(super) fn format(&self) -> CameraFormat {
        self.format.clone()
    }

    /// The state as the shell reported it last.
    pub(super) fn state(&self) -> CaptureState {
        self.state.reported.state.borrow().clone()
    }

    /// Each state the shell reports from now on, on a stream of its own: one read late hears the
    /// newest, passing over those between; it ends once the session is let go.
    pub(super) fn state_changes(&self) -> BoxStream<'static, CaptureState> {
        changes_of(self.state.reported.state.subscribe())
    }

    /// The rotation angles as the shell reported them last.
    pub(super) fn video_rotation_angles(&self) -> VideoRotationAngles {
        *self.state.reported.angles.borrow()
    }

    /// The rotation angles the shell reports from now on, as `state_changes` the state.
    pub(super) fn video_rotation_angle_changes(&self) -> BoxStream<'static, VideoRotationAngles> {
        changes_of(self.state.reported.angles.subscribe())
    }

    /// A photo, encoded by the system; a trap for a session started without photos.
    pub(super) fn take_photo(&self) -> wasmtime::Result<PhotoFuture> {
        if !self.state.takes_photos {
            bail!("the program asked for a photo of a session it started without photos");
        }
        Ok(self.shell.take_photo())
    }
}

/// Where the shell reports one session's state and rotation angles, from any thread. Reports on a
/// session the program has let go of are dropped.
#[derive(Clone)]
pub struct CaptureSessionReports(Weak<Reported>);

impl CaptureSessionReports {
    /// The session's state is `state` from now on: what `state` answers, and what every stream of
    /// its changes hears next.
    pub fn state_changed(&self, state: CaptureState) {
        if let Some(reported) = self.0.upgrade() {
            reported.state.send_replace(state);
        }
    }

    /// The session's rotation angles are `angles` from now on, as `state_changed` the state; a
    /// report of the same angles is no change.
    pub fn video_rotation_angles_changed(&self, angles: VideoRotationAngles) {
        if let Some(reported) = self.0.upgrade() {
            reported.angles.send_if_modified(|kept| {
                let changed = !same_angles(kept, &angles);
                *kept = angles;
                changed
            });
        }
    }
}

/// Whether `a` and `b` are the same turns.
fn same_angles(a: &VideoRotationAngles, b: &VideoRotationAngles) -> bool {
    a.for_horizon_level_preview == b.for_horizon_level_preview
        && a.for_horizon_level_capture == b.for_horizon_level_capture
}

#[cfg(test)]
mod tests {
    use futures::StreamExt;
    use futures::executor::block_on;

    use super::*;
    use crate::bindings::wasdk::media::cameras::{CameraError, CameraInterruption};

    /// A state's reports, as a session holds them, outside a store.
    fn reported(takes_photos: bool) -> (SessionState, CaptureSessionReports) {
        let state = SessionState::new(takes_photos);
        let reports = state.reports();
        (state, reports)
    }

    #[test]
    fn a_session_runs_until_the_shell_reports_otherwise_and_a_late_stream_hears_the_newest() {
        let (state, reports) = reported(false);
        assert!(matches!(
            *state.reported.state.borrow(),
            CaptureState::Running
        ));
        let mut changes = changes_of(state.reported.state.subscribe());
        reports.state_changed(CaptureState::Interrupted(Some(
            CameraInterruption::Background,
        )));
        reports.state_changed(CaptureState::Ended(CameraError::Unplugged));
        assert!(matches!(
            block_on(changes.next()),
            Some(CaptureState::Ended(CameraError::Unplugged))
        ));
    }

    /// The angles start unturned; a report of new ones reaches a stream, and the same report again
    /// does not.
    #[test]
    fn a_sessions_angles_are_heard_as_they_change_and_not_again() {
        let (state, reports) = reported(false);
        let turned = |preview, capture| VideoRotationAngles {
            for_horizon_level_preview: preview,
            for_horizon_level_capture: capture,
        };
        assert!(same_angles(&state.reported.angles.borrow(), &turned(0, 0)));
        let mut changes = changes_of(state.reported.angles.subscribe());
        reports.video_rotation_angles_changed(turned(90, 90));
        let heard = block_on(changes.next()).expect("a change");
        assert!(same_angles(&heard, &turned(90, 90)));
        reports.video_rotation_angles_changed(turned(90, 90));
        drop(state);
        assert!(
            block_on(changes.next()).is_none(),
            "the same angles are no change"
        );
    }

    #[test]
    fn a_state_let_go_ends_its_streams_and_drops_what_is_reported() {
        let (state, reports) = reported(false);
        let mut changes = changes_of(state.reported.state.subscribe());
        drop(state);
        reports.state_changed(CaptureState::Running);
        assert!(block_on(changes.next()).is_none());
    }
}
