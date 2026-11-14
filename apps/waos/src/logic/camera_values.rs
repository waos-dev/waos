//! A camera's values in the program's terms, from Waft's, field for field: a camera listed, a
//! format, a session's state and rotation angles, why a camera did not open, start or run, and a
//! photo, or why a photo
//! was not taken, which the program hears as a camera's error; and a format and a photo's size the
//! program names, in Waft's.
//!
//! The invariant: a format goes over and back whole, every field the same type on both sides, so
//! a format the program names is one of Waft's as Waft compares them.

use waft::ui;
use waos::bindings::wasdk::media::cameras::{
    CameraError, CameraFacing, CameraFormat, CameraInfo, CameraInterruption, CaptureState, Photo,
    VideoRotationAngles,
};
use waos::bindings::wasdk::platform::windows::Size;

/// Waft's `camera`, as the program reads it.
pub(crate) fn program_camera(camera: ui::Camera) -> CameraInfo {
    CameraInfo {
        id: camera.id.0,
        name: camera.name,
        facing: match camera.facing {
            ui::CameraFacing::Front => CameraFacing::Front,
            ui::CameraFacing::Back => CameraFacing::Back,
            ui::CameraFacing::External => CameraFacing::External,
        },
    }
}

/// Waft's `format`, as the program reads it.
pub(crate) fn program_format(format: &ui::CameraFormat) -> CameraFormat {
    CameraFormat {
        size: program_size(format.size),
        min_frame_rate: format.min_frame_rate,
        max_frame_rate: format.max_frame_rate,
        photo_sizes: format
            .photo_sizes
            .iter()
            .copied()
            .map(program_size)
            .collect(),
    }
}

/// A format the program names, in Waft's terms.
pub(crate) fn waft_format(format: &CameraFormat) -> ui::CameraFormat {
    ui::CameraFormat {
        size: waft_size(format.size),
        min_frame_rate: format.min_frame_rate,
        max_frame_rate: format.max_frame_rate,
        photo_sizes: format.photo_sizes.iter().copied().map(waft_size).collect(),
    }
}

/// Waft's `state` of a session, as the program reads it.
pub(crate) fn program_state(state: ui::CaptureState) -> CaptureState {
    match state {
        ui::CaptureState::Running => CaptureState::Running,
        ui::CaptureState::Interrupted(why) => {
            CaptureState::Interrupted(why.map(program_interruption))
        }
        ui::CaptureState::Ended(why) => CaptureState::Ended(program_error(why)),
    }
}

fn program_interruption(interruption: ui::CameraInterruption) -> CameraInterruption {
    match interruption {
        ui::CameraInterruption::InUseByAnotherApp => CameraInterruption::InUseByAnotherApp,
        ui::CameraInterruption::Background => CameraInterruption::Background,
        ui::CameraInterruption::SystemPressure => CameraInterruption::SystemPressure,
    }
}

/// Why Waft's camera did not open, start or run, as the program reads it.
pub(crate) fn program_error(error: ui::CameraError) -> CameraError {
    match error {
        ui::CameraError::NotAllowed => CameraError::NotAllowed,
        ui::CameraError::InUse => CameraError::InUse,
        ui::CameraError::Unplugged => CameraError::Unplugged,
        ui::CameraError::NotSupported(reason) => CameraError::NotSupported(reason),
        ui::CameraError::Failed(reason) => CameraError::Failed(reason),
    }
}

/// `session`'s rotation angles, as the program reads them: Waft's two methods, one record.
pub(crate) fn program_angles(session: &dyn ui::CaptureSession) -> VideoRotationAngles {
    VideoRotationAngles {
        for_horizon_level_preview: session.video_rotation_angle_for_horizon_level_preview(),
        for_horizon_level_capture: session.video_rotation_angle_for_horizon_level_capture(),
    }
}

/// Waft's photo, or why it was not taken, as the program reads it: a session that does not run
/// fails with Waft's words for it, since the program's errors have no case of their own for it.
pub(crate) fn program_photo(
    photo: Result<ui::Photo, ui::MediaError>,
) -> Result<Photo, CameraError> {
    match photo {
        Ok(photo) => Ok(Photo {
            mime_type: photo.mime_type,
            data: photo.bytes,
        }),
        Err(ui::MediaError::Failed(reason)) => Err(CameraError::Failed(reason)),
        Err(not_running @ ui::MediaError::NotRunning) => {
            Err(CameraError::Failed(not_running.to_string()))
        }
    }
}

/// A photo's size the program named, in Waft's terms.
pub(crate) fn waft_size(size: Size) -> ui::Size {
    ui::Size::new(size.width, size.height)
}

fn program_size(size: ui::Size) -> Size {
    Size {
        width: size.width(),
        height: size.height(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A format at the frame rates a camera reports in NTSC's timing.
    fn ntsc() -> ui::CameraFormat {
        ui::CameraFormat {
            size: ui::Size::new(1920.0, 1080.0),
            min_frame_rate: 29.97,
            max_frame_rate: 29.97,
            photo_sizes: vec![ui::Size::new(1920.0, 1080.0), ui::Size::new(640.0, 480.0)],
        }
    }

    #[test]
    fn wafts_camera_and_format_are_the_programs_and_a_format_comes_back_whole() {
        let camera = program_camera(ui::Camera {
            id: ui::CameraId("0x1420000005ac8600".to_owned()),
            name: "FaceTime HD Camera".to_owned(),
            facing: ui::CameraFacing::Front,
        });
        assert_eq!(camera.id, "0x1420000005ac8600");
        assert_eq!(camera.name, "FaceTime HD Camera");
        assert_eq!(camera.facing, CameraFacing::Front);

        let format = program_format(&ntsc());
        assert_eq!((format.size.width, format.size.height), (1920.0, 1080.0));
        assert_eq!(format.max_frame_rate, 29.97);
        assert_eq!(format.photo_sizes.len(), 2);
        assert_eq!(waft_format(&format), ntsc());
    }

    #[test]
    fn a_sessions_state_and_errors_are_the_programs() {
        assert!(matches!(
            program_state(ui::CaptureState::Interrupted(Some(
                ui::CameraInterruption::SystemPressure
            ))),
            CaptureState::Interrupted(Some(CameraInterruption::SystemPressure))
        ));
        assert!(matches!(
            program_state(ui::CaptureState::Interrupted(None)),
            CaptureState::Interrupted(None)
        ));
        assert!(matches!(
            program_state(ui::CaptureState::Ended(ui::CameraError::Failed(
                "gone".to_owned()
            ))),
            CaptureState::Ended(CameraError::Failed(reason)) if reason == "gone"
        ));
        assert!(matches!(
            program_error(ui::CameraError::NotAllowed),
            CameraError::NotAllowed
        ));
    }

    #[test]
    fn a_photo_is_the_programs_and_a_session_not_running_fails_with_why() {
        let photo = program_photo(Ok(ui::Photo {
            bytes: vec![0xff, 0xd8],
            mime_type: "image/jpeg".to_owned(),
        }))
        .unwrap();
        assert_eq!(
            (photo.mime_type.as_str(), photo.data),
            ("image/jpeg", vec![0xff, 0xd8])
        );
        assert!(matches!(
            program_photo(Err(ui::MediaError::NotRunning)),
            Err(CameraError::Failed(reason)) if reason == "the camera session does not run"
        ));
    }
}
