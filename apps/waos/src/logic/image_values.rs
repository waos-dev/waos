//! An image's values in the program's terms and in Waft's, field for field: what an image says of
//! itself, its pixels, and why it was not opened, decoded or encoded; a time in the nanoseconds of
//! a WIT `duration`.
//!
//! The invariant: pixels go over and back whole, their rows as they lie, every field the same on
//! both sides.

use waft::ui;
use waft::ui::image_codecs as codecs;
use waos::bindings::wasdk::media::image_codecs::{
    DateTime, ImageFrame, ImageMetadata, ImageOrientation, ImageRotation, Location,
};
use waos::bindings::wasdk::platform::hardware_buffers::{
    AlphaType, ImageError, ImageErrorKind, PixelBuffer, PixelFormat,
};
use waos::bindings::wasdk::platform::windows::Size;

/// What an image says of itself, in the program's terms, read as it opened.
pub(crate) struct ImageFacts {
    pub(crate) mime_type: String,
    pub(crate) size: Size,
    pub(crate) frames: Vec<ImageFrame>,
    pub(crate) orientation: ImageOrientation,
    pub(crate) metadata: ImageMetadata,
}

/// What Waft's `image` says of itself, in the program's terms.
pub(crate) fn program_facts(image: &dyn codecs::ImageSource) -> ImageFacts {
    let size = image.size();
    ImageFacts {
        mime_type: image.mime_type(),
        size: Size {
            width: size.width(),
            height: size.height(),
        },
        frames: image.frames().into_iter().map(program_frame).collect(),
        orientation: program_orientation(image.orientation()),
        metadata: program_metadata(image.metadata()),
    }
}

fn program_frame(frame: codecs::ImageFrame) -> ImageFrame {
    ImageFrame {
        duration: frame
            .duration
            .map(|duration| u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)),
    }
}

fn program_orientation(orientation: codecs::ImageOrientation) -> ImageOrientation {
    ImageOrientation {
        mirrored: orientation.mirrored,
        rotation: match orientation.rotation {
            codecs::ImageRotation::None => ImageRotation::None,
            codecs::ImageRotation::QuarterTurn => ImageRotation::QuarterTurn,
            codecs::ImageRotation::HalfTurn => ImageRotation::HalfTurn,
            codecs::ImageRotation::ThreeQuarterTurn => ImageRotation::ThreeQuarterTurn,
        },
    }
}

fn program_metadata(metadata: codecs::ImageMetadata) -> ImageMetadata {
    ImageMetadata {
        taken: metadata.taken.map(|taken| DateTime {
            year: taken.year,
            month: taken.month,
            day: taken.day,
            hour: taken.hour,
            minute: taken.minute,
            second: taken.second,
            utc_offset: taken.utc_offset,
        }),
        camera_make: metadata.camera_make,
        camera_model: metadata.camera_model,
        lens_model: metadata.lens_model,
        exposure_time: metadata.exposure_time,
        f_number: metadata.f_number,
        iso: metadata.iso,
        focal_length: metadata.focal_length,
        equivalent_focal_length: metadata.equivalent_focal_length,
        exposure_bias: metadata.exposure_bias,
        flash_fired: metadata.flash_fired,
        location: metadata.location.map(|location| Location {
            latitude: location.latitude,
            longitude: location.longitude,
            altitude: location.altitude,
        }),
    }
}

/// Waft's pixels in the program's terms.
pub(crate) fn program_pixels(pixels: codecs::PixelBuffer) -> PixelBuffer {
    let layout = pixels.layout();
    PixelBuffer {
        width: layout.size[0],
        height: layout.size[1],
        row_bytes: layout.row_bytes,
        format: match layout.format {
            codecs::PixelFormat::Rgba8 => PixelFormat::Rgba8,
            codecs::PixelFormat::Bgra8 => PixelFormat::Bgra8,
        },
        alpha: match layout.alpha {
            codecs::AlphaType::Premultiplied => AlphaType::Premultiplied,
            codecs::AlphaType::Straight => AlphaType::Straight,
        },
        bytes: pixels.into_bytes(),
    }
}

/// The program's pixels in Waft's terms; why not, for pixels whose bytes do not hold their rows,
/// which the engine traps a program for before they come here.
pub(crate) fn waft_pixels(pixels: PixelBuffer) -> Result<codecs::PixelBuffer, ImageError> {
    let layout = codecs::PixelLayout {
        size: [pixels.width, pixels.height],
        row_bytes: pixels.row_bytes,
        format: match pixels.format {
            PixelFormat::Rgba8 => codecs::PixelFormat::Rgba8,
            PixelFormat::Bgra8 => codecs::PixelFormat::Bgra8,
        },
        alpha: match pixels.alpha {
            AlphaType::Premultiplied => codecs::AlphaType::Premultiplied,
            AlphaType::Straight => codecs::AlphaType::Straight,
        },
    };
    codecs::PixelBuffer::new(layout, pixels.bytes).map_err(|error| ImageError {
        kind: ImageErrorKind::Other,
        message: format!("{error:?}"),
    })
}

pub(crate) fn program_error(error: codecs::ImageError) -> ImageError {
    ImageError {
        kind: match error.kind {
            codecs::ImageErrorKind::Unsupported => ImageErrorKind::Unsupported,
            codecs::ImageErrorKind::Damaged => ImageErrorKind::Damaged,
            codecs::ImageErrorKind::TooLarge => ImageErrorKind::TooLarge,
            codecs::ImageErrorKind::NotFound => ImageErrorKind::NotFound,
            codecs::ImageErrorKind::OutOfMemory => ImageErrorKind::OutOfMemory,
            codecs::ImageErrorKind::Other => ImageErrorKind::Other,
        },
        message: error.message,
    }
}

pub(crate) fn waft_size(size: Size) -> ui::Size {
    ui::Size::new(size.width, size.height)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn pixels_go_over_and_back_whole() {
        let pixels = PixelBuffer {
            width: 2,
            height: 2,
            row_bytes: 12,
            format: PixelFormat::Bgra8,
            alpha: AlphaType::Straight,
            bytes: (0..24).collect(),
        };
        let back = program_pixels(waft_pixels(pixels).unwrap());
        assert_eq!((back.width, back.height, back.row_bytes), (2, 2, 12));
        assert!(matches!(back.format, PixelFormat::Bgra8));
        assert!(matches!(back.alpha, AlphaType::Straight));
        assert_eq!(back.bytes, (0..24).collect::<Vec<u8>>());
    }

    #[test]
    fn a_frames_delay_is_a_wit_duration() {
        let frame = codecs::ImageFrame {
            duration: Some(Duration::from_millis(100)),
        };
        assert_eq!(program_frame(frame).duration, Some(100_000_000));
        assert_eq!(program_frame(codecs::ImageFrame::default()).duration, None);
    }
}
