//! `cargo waos resize`: a size for a window of the program a run of `cargo waos run` runs, sent to
//! the WaOS running it, which sizes the window as the program's own `set-frame` would, so the
//! program can be seen at a phone's width or a narrow window's (the design's decision 60). A size
//! is in logical points, written `<width>x<height>`: a screenshot's pixels are points times the
//! window's scale, 2 on a Retina screen.
//!
//! The invariant: the command returns only once WaOS has answered, with the size the window took,
//! which a window system may have clamped, or why it took none.

use std::fmt;
use std::process::ExitCode;
use std::str::FromStr;

use anyhow::{Result, bail};
use waos_dev_protocol::{Request, ResizeAnswer, WindowId};
use waos_package::ProgramId;

use crate::asking::ask;

/// A size in logical points, written `<width>x<height>`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Points {
    pub width: f64,
    pub height: f64,
}

impl FromStr for Points {
    type Err = String;

    fn from_str(text: &str) -> Result<Points, String> {
        let not_a_size =
            || format!("{text:?} is not a size written <width>x<height>, such as 390x844");
        let (width, height) = text.split_once('x').ok_or_else(not_a_size)?;
        let length = |part: &str| {
            part.trim()
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite() && *n > 0.0)
        };
        match (length(width), length(height)) {
            (Some(width), Some(height)) => Ok(Points { width, height }),
            _ => Err(not_a_size()),
        }
    }
}

impl Points {
    /// Whether these points are `width` by `height` pixels at `scale`, to the nearest pixel.
    fn is_in_pixels(self, width: u32, height: u32, scale: f64) -> bool {
        let pixels = |points: f64| (points * scale).round();
        pixels(self.width) == f64::from(width) && pixels(self.height) == f64::from(height)
    }
}

impl fmt::Display for Points {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

/// Sizes the window `window` of the program `id` so its view is `size`, waits for WaOS's
/// answer, and says the size the window took.
pub async fn resize(id: ProgramId, window: WindowId, size: Points) -> Result<ExitCode> {
    let answer = ask(&Request::Resize {
        id,
        window,
        width: size.width,
        height: size.height,
    })
    .await?;
    match answer {
        ResizeAnswer::Done(waos_dev_protocol::Resized {
            width,
            height,
            scale,
            new_picture,
        }) => {
            let taken = Points {
                width: f64::from(width) / scale,
                height: f64::from(height) / scale,
            };
            println!("window {window}: {taken} points at scale {scale}, {width}x{height} pixels");
            if !size.is_in_pixels(width, height, scale) {
                eprintln!(
                    "cargo waos: the window system gave window {window} {taken} points, not {size}"
                );
            }
            if !new_picture {
                eprintln!(
                    "cargo waos: window {window} showed no new picture at its size: the program drew nothing after it, or the window is out of sight"
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        ResizeAnswer::Refused(refusal) => {
            eprintln!("cargo waos: {}", refusal.message);
            Ok(ExitCode::FAILURE)
        }
        ResizeAnswer::Unknown => bail!(
            "WaOS answered the size in a way this cargo waos does not know: update cargo waos with `cargo install cargo-waos`"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_size_is_two_positive_numbers_with_an_x_between() {
        assert_eq!(
            "390x844".parse::<Points>().unwrap(),
            Points {
                width: 390.0,
                height: 844.0
            }
        );
        assert_eq!(
            " 320.5 x 568".parse::<Points>().unwrap(),
            Points {
                width: 320.5,
                height: 568.0
            }
        );
        for text in [
            "390", "390x", "x844", "0x844", "-390x844", "390,844", "infx1",
        ] {
            assert!(text.parse::<Points>().is_err(), "{text}");
        }
    }

    #[test]
    fn a_size_taken_is_the_one_asked_for_to_the_nearest_pixel() {
        let size = Points {
            width: 390.3,
            height: 844.0,
        };
        assert!(size.is_in_pixels(781, 1688, 2.0));
        assert!(!size.is_in_pixels(780, 1688, 2.0), "clamped");
    }

    #[test]
    fn a_size_is_written_as_it_is_read() {
        let size = Points {
            width: 390.0,
            height: 844.5,
        };
        assert_eq!(size.to_string(), "390x844.5");
    }
}
