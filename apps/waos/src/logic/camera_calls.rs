//! Where the programs' cameras go: each listing, wait for the list's next change, opening, start
//! and photo to the main thread, where Waft's cameras are (`state::cameras`), and its answer back,
//! a camera's formats with its opening and a session's format with its start. The engine calls them from a program's thread; a
//! session's state comes back through the reports the engine handed with its start.
//!
//! The invariant: the calls reach the main thread in the order made, a camera's or a session's
//! removal sent as the engine lets go of it, or of the future that would have answered it, so the
//! main thread holds nothing the engine no longer does; and a call that cannot reach it, as WaOS
//! quits, is answered at once, so no program waits for a camera that cannot come, but for a wait
//! for the list's next change, which no change then ends.

use std::future::pending;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use futures::channel::oneshot;
use waft::ui::{self, CompositionLayer};
use waos::bindings::wasdk::media::cameras::{CameraError, CameraFormat, CameraInfo, Photo};
use waos::{
    CameraChangeFuture, CameraListFuture, CameraOpenFuture, CameraOutputs, CaptureSessionReports,
    CaptureStartFuture, OpenedCamera, PhotoFuture, RunningProgram, ShellCamera, ShellCameras,
    ShellCaptureSession, StartedSession,
};
use waos_program_window::waft_layer;

use super::camera_values::waft_size;
use super::frame_readers::ReaderOnRequest;

/// The id the app gives each camera and each session a program opens or starts, which its calls
/// carry.
pub(crate) type CameraKey = u64;

/// One call of the programs' cameras, on its way to the main thread.
pub(crate) enum CameraCall {
    /// The cameras attached now go to `listed`.
    List(oneshot::Sender<Vec<CameraInfo>>),
    /// `changed` hears the next change of the cameras attached.
    Changed(oneshot::Sender<()>),
    /// A program opens the camera `id` as `camera`; its formats, or why it did not open, go to
    /// `opened`.
    Open {
        camera: CameraKey,
        id: String,
        opened: oneshot::Sender<Result<Vec<CameraFormat>, CameraError>>,
    },
    /// A program starts `camera` in `format` as `session`, feeding `outputs`, its state into
    /// `reports`; the format it runs in, or why it did not start, goes to `started`.
    Start {
        camera: CameraKey,
        session: CameraKey,
        format: CameraFormat,
        outputs: SessionOutputs,
        reports: CaptureSessionReports,
        started: oneshot::Sender<Result<CameraFormat, CameraError>>,
    },
    /// A photo of `session` goes to `taken`.
    TakePhoto {
        session: CameraKey,
        taken: oneshot::Sender<Result<Photo, CameraError>>,
    },
    /// The program let one of its cameras go.
    RemoveCamera(CameraKey),
    /// The program let one of its sessions go.
    RemoveSession(CameraKey),
}

/// What a session feeds, as the main thread gives it to Waft's camera.
pub(crate) struct SessionOutputs {
    /// Waft's layer behind the program's.
    pub(crate) preview: Option<Arc<dyn CompositionLayer>>,
    /// The program's reader, Waft's once a camera or a player is given it.
    pub(crate) frames: Option<Arc<ReaderOnRequest>>,
    pub(crate) photos: Option<ui::Size>,
    /// Whether the frames are to come turned and mirrored as they show.
    pub(crate) upright_frames: bool,
}

/// Where the calls go, with the keys they carry.
#[derive(Clone)]
struct Calls {
    sender: UnboundedSender<CameraCall>,
    last_key: Arc<AtomicU64>,
}

impl Calls {
    fn send(&self, call: CameraCall) {
        if self.sender.unbounded_send(call).is_err() {
            tracing::info!("WaOS is quitting: a camera's call goes nowhere");
        }
    }

    fn next_key(&self) -> CameraKey {
        self.last_key.fetch_add(1, Ordering::Relaxed) + 1
    }
}

/// The app's cameras, as the engine reaches them: every call goes to the main thread.
pub(crate) struct CamerasOnRequest(Calls);

impl CamerasOnRequest {
    /// The cameras, and where their calls arrive on the main thread.
    pub(crate) fn new() -> (CamerasOnRequest, UnboundedReceiver<CameraCall>) {
        let (sender, arriving) = mpsc::unbounded();
        let calls = Calls {
            sender,
            last_key: Arc::default(),
        };
        (CamerasOnRequest(calls), arriving)
    }
}

impl ShellCameras for CamerasOnRequest {
    /// None once WaOS quits.
    fn list(&self) -> CameraListFuture {
        let (listed, answer) = oneshot::channel();
        self.0.send(CameraCall::List(listed));
        Box::pin(async move { answer.await.unwrap_or_default() })
    }

    /// Never once WaOS quits, when no change can be heard.
    fn changed(&self) -> CameraChangeFuture {
        let (changed, heard) = oneshot::channel();
        self.0.send(CameraCall::Changed(changed));
        Box::pin(async move {
            if heard.await.is_err() {
                pending::<()>().await;
            }
        })
    }

    fn open(&self, program: &RunningProgram, id: &str) -> CameraOpenFuture {
        tracing::info!(program = program.name(), id, "a program opens a camera");
        let camera = self.0.next_key();
        let held = Held::new(camera, CameraCall::RemoveCamera, self.0.clone());
        let (opened, answer) = oneshot::channel();
        self.0.send(CameraCall::Open {
            camera,
            id: id.to_owned(),
            opened,
        });
        Box::pin(async move {
            let formats = answer.await.unwrap_or_else(|_| Err(quitting()))?;
            let camera = Box::new(CameraOnRequest(held));
            Ok(OpenedCamera { camera, formats })
        })
    }
}

/// One camera, as the engine holds it.
struct CameraOnRequest(Held);

impl ShellCamera for CameraOnRequest {
    fn start(
        &self,
        format: CameraFormat,
        outputs: CameraOutputs,
        reports: CaptureSessionReports,
    ) -> CaptureStartFuture {
        let calls = &self.0.calls;
        let session = calls.next_key();
        let held = Held::new(session, CameraCall::RemoveSession, calls.clone());
        let (started, answer) = oneshot::channel();
        calls.send(CameraCall::Start {
            camera: self.0.key,
            session,
            format,
            outputs: session_outputs(outputs),
            reports,
            started,
        });
        Box::pin(async move {
            let format = answer.await.unwrap_or_else(|_| Err(quitting()))?;
            let session = Box::new(SessionOnRequest(held));
            Ok(StartedSession { session, format })
        })
    }
}

/// One session, as the engine holds it.
struct SessionOnRequest(Held);

impl ShellCaptureSession for SessionOnRequest {
    fn take_photo(&self) -> PhotoFuture {
        let (taken, answer) = oneshot::channel();
        self.0.calls.send(CameraCall::TakePhoto {
            session: self.0.key,
            taken,
        });
        Box::pin(async move { answer.await.unwrap_or_else(|_| Err(quitting())) })
    }
}

/// A camera or a session the main thread holds under `key`, removed there as this is dropped.
struct Held {
    key: CameraKey,
    removal: fn(CameraKey) -> CameraCall,
    calls: Calls,
}

impl Held {
    fn new(key: CameraKey, removal: fn(CameraKey) -> CameraCall, calls: Calls) -> Held {
        Held {
            key,
            removal,
            calls,
        }
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        self.calls.send((self.removal)(self.key));
    }
}

/// What a call answers once the main thread no longer takes calls.
fn quitting() -> CameraError {
    CameraError::Failed("WaOS is quitting".to_owned())
}

/// `outputs` as the main thread gives them to Waft's camera: Waft's layer behind the program's,
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
