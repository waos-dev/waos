//! The programs' cameras, over Waft's: the engine lists, opens, starts and takes photos from a
//! program's thread; each call is posted to the App's thread (`app_thread_bound`), where it is
//! carried out on Waft's cameras and answered once Waft has: the list, its next change as it
//! comes, a camera named by its id among those Waft lists opened and kept with its formats, a
//! session started on one in the Waft format the program named, its preview on Waft's layer behind
//! the program's, its frames in the program's reader, Waft's from the first time a camera or a
//! player is given it, and its photos at the size named. Each camera and session is kept until the
//! program lets it go, its photos taken as the program asks and its state reported to the program,
//! through the reports the engine handed with its start, as it starts and at each change after, as
//! Waft's own `CameraSession` follows the host's. The cameras are the engine's shell's where Waft's
//! platform has them.
//!
//! The invariant: a camera's or a session's removal is posted as the engine lets go of it, or as
//! its opening's or start's answer is dropped unheard, so the App's thread holds nothing the
//! engine no longer does; and every call that asks for an answer is answered once Waft has, or at
//! once as WaOS quits, so no program waits for a camera that cannot come, but for a wait for the
//! list's next change, which no change then ends.

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::{Future, pending, ready};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use futures::FutureExt;
use waft::ui::{self, CameraDevice, CaptureSession, CompositionLayer, Platform, PlatformRef};
use waft::{App, AsyncApp};
use waos::bindings::wasdk::media::cameras::{CameraError, CameraFormat, CameraInfo, Photo};
use waos::{
    CameraChangeFuture, CameraListFuture, CameraOpenFuture, CameraOutputs, CaptureSessionReports,
    CaptureStartFuture, OpenedCamera, PhotoFuture, RunningProgram, ShellCamera, ShellCameras,
    ShellCaptureSession, StartedSession,
};
use waos_program_window::waft_layer;

use super::app_thread_bound::{AppThreadBound, AppThreadCalls};
use crate::logic::camera_values::{
    program_angles, program_camera, program_error, program_format, program_photo, program_state,
    waft_format, waft_size,
};
use crate::logic::frame_readers::ReaderOnRequest;

/// The id the app gives each camera and each session a program opens or starts, which its calls
/// carry.
type CameraKey = u64;

/// Waft's cameras, served to the engine as its shell's.
pub(crate) struct Cameras {
    shell: Arc<CamerasOnRequest>,
    calls: AppThreadCalls<OpenCameras>,
}

impl Cameras {
    /// Waft's cameras on `platform`, to serve to the engine; none where it has none.
    pub(crate) fn of(platform: &dyn Platform) -> Option<Cameras> {
        platform.cameras()?;
        let (cameras, calls) = AppThreadBound::new();
        let shell = CamerasOnRequest {
            cameras,
            last_key: Arc::default(),
        };
        Some(Cameras {
            shell: Arc::new(shell),
            calls,
        })
    }

    /// The cameras as the engine's shell has them: each call is posted to the App's thread, where
    /// [`serve`](Self::serve) runs it.
    pub(crate) fn shell(&self) -> Arc<dyn ShellCameras> {
        Arc::clone(&self.shell) as Arc<dyn ShellCameras>
    }

    /// From now on, each camera a program opens is one of Waft's, and does as the program asks.
    pub(crate) fn serve(self, app: &mut App) {
        let cameras = OpenCameras {
            platform: app.platform(),
            app: app.to_async(),
            held: Rc::default(),
        };
        self.calls.serve(app, cameras);
    }
}

/// The app's cameras, as the engine reaches them: every call is posted to the App's thread, with
/// the keys it carries.
#[derive(Clone)]
struct CamerasOnRequest {
    cameras: AppThreadBound<OpenCameras>,
    last_key: Arc<AtomicU64>,
}

impl CamerasOnRequest {
    fn next_key(&self) -> CameraKey {
        self.last_key.fetch_add(1, Ordering::Relaxed) + 1
    }
}

impl ShellCameras for CamerasOnRequest {
    /// None once WaOS quits.
    fn list(&self) -> CameraListFuture {
        let listed = self.cameras.ask(|cameras| cameras.list());
        Box::pin(listed.map(Result::unwrap_or_default))
    }

    /// Never once WaOS quits, when no change can be heard.
    fn changed(&self) -> CameraChangeFuture {
        let heard = self.cameras.ask(|cameras| cameras.changed());
        Box::pin(async move {
            if heard.await.is_err() {
                pending::<()>().await;
            }
        })
    }

    fn open(&self, program: &RunningProgram, id: &str) -> CameraOpenFuture {
        tracing::info!(program = program.name(), id, "a program opens a camera");
        let camera = self.next_key();
        let id = id.to_owned();
        let handle = self.clone();
        let opened = self
            .cameras
            .ask(move |cameras| cameras.open(camera, id, handle));
        Box::pin(opened.map(|opened| opened.unwrap_or_else(|_| Err(quitting()))))
    }
}

/// One camera, as the engine holds it: its starts are posted to the App's thread, and its removal
/// as it drops.
struct CameraOnRequest {
    camera: CameraKey,
    cameras: CamerasOnRequest,
}

impl ShellCamera for CameraOnRequest {
    fn start(
        &self,
        format: CameraFormat,
        outputs: CameraOutputs,
        reports: CaptureSessionReports,
    ) -> CaptureStartFuture {
        let starting = Starting {
            camera: self.camera,
            session: self.cameras.next_key(),
            format,
            outputs: session_outputs(outputs),
            reports,
        };
        let handle = self.cameras.cameras.clone();
        let started = self
            .cameras
            .cameras
            .ask(move |cameras| cameras.start(starting, handle));
        Box::pin(started.map(|started| started.unwrap_or_else(|_| Err(quitting()))))
    }
}

impl Drop for CameraOnRequest {
    fn drop(&mut self) {
        let camera = self.camera;
        let _ = self.cameras.cameras.post(move |cameras| {
            cameras.held.borrow_mut().cameras.remove(&camera);
        });
    }
}

/// One session, as the engine holds it: its photos are posted to the App's thread, and its
/// removal as it drops.
struct SessionOnRequest {
    session: CameraKey,
    cameras: AppThreadBound<OpenCameras>,
}

impl ShellCaptureSession for SessionOnRequest {
    fn take_photo(&self) -> PhotoFuture {
        let session = self.session;
        let taken = self.cameras.ask(move |cameras| cameras.take_photo(session));
        Box::pin(taken.map(|taken| taken.unwrap_or_else(|_| Err(quitting()))))
    }
}

impl Drop for SessionOnRequest {
    fn drop(&mut self) {
        let session = self.session;
        let _ = self.cameras.post(move |cameras| {
            cameras.held.borrow_mut().sessions.remove(&session);
        });
    }
}

/// What a session feeds, as the App's thread gives it to Waft's camera.
struct SessionOutputs {
    /// Waft's layer behind the program's.
    preview: Option<Arc<dyn CompositionLayer>>,
    /// The program's reader, Waft's once a camera or a player is given it.
    frames: Option<Arc<ReaderOnRequest>>,
    photos: Option<ui::Size>,
    /// Whether the frames are to come turned and mirrored as they show.
    upright_frames: bool,
}

/// A session to start: on which camera, under which key, in which format, feeding what, and where
/// its state is reported.
struct Starting {
    camera: CameraKey,
    session: CameraKey,
    format: CameraFormat,
    outputs: SessionOutputs,
    reports: CaptureSessionReports,
}

/// Waft's cameras on the App's thread, with what the programs opened and started on them; shared
/// with the openings and starts, which keep a camera or a session once Waft has it.
struct OpenCameras {
    platform: PlatformRef,
    app: AsyncApp,
    held: Rc<RefCell<Held>>,
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

impl OpenCameras {
    /// The cameras Waft lists, once it has.
    fn list(&self) -> impl Future<Output = Vec<CameraInfo>> + use<> {
        let listing = waft_cameras(self.platform.as_ref());
        listing.map(|cameras| cameras.into_iter().map(program_camera).collect())
    }

    /// Resolves once Waft's list may answer something else than now; never where Waft's platform
    /// has no cameras.
    fn changed(&self) -> impl Future<Output = ()> + use<> {
        let next = self.platform.cameras().map(|cameras| cameras.changed());
        async move {
            match next {
                Some(next) => next.await,
                None => pending().await,
            }
        }
    }

    /// Opens the camera `id` on Waft's cameras, kept under `camera` once it opened, and answers it
    /// as the engine holds it, through `cameras`, with its formats; `unplugged` for an id Waft no
    /// longer lists. Dropped unheard, the answer lets the camera go.
    fn open(
        &self,
        camera: CameraKey,
        id: String,
        cameras: CamerasOnRequest,
    ) -> impl Future<Output = Result<OpenedCamera, CameraError>> + use<> {
        let platform = Rc::clone(&self.platform);
        let held = Rc::clone(&self.held);
        async move {
            let listed = waft_cameras(platform.as_ref()).await;
            let listed = listed.into_iter().find(|listed| listed.id.0 == id);
            let opening = listed
                .zip(platform.cameras())
                .map(|(listed, waft)| waft.open(&listed))
                .ok_or(CameraError::Unplugged)?;
            let device = opening.await.map_err(program_error)?;
            let formats = device.formats();
            let answer = formats.iter().map(program_format).collect();
            held.borrow_mut()
                .cameras
                .insert(camera, OpenCamera { device, formats });
            Ok(OpenedCamera {
                camera: Box::new(CameraOnRequest { camera, cameras }),
                formats: answer,
            })
        }
    }

    /// Starts the session `starting` names, kept once Waft has started it, its state reported from
    /// then on, and answers it as the engine holds it, through `cameras`, with the format it runs
    /// in; why not, where the camera is no longer open or does not list the format. Dropped
    /// unheard, the answer lets the session go.
    fn start(
        &self,
        starting: Starting,
        cameras: AppThreadBound<OpenCameras>,
    ) -> impl Future<Output = Result<StartedSession, CameraError>> + use<> {
        let Starting {
            camera,
            session,
            format,
            outputs,
            reports,
        } = starting;
        let waft = start_on(
            self.platform.as_ref(),
            &self.held.borrow(),
            camera,
            &format,
            outputs,
        );
        let held = Rc::clone(&self.held);
        let app = self.app.clone();
        async move {
            let started: Rc<dyn CaptureSession> = Rc::from(waft?.await.map_err(program_error)?);
            app.update(|app| report_changes(app, &started, reports));
            let format = program_format(&started.format());
            held.borrow_mut().sessions.insert(session, started);
            Ok(StartedSession {
                session: Box::new(SessionOnRequest { session, cameras }),
                format,
            })
        }
    }

    /// A photo of `session`, once Waft has taken it.
    fn take_photo(
        &self,
        session: CameraKey,
    ) -> impl Future<Output = Result<Photo, CameraError>> + use<> {
        let taking = self
            .held
            .borrow()
            .sessions
            .get(&session)
            .map(|session| session.take_photo());
        async move {
            match taking {
                Some(taking) => program_photo(taking.await),
                None => Err(CameraError::Failed("the session is gone".to_owned())),
            }
        }
    }
}

/// The cameras Waft's `platform` lists, once it has; none where it has none.
fn waft_cameras(platform: &dyn Platform) -> ui::CameraListFuture {
    match platform.cameras() {
        Some(cameras) => cameras.cameras(),
        None => Box::pin(ready(Vec::new())),
    }
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

/// `outputs` as the App's thread gives them to Waft's camera: Waft's layer behind the program's,
/// and the program's reader; none, said so, for a layer Waft's compositor did not make or a reader
/// the app's readers did not, which would show nothing.
fn session_outputs(outputs: CameraOutputs) -> SessionOutputs {
    let preview = outputs.preview.and_then(|layer| {
        let waft = waft_layer(layer);
        if waft.is_none() {
            tracing::warn!("a camera was given a layer the app did not make; it shows nowhere");
        }
        waft
    });
    let frames = outputs.frames.and_then(|reader| {
        let reader: Arc<dyn std::any::Any + Send + Sync> = reader;
        let reader = reader.downcast().ok();
        if reader.is_none() {
            tracing::warn!("a camera was given a reader the app did not make; it reads nothing");
        }
        reader
    });
    SessionOutputs {
        preview,
        frames,
        photos: outputs.photos.map(waft_size),
        upright_frames: outputs.upright_frames,
    }
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

/// What a call answers once the App's thread no longer takes calls.
fn quitting() -> CameraError {
    CameraError::Failed("WaOS is quitting".to_owned())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use waft_test::{MockCameras, MockCompositor, MockVideoFrameReaders, TestPlatform};
    use waos::{Answer, ProgramEnd, Question, RecordingDevice};
    use waos_package::ProgramId;
    use waos_permissions::Setting;

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
        let answered = system.permissions.change_app_settings(&id, |own| {
            own.answers
                .set(Question::Camera, Setting::Own(Answer::Granted));
        });
        answered.expect("the answer kept");
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
        assert!(std::ptr::addr_eq(
            Arc::as_ptr(&layer),
            Arc::as_ptr(&compositor.made()[0])
        ));
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
        let answered = system.permissions.change_app_settings(&id, |own| {
            own.answers
                .set(Question::Camera, Setting::Own(Answer::Simulated));
        });
        answered.expect("the answer kept");
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
