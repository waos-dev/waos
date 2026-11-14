//! The system's cameras as the test shell keeps them: two cameras listed, a front one and an
//! external one, until a test lists others, which tells whoever waits for a change; each running
//! in two formats; each opening recorded with the program that opened
//! it, and each session started with the format picked, its outputs, its layer and its reader by
//! the test's numbers for them, and where it reports; a test reports a session's state as the
//! system would. A session runs in the format picked, unless the test says the camera runs in
//! another for someone else, which it joins. Every photo is the first bytes of a JPEG.

use std::any::Any;
use std::future::ready;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use futures::FutureExt;
use futures::channel::oneshot;

use waos::bindings::wasdk::media::cameras::{
    CameraError, CameraFacing, CameraFormat, CameraInfo, CaptureState, Photo,
};
use waos::bindings::wasdk::platform::windows::Size;
use waos::{
    CameraChangeFuture, CameraListFuture, CameraOpenFuture, CameraOutputs, CaptureSessionReports,
    CaptureStartFuture, OpenedCamera, PhotoFuture, RunningProgram, ShellCamera, ShellCameras,
    ShellCaptureSession, StartedSession,
};

use super::composition::TestLayer;
use super::video_frames::number_of;

/// The bytes of every photo: a JPEG's start of image and the start of its next marker.
pub const PHOTO: [u8; 3] = [0xff, 0xd8, 0xff];

/// Cameras that keep each opening and each session started.
pub struct TestCameras {
    /// The cameras listed now.
    listed: Mutex<Vec<CameraInfo>>,
    /// Who waits for the list's next change.
    waiting: Mutex<Vec<oneshot::Sender<()>>>,
    opened: Mutex<Vec<(String, String)>>,
    sessions: Arc<Mutex<Vec<Started>>>,
    /// The format the cameras run in for someone else, which a session joins; none while they run
    /// for nobody else.
    elsewhere: Arc<Mutex<Option<CameraFormat>>>,
}

/// One session a program started, as the test sees it.
#[derive(Clone)]
pub struct Started {
    /// The format the program picked.
    pub picked: CameraFormat,
    /// The format it runs in: the one picked, or the one it joined.
    pub format: CameraFormat,
    /// The number of the test compositor's layer the preview shows on.
    pub preview: Option<usize>,
    /// The number of the test readers' reader the frames go to.
    pub frames: Option<usize>,
    /// The size photos are taken at.
    pub photos: Option<Size>,
    reports: CaptureSessionReports,
    dropped: Arc<AtomicBool>,
}

impl Default for TestCameras {
    fn default() -> TestCameras {
        TestCameras {
            listed: Mutex::new(listed()),
            waiting: Mutex::default(),
            opened: Mutex::default(),
            sessions: Arc::default(),
            elsewhere: Arc::default(),
        }
    }
}

impl TestCameras {
    /// The cameras of WaOS's own, as the shell gives them: one test pattern.
    pub fn simulated() -> TestCameras {
        TestCameras {
            listed: Mutex::new(vec![CameraInfo {
                id: "waos-test-pattern".to_owned(),
                name: "WaOS test pattern".to_owned(),
                facing: CameraFacing::External,
            }]),
            ..TestCameras::default()
        }
    }

    /// Lists `cameras` from now on, as the system would once a camera came or went, and tells
    /// whoever waits for a change.
    pub fn set_listed(&self, cameras: Vec<CameraInfo>) {
        *self.listed.lock().unwrap() = cameras;
        for waiting in self.waiting.lock().unwrap().drain(..) {
            // Who stopped waiting hears nothing.
            let _ = waiting.send(());
        }
    }

    /// Each camera opened so far, by the program that opened it and its id, in order.
    pub fn opened(&self) -> Vec<(String, String)> {
        self.opened.lock().unwrap().clone()
    }

    /// The sessions started so far, in order.
    pub fn started(&self) -> Vec<Started> {
        self.sessions.lock().unwrap().clone()
    }

    /// The cameras run in `format` for someone else from now on, which each session joins.
    pub fn run_elsewhere_in(&self, format: CameraFormat) {
        *self.elsewhere.lock().unwrap() = Some(format);
    }
}

impl Started {
    /// Reports `state`, as the system would have changed it.
    pub fn report(&self, state: CaptureState) {
        self.reports.state_changed(state);
    }

    /// Whether the engine let go of the session.
    pub fn is_dropped(&self) -> bool {
        self.dropped.load(Ordering::SeqCst)
    }
}

/// The cameras listed at first: the front one, then the external one.
pub fn listed() -> Vec<CameraInfo> {
    vec![
        CameraInfo {
            id: "front".to_owned(),
            name: "FaceTime HD Camera".to_owned(),
            facing: CameraFacing::Front,
        },
        CameraInfo {
            id: "desk".to_owned(),
            name: "Desk View".to_owned(),
            facing: CameraFacing::External,
        },
    ]
}

/// The formats every camera runs in: 1920 by 1080 with photos at two sizes, then 640 by 480.
pub fn formats() -> Vec<CameraFormat> {
    vec![
        format(
            size(1920.0, 1080.0),
            vec![size(1920.0, 1080.0), size(1280.0, 720.0)],
        ),
        format(size(640.0, 480.0), vec![size(640.0, 480.0)]),
    ]
}

fn format(size: Size, photo_sizes: Vec<Size>) -> CameraFormat {
    CameraFormat {
        size,
        min_frame_rate: 15.0,
        max_frame_rate: 30.0,
        photo_sizes,
    }
}

fn size(width: f64, height: f64) -> Size {
    Size { width, height }
}

impl ShellCameras for TestCameras {
    fn list(&self) -> CameraListFuture {
        Box::pin(ready(self.listed.lock().unwrap().clone()))
    }

    fn changed(&self) -> CameraChangeFuture {
        let (changed, heard) = oneshot::channel();
        self.waiting.lock().unwrap().push(changed);
        Box::pin(heard.map(|_heard| ()))
    }

    fn open(&self, program: &RunningProgram, id: &str) -> CameraOpenFuture {
        self.opened
            .lock()
            .unwrap()
            .push((program.name().to_owned(), id.to_owned()));
        if !self
            .listed
            .lock()
            .unwrap()
            .iter()
            .any(|camera| camera.id == id)
        {
            return Box::pin(ready(Err(CameraError::Unplugged)));
        }
        let camera = TestCamera {
            sessions: Arc::clone(&self.sessions),
            elsewhere: Arc::clone(&self.elsewhere),
        };
        Box::pin(ready(Ok(OpenedCamera {
            camera: Box::new(camera),
            formats: formats(),
        })))
    }
}

/// A camera a program opened.
struct TestCamera {
    sessions: Arc<Mutex<Vec<Started>>>,
    elsewhere: Arc<Mutex<Option<CameraFormat>>>,
}

impl ShellCamera for TestCamera {
    fn start(
        &self,
        format: CameraFormat,
        outputs: CameraOutputs,
        reports: CaptureSessionReports,
    ) -> CaptureStartFuture {
        let runs_in = self.elsewhere.lock().unwrap().clone();
        let started = Started {
            format: runs_in.unwrap_or_else(|| format.clone()),
            picked: format,
            preview: outputs.preview.map(layer_number),
            frames: outputs.frames.map(number_of),
            photos: outputs.photos,
            reports,
            dropped: Arc::default(),
        };
        let session = StartedSession {
            session: Box::new(TestSession {
                dropped: Arc::clone(&started.dropped),
            }),
            format: started.format.clone(),
        };
        self.sessions.lock().unwrap().push(started);
        Box::pin(ready(Ok(session)))
    }
}

/// The number of the test compositor's layer behind `layer`.
fn layer_number(layer: Arc<dyn waos::ShellCompositionLayer>) -> usize {
    let layer: Arc<dyn Any + Send + Sync> = layer;
    layer
        .downcast::<TestLayer>()
        .expect("a layer the test compositor made")
        .number
}

/// The session the engine holds.
struct TestSession {
    dropped: Arc<AtomicBool>,
}

impl ShellCaptureSession for TestSession {
    fn take_photo(&self) -> PhotoFuture {
        let photo = Photo {
            mime_type: "image/jpeg".to_owned(),
            data: PHOTO.to_vec(),
        };
        Box::pin(ready(Ok(photo)))
    }
}

impl Drop for TestSession {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}
