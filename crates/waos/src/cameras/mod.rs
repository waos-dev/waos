//! `wasdk:platform/cameras`: the system's cameras, run for a program. The shell lists and opens
//! them, and says when the list changes ([`ShellCameras`], each camera a `ShellCamera`, each
//! session a `ShellCaptureSession`); the engine keeps the rules, and reads the list again at each
//! change for each stream of its changes a program opens. A camera opens once the user allows it, the camera question put at the
//! first use through the program's permissions, and its opening is decided and held through the
//! program's access (`access`); a session starts in a format its
//! camera lists, its photos at a size that format lists, with its outputs as the program set them on a
//! [`CameraOutputs`], the shell's own layer and reader; and a photo is asked of a session started
//! with photos. A program whose camera answer is simulated lists and opens the shell's cameras of
//! WaOS's own, a test pattern, in place of the system's, read from its grant at each call, so the
//! shell's `list` names no program; with none, as on a system the shell draws no pattern on, it
//! lists no camera. Each session's state, as the shell reports it, its being shown by the shell's
//! indicator of who records, and its keeping the program from freezing, are `session`'s. The interface is served to a program by `host`.
//!
//! The invariant: no shell's camera is opened for a program whose user has not allowed it the
//! camera, and no shell's session is started in a format, or with photos at a size, its camera does
//! not list: those are the program's mistakes, which trap.

pub(crate) mod host;
mod session;

use std::future::Future;
use std::sync::Arc;

use futures::StreamExt;
use futures::future::{self, FutureExt};
use futures::stream::{self, BoxStream};
use waos_capabilities::{Answer, ProgramGrants, Question};
use wasmtime::bail;

use crate::access::{Asked, ProgramAccess, Refusal, Resolution, Resolved};
use crate::bindings::wasdk::media::cameras::{CameraError, CameraFormat, CameraInfo};
use crate::bindings::wasdk::platform::permissions::State;
use crate::bindings::wasdk::platform::windows::Size;
use crate::lifecycle::Lifecycle;
use crate::permissions::{Permission, state_of};
use crate::running::AppInstance;
use crate::shell::{
    CaptureStartFuture, OpenedCamera, RecordingDevice, ShellCamera, ShellCameras,
    ShellCompositionLayer, ShellVideoFrameReader,
};
use session::SessionState;
pub use session::{CaptureSession, CaptureSessionReports};

/// A program's cameras: the shell's, where they may be used, and what its sessions keep from
/// freezing.
pub(crate) struct ProgramCameras {
    /// The shell's cameras; none where the shell has none, or cannot show a program using one.
    shells: Option<CameraShells>,
    /// Where the program's camera answer is read, which allows a camera and picks the system's or
    /// WaOS's own, and each session held, which the shell's indicator shows.
    access: ProgramAccess,
    lifecycle: Lifecycle,
}

/// The shell's two kinds of camera: the system's, and WaOS's own, simulated.
#[derive(Clone)]
pub(crate) struct CameraShells {
    pub(crate) system: Option<Arc<dyn ShellCameras>>,
    pub(crate) simulated: Option<Arc<dyn ShellCameras>>,
}

impl ProgramCameras {
    /// The cameras `shells` gives, for a program whose camera answer `access` reads and whose
    /// `lifecycle` its sessions keep from freezing; none where the shell has neither kind, or
    /// cannot show that the program records (`recordings_shown`).
    pub(crate) fn new(
        shells: CameraShells,
        access: ProgramAccess,
        recordings_shown: bool,
        lifecycle: Lifecycle,
    ) -> ProgramCameras {
        let any = shells.system.is_some() || shells.simulated.is_some();
        ProgramCameras {
            shells: (any && recordings_shown).then_some(shells),
            access,
            lifecycle,
        }
    }
}

/// `cameras.cameras`: the shell's cameras, as the program reaches them: the system's, or WaOS's
/// own while the program's camera answer is simulated.
pub struct Cameras {
    shells: CameraShells,
    access: ProgramAccess,
}

/// Which of the shell's kinds of camera a program uses now, and that kind's cameras, if the shell
/// has them.
struct InUse {
    simulated: bool,
    shell: Option<Arc<dyn ShellCameras>>,
}

impl Cameras {
    /// The kind the program's camera answer picks now.
    fn in_use(&self) -> InUse {
        in_use(&self.shells, self.access.grants())
    }

    /// The cameras of the kind in use; none where the shell has none of it.
    fn list(&self) -> impl Future<Output = Vec<CameraInfo>> + Send + 'static + use<> {
        match self.in_use().shell {
            Some(shell) => shell.list().left_future(),
            None => future::ready(Vec::new()).right_future(),
        }
    }

    /// The camera `id`, opened by the shell for `program` once `asked`, the camera question, is
    /// answered, if the answer allows it, of the kind the answer picks then; `not-allowed`, the
    /// shell asked nothing, otherwise, and `unplugged` for a simulated camera the shell has none
    /// of.
    fn open<Asked: Future<Output = ()> + Send + 'static>(
        &self,
        program: &AppInstance,
        asked: Asked,
        id: String,
    ) -> impl Future<Output = Result<Camera, CameraError>> + Send + 'static + use<Asked> {
        let (shells, access, program) = (self.shells.clone(), self.access.clone(), program.clone());
        async move {
            asked.await;
            let InUse { simulated, shell } = decide_open(&access, &shells, &id)?;
            let shell = shell.ok_or(CameraError::Unplugged)?;
            let opened = shell.open(&program, &id).await?;
            Ok(Camera::of(opened, simulated))
        }
    }

    /// The list again each time the shell says it changed, or the program's answer changes which
    /// kind it uses, from now on, on a stream of its own: the next change is asked for before the
    /// list is read, so none goes unheard, and one read late hears the list as it is then, passing
    /// over those between.
    fn changes(&self) -> BoxStream<'static, Vec<CameraInfo>> {
        let (shells, access) = (self.shells.clone(), self.access.clone());
        let changed = next_change(&shells, access.grants());
        stream::unfold(
            (shells, access, changed),
            |(shells, access, changed)| async move {
                changed.await;
                let next = next_change(&shells, access.grants());
                let listed = match in_use(&shells, access.grants()).shell {
                    Some(shell) => shell.list().await,
                    None => Vec::new(),
                };
                Some((listed, (shells, access, next)))
            },
        )
        .boxed()
    }
}

/// The kind of camera the program `access` is of opens `id` among, if its camera answer allows
/// it a camera now; `not-allowed` otherwise.
fn decide_open(
    access: &ProgramAccess,
    shells: &CameraShells,
    id: &str,
) -> Result<InUse, CameraError> {
    let used = Asked::OpenCamera {
        id,
        simulated: in_use(shells, access.grants()).simulated,
    };
    let allowed = |grants: &dyn ProgramGrants| {
        if state_of(grants, Permission::Question(Question::Camera)) == State::Granted {
            Ok(in_use(shells, grants))
        } else {
            Err(Refusal::by("camera"))
        }
    };
    access
        .decide(used, allowed)
        .map_err(|_refusal| CameraError::NotAllowed)
}

impl Resolved for InUse {
    fn resolution(&self) -> Resolution<'_> {
        Resolution::Nothing
    }
}

/// The kind of camera `grants`' camera answer picks among `shells`.
fn in_use(shells: &CameraShells, grants: &dyn ProgramGrants) -> InUse {
    let simulated = grants.answer(Question::Camera) == Some(Answer::Simulated);
    let shell = if simulated {
        shells.simulated.clone()
    } else {
        shells.system.clone()
    };
    InUse { simulated, shell }
}

/// Resolves at the next change of the cameras of the kind in use, or of the grants.
fn next_change(
    shells: &CameraShells,
    grants: &dyn ProgramGrants,
) -> impl Future<Output = ()> + Send + 'static + use<> {
    let listed: futures::future::BoxFuture<'static, ()> = match in_use(shells, grants).shell {
        Some(shell) => shell.changed(),
        None => future::pending().boxed(),
    };
    future::select(listed, grants.changed()).map(drop)
}

/// `cameras.camera`: a camera the shell opened for the program, with its formats, and whether it
/// is one of WaOS's own.
pub struct Camera {
    shell: Box<dyn ShellCamera>,
    formats: Vec<CameraFormat>,
    simulated: bool,
}

impl Camera {
    fn of(opened: OpenedCamera, simulated: bool) -> Camera {
        Camera {
            shell: opened.camera,
            formats: opened.formats,
            simulated,
        }
    }

    /// Starts the shell's camera in `format`, feeding `outputs`: answers the session's state, which
    /// the shell reports into from its start on, with the start; a trap for a format the camera
    /// does not list, or photos at a size the format does not.
    fn start(
        &self,
        format: CameraFormat,
        outputs: &CameraOutputs,
    ) -> wasmtime::Result<(SessionState, CaptureStartFuture)> {
        if !self
            .formats
            .iter()
            .any(|listed| same_format(listed, &format))
        {
            bail!("the program started a camera in a format the camera does not list");
        }
        if let Some(size) = &outputs.photos
            && !format
                .photo_sizes
                .iter()
                .any(|listed| same_size(listed, size))
        {
            bail!(
                "the program asked for photos at {}x{}, a size its format does not list",
                size.width,
                size.height
            );
        }
        let state = SessionState::new(outputs.photos.is_some());
        let started = self.shell.start(format, outputs.clone(), state.reports());
        Ok((state, started))
    }

    fn formats(&self) -> Vec<CameraFormat> {
        self.formats.clone()
    }

    /// What a session of this camera records with, as the shell's indicator shows it.
    fn device(&self) -> RecordingDevice {
        if self.simulated {
            RecordingDevice::SimulatedCamera
        } else {
            RecordingDevice::Camera
        }
    }
}

/// `cameras.camera-outputs`: what a camera feeds, as the program set it, given whole to the shell's
/// camera as it starts: the shell's own layer and reader, which its compositor and its readers
/// made. An output left unset is not fed.
#[derive(Clone, Default)]
pub struct CameraOutputs {
    /// The layer the system draws the preview on.
    pub preview: Option<Arc<dyn ShellCompositionLayer>>,
    /// The reader the frames go to, for the program to draw.
    pub frames: Option<Arc<dyn ShellVideoFrameReader>>,
    /// The size photos are taken at, one of the format's photo sizes.
    pub photos: Option<Size>,
    /// Whether the frames are to come turned and mirrored as they show, which a system that cannot
    /// answers `not-supported` to.
    pub upright_frames: bool,
}

/// Whether `a` and `b` are the same format, field for field: a format the program names is one it
/// read from the camera.
fn same_format(a: &CameraFormat, b: &CameraFormat) -> bool {
    same_size(&a.size, &b.size)
        && a.min_frame_rate == b.min_frame_rate
        && a.max_frame_rate == b.max_frame_rate
        && a.photo_sizes.len() == b.photo_sizes.len()
        && a.photo_sizes
            .iter()
            .zip(&b.photo_sizes)
            .all(|(a, b)| same_size(a, b))
}

fn same_size(a: &Size, b: &Size) -> bool {
    a.width == b.width && a.height == b.height
}

#[cfg(test)]
mod tests {
    use std::future::ready;
    use std::sync::Mutex;

    use super::*;
    use crate::access::{Answers, Told};

    /// A camera which keeps each format it is started in and starts nothing.
    #[derive(Default)]
    struct Starting(Arc<Mutex<Vec<CameraFormat>>>);

    impl ShellCamera for Starting {
        fn start(
            &self,
            format: CameraFormat,
            _outputs: CameraOutputs,
            _reports: CaptureSessionReports,
        ) -> CaptureStartFuture {
            self.0.lock().unwrap().push(format);
            Box::pin(ready(Err(CameraError::InUse)))
        }
    }

    fn vga() -> CameraFormat {
        CameraFormat {
            size: size(640.0, 480.0),
            min_frame_rate: 15.0,
            max_frame_rate: 30.0,
            photo_sizes: vec![size(640.0, 480.0)],
        }
    }

    fn size(width: f64, height: f64) -> Size {
        Size { width, height }
    }

    fn photos_at(size: Size) -> CameraOutputs {
        CameraOutputs {
            photos: Some(size),
            ..CameraOutputs::default()
        }
    }

    #[test]
    fn a_listed_format_with_a_listed_photo_size_reaches_the_shell_and_nothing_else_does() {
        let started = Arc::default();
        let camera = Camera {
            shell: Box::new(Starting(Arc::clone(&started))),
            formats: vec![vga()],
            simulated: false,
        };
        assert!(camera.start(vga(), &photos_at(size(640.0, 480.0))).is_ok());

        let unlisted = CameraFormat {
            max_frame_rate: 60.0,
            ..vga()
        };
        let error = camera.start(unlisted, &CameraOutputs::default()).err();
        assert!(format!("{error:?}").contains("a format the camera does not list"));
        let error = camera.start(vga(), &photos_at(size(1280.0, 720.0))).err();
        assert!(format!("{error:?}").contains("photos at 1280x720"));
        assert_eq!(
            started.lock().unwrap().len(),
            1,
            "the mistakes reach no shell"
        );
    }

    #[test]
    fn a_camera_its_answer_does_not_allow_is_told_refused_by_the_camera_question() {
        let answers = Arc::new(Answers::default());
        let access = answers.access();
        let shells = CameraShells {
            system: None,
            simulated: None,
        };
        let not_asked = decide_open(&access, &shells, "front");
        assert!(matches!(not_asked, Err(CameraError::NotAllowed)));
        answers.set_answer(Question::Camera, Answer::Denied);
        let denied = decide_open(&access, &shells, "front");
        assert!(matches!(denied, Err(CameraError::NotAllowed)));
        assert_eq!(access.refusals(), ["camera", "camera"]);

        answers.set_answer(Question::Camera, Answer::Simulated);
        let in_use = decide_open(&access, &shells, "front").unwrap();
        assert!(in_use.simulated);
        assert!(matches!(access.told().last(), Some(Told::Allowed(..))));
    }
}
