//! The code serving `wasdk:platform/cameras` to a program: the shell's cameras, where they may be
//! used, listed with the list's changes and opened behind the camera question, a camera's formats,
//! its outputs as the program sets them, with the shell's own layer and reader, a session started
//! on them, and its format, its state with its changes and its photos.

use std::sync::Arc;

use waos_capabilities::Question;
use wasmtime::component::{
    Access, Accessor, HasData, Linker, Resource, ResourceTable, StreamReader,
};

use super::{Camera, CameraOutputs, Cameras, CaptureSession, ProgramCameras};
use crate::access::ActiveUseKind;
use crate::bindings::wasdk::media::cameras::{
    self, CameraError, CameraFormat, CameraInfo, CaptureState, HostCamera, HostCameraOutputs,
    HostCameraWithStore, HostCameras, HostCamerasWithStore, HostCaptureSession,
    HostCaptureSessionWithStore, Photo, VideoRotationAngles,
};
use crate::bindings::wasdk::platform::windows::Size;
use crate::lifecycle::FreezeInhibitorKind;
use crate::permissions::ProgramPermissions;
use crate::running::AppInstance;
use crate::stream::stream_of;
use crate::video_frames::VideoFrameReader;
use crate::views::host::CompositionLayer;

/// What the cameras interface reads and writes in a program's store.
pub(crate) struct CamerasCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    pub(crate) program: &'a AppInstance,
    pub(crate) cameras: &'a ProgramCameras,
    /// What the user lets the program use, which asks the camera question.
    pub(crate) permissions: &'a ProgramPermissions,
}

pub(crate) trait CamerasCtxView: Send {
    fn cameras_ctx(&mut self) -> CamerasCtx<'_>;
}

struct HasCameras;

impl HasData for HasCameras {
    type Data<'a> = CamerasCtx<'a>;
}

pub(crate) fn add_to_linker<T: CamerasCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    cameras::add_to_linker::<_, HasCameras>(linker, T::cameras_ctx)
}

impl cameras::Host for CamerasCtx<'_> {
    fn get_cameras(&mut self) -> wasmtime::Result<Option<Resource<Cameras>>> {
        let cameras = self.cameras.shells.clone().map(|shells| Cameras {
            shells,
            access: self.cameras.access.clone(),
        });
        Ok(cameras
            .map(|cameras| self.table.push(cameras))
            .transpose()?)
    }
}

impl HostCameras for CamerasCtx<'_> {
    fn drop(&mut self, cameras: Resource<Cameras>) -> wasmtime::Result<()> {
        self.table.delete(cameras)?;
        Ok(())
    }
}

impl<T: CamerasCtxView + 'static> HostCamerasWithStore<T> for HasCameras {
    async fn list(
        accessor: &Accessor<T, Self>,
        cameras: Resource<Cameras>,
    ) -> wasmtime::Result<Vec<CameraInfo>> {
        let listed = accessor.with(|mut access| {
            wasmtime::Result::<_>::Ok(access.get().table.get(&cameras)?.list())
        })?;
        Ok(listed.await)
    }

    /// Each list enters the program's stream as it reads it.
    fn changes(
        mut access: Access<T, Self>,
        cameras: Resource<Cameras>,
    ) -> wasmtime::Result<StreamReader<Vec<CameraInfo>>> {
        let changes = access.get().table.get(&cameras)?.changes();
        stream_of(access, changes, |_: &mut T, listed| Ok(listed))
    }

    /// Asks the user at the first use, through the program's camera permission.
    async fn open(
        accessor: &Accessor<T, Self>,
        cameras: Resource<Cameras>,
        id: String,
    ) -> wasmtime::Result<Result<Resource<Camera>, CameraError>> {
        let opening = accessor.with(|mut access| {
            let context = access.get();
            let asked = context.permissions.answered(Question::Camera);
            let cameras = context.table.get(&cameras)?;
            wasmtime::Result::<_>::Ok(cameras.open(context.program, asked, id))
        })?;
        match opening.await {
            Ok(camera) => accessor.with(|mut access| Ok(Ok(access.get().table.push(camera)?))),
            Err(error) => Ok(Err(error)),
        }
    }
}

impl HostCamera for CamerasCtx<'_> {
    fn formats(&mut self, camera: Resource<Camera>) -> wasmtime::Result<Vec<CameraFormat>> {
        Ok(self.table.get(&camera)?.formats())
    }

    /// The program lets go of the camera; a session started on it runs on.
    fn drop(&mut self, camera: Resource<Camera>) -> wasmtime::Result<()> {
        self.table.delete(camera)?;
        Ok(())
    }
}

impl<T: CamerasCtxView + 'static> HostCameraWithStore<T> for HasCameras {
    /// Shown by the shell's indicator, and keeping the program from freezing, from the start on.
    async fn start(
        accessor: &Accessor<T, Self>,
        camera: Resource<Camera>,
        format: CameraFormat,
        outputs: Resource<CameraOutputs>,
    ) -> wasmtime::Result<Result<Resource<CaptureSession>, CameraError>> {
        let (state, started, device, cameras) = accessor.with(|mut access| {
            let context = access.get();
            let outputs = context.table.get(&outputs)?;
            let camera = context.table.get(&camera)?;
            let (state, started) = camera.start(format, outputs)?;
            let cameras = context.cameras;
            wasmtime::Result::<_>::Ok((
                state,
                started,
                camera.device(),
                (cameras.access.clone(), cameras.lifecycle.clone()),
            ))
        })?;
        match started.await {
            Ok(started) => {
                let (access, lifecycle) = cameras;
                let recording = access.hold(ActiveUseKind::Recording(device));
                let session = CaptureSession::new(
                    started,
                    state,
                    recording,
                    lifecycle.inhibit_freeze(FreezeInhibitorKind::CameraCapture),
                );
                accessor.with(|mut access| Ok(Ok(access.get().table.push(session)?)))
            }
            Err(error) => Ok(Err(error)),
        }
    }
}

impl HostCameraOutputs for CamerasCtx<'_> {
    fn new(&mut self) -> wasmtime::Result<Resource<CameraOutputs>> {
        Ok(self.table.push(CameraOutputs::default())?)
    }

    /// Kept as the layer the shell's compositor made.
    fn set_preview(
        &mut self,
        outputs: Resource<CameraOutputs>,
        layer: Resource<CompositionLayer>,
    ) -> wasmtime::Result<()> {
        let layer = Arc::clone(self.table.get(&layer)?.shell_layer());
        self.table.get_mut(&outputs)?.preview = Some(layer);
        Ok(())
    }

    /// Kept as the reader the shell's readers made.
    fn set_frames(
        &mut self,
        outputs: Resource<CameraOutputs>,
        reader: Resource<VideoFrameReader>,
    ) -> wasmtime::Result<()> {
        let reader = Arc::clone(self.table.get(&reader)?.shell_reader());
        self.table.get_mut(&outputs)?.frames = Some(reader);
        Ok(())
    }

    fn set_photos(&mut self, outputs: Resource<CameraOutputs>, size: Size) -> wasmtime::Result<()> {
        self.table.get_mut(&outputs)?.photos = Some(size);
        Ok(())
    }

    fn set_upright_frames(&mut self, outputs: Resource<CameraOutputs>) -> wasmtime::Result<()> {
        self.table.get_mut(&outputs)?.upright_frames = true;
        Ok(())
    }

    fn drop(&mut self, outputs: Resource<CameraOutputs>) -> wasmtime::Result<()> {
        self.table.delete(outputs)?;
        Ok(())
    }
}

impl HostCaptureSession for CamerasCtx<'_> {
    fn format(&mut self, session: Resource<CaptureSession>) -> wasmtime::Result<CameraFormat> {
        Ok(self.table.get(&session)?.format())
    }

    fn state(&mut self, session: Resource<CaptureSession>) -> wasmtime::Result<CaptureState> {
        Ok(self.table.get(&session)?.state())
    }

    fn video_rotation_angles(
        &mut self,
        session: Resource<CaptureSession>,
    ) -> wasmtime::Result<VideoRotationAngles> {
        Ok(self.table.get(&session)?.video_rotation_angles())
    }

    /// The program lets go of the session: the camera stops for it.
    fn drop(&mut self, session: Resource<CaptureSession>) -> wasmtime::Result<()> {
        self.table.delete(session)?;
        Ok(())
    }
}

impl<T: CamerasCtxView + 'static> HostCaptureSessionWithStore<T> for HasCameras {
    /// Each state enters the program's stream as it reads it.
    fn state_changes(
        mut access: Access<T, Self>,
        session: Resource<CaptureSession>,
    ) -> wasmtime::Result<StreamReader<CaptureState>> {
        let changes = access.get().table.get(&session)?.state_changes();
        stream_of(access, changes, |_: &mut T, state| Ok(state))
    }

    /// Each pair of angles enters the program's stream as it reads it.
    fn video_rotation_angle_changes(
        mut access: Access<T, Self>,
        session: Resource<CaptureSession>,
    ) -> wasmtime::Result<StreamReader<VideoRotationAngles>> {
        let changes = access
            .get()
            .table
            .get(&session)?
            .video_rotation_angle_changes();
        stream_of(access, changes, |_: &mut T, angles| Ok(angles))
    }

    async fn take_photo(
        accessor: &Accessor<T, Self>,
        session: Resource<CaptureSession>,
    ) -> wasmtime::Result<Result<Photo, CameraError>> {
        let taken = accessor.with(|mut access| access.get().table.get(&session)?.take_photo())?;
        Ok(taken.await)
    }
}
