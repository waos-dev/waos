//! WaOS's simulated camera, which a program whose camera answer is simulated lists and opens in
//! place of the system's (the engine's `Shell::simulated_cameras`): one camera, "WaOS test
//! pattern", in two formats, whose session draws the test pattern (`test_pattern`) on a thread of
//! its own at the format's frame rate into frames as the system hands a camera's out (`os`): a
//! `CVPixelBuffer` on macOS, an RGBA `AHardwareBuffer` on Android; shows each
//! on the preview's layer, feeds each to the program's reader (`frame_readers`' WaOS source), and
//! encodes the frame of the moment as a JPEG through the app's image codecs for a photo. No device
//! is touched, so the system asks nothing.
//!
//! Built on macOS and Android, where the engine takes frames (`waos::VideoFrame`); elsewhere the app
//! gives no simulated cameras, and the settings page says so (`SIMULATED_CAMERA` in each system's shell, `state::shells`).
//!
//! The invariant: a session's thread stops once the engine lets go of the session, and a reader
//! WaOS feeds is never Waft's, so its frames are this camera's alone.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use futures::future::{self, pending};
use waos::bindings::wasdk::media::cameras::{
    CameraError, CameraFacing, CameraFormat, CameraInfo, CaptureState, Photo,
};
use waos::bindings::wasdk::platform::hardware_buffers::{AlphaType, PixelBuffer, PixelFormat};
use waos::bindings::wasdk::platform::windows::Size;
use waos::{
    AppInstance, CameraChangeFuture, CameraListFuture, CameraOpenFuture, CameraOutputs,
    CaptureSessionReports, CaptureStartFuture, OpenedCamera, PhotoFuture, ShellCamera,
    ShellCameras, ShellCaptureSession, ShellCompositionLayer, ShellImageCodecs, StartedSession,
};

use super::frame_readers::{FedFrames, ReaderOnRequest};
use super::test_pattern;
use crate::os::{CrossThreadFrame, pattern_frame, surface_of};

/// The camera's id and its name.
pub(crate) const ID: &str = "waos-test-pattern";
pub(crate) const NAME: &str = "WaOS test pattern";

/// The rate every format runs at.
const FRAME_RATE: f64 = 30.0;

/// The encoding of a photo.
const PHOTO_TYPE: &str = "image/jpeg";

/// The simulated cameras, as the engine reaches them.
pub(crate) struct SimulatedCameras {
    /// Where a photo is encoded; none where the app has no image codecs.
    codecs: Option<Arc<dyn ShellImageCodecs>>,
}

impl SimulatedCameras {
    pub(crate) fn new(codecs: Option<Arc<dyn ShellImageCodecs>>) -> SimulatedCameras {
        SimulatedCameras { codecs }
    }
}

/// The camera as a program lists it.
pub(crate) fn camera_info() -> CameraInfo {
    CameraInfo {
        id: ID.to_owned(),
        name: NAME.to_owned(),
        facing: CameraFacing::External,
    }
}

/// The formats it runs in: 640 by 480 and 1280 by 720, each at 30 frames a second with photos at
/// its own size.
pub(crate) fn formats() -> Vec<CameraFormat> {
    [(640.0, 480.0), (1280.0, 720.0)]
        .into_iter()
        .map(|(width, height)| {
            let size = Size { width, height };
            CameraFormat {
                size,
                min_frame_rate: FRAME_RATE,
                max_frame_rate: FRAME_RATE,
                photo_sizes: vec![size],
            }
        })
        .collect()
}

impl ShellCameras for SimulatedCameras {
    fn list(&self) -> CameraListFuture {
        Box::pin(future::ready(vec![camera_info()]))
    }

    /// Never: the camera is always there.
    fn changed(&self) -> CameraChangeFuture {
        Box::pin(pending())
    }

    fn open(&self, program: &AppInstance, id: &str) -> CameraOpenFuture {
        if id != ID {
            return Box::pin(future::ready(Err(CameraError::Unplugged)));
        }
        tracing::info!(
            program = program.name(),
            "a program opens WaOS's simulated camera"
        );
        let camera = Box::new(SimulatedCamera {
            codecs: self.codecs.clone(),
        });
        Box::pin(future::ready(Ok(OpenedCamera {
            camera,
            formats: formats(),
        })))
    }
}

/// The camera opened.
struct SimulatedCamera {
    codecs: Option<Arc<dyn ShellImageCodecs>>,
}

impl ShellCamera for SimulatedCamera {
    /// Starts drawing at once; the engine checked the format is one of the camera's.
    fn start(
        &self,
        format: CameraFormat,
        outputs: CameraOutputs,
        reports: CaptureSessionReports,
    ) -> CaptureStartFuture {
        let fed = match outputs.frames.as_ref().map(fed_by_waos) {
            Some(Ok(fed)) => Some(fed),
            Some(Err(error)) => return Box::pin(future::ready(Err(error))),
            None => None,
        };
        let drawing = Drawing {
            width: format.size.width as u32,
            height: format.size.height as u32,
            frame: AtomicU64::new(0),
            stopped: AtomicBool::new(false),
            preview: outputs.preview.clone(),
            fed,
        };
        let drawing = Arc::new(drawing);
        let thread = Arc::clone(&drawing);
        let spawned = thread::Builder::new()
            .name("simulated camera".into())
            .spawn(move || thread.run());
        if let Err(error) = spawned {
            let error = CameraError::Failed(format!("the test pattern did not start: {error}"));
            return Box::pin(future::ready(Err(error)));
        }
        reports.state_changed(CaptureState::Running);
        let session = Box::new(SimulatedSession {
            drawing,
            codecs: self.codecs.clone(),
        });
        Box::pin(future::ready(Ok(StartedSession { session, format })))
    }
}

/// What WaOS feeds the program's `reader` with; why not for a reader a player gave Waft already.
fn fed_by_waos(
    reader: &Arc<dyn waos::ShellVideoFrameReader>,
) -> Result<Arc<FedFrames>, CameraError> {
    let reader: Arc<dyn std::any::Any + Send + Sync> = Arc::clone(reader) as _;
    reader
        .downcast::<ReaderOnRequest>()
        .ok()
        .and_then(|reader| reader.fed_by_waos())
        .ok_or_else(|| {
            CameraError::Failed("the reader is fed by something else already".to_owned())
        })
}

/// One session's drawing, on its thread.
struct Drawing {
    width: u32,
    height: u32,
    /// The number of the frame drawn last.
    frame: AtomicU64,
    stopped: AtomicBool,
    preview: Option<Arc<dyn ShellCompositionLayer>>,
    fed: Option<Arc<FedFrames>>,
}

impl Drawing {
    /// Draws a frame each 1/30 s until stopped, each shown on the preview and fed to the reader.
    fn run(&self) {
        let every = Duration::from_secs_f64(1.0 / FRAME_RATE);
        let mut next = Instant::now();
        let mut number = 0;
        while !self.stopped.load(Ordering::Relaxed) {
            match self.drawn(number) {
                Ok(frame) => self.hand_out(frame),
                Err(error) => tracing::warn!(%error, "a frame of the test pattern was not drawn"),
            }
            self.frame.store(number, Ordering::Relaxed);
            number += 1;
            next += every;
            thread::sleep(next.saturating_duration_since(Instant::now()));
        }
    }

    fn drawn(&self, number: u64) -> std::io::Result<waos::VideoPicture> {
        let (width, height) = (self.width, self.height);
        pattern_frame(width, height, |row_bytes, pixels| {
            test_pattern::draw(number, width, height, row_bytes, pixels);
        })
    }

    /// Shows `frame` on the preview, and feeds it to the reader.
    fn hand_out(&self, frame: waos::VideoPicture) {
        if let Some(preview) = &self.preview
            && let Some(surface) = surface_of(&frame)
        {
            preview.present(surface, Box::new(|| {}));
        }
        if let Some(fed) = &self.fed {
            fed.feed(CrossThreadFrame::new(frame));
        }
    }

    /// The pixels of the frame drawn last, drawn again in the program's memory for a photo.
    fn photo_pixels(&self) -> PixelBuffer {
        let (width, height) = (self.width, self.height);
        let row_bytes = width as usize * 4;
        let mut bytes = vec![0; row_bytes * height as usize];
        let number = self.frame.load(Ordering::Relaxed);
        test_pattern::draw(number, width, height, row_bytes, &mut bytes);
        PixelBuffer {
            width,
            height,
            row_bytes: row_bytes as u32,
            format: PixelFormat::Bgra8,
            alpha: AlphaType::Premultiplied,
            bytes,
        }
    }
}

/// A session running, as the engine holds it: its drawing stops as it is dropped.
struct SimulatedSession {
    drawing: Arc<Drawing>,
    codecs: Option<Arc<dyn ShellImageCodecs>>,
}

impl ShellCaptureSession for SimulatedSession {
    /// The frame of the moment, as a JPEG the app's image codecs encode.
    fn take_photo(&self) -> PhotoFuture {
        let Some(codecs) = &self.codecs else {
            let error = CameraError::Failed("this system encodes no photos".to_owned());
            return Box::pin(future::ready(Err(error)));
        };
        let encoded = codecs.encode_pixels(self.drawing.photo_pixels(), PHOTO_TYPE, None);
        Box::pin(async move {
            let data = encoded
                .await
                .map_err(|error| CameraError::Failed(error.message))?;
            Ok(Photo {
                mime_type: PHOTO_TYPE.to_owned(),
                data,
            })
        })
    }
}

impl Drop for SimulatedSession {
    fn drop(&mut self) {
        self.drawing.stopped.store(true, Ordering::Relaxed);
    }
}

/// The photos the session took, for a test.
#[cfg(test)]
pub(crate) fn photo_pixels_of(width: u32, height: u32, frame: u64) -> PixelBuffer {
    let drawing = Drawing {
        width,
        height,
        frame: AtomicU64::new(frame),
        stopped: AtomicBool::new(true),
        preview: None,
        fed: None,
    };
    drawing.photo_pixels()
}

#[cfg(test)]
mod tests {
    use std::time::Instant as Now;

    use futures::executor::block_on;
    use waos::ShellVideoFrameReader;

    use super::*;
    use crate::logic::test_pattern::{Corner, read_frame_number};

    #[test]
    fn the_camera_is_listed_with_its_two_formats_at_30_frames_a_second() {
        let cameras = SimulatedCameras::new(None);
        let listed: Vec<_> = block_on(cameras.list())
            .into_iter()
            .map(|camera| camera.id)
            .collect();
        assert_eq!(listed, [ID]);
        let sizes: Vec<_> = formats()
            .iter()
            .map(|format| (format.size.width, format.size.height, format.max_frame_rate))
            .collect();
        assert_eq!(sizes, [(640.0, 480.0, 30.0), (1280.0, 720.0, 30.0)]);
    }

    #[test]
    fn a_photo_is_the_pattern_of_the_frame_of_the_moment() {
        let pixels = photo_pixels_of(640, 480, 42);
        let row_bytes = pixels.row_bytes as usize;
        assert_eq!(read_frame_number(640, 480, row_bytes, &pixels.bytes), 42);
        assert_eq!(
            test_pattern::corner_colour(Corner::TopLeft, 640, 480, row_bytes, &pixels.bytes),
            Corner::TopLeft.colour()
        );
    }

    #[test]
    fn a_session_feeds_the_programs_reader_frames_that_count_up_until_it_is_stopped() {
        let reader = Arc::new(ReaderOnRequest::default());
        let drawing = Arc::new(Drawing {
            width: 640,
            height: 480,
            frame: AtomicU64::new(0),
            stopped: AtomicBool::new(false),
            preview: None,
            fed: reader.fed_by_waos(),
        });
        let running = Arc::clone(&drawing);
        let thread = thread::spawn(move || running.run());
        let first = frame_number(&wait_for_frame(reader.as_ref()));
        let later = frame_number(&wait_for_frame(reader.as_ref()));
        assert!(later > first, "a new frame each time: {first} then {later}");
        drawing.stopped.store(true, Ordering::Relaxed);
        thread.join().unwrap();
    }

    /// The next frame `reader` answers, which the pattern draws upright.
    fn wait_for_frame(reader: &dyn ShellVideoFrameReader) -> waos::VideoPicture {
        let deadline = Now::now() + Duration::from_secs(5);
        loop {
            if let Some(frame) = reader.frame_for(Now::now()) {
                assert_eq!((frame.rotation, frame.flip), (0, false), "drawn upright");
                return frame.picture;
            }
            assert!(Now::now() < deadline, "no frame came");
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn frame_number(frame: &waos::VideoPicture) -> u16 {
        crate::os::read_pattern_frame(frame, read_frame_number)
    }
}
