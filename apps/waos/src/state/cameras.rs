//! The programs' cameras on the main thread, over Waft's: listed and opened on Waft's cameras as
//! the engine asks, each answered once Waft has, the next change of Waft's list told as it comes,
//! the camera named by its id among those Waft lists, each opened camera kept
//! with its formats; a session started on one in the Waft format the program named, its preview on
//! Waft's layer behind the program's, its frames in the program's reader, Waft's from the first
//! time a camera or a player is given it, and its photos at the size named; each kept until the
//! program lets it go, its photos taken as the program asks and its state reported to the program
//! as it starts and at each change after, as Waft's own `CameraSession` follows the host's. The
//! cameras are the engine's shell's where Waft's platform has them.
//!
//! The invariant: every call that asks for an answer is answered once Waft has, so no program
//! waits for a camera that cannot come, and a wait for the list's next change ends at the change;
//! and what the engine let go of meanwhile, its answer refused, is let go here too.

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::ready;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use futures::channel::oneshot;
use waft::App;
use waft::ui::{self, CameraDevice, CaptureSession, Platform};
use waos::bindings::wasdk::media::cameras::{CameraError, CameraFormat, CameraInfo};
use waos::{CaptureSessionReports, ShellCameras};

use crate::logic::camera_calls::{CameraCall, CameraKey, CamerasOnRequest, SessionOutputs};
use crate::logic::camera_values::{
    program_angles, program_camera, program_error, program_format, program_photo, program_state,
    waft_format,
};

/// Waft's cameras, served to the engine as its shell's.
pub(crate) struct Cameras {
    shell: Arc<CamerasOnRequest>,
    arriving: UnboundedReceiver<CameraCall>,
}

impl Cameras {
    /// Waft's cameras on `platform`, to serve to the engine; none where it has none.
    pub(crate) fn of(platform: &dyn Platform) -> Option<Cameras> {
        platform.cameras()?;
        let (shell, arriving) = CamerasOnRequest::new();
        Some(Cameras {
            shell: Arc::new(shell),
            arriving,
        })
    }

    /// The cameras as the engine's shell has them: each call goes to the main thread, where
    /// [`serve`](Self::serve) carries it out.
    pub(crate) fn shell(&self) -> Arc<dyn ShellCameras> {
        Arc::clone(&self.shell) as Arc<dyn ShellCameras>
    }

    /// From now on, each camera a program opens is one of Waft's, and does as the program asks.
    pub(crate) fn serve(self, app: &mut App) {
        let mut arriving = self.arriving;
        app.spawn(async move |cx| {
            let held = Rc::new(RefCell::new(Held::default()));
            while let Some(call) = arriving.next().await {
                cx.update(|app| take(app, &held, call));
            }
        });
    }
}

/// What the programs opened and started on Waft's cameras, by the app's key for each.
#[derive(Default)]
struct Held {
    cameras: HashMap<CameraKey, OpenCamera>,
    sessions: HashMap<CameraKey, Rc<dyn CaptureSession>>,
}

/// A camera Waft opened, with its formats as it listed them.
struct OpenCamera {
    device: Box<dyn CameraDevice>,
    formats: Vec<ui::CameraFormat>,
}

/// Carries out `call`, from the engine.
fn take(app: &mut App, held: &Rc<RefCell<Held>>, call: CameraCall) {
    match call {
        CameraCall::List(listed) => list(app, listed),
        CameraCall::Changed(changed) => tell_next_change(app, changed),
        CameraCall::Open { camera, id, opened } => open(app, held, camera, id, opened),
        CameraCall::Start {
            camera,
            session,
            format,
            outputs,
            reports,
            started,
        } => {
            let starting = Starting {
                session,
                reports,
                started,
            };
            start(app, held, camera, &format, outputs, starting);
        }
        CameraCall::TakePhoto { session, taken } => take_photo(app, &held.borrow(), session, taken),
        CameraCall::RemoveCamera(camera) => {
            held.borrow_mut().cameras.remove(&camera);
        }
        CameraCall::RemoveSession(session) => {
            held.borrow_mut().sessions.remove(&session);
        }
    }
}

/// Answers the cameras Waft lists into `listed`, once Waft has.
fn list(app: &mut App, listed: oneshot::Sender<Vec<CameraInfo>>) {
    let listing = waft_cameras(app.platform().as_ref());
    app.spawn(async move |_cx| {
        let cameras = listing.await;
        // A program that ended meanwhile hears nothing.
        let _ = listed.send(cameras.into_iter().map(program_camera).collect());
    });
}

/// The cameras Waft's `platform` lists, once it has; none where it has none.
fn waft_cameras(platform: &dyn Platform) -> ui::CameraListFuture {
    match platform.cameras() {
        Some(cameras) => cameras.cameras(),
        None => Box::pin(ready(Vec::new())),
    }
}

/// Tells `changed` once Waft's list may answer something else than now; never where Waft's
/// platform has no cameras.
fn tell_next_change(app: &mut App, changed: oneshot::Sender<()>) {
    let Some(next) = app.platform().cameras().map(|cameras| cameras.changed()) else {
        return;
    };
    app.spawn(async move |_cx| {
        next.await;
        // A program that ended meanwhile hears nothing.
        let _ = changed.send(());
    });
}

/// Opens the camera `id` on Waft's cameras as `camera`, answering its formats into `opened` once
/// Waft has, or why it did not open: `unplugged` for an id Waft no longer lists.
fn open(
    app: &mut App,
    held: &Rc<RefCell<Held>>,
    camera: CameraKey,
    id: String,
    opened: oneshot::Sender<Result<Vec<CameraFormat>, CameraError>>,
) {
    let platform = app.platform();
    let listing = waft_cameras(platform.as_ref());
    let held = Rc::downgrade(held);
    app.spawn(async move |_cx| {
        let listed = listing.await.into_iter().find(|listed| listed.id.0 == id);
        let opening = listed
            .zip(platform.cameras())
            .map(|(listed, cameras)| cameras.open(&listed));
        let Some(opening) = opening else {
            let _ = opened.send(Err(CameraError::Unplugged));
            return;
        };
        match opening.await {
            Ok(device) => keep_opened(&held, camera, device, opened),
            Err(error) => {
                let _ = opened.send(Err(program_error(error)));
            }
        }
    });
}

/// Keeps `device`, opened as `camera`, and answers its formats into `opened`; lets it go where the
/// engine no longer waits for it.
fn keep_opened(
    held: &Weak<RefCell<Held>>,
    camera: CameraKey,
    device: Box<dyn CameraDevice>,
    opened: oneshot::Sender<Result<Vec<CameraFormat>, CameraError>>,
) {
    let Some(held) = held.upgrade() else {
        return;
    };
    let formats = device.formats();
    let answer = formats.iter().map(program_format).collect();
    held.borrow_mut()
        .cameras
        .insert(camera, OpenCamera { device, formats });
    if opened.send(Ok(answer)).is_err() {
        held.borrow_mut().cameras.remove(&camera);
    }
}

/// A session being started: the app's key for it, where its state is reported, and where the
/// format it runs in, or why it did not start, is answered.
struct Starting {
    session: CameraKey,
    reports: CaptureSessionReports,
    started: oneshot::Sender<Result<CameraFormat, CameraError>>,
}

/// Starts `camera` in the Waft format `format` names, feeding `outputs`, and once Waft has, keeps
/// the session as `starting` says.
fn start(
    app: &mut App,
    held: &Rc<RefCell<Held>>,
    camera: CameraKey,
    format: &CameraFormat,
    outputs: SessionOutputs,
    starting: Starting,
) {
    let waft = match start_on(
        app.platform().as_ref(),
        &held.borrow(),
        camera,
        format,
        outputs,
    ) {
        Ok(waft) => waft,
        Err(error) => {
            let _ = starting.started.send(Err(error));
            return;
        }
    };
    let held = Rc::downgrade(held);
    app.spawn(async move |cx| {
        let answer = match waft.await {
            Ok(session) => cx.update(|app| keep_started(app, &held, session, &starting)),
            Err(error) => Err(program_error(error)),
        };
        if starting.started.send(answer).is_err()
            && let Some(held) = held.upgrade()
        {
            held.borrow_mut().sessions.remove(&starting.session);
        }
    });
}

/// Waft's start of `camera` in the Waft format `format` names, feeding `outputs`, on `platform`'s
/// readers; why not, where the camera is no longer open or does not list the format.
fn start_on(
    platform: &dyn Platform,
    held: &Held,
    camera: CameraKey,
    format: &CameraFormat,
    outputs: SessionOutputs,
) -> Result<ui::CameraStartFuture, CameraError> {
    let open = held
        .cameras
        .get(&camera)
        .ok_or_else(|| CameraError::Failed("the camera is no longer open".to_owned()))?;
    let format = waft_format(format);
    let format = open
        .formats
        .iter()
        .find(|listed| **listed == format)
        .ok_or_else(|| CameraError::Failed("the camera does not list the format".to_owned()))?;
    Ok(open.device.start(format, waft_outputs(platform, outputs)))
}

/// Keeps `session`, Waft's, as `starting` says, its state reported from now on; answers the format
/// it runs in.
fn keep_started(
    app: &mut App,
    held: &Weak<RefCell<Held>>,
    session: Box<dyn CaptureSession>,
    starting: &Starting,
) -> Result<CameraFormat, CameraError> {
    let held = held
        .upgrade()
        .ok_or_else(|| CameraError::Failed("WaOS is quitting".to_owned()))?;
    let session: Rc<dyn CaptureSession> = Rc::from(session);
    report_changes(app, &session, starting.reports.clone());
    let format = program_format(&session.format());
    held.borrow_mut().sessions.insert(starting.session, session);
    Ok(format)
}

/// `outputs` as Waft's camera is given them: the program's reader as Waft's, made on
/// `platform`'s readers, none where the platform has none or the reader is fed by WaOS's
/// simulated camera already.
fn waft_outputs(platform: &dyn Platform, outputs: SessionOutputs) -> ui::CameraOutputs {
    let frames = outputs
        .frames
        .zip(platform.video_frame_readers())
        .and_then(|(reader, readers)| reader.waft_reader(readers));
    ui::CameraOutputs {
        preview: outputs.preview,
        frames,
        photos: outputs.photos,
        movies: None,
        upright_frames: outputs.upright_frames,
    }
}

/// Reports `session`'s state and rotation angles into `reports` now, and again each time either
/// changes, until the session goes.
fn report_changes(app: &mut App, session: &Rc<dyn CaptureSession>, reports: CaptureSessionReports) {
    let changed = session.changed();
    let mut reported = session.state();
    reports.state_changed(program_state(reported.clone()));
    reports.video_rotation_angles_changed(program_angles(session.as_ref()));
    let session = Rc::downgrade(session);
    app.spawn(async move |_cx| {
        let mut changed = changed;
        loop {
            changed.await;
            let Some(session) = session.upgrade() else {
                return;
            };
            changed = session.changed();
            let now = session.state();
            if now != reported {
                reports.state_changed(program_state(now.clone()));
                reported = now;
            }
            reports.video_rotation_angles_changed(program_angles(session.as_ref()));
        }
    });
}

/// Takes a photo of `session`, answering it into `taken` once Waft has.
fn take_photo(
    app: &mut App,
    held: &Held,
    session: CameraKey,
    taken: oneshot::Sender<Result<waos::bindings::wasdk::media::cameras::Photo, CameraError>>,
) {
    let Some(session) = held.sessions.get(&session) else {
        let _ = taken.send(Err(CameraError::Failed("the session is gone".to_owned())));
        return;
    };
    let photo = session.take_photo();
    app.spawn(async move |_cx| {
        // A program that ended meanwhile hears nothing.
        let _ = taken.send(program_photo(photo.await));
    });
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use waft_test::{MockCameras, MockCompositor, MockVideoFrameReaders, TestPlatform};
    use waos::{Answer, ProgramEnd, Question, RecordingDevice};
    use waos_package::ProgramId;

    use super::*;
    use crate::state::test_system::{Lines, TestSystem};

    const CAMERAS: &str = "dev.example.cameras";

    fn front() -> ui::Camera {
        ui::Camera {
            id: ui::CameraId("front".to_owned()),
            name: "Front camera".to_owned(),
            facing: ui::CameraFacing::Front,
        }
    }

    fn desk() -> ui::Camera {
        ui::Camera {
            id: ui::CameraId("desk".to_owned()),
            name: "Desk View".to_owned(),
            facing: ui::CameraFacing::External,
        }
    }

    fn format(width: f64, height: f64) -> ui::CameraFormat {
        ui::CameraFormat {
            size: ui::Size::new(width, height),
            min_frame_rate: 15.0,
            max_frame_rate: 29.97,
            photo_sizes: vec![ui::Size::new(width, height)],
        }
    }

    /// `examples/cameras`, installed, its user having allowed it the camera, gives Waft's camera
    /// the layer and the reader it made, and hears the state Waft's session reports, and the list
    /// Waft's cameras tell again as one comes.
    #[test]
    fn a_programs_camera_is_wafts_on_its_layer_and_reader_and_its_state_reaches_the_program() {
        let cameras = MockCameras::new(vec![front()])
            .with_formats(vec![format(1920.0, 1080.0), format(640.0, 480.0)]);
        let compositor = MockCompositor::new();
        let readers = MockVideoFrameReaders::new();
        let platform = TestPlatform::new()
            .with_cameras(Rc::new(cameras.clone()))
            .with_compositor(Arc::new(compositor.clone()))
            .with_video_frame_readers(Rc::new(readers.clone()));
        let system = TestSystem::new(platform);
        system.install(waos_test_programs::CAMERAS, CAMERAS, "Cameras");
        let id: ProgramId = CAMERAS.parse().unwrap();
        system
            .store
            .grants(&id)
            .set_answer(Question::Camera, Answer::Granted);
        let lines = Arc::new(Lines::default());
        let running = system.run_as(waos_test_programs::CAMERAS, CAMERAS, &lines);

        system.run_tasks_until("the program starts the camera", || {
            lines.has("started in 640x480")
        });
        assert!(lines.has("camera front: Front camera (front)"));
        assert!(lines.has("format 1920x1080 at 15-29.97 fps, photos 1920x1080"));
        let session = &cameras.sessions()[0];
        assert_eq!(session.picked(), format(640.0, 480.0), "Waft's own format");
        let layer = session
            .preview_layer()
            .expect("a preview on the program's layer");
        assert!(Arc::ptr_eq(&layer, &compositor.made()[0].layer));
        let reader = session
            .frame_reader()
            .expect("frames to the program's reader");
        assert!(Arc::ptr_eq(&reader, &readers.made()[0]));
        assert_eq!(session.photo_size(), Some(ui::Size::new(640.0, 480.0)));
        system.run_tasks_until("the program hears the photo was not taken", || {
            lines.has("photo: failed (the test gave no photo)")
        });
        system.run_tasks_until("the program hears it runs", || lines.has("state: running"));
        assert!(lines.has("turned: preview 0, capture 0"));
        session.set_rotation_angles(90, 90);
        system.run_tasks_until("the program hears the turns change", || {
            lines.has("turned: preview 90, capture 90")
        });
        assert_eq!(
            system.recording_told(),
            [(CAMERAS.to_owned(), RecordingDevice::Camera, true)]
        );

        cameras.set_cameras(vec![front(), desk()]);
        system.run_tasks_until("the program hears a camera came", || {
            lines.has("cameras now: front, desk")
        });
        session.set_state(ui::CaptureState::Interrupted(Some(
            ui::CameraInterruption::Background,
        )));
        system.run_tasks_until("the program hears it interrupted", || {
            lines.has("state: interrupted (background)")
        });
        session.set_state(ui::CaptureState::Ended(ui::CameraError::Unplugged));
        system.run_tasks_until("the program hears it ended", || {
            lines.has("state: ended (unplugged)")
        });
        let end = futures::executor::block_on(running.ended());
        assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
        system.run_tasks_until("Waft's session is let go", || session.is_dropped());
        assert_eq!(
            system.recording_told(),
            [
                (CAMERAS.to_owned(), RecordingDevice::Camera, true),
                (CAMERAS.to_owned(), RecordingDevice::Camera, false)
            ]
        );
    }

    /// `examples/cameras`, installed, its user having answered the camera simulated, opens WaOS's
    /// test pattern in place of Waft's cameras, asked nothing, takes its photo through the app's
    /// image codecs, and is shown recording with a camera of WaOS's own.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_program_answered_simulated_uses_waoss_test_pattern_and_never_wafts_cameras() {
        use waft_test::{ImageAnswers, ImageFacts, MockImageCodecs};

        let cameras = MockCameras::new(vec![front()]).with_formats(vec![format(640.0, 480.0)]);
        let codecs = MockImageCodecs::new(ImageAnswers {
            decoded_formats: Vec::new(),
            encoded_formats: vec!["image/jpeg".to_owned()],
            opened: Ok(ImageFacts::still("image/jpeg", ui::Size::new(1.0, 1.0))),
            pixels: Ok([0, 0, 0, 255]),
            encoded: Ok(vec![0xff, 0xd8, 0xff, 0xd9]),
        });
        let platform = TestPlatform::new()
            .with_cameras(Rc::new(cameras.clone()))
            .with_compositor(Arc::new(MockCompositor::new()))
            .with_video_frame_readers(Rc::new(MockVideoFrameReaders::new()))
            .with_image_codecs(Rc::new(codecs.clone()));
        let system = TestSystem::new(platform);
        system.install(waos_test_programs::CAMERAS, CAMERAS, "Cameras");
        let id: ProgramId = CAMERAS.parse().unwrap();
        system
            .store
            .grants(&id)
            .set_answer(Question::Camera, Answer::Simulated);
        let lines = Arc::new(Lines::default());
        let running = system.run_as(waos_test_programs::CAMERAS, CAMERAS, &lines);

        system.run_tasks_until("the program takes its photo", || {
            lines.has("photo: image/jpeg, 4 bytes")
        });
        assert!(lines.has("camera waos-test-pattern: WaOS test pattern (external)"));
        assert!(lines.has("format 640x480 at 30-30 fps, photos 640x480"));
        assert!(lines.has("format 1280x720 at 30-30 fps, photos 1280x720"));
        assert!(
            lines.has("started in 1280x720"),
            "the last format, as the program picks"
        );
        assert!(cameras.sessions().is_empty(), "Waft's cameras untouched");
        assert!(
            codecs.asked().iter().any(|call| matches!(
                call,
                waft_test::CodecCall::Encode {
                    width: 1280,
                    height: 720,
                    ..
                }
            )),
            "the frame of the moment encoded: {:?}",
            codecs.asked()
        );
        assert_eq!(
            system.recording_told(),
            [(CAMERAS.to_owned(), RecordingDevice::SimulatedCamera, true)]
        );
        running.stop();
        futures::executor::block_on(running.ended());
    }

    #[test]
    fn a_platform_without_cameras_gives_the_engine_none() {
        assert!(Cameras::of(&TestPlatform::new()).is_none());
        let cameras = Rc::new(MockCameras::new(vec![front()]));
        assert!(Cameras::of(&TestPlatform::new().with_cameras(cameras)).is_some());
    }
}
