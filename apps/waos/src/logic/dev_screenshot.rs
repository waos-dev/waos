//! A screenshot `waos ui screenshot` asks for: the last picture of each window of a program
//! running, however it was installed or started, or of the one window asked for, each with the id
//! its run gave it as it opened, its title, its size, whether it is in sight, and the picture as a
//! PNG. A picture is what the window shows, at its view's size (`ProgramView::picture`): the
//! program's drawing over what each composition layer beneath it shows now, a buffer it presented,
//! a player's video or a camera's picture, drawn together by the engine on the GPU; BGRA with its
//! alpha premultiplied, as the compositor shows it, made a PNG's RGBA with its alpha straight,
//! marked sRGB.
//!
//! What WaOS asks the program's user over its windows is the shell's, not the program's picture:
//! each window says what is asked (`asking_now`).
//!
//! The invariant: a program is read only while it runs, through its view's own picture, never a
//! window of the app's reached around the engine.

use waos::ProgramView;
use waos::bindings::wasdk::platform::hardware_buffers::PixelBuffer;
use waos_dev_protocol::{
    Asked, Refusal, RefusalKind, ScreenshotAnswer, ServerConnection, WindowId, WindowPicture,
};
use waos_package::ProgramId;

use super::asking_now::AskingNow;
use super::dev_refusal;
use super::runs::{DrivenProgram, Runs};

/// Answers a connection's `Screenshot`: the pictures of the windows of the program `id`, or of
/// `window` alone, or why neither was taken.
pub(crate) async fn answer(
    mut connection: ServerConnection,
    runs: &Runs,
    asking: &AskingNow,
    id: &ProgramId,
    window: Option<WindowId>,
) {
    let answer = match runs.driven(id) {
        Some(run) => {
            let asked = asking.of(id.as_str());
            let id = id.clone();
            // Reading and encoding a picture takes a while; the server's thread serves others.
            match tokio::task::spawn_blocking(move || pictures(&id, &run, asked.as_ref(), window))
                .await
            {
                Ok(Ok(pictures)) => ScreenshotAnswer::Pictures(pictures),
                Ok(Err(refusal)) => ScreenshotAnswer::Refused(refusal),
                // A panic while encoding is this build's own failure.
                Err(error) => ScreenshotAnswer::Refused(Refusal::new(
                    RefusalKind::Failed,
                    format!("taking the screenshot: {error}"),
                )),
            }
        }
        None => ScreenshotAnswer::Refused(dev_refusal::not_running(id)),
    };
    // A client gone meanwhile has nobody to tell.
    let _ = connection.send(&answer).await;
}

/// The picture of each of `program`'s windows, each with the id the run gave it as it opened; or
/// of `window` alone, or why it has no such window. Each says what WaOS `asked` the program's user
/// over it.
fn pictures(
    id: &ProgramId,
    run: &DrivenProgram,
    asked: Option<&Asked>,
    window: Option<WindowId>,
) -> Result<Vec<WindowPicture>, Refusal> {
    let windows = run.windows();
    let count = windows.len();
    match window {
        Some(window) => {
            let wanted = windows
                .into_iter()
                .find(|(each, _)| *each == window)
                .map(|(_, view)| view)
                .ok_or_else(|| dev_refusal::no_window(id, window, count))?;
            Ok(vec![WindowPicture {
                asking: asked.cloned(),
                ..picture_of(window, &wanted)
            }])
        }
        None => Ok(windows
            .into_iter()
            .map(|(window, view)| WindowPicture {
                asking: asked.cloned(),
                ..picture_of(window, &view)
            })
            .collect()),
    }
}

/// The window `window`, showing `view`, as `waos screenshot` writes it.
fn picture_of(window: WindowId, view: &ProgramView) -> WindowPicture {
    let pixels = match view.picture() {
        Ok(pixels) => pixels,
        Err(error) => {
            tracing::warn!(%window, ?error, "a window's picture could not be read");
            None
        }
    };
    let metrics = view.metrics();
    let (width, height) = pixels
        .as_ref()
        .map_or((metrics.width, metrics.height), |pixels| {
            (pixels.width, pixels.height)
        });
    WindowPicture {
        window,
        title: view.window_title().unwrap_or_default(),
        width,
        height,
        in_sight: view.in_sight(),
        png: pixels.as_ref().map(png_of),
        asking: None,
    }
}

/// `pixels`, BGRA and premultiplied, as a PNG of straight RGBA, marked sRGB.
fn png_of(pixels: &PixelBuffer) -> Vec<u8> {
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, pixels.width, pixels.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut writer = encoder
        .write_header()
        .expect("a PNG's header is written into memory");
    writer
        .write_image_data(&straight_rgba(pixels))
        .expect("the pixels are the header's size");
    writer.finish().expect("a PNG is finished in memory");
    png
}

/// The rows of `pixels`, without what pads each, as RGBA with straight alpha.
fn straight_rgba(pixels: &PixelBuffer) -> Vec<u8> {
    let row = pixels.width as usize * 4;
    let mut rgba = Vec::with_capacity(row * pixels.height as usize);
    let rows = pixels.bytes.chunks(pixels.row_bytes as usize);
    for padded in rows.take(pixels.height as usize) {
        let (bgra, _) = padded[..row].as_chunks::<4>();
        for &[blue, green, red, alpha] in bgra {
            rgba.extend([red, green, blue].map(|channel| unpremultiplied(channel, alpha)));
            rgba.push(alpha);
        }
    }
    rgba
}

/// `channel`, multiplied by `alpha` as it lies, as it is before; nothing where nothing shows.
fn unpremultiplied(channel: u8, alpha: u8) -> u8 {
    if alpha == 0 {
        return 0;
    }
    let straight = (u32::from(channel) * 255 + u32::from(alpha) / 2) / u32::from(alpha);
    straight.min(255) as u8
}

#[cfg(test)]
mod tests {
    use std::fs;

    use waos::bindings::wasdk::platform::hardware_buffers::{AlphaType, PixelFormat};
    use waos_dev_protocol::{InspectAnswer, RefusalKind, Request, ScreenshotAnswer, WindowId};

    use super::*;
    use crate::logic::dev_server::tests::TestServer;
    use crate::logic::open_command::{self, Sent};
    use crate::logic::opened_package::tests::write_package;

    /// The id `examples/opener` runs as.
    const OPENER: &str = "dev.waos.examples.opener";

    fn screenshot(id: &str) -> Request {
        Request::Screenshot {
            id: id.parse().unwrap(),
            window: None,
        }
    }

    #[tokio::test]
    async fn a_program_the_library_opened_is_read_and_inspected_as_any_running_one() {
        let server = TestServer::start("screenshot-library");
        let ScreenshotAnswer::Refused(refusal) =
            server.ask::<ScreenshotAnswer>(&screenshot(OPENER)).await
        else {
            panic!("nothing runs");
        };
        assert_eq!(refusal.kind, RefusalKind::NotRunning);
        let packages = server.folder.join("packages");
        fs::create_dir_all(&packages).unwrap();
        let component = fs::read(waos_test_programs::OPENER).unwrap();
        let key = waos_package::SigningKey::generate();
        let package = write_package(&packages, OPENER, "Opener", "1.0.0", &component, &key);
        crate::logic::test_installs::install_package(
            &server.store,
            &server.installed.engine,
            waos_package::Package::open(&package).unwrap(),
            waos_test_programs::OPENER,
        );
        let file = fs::canonicalize(&server.folder).unwrap().join("notes.md");
        fs::write(&file, "my notes").unwrap();
        let id = OPENER.parse().unwrap();
        let sent = open_command::send_to(&server.socket, &id, vec![file]).await;
        assert_eq!(sent.unwrap(), Sent::Opened);
        server.wait_for_runs(&[OPENER]).await;
        assert_eq!(
            server.ask::<ScreenshotAnswer>(&screenshot(OPENER)).await,
            ScreenshotAnswer::Pictures(Vec::new()),
            "read, though no run of waos started it: its engine here shows no window"
        );
        let active_uses = Request::Inspect {
            id: OPENER.parse().unwrap(),
            asked: waos_dev_protocol::Inspection::ActiveUses,
        };
        let InspectAnswer::Inspected(lines) = server.ask::<InspectAnswer>(&active_uses).await
        else {
            panic!("inspected, though no run of waos started it");
        };
        assert_eq!(lines.len(), 1, "everything it uses now, as one line");
    }

    /// Two pixels on each of two rows, each row padded to 12 bytes.
    fn pixels() -> PixelBuffer {
        let mut bytes = Vec::new();
        for row in [
            [[255, 0, 0, 255], [0, 64, 0, 128]],
            [[0, 0, 0, 0], [10, 20, 30, 255]],
        ] {
            for pixel in row {
                bytes.extend(pixel);
            }
            bytes.extend([9, 9, 9, 9]);
        }
        PixelBuffer {
            width: 2,
            height: 2,
            row_bytes: 12,
            format: PixelFormat::Bgra8,
            alpha: AlphaType::Premultiplied,
            bytes,
        }
    }

    #[test]
    fn pixels_become_straight_rgba_without_the_rows_padding() {
        assert_eq!(
            straight_rgba(&pixels()),
            [
                [0, 0, 255, 255],
                [0, 128, 0, 128],
                [0, 0, 0, 0],
                [30, 20, 10, 255],
            ]
            .concat()
        );
    }

    #[test]
    fn a_picture_is_a_png_of_its_size_and_its_pixels() {
        let png = png_of(&pixels());
        let mut reader = png::Decoder::new(std::io::Cursor::new(png))
            .read_info()
            .unwrap();
        let mut decoded = vec![0; reader.output_buffer_size().unwrap()];
        let frame = reader.next_frame(&mut decoded).unwrap();
        assert_eq!((frame.width, frame.height), (2, 2));
        assert_eq!(frame.color_type, png::ColorType::Rgba);
        assert_eq!(&decoded[..4], [0, 0, 255, 255]);
        assert!(reader.info().srgb.is_some(), "marked sRGB");
    }

    /// `examples/input`'s pictures, which it draws where the engine makes hardware buffers, on
    /// Apple's systems.
    #[cfg(any(target_vendor = "apple", target_os = "linux"))]
    mod drawn {
        use std::path::Path;
        use std::time::{Duration, Instant};

        use waos::Presence;
        use waos_dev_protocol::Input;

        use super::*;
        use crate::logic::test_engine::WITHIN;
        use crate::logic::test_engine::drawing::{WINDOW_HEIGHT, WINDOW_WIDTH};

        /// The id `examples/input` is installed as, and the development id it runs as.
        const INPUT: &str = "dev.waos.examples.input";
        const INPUT_DEVELOPMENT: &str = "dev.waos.examples.input.development";

        /// Its first two colours, as RGBA: 0.1, 0.1, 0.4 and 0.9, 0.5, 0.1.
        const FIRST: [u8; 4] = [26, 26, 102, 255];
        const SECOND: [u8; 4] = [230, 128, 26, 255];

        /// The pictures `server` answers for the program `id` once its first window shows one.
        async fn pictures_once_shown(server: &TestServer, id: &str) -> Vec<WindowPicture> {
            let deadline = Instant::now() + WITHIN;
            loop {
                if let ScreenshotAnswer::Pictures(pictures) =
                    server.ask::<ScreenshotAnswer>(&screenshot(id)).await
                    && pictures
                        .first()
                        .is_some_and(|picture| picture.png.is_some())
                {
                    return pictures;
                }
                assert!(
                    Instant::now() < deadline,
                    "the window never showed a picture"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }

        /// The pictures `server` answers for the program `id` once its first window's middle is
        /// `colour`, but for a rounding of the GPU's.
        async fn pictures_once_middle_is(
            server: &TestServer,
            id: &str,
            colour: [u8; 4],
        ) -> Vec<WindowPicture> {
            let deadline = Instant::now() + WITHIN;
            loop {
                let pictures = pictures_once_shown(server, id).await;
                let (middle, _) = middle_of(pictures[0].png.as_ref().unwrap());
                if is_colour(middle, colour) {
                    return pictures;
                }
                assert!(
                    Instant::now() < deadline,
                    "the window never showed {colour:?}: {middle:?}"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }

        /// The middle pixel of `png`, as RGBA, and its size.
        fn middle_of(png: &[u8]) -> ([u8; 4], (u32, u32)) {
            let mut reader = png::Decoder::new(std::io::Cursor::new(png))
                .read_info()
                .unwrap();
            let mut rgba = vec![0; reader.output_buffer_size().unwrap()];
            let frame = reader.next_frame(&mut rgba).unwrap();
            let at = ((frame.height / 2 * frame.width + frame.width / 2) * 4) as usize;
            (
                rgba[at..at + 4].try_into().unwrap(),
                (frame.width, frame.height),
            )
        }

        /// Whether `pixel` is `colour`, but for a rounding of the GPU's.
        fn is_colour(pixel: [u8; 4], colour: [u8; 4]) -> bool {
            pixel
                .iter()
                .zip(colour)
                .all(|(byte, expected)| byte.abs_diff(expected) <= 2)
        }

        /// Fails unless `pixel` is `colour`, but for a rounding of the GPU's.
        fn assert_colour(pixel: [u8; 4], colour: [u8; 4]) {
            assert!(is_colour(pixel, colour), "{pixel:?}, not {colour:?}");
        }

        #[tokio::test]
        async fn a_screenshot_is_each_windows_last_picture_with_its_title_size_and_sight() {
            let server = TestServer::drawing("screenshot");
            let _client = server
                .run(INPUT, Path::new(waos_test_programs::INPUT))
                .await;
            let pictures = pictures_once_shown(&server, INPUT_DEVELOPMENT).await;
            let [picture] = &pictures[..] else {
                panic!("one window: {pictures:?}");
            };
            assert_eq!(
                (picture.window, picture.title.as_str(), picture.in_sight),
                (WindowId(1), "Input", true)
            );
            assert_eq!(
                (picture.width, picture.height),
                (WINDOW_WIDTH, WINDOW_HEIGHT)
            );
            let (middle, size) = middle_of(picture.png.as_ref().unwrap());
            assert_eq!(size, (WINDOW_WIDTH, WINDOW_HEIGHT));
            assert_colour(middle, FIRST);

            let tap = Request::Input {
                id: INPUT_DEVELOPMENT.parse().unwrap(),
                window: Some(WindowId(1)),
                input: Input::Tap { x: 1.0, y: 1.0 },
            };
            assert_eq!(
                server.ask::<waos_dev_protocol::InputAnswer>(&tap).await,
                waos_dev_protocol::InputAnswer::Done(waos_dev_protocol::InputDone {
                    in_sight: true
                })
            );
            pictures_once_middle_is(&server, INPUT_DEVELOPMENT, SECOND).await;
        }

        /// A program is watched while an attach to it is open, so its covered windows draw.
        #[tokio::test]
        async fn a_covered_window_is_in_sight_and_draws_while_attached_and_not_after() {
            let server = TestServer::drawing("screenshot-covered");
            let client = server
                .run(INPUT, Path::new(waos_test_programs::INPUT))
                .await;
            pictures_once_shown(&server, INPUT_DEVELOPMENT).await;
            let view = server.programs().await[0].views().remove(0);
            view.presence_changed(Presence {
                on_screen: true,
                occluded: true,
                focused: false,
            });
            assert!(view.in_sight(), "covered, but watched");
            let pictures = pictures_once_shown(&server, INPUT_DEVELOPMENT).await;
            assert!(pictures[0].in_sight);
            let tap = Request::Input {
                id: INPUT_DEVELOPMENT.parse().unwrap(),
                window: Some(WindowId(1)),
                input: Input::Tap { x: 1.0, y: 1.0 },
            };
            assert_eq!(
                server.ask::<waos_dev_protocol::InputAnswer>(&tap).await,
                waos_dev_protocol::InputAnswer::Done(waos_dev_protocol::InputDone {
                    in_sight: true
                })
            );
            // A covered window draws what the input changed.
            pictures_once_middle_is(&server, INPUT_DEVELOPMENT, SECOND).await;

            drop(client);
            let deadline = Instant::now() + WITHIN;
            while view.in_sight() {
                assert!(
                    Instant::now() < deadline,
                    "once detached, covered is out of sight"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            server.wait_for_runs(&[INPUT_DEVELOPMENT]).await;
        }

        #[tokio::test]
        async fn window_ids_survive_a_window_closing_and_screenshot_takes_one_window() {
            const TWO_WINDOWS: &str = "dev.waos.examples.twowindows";
            const TWO_WINDOWS_DEVELOPMENT: &str = "dev.waos.examples.twowindows.development";
            let server = TestServer::drawing("window-ids");
            let _client = server
                .run(TWO_WINDOWS, Path::new(waos_test_programs::TWO_WINDOWS))
                .await;
            let deadline = Instant::now() + WITHIN;
            let both = loop {
                if let ScreenshotAnswer::Pictures(pictures) = server
                    .ask::<ScreenshotAnswer>(&screenshot(TWO_WINDOWS_DEVELOPMENT))
                    .await
                    && pictures.len() == 2
                    && pictures.iter().all(|picture| picture.png.is_some())
                {
                    break pictures;
                }
                assert!(Instant::now() < deadline, "both windows never drew");
                tokio::time::sleep(Duration::from_millis(10)).await;
            };
            assert_eq!(
                (both[0].window, both[0].title.as_str()),
                (WindowId(1), "first")
            );
            assert_eq!(
                (both[1].window, both[1].title.as_str()),
                (WindowId(2), "second")
            );

            // Asking for one window returns that window alone.
            let only_second = Request::Screenshot {
                id: TWO_WINDOWS_DEVELOPMENT.parse().unwrap(),
                window: Some(WindowId(2)),
            };
            let ScreenshotAnswer::Pictures(one) =
                server.ask::<ScreenshotAnswer>(&only_second).await
            else {
                panic!("one window");
            };
            assert_eq!(one.len(), 1);
            assert_eq!(
                (one[0].window, one[0].title.as_str()),
                (WindowId(2), "second")
            );

            // Closing the first window leaves the second with id 2, refuses id 1, and gives the
            // next window id 3 rather than reusing 1.
            let program = server.programs().await.remove(0);
            let first_view = program.views()[0].id();
            program.remove_view(first_view);
            let ScreenshotAnswer::Pictures(after_close) = server
                .ask::<ScreenshotAnswer>(&screenshot(TWO_WINDOWS_DEVELOPMENT))
                .await
            else {
                panic!("the remaining window");
            };
            assert_eq!(after_close.len(), 1);
            assert_eq!(
                (after_close[0].window, after_close[0].title.as_str()),
                (WindowId(2), "second")
            );
            let ScreenshotAnswer::Refused(refusal) = server
                .ask::<ScreenshotAnswer>(&Request::Screenshot {
                    id: TWO_WINDOWS_DEVELOPMENT.parse().unwrap(),
                    window: Some(WindowId(1)),
                })
                .await
            else {
                panic!("a closed window is refused");
            };
            assert_eq!(
                refusal.kind,
                RefusalKind::NoWindow {
                    window: WindowId(1)
                }
            );
            let ScreenshotAnswer::Pictures(still_second) =
                server.ask::<ScreenshotAnswer>(&only_second).await
            else {
                panic!("window 2 survives");
            };
            assert_eq!(still_second[0].window, WindowId(2));

            // With window 1 closed, input and resize without `--window` reach window 2.
            let default_input = server
                .ask::<waos_dev_protocol::InputAnswer>(&Request::Input {
                    id: TWO_WINDOWS_DEVELOPMENT.parse().unwrap(),
                    window: None,
                    input: Input::Tap { x: 1.0, y: 1.0 },
                })
                .await;
            assert!(
                matches!(default_input, waos_dev_protocol::InputAnswer::Done(_)),
                "input without --window reached window 2: {default_input:?}"
            );
            let default_resize = server
                .ask::<waos_dev_protocol::ResizeAnswer>(&Request::Resize {
                    id: TWO_WINDOWS_DEVELOPMENT.parse().unwrap(),
                    window: None,
                    width: 320.0,
                    height: 240.0,
                })
                .await;
            assert!(
                matches!(default_resize, waos_dev_protocol::ResizeAnswer::Done(_)),
                "resize without --window reached window 2: {default_resize:?}"
            );

            // As a third request would be answered: the program made two.
            let third = crate::logic::test_engine::drawing::add_window(&program, 3);
            assert_eq!(third.id(), 3);
            let ScreenshotAnswer::Pictures(with_third) = server
                .ask::<ScreenshotAnswer>(&screenshot(TWO_WINDOWS_DEVELOPMENT))
                .await
            else {
                panic!("windows 2 and 3");
            };
            assert_eq!(
                with_third
                    .iter()
                    .map(|picture| picture.window)
                    .collect::<Vec<_>>(),
                [WindowId(2), WindowId(3)]
            );
        }
    }
}
