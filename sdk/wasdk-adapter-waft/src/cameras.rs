//! Waft's cameras over `wasdk:platform/cameras`, where the system has them: the cameras listed are
//! the WIT's, listed when the system answers, and `changed` resolves at the WIT's next list on
//! `changes`; a camera opens by its id, which asks the user at the first use. A camera's formats
//! and a session's are the WIT's, field for field. A session starts, once the system answers, with
//! its preview on the WIT's layer behind a composition layer this program's compositor made
//! (`composition`) and its frames in the WIT's reader behind a reader this program's readers made
//! (`video_frames`), each left out for any other, as Waft says of an output this host did not
//! make, and its photos at the size named. Recording is not offered to programs yet, so a session
//! starts without its movies, and a recording fails. A session's state is the WIT's, an
//! interruption's reason where the system gives one; a photo is the WIT's, and a camera's error on
//! the way is a failure with Waft's words for it.
//!
//! The invariant: where Waft answers a failure and the WIT would trap the program, as a mistake of
//! its own, the failure is answered here before the WIT is asked: a format the camera does not
//! list, photos at a size the format does not list, and a photo of a session without photos. A
//! `changed` future resolves at the first change after it was asked for, of the cameras' list or a
//! session's state, or once the session is gone, since the stream it reads is opened as it is
//! asked for.

use std::future::ready;
use std::path::PathBuf;
use std::rc::Rc;

use waft_embedder::{
    Camera, CameraDevice, CameraError, CameraFacing, CameraFormat, CameraId, CameraInterruption,
    CameraListFuture, CameraOpenFuture, CameraOutputs, CameraStartFuture, Cameras, CaptureSession,
    CaptureState, MediaChangeFuture, MediaError, Photo, PhotoFuture, RecordingFuture, Size,
};
use wasdk::cameras as platform;
use wasdk::windows;

use crate::composition::wit_layer;
use crate::video_frames::wit_reader;

/// The system's cameras.
pub(crate) struct WasdkCameras(Rc<platform::Cameras>);

impl WasdkCameras {
    /// The system's cameras, where it has them.
    pub(crate) fn get() -> Option<WasdkCameras> {
        platform::get_cameras().map(|cameras| WasdkCameras(Rc::new(cameras)))
    }
}

impl Cameras for WasdkCameras {
    fn cameras(&self) -> CameraListFuture {
        let cameras = Rc::clone(&self.0);
        Box::pin(async move {
            let listed = cameras.list().await;
            listed.into_iter().map(framework_camera).collect()
        })
    }

    /// The first list the WIT's `changes` says after this was called.
    fn changed(&self) -> MediaChangeFuture {
        let mut changes = self.0.changes();
        Box::pin(async move {
            changes.next().await;
        })
    }

    /// Asks the user at the first use, through the program's camera permission.
    fn open(&self, camera: &Camera) -> CameraOpenFuture {
        let (cameras, id) = (Rc::clone(&self.0), camera.id.0.clone());
        Box::pin(async move {
            let opened = cameras.open(id).await.map_err(framework_error)?;
            Ok(Box::new(WasdkCamera(Rc::new(opened))) as Box<dyn CameraDevice>)
        })
    }
}

/// A camera the system opened for the program.
struct WasdkCamera(Rc<platform::Camera>);

impl CameraDevice for WasdkCamera {
    fn formats(&self) -> Vec<CameraFormat> {
        self.0.formats().iter().map(framework_format).collect()
    }

    fn start(&self, format: &CameraFormat, outputs: CameraOutputs) -> CameraStartFuture {
        let format = wit_format(format);
        if let Err(error) = check_listed(&self.0.formats(), &format, outputs.photos) {
            return Box::pin(ready(Err(error)));
        }
        let camera = Rc::clone(&self.0);
        Box::pin(async move {
            let wit = wit_outputs(&outputs);
            let session = camera.start(format, &wit).await.map_err(framework_error)?;
            Ok(Box::new(WasdkSession {
                session: Rc::new(session),
                takes_photos: outputs.photos.is_some(),
            }) as Box<dyn CaptureSession>)
        })
    }
}

/// Fails as Waft does for `format` where `formats` does not list it, and for photos at `photos`
/// where `format` does not list that size, which the WIT would trap the program for.
fn check_listed(
    formats: &[platform::CameraFormat],
    format: &platform::CameraFormat,
    photos: Option<Size>,
) -> Result<(), CameraError> {
    if !formats.iter().any(|listed| same_format(listed, format)) {
        return Err(CameraError::Failed(
            "the camera does not list this format".to_owned(),
        ));
    }
    let listed_size = |size: Size| {
        format
            .photo_sizes
            .iter()
            .any(|listed| framework_size(*listed) == size)
    };
    match photos {
        Some(size) if !listed_size(size) => Err(CameraError::Failed(format!(
            "the format does not list photos at {}x{}",
            size.width(),
            size.height()
        ))),
        _ => Ok(()),
    }
}

/// A camera running for the program.
struct WasdkSession {
    session: Rc<platform::CaptureSession>,
    /// Whether photos are among its outputs, and so whether a photo is asked of the WIT.
    takes_photos: bool,
}

impl CaptureSession for WasdkSession {
    fn format(&self) -> CameraFormat {
        framework_format(&self.session.format())
    }

    fn state(&self) -> CaptureState {
        framework_state(self.session.state())
    }

    /// The WIT gives both angles in one record.
    fn video_rotation_angle_for_horizon_level_preview(&self) -> u32 {
        self.session
            .video_rotation_angles()
            .for_horizon_level_preview
    }

    fn video_rotation_angle_for_horizon_level_capture(&self) -> u32 {
        self.session
            .video_rotation_angles()
            .for_horizon_level_capture
    }

    /// Resolves at the next change of the state or of the angles, each a stream of the WIT's.
    fn changed(&self) -> MediaChangeFuture {
        let mut states = self.session.state_changes();
        let mut angles = self.session.video_rotation_angle_changes();
        Box::pin(async move {
            // A change, or the session gone: the state or the angles may answer otherwise.
            let state = std::pin::pin!(states.next());
            let turn = std::pin::pin!(angles.next());
            futures::future::select(state, turn).await;
        })
    }

    fn take_photo(&self) -> PhotoFuture {
        if !self.takes_photos {
            let reason = "the session has no photos: they were not among its outputs";
            return Box::pin(ready(Err(MediaError::Failed(reason.to_owned()))));
        }
        let session = Rc::clone(&self.session);
        Box::pin(async move { framework_photo(session.take_photo().await) })
    }

    /// Fails: recording is not offered to programs yet.
    fn start_recording(&self, _path: PathBuf) -> RecordingFuture {
        let reason = "a program cannot record from a camera yet";
        Box::pin(ready(Err(MediaError::Failed(reason.to_owned()))))
    }

    /// Nothing records.
    fn stop_recording(&self) {}
}

/// `outputs` as the WIT's: the preview on the WIT's layer and the frames in the WIT's reader, each
/// where this program made them, upright where asked, and the photos' size; the movies left out.
fn wit_outputs(outputs: &CameraOutputs) -> platform::CameraOutputs {
    let wit = platform::CameraOutputs::new();
    if let Some(layer) = outputs.preview.as_ref().and_then(wit_layer) {
        wit.set_preview(layer);
    }
    if let Some(reader) = outputs.frames.as_ref().and_then(wit_reader) {
        wit.set_frames(reader);
    }
    if let Some(size) = outputs.photos {
        wit.set_photos(wit_size(size));
    }
    if outputs.upright_frames {
        wit.set_upright_frames();
    }
    wit
}

/// The WIT's camera, in Waft's terms.
fn framework_camera(camera: platform::CameraInfo) -> Camera {
    Camera {
        id: CameraId(camera.id),
        name: camera.name,
        facing: match camera.facing {
            platform::CameraFacing::Front => CameraFacing::Front,
            platform::CameraFacing::Back => CameraFacing::Back,
            platform::CameraFacing::External => CameraFacing::External,
        },
    }
}

/// The WIT's format, in Waft's terms.
fn framework_format(format: &platform::CameraFormat) -> CameraFormat {
    CameraFormat {
        size: framework_size(format.size),
        min_frame_rate: format.min_frame_rate,
        max_frame_rate: format.max_frame_rate,
        photo_sizes: format
            .photo_sizes
            .iter()
            .copied()
            .map(framework_size)
            .collect(),
    }
}

/// Waft's format, in the WIT's terms.
fn wit_format(format: &CameraFormat) -> platform::CameraFormat {
    platform::CameraFormat {
        size: wit_size(format.size),
        min_frame_rate: format.min_frame_rate,
        max_frame_rate: format.max_frame_rate,
        photo_sizes: format.photo_sizes.iter().copied().map(wit_size).collect(),
    }
}

fn same_format(a: &platform::CameraFormat, b: &platform::CameraFormat) -> bool {
    framework_format(a) == framework_format(b)
}

fn framework_size(size: windows::Size) -> Size {
    Size::new(size.width, size.height)
}

fn wit_size(size: Size) -> windows::Size {
    windows::Size {
        width: size.width(),
        height: size.height(),
    }
}

/// The WIT's state of a session, in Waft's terms.
fn framework_state(state: platform::CaptureState) -> CaptureState {
    match state {
        platform::CaptureState::Running => CaptureState::Running,
        platform::CaptureState::Interrupted(why) => {
            CaptureState::Interrupted(why.map(framework_interruption))
        }
        platform::CaptureState::Ended(why) => CaptureState::Ended(framework_error(why)),
    }
}

fn framework_interruption(interruption: platform::CameraInterruption) -> CameraInterruption {
    match interruption {
        platform::CameraInterruption::InUseByAnotherApp => CameraInterruption::InUseByAnotherApp,
        platform::CameraInterruption::Background => CameraInterruption::Background,
        platform::CameraInterruption::SystemPressure => CameraInterruption::SystemPressure,
    }
}

/// Why the WIT's camera did not open, start or run, in Waft's terms.
fn framework_error(error: platform::CameraError) -> CameraError {
    match error {
        platform::CameraError::NotAllowed => CameraError::NotAllowed,
        platform::CameraError::InUse => CameraError::InUse,
        platform::CameraError::Unplugged => CameraError::Unplugged,
        platform::CameraError::NotSupported(reason) => CameraError::NotSupported(reason),
        platform::CameraError::Failed(reason) => CameraError::Failed(reason),
    }
}

/// The WIT's photo, or why it was not taken, in Waft's terms, whose photos fail with a reason
/// alone.
fn framework_photo(
    photo: Result<platform::Photo, platform::CameraError>,
) -> Result<Photo, MediaError> {
    match photo {
        Ok(photo) => Ok(Photo {
            bytes: photo.data,
            mime_type: photo.mime_type,
        }),
        Err(platform::CameraError::Failed(reason)) => Err(MediaError::Failed(reason)),
        Err(error) => Err(MediaError::Failed(framework_error(error).to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wit_format_at(rate: f64, photo_sizes: Vec<windows::Size>) -> platform::CameraFormat {
        platform::CameraFormat {
            size: windows::Size {
                width: 1920.0,
                height: 1080.0,
            },
            min_frame_rate: 1.0,
            max_frame_rate: rate,
            photo_sizes,
        }
    }

    fn hd() -> windows::Size {
        windows::Size {
            width: 1920.0,
            height: 1080.0,
        }
    }

    #[test]
    fn the_wits_camera_is_wafts() {
        let camera = framework_camera(platform::CameraInfo {
            id: "desk".to_owned(),
            name: "Desk View".to_owned(),
            facing: platform::CameraFacing::External,
        });
        assert_eq!(
            camera,
            Camera {
                id: CameraId("desk".to_owned()),
                name: "Desk View".to_owned(),
                facing: CameraFacing::External,
            }
        );
    }

    #[test]
    fn a_format_read_from_the_wit_goes_back_whole_and_is_found_among_the_cameras() {
        let wit = wit_format_at(29.97, vec![hd()]);
        let waft = framework_format(&wit);
        assert_eq!(waft.size, Size::new(1920.0, 1080.0));
        assert_eq!(waft.max_frame_rate, 29.97);
        let back = wit_format(&waft);
        assert_eq!(back.max_frame_rate, 29.97);
        assert!(same_format(&back, &wit));
        assert!(
            check_listed(
                std::slice::from_ref(&wit),
                &back,
                Some(Size::new(1920.0, 1080.0))
            )
            .is_ok()
        );
    }

    #[test]
    fn what_the_wit_would_trap_for_fails_as_waft_says() {
        let listed = wit_format_at(30.0, vec![hd()]);
        let unlisted = wit_format_at(60.0, vec![hd()]);
        assert!(matches!(
            check_listed(std::slice::from_ref(&listed), &unlisted, None),
            Err(CameraError::Failed(reason)) if reason == "the camera does not list this format"
        ));
        assert!(matches!(
            check_listed(std::slice::from_ref(&listed), &listed, Some(Size::new(640.0, 480.0))),
            Err(CameraError::Failed(reason)) if reason == "the format does not list photos at 640x480"
        ));
    }

    #[test]
    fn the_wits_state_errors_and_photos_are_wafts() {
        assert_eq!(
            framework_state(platform::CaptureState::Interrupted(Some(
                platform::CameraInterruption::InUseByAnotherApp
            ))),
            CaptureState::Interrupted(Some(CameraInterruption::InUseByAnotherApp))
        );
        assert_eq!(
            framework_state(platform::CaptureState::Interrupted(None)),
            CaptureState::Interrupted(None),
            "none where the system gives no reason"
        );
        assert_eq!(
            framework_state(platform::CaptureState::Ended(
                platform::CameraError::Unplugged
            )),
            CaptureState::Ended(CameraError::Unplugged)
        );
        assert_eq!(
            framework_photo(Ok(platform::Photo {
                mime_type: "image/jpeg".to_owned(),
                data: vec![0xff, 0xd8],
            })),
            Ok(Photo {
                bytes: vec![0xff, 0xd8],
                mime_type: "image/jpeg".to_owned(),
            })
        );
        assert_eq!(
            framework_photo(Err(platform::CameraError::InUse)),
            Err(MediaError::Failed("another app has the camera".to_owned()))
        );
        assert_eq!(
            framework_photo(Err(platform::CameraError::Failed("busy".to_owned()))),
            Err(MediaError::Failed("busy".to_owned()))
        );
    }
}
