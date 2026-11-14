//! The viewfinder: the camera running for the app, and where it stands when none does. It lists
//! the system's cameras, opens the one chosen, which has WaOS ask the user the first time, and
//! starts it with its frames in a reader of the app's own, for the live picture to draw, and its
//! photos at the largest size the format takes them at. It starts again by itself when what kept
//! it from running goes: a camera is plugged in, or the user allows the cameras in WaOS's
//! settings; and it rests, the camera stopped, while the app is out of sight.
//!
//! The invariant a later editor will break: every start is numbered, and a start's answer counts
//! only while its number is the newest, so a start overtaken by a rest, a switch or another start
//! lets its session go instead of showing it.

use std::rc::Rc;
use std::sync::Arc;

use waft::ui::{
    Camera, CameraDevice, CameraError, CameraFormat, CameraId, CameraOutputs, CaptureState,
    VideoFrameReader,
};
use waft::{
    App, AsyncApp, CameraSession, Entity, Handle, Listenable, Listener, Size, available_cameras,
    available_cameras_changed, open_camera,
};

use crate::looks::Look;
use crate::permission;

/// The largest picture the camera is started at: more only costs the live picture's drawing.
const LARGEST: (f64, f64) = (1920.0, 1080.0);

/// Where the viewfinder stands.
pub(crate) enum Status {
    /// The cameras are being listed, or one is being opened or started.
    Starting,
    /// A camera is being opened, and WaOS is asking the user whether the app may use it.
    Asking,
    /// The system lists no camera.
    NoCamera,
    /// The user does not let the app use the cameras.
    NotAllowed,
    /// The camera is stopped while the app is out of sight.
    Resting,
    /// The camera did not start, or ended, for this reason, worded for the user.
    Failed(String),
    Running(Running),
}

/// A camera running for the app.
#[derive(Clone)]
pub(crate) struct Running {
    pub(crate) camera: Camera,
    pub(crate) session: Handle<CameraSession>,
    /// The camera's frames, read by the live picture.
    pub(crate) frames: Arc<dyn VideoFrameReader>,
}

pub(crate) struct Viewfinder {
    status: Status,
    /// The cameras the system listed last.
    cameras: Vec<Camera>,
    /// The camera the user switched to; the system's first until they switch.
    chosen: Option<CameraId>,
    look: Look,
    taking_photo: bool,
    /// The number of the newest start.
    starts: u64,
}

impl Viewfinder {
    /// A viewfinder starting the camera, which follows the system's cameras and the user's
    /// permission from now on.
    pub(crate) fn open(app: &mut App) -> Entity<Viewfinder> {
        let viewfinder = app.new_entity(|_cx| Viewfinder {
            status: Status::Starting,
            cameras: Vec::new(),
            chosen: None,
            look: Look::default(),
            taking_photo: false,
            starts: 0,
        });
        follow_cameras(app, &viewfinder);
        let refused = viewfinder.clone();
        let allowed = Listener::new(move |app: &mut App| {
            start_where(app, &refused, |status| matches!(status, Status::NotAllowed));
        });
        permission::follow_allowing(app, allowed);
        start(app, &viewfinder);
        viewfinder
    }

    pub(crate) fn status(&self) -> &Status {
        &self.status
    }

    pub(crate) fn running(&self) -> Option<&Running> {
        match &self.status {
            Status::Running(running) => Some(running),
            _ => None,
        }
    }

    pub(crate) fn look(&self) -> Look {
        self.look
    }

    pub(crate) fn set_look(&mut self, look: Look) {
        self.look = look;
    }

    /// Whether the user has another camera to switch to.
    pub(crate) fn has_another_camera(&self) -> bool {
        self.cameras.len() > 1
    }

    pub(crate) fn is_taking_photo(&self) -> bool {
        self.taking_photo
    }

    pub(crate) fn set_taking_photo(&mut self, taking: bool) {
        self.taking_photo = taking;
    }

    /// The camera to run: the one chosen while the system lists it, else its first.
    fn camera_to_run(&self) -> Option<Camera> {
        let chosen = self
            .cameras
            .iter()
            .find(|camera| Some(&camera.id) == self.chosen.as_ref());
        chosen.or(self.cameras.first()).cloned()
    }

    /// The camera listed after the one running, around to the first.
    fn camera_after_running(&self) -> Option<CameraId> {
        let running = &self.running()?.camera.id;
        let at = self
            .cameras
            .iter()
            .position(|camera| &camera.id == running)?;
        let next = self.cameras.get((at + 1) % self.cameras.len())?;
        Some(next.id.clone())
    }
}

/// Starts the camera anew, stopping the one running.
pub(crate) fn start(app: &mut App, viewfinder: &Entity<Viewfinder>) {
    let first = if permission::is_unanswered() {
        Status::Asking
    } else {
        Status::Starting
    };
    let number = stand(app, viewfinder, first);
    let viewfinder = viewfinder.clone();
    app.spawn(async move |cx| {
        let status = started(cx, &viewfinder).await;
        cx.update(|app| finish_start(app, &viewfinder, number, status));
    });
}

/// Stops the camera while the app is out of sight.
pub(crate) fn rest(app: &mut App, viewfinder: &Entity<Viewfinder>) {
    stand(app, viewfinder, Status::Resting);
}

/// Starts the camera again where it rests.
pub(crate) fn wake(app: &mut App, viewfinder: &Entity<Viewfinder>) {
    start_where(app, viewfinder, |status| matches!(status, Status::Resting));
}

/// Runs the camera listed after the one running.
pub(crate) fn switch_camera(app: &mut App, viewfinder: &Entity<Viewfinder>) {
    let switched = viewfinder.update(app, |viewfinder, _cx| {
        let next = viewfinder.camera_after_running();
        let switched = next.is_some();
        viewfinder.chosen = next.or(viewfinder.chosen.take());
        switched
    });
    if switched {
        start(app, viewfinder);
    }
}

/// Starts the camera where the viewfinder stands as `standing` says.
fn start_where(
    app: &mut App,
    viewfinder: &Entity<Viewfinder>,
    standing: impl FnOnce(&Status) -> bool,
) {
    if standing(viewfinder.read(app).status()) {
        start(app, viewfinder);
    }
}

/// Has the viewfinder stand at `status`, the session that ran let go and any start under way
/// overtaken; answers the number a start from here has.
fn stand(app: &mut App, viewfinder: &Entity<Viewfinder>, status: Status) -> u64 {
    let (was, number) = viewfinder.update(app, |viewfinder, cx| {
        viewfinder.starts += 1;
        viewfinder.taking_photo = false;
        cx.notify();
        (
            std::mem::replace(&mut viewfinder.status, status),
            viewfinder.starts,
        )
    });
    if let Status::Running(running) = was {
        running.session.dispose(app);
    }
    number
}

/// Takes the answer of start `number`: shown while it is the newest, let go otherwise.
fn finish_start(app: &mut App, viewfinder: &Entity<Viewfinder>, number: u64, status: Status) {
    if viewfinder.read(app).starts != number {
        if let Status::Running(overtaken) = status {
            overtaken.session.dispose(app);
        }
        return;
    }
    if let Status::Running(running) = &status {
        follow_session(app, viewfinder, running.session);
    }
    viewfinder.update(app, |viewfinder, cx| {
        viewfinder.status = status;
        cx.notify();
    });
}

/// Lists the cameras, opens the one to run and starts it: where that leaves the viewfinder.
async fn started(cx: &AsyncApp, viewfinder: &Entity<Viewfinder>) -> Status {
    let cameras = cx.update(|app| available_cameras(app)).await;
    let camera = cx.update(|app| {
        viewfinder.update(app, |viewfinder, cx| {
            viewfinder.cameras = cameras;
            cx.notify();
            viewfinder.camera_to_run()
        })
    });
    let Some(camera) = camera else {
        return Status::NoCamera;
    };
    let Some(opening) = cx.update(|app| open_camera(app, &camera)) else {
        return Status::NoCamera;
    };
    let running = match opening.await {
        Ok(device) => run(cx, camera, &*device).await,
        Err(error) => Err(error),
    };
    match running {
        Ok(running) => Status::Running(running),
        Err(CameraError::NotAllowed) => Status::NotAllowed,
        Err(error) => Status::Failed(failure_words(&error)),
    }
}

/// Starts `device`, the camera `camera` opened, in its preferred format, its frames in a new
/// reader and its photos at that format's largest size.
async fn run(
    cx: &AsyncApp,
    camera: Camera,
    device: &dyn CameraDevice,
) -> Result<Running, CameraError> {
    let formats = device.formats();
    let format = preferred(&formats)
        .ok_or_else(|| CameraError::Failed("the camera lists no format".to_owned()))?;
    let frames = cx
        .update(|app| Some(app.platform().video_frame_readers()?.make_reader()))
        .ok_or_else(|| {
            CameraError::Failed("this system hands a program no frames to draw".to_owned())
        })?;
    let mut outputs = CameraOutputs::new().frames(Arc::clone(&frames));
    if let Some(size) = largest(&format.photo_sizes) {
        outputs = outputs.photos(size);
    }
    let session = cx
        .update(|app| CameraSession::start(app, device, format, outputs))
        .await?;
    Ok(Running {
        camera,
        session,
        frames,
    })
}

/// The largest format up to [`LARGEST`], the fastest of those that size; else the first.
fn preferred(formats: &[CameraFormat]) -> Option<&CameraFormat> {
    let fits = |format: &&CameraFormat| {
        format.size.width() <= LARGEST.0 && format.size.height() <= LARGEST.1
    };
    formats
        .iter()
        .filter(fits)
        .max_by(|a, b| {
            area(a.size)
                .total_cmp(&area(b.size))
                .then(a.max_frame_rate.total_cmp(&b.max_frame_rate))
        })
        .or(formats.first())
}

fn largest(sizes: &[Size]) -> Option<Size> {
    sizes
        .iter()
        .copied()
        .max_by(|a, b| area(*a).total_cmp(&area(*b)))
}

fn area(size: Size) -> f64 {
    size.width() * size.height()
}

/// Has the viewfinder hear `session`'s state change: shown again while it runs or is
/// interrupted, and acted on once it has ended.
fn follow_session(app: &mut App, viewfinder: &Entity<Viewfinder>, session: Handle<CameraSession>) {
    let viewfinder = viewfinder.clone();
    let changed =
        Listener::new(
            move |app: &mut App| match CameraSession::state(session, app).clone() {
                CaptureState::Ended(error) => session_ended(app, &viewfinder, error),
                CaptureState::Running | CaptureState::Interrupted(_) => {
                    viewfinder.update(app, |_viewfinder, cx| cx.notify());
                }
            },
        );
    let listenable: Rc<dyn Listenable> = Rc::new(session);
    listenable.add_listener(app, changed);
}

/// The session ended for good: another camera is started where this one was unplugged, and the
/// viewfinder says why it stopped otherwise.
fn session_ended(app: &mut App, viewfinder: &Entity<Viewfinder>, error: CameraError) {
    match error {
        CameraError::Unplugged => start(app, viewfinder),
        CameraError::NotAllowed => {
            stand(app, viewfinder, Status::NotAllowed);
        }
        error => {
            stand(app, viewfinder, Status::Failed(failure_words(&error)));
        }
    }
}

/// Keeps the viewfinder's cameras as the system lists them, and starts one where it waited for
/// a camera.
fn follow_cameras(app: &mut App, viewfinder: &Entity<Viewfinder>) {
    let viewfinder = viewfinder.clone();
    app.spawn(async move |cx| {
        loop {
            let Some(changed) = cx.update(|app| available_cameras_changed(app)) else {
                return;
            };
            changed.await;
            let cameras = cx.update(|app| available_cameras(app)).await;
            cx.update(|app| cameras_changed(app, &viewfinder, cameras));
        }
    });
}

fn cameras_changed(app: &mut App, viewfinder: &Entity<Viewfinder>, cameras: Vec<Camera>) {
    viewfinder.update(app, |viewfinder, cx| {
        viewfinder.cameras = cameras;
        cx.notify();
    });
    start_where(app, viewfinder, |status| matches!(status, Status::NoCamera));
}

/// Why the camera does not run, as the window says it.
fn failure_words(error: &CameraError) -> String {
    match error {
        CameraError::NotAllowed => "Camera is not allowed to use the camera.".to_owned(),
        CameraError::InUse => "Another app is using the camera.".to_owned(),
        CameraError::Unplugged => "The camera was unplugged.".to_owned(),
        CameraError::NotSupported(reason) | CameraError::Failed(reason) => {
            format!("The camera did not start: {reason}.")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(width: f64, height: f64, fastest: f64) -> CameraFormat {
        CameraFormat {
            size: Size::new(width, height),
            min_frame_rate: 1.0,
            max_frame_rate: fastest,
            photo_sizes: vec![Size::new(width, height)],
        }
    }

    #[test]
    fn the_largest_format_up_to_full_hd_is_preferred_and_the_fastest_of_its_size() {
        let formats = [
            format(640.0, 480.0, 60.0),
            format(1920.0, 1080.0, 15.0),
            format(1920.0, 1080.0, 30.0),
            format(3840.0, 2160.0, 30.0),
        ];
        assert_eq!(preferred(&formats), Some(&formats[2]));
    }

    #[test]
    fn a_camera_with_only_larger_formats_runs_in_its_first() {
        let formats = [format(3840.0, 2160.0, 30.0), format(4096.0, 2160.0, 24.0)];
        assert_eq!(preferred(&formats), Some(&formats[0]));
        assert_eq!(preferred(&[]), None);
    }

    #[test]
    fn photos_are_taken_at_the_largest_size() {
        let sizes = [Size::new(1280.0, 720.0), Size::new(4032.0, 3024.0)];
        assert_eq!(largest(&sizes), Some(sizes[1]));
    }
}
