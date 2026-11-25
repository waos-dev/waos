//! The programs' video streams and frames in Waft's terms and back. Waft's video decoders follow
//! the program's interface one to one, so each value maps onto its like; a frame is Waft's own,
//! which the engine takes as the platform's picture with what its decoder said of it.
//!
//! The invariant: nothing is changed on the way. A value Waft has no like of does not exist.

use std::sync::Arc;

use waft::ui;
use waft::ui::video_decoders as decoders;
use waos::DecodedFrame;
use waos::bindings::wasdk::media::video_decoders::{
    DecodeSupport, DecoderError, DecodingPurpose, VideoDecodeSupport, VideoDecoderOptions,
    VideoStreamDescription,
};
use waos::bindings::wasdk::media::video_frames::{
    ChromaAlignment, ChromaLocation, Chromaticity, ColorRange, ContentLight, HdrMetadata,
    MasteringDisplay, PixelAspect, PixelRect, VideoColor,
};
use waos::bindings::wasdk::media::video_streams::{
    ChromaSampling, CodecChoice, CodecKind, EncodedChunk, NalFormat, PictureSize, VideoCodec,
    VideoStreamSetup,
};

use super::frame_readers::engine_frame;

pub(crate) fn waft_setup(setup: VideoStreamSetup) -> decoders::VideoStreamSetup {
    match setup {
        VideoStreamSetup::H264(format) => decoders::VideoStreamSetup::H264(waft_nal_format(format)),
        VideoStreamSetup::Hevc(format) => decoders::VideoStreamSetup::Hevc(waft_nal_format(format)),
        VideoStreamSetup::Vp8 => decoders::VideoStreamSetup::Vp8,
        VideoStreamSetup::Vp9(record) => decoders::VideoStreamSetup::Vp9(record),
        VideoStreamSetup::Av1(record) => decoders::VideoStreamSetup::Av1(record),
    }
}

fn waft_nal_format(format: NalFormat) -> decoders::NalFormat {
    match format {
        NalFormat::LengthPrefixed(record) => decoders::NalFormat::LengthPrefixed(record),
        NalFormat::StartCodes(ahead) => decoders::NalFormat::StartCodes(ahead),
    }
}

pub(crate) fn waft_options(options: VideoDecoderOptions) -> decoders::VideoDecoderOptions {
    decoders::VideoDecoderOptions {
        decoder: match options.decoder {
            CodecChoice::Any => decoders::CodecChoice::Any,
            CodecChoice::Hardware => decoders::CodecChoice::Hardware,
            CodecChoice::Software => decoders::CodecChoice::Software,
        },
        purpose: match options.purpose {
            DecodingPurpose::Live => decoders::DecodingPurpose::Live,
            DecodingPurpose::Playback => decoders::DecodingPurpose::Playback,
            DecodingPurpose::Background => decoders::DecodingPurpose::Background,
        },
        frame_limit: options.frame_limit,
        largest_size: options.largest_size.map(waft_size),
        default_color: options.default_color.map(waft_color),
    }
}

pub(crate) fn waft_chunk(chunk: EncodedChunk) -> decoders::EncodedChunk {
    decoders::EncodedChunk {
        data: chunk.data,
        timestamp: chunk.timestamp,
        duration: chunk.duration,
        key_frame: chunk.key_frame,
    }
}

pub(crate) fn waft_description(
    description: VideoStreamDescription,
) -> decoders::VideoStreamDescription {
    decoders::VideoStreamDescription {
        codec: match description.codec {
            VideoCodec::H264 => decoders::VideoCodec::H264,
            VideoCodec::Hevc => decoders::VideoCodec::Hevc,
            VideoCodec::Vp8 => decoders::VideoCodec::Vp8,
            VideoCodec::Vp9 => decoders::VideoCodec::Vp9,
            VideoCodec::Av1 => decoders::VideoCodec::Av1,
        },
        profile: description.profile,
        level: description.level,
        bit_depth: description.bit_depth,
        chroma: match description.chroma {
            ChromaSampling::Yuv420 => decoders::ChromaSampling::Yuv420,
            ChromaSampling::Yuv422 => decoders::ChromaSampling::Yuv422,
            ChromaSampling::Yuv444 => decoders::ChromaSampling::Yuv444,
            ChromaSampling::Gray => decoders::ChromaSampling::Gray,
        },
        size: waft_size(description.size),
        frame_rate: description.frame_rate,
    }
}

fn waft_size(size: PictureSize) -> decoders::PictureSize {
    decoders::PictureSize {
        width: size.width,
        height: size.height,
    }
}

fn waft_color(color: VideoColor) -> ui::VideoColor {
    ui::VideoColor {
        primaries: color.primaries,
        transfer: color.transfer,
        matrix: color.matrix,
        range: color.range.map(|range| match range {
            ColorRange::Limited => ui::ColorRange::Limited,
            ColorRange::Full => ui::ColorRange::Full,
        }),
        chroma_location: color.chroma_location.map(|location| ui::ChromaLocation {
            horizontal: waft_alignment(location.horizontal),
            vertical: waft_alignment(location.vertical),
        }),
    }
}

fn waft_alignment(alignment: ChromaAlignment) -> ui::ChromaAlignment {
    match alignment {
        ChromaAlignment::Cosited => ui::ChromaAlignment::Cosited,
        ChromaAlignment::Centered => ui::ChromaAlignment::Centered,
    }
}

pub(crate) fn program_support(support: decoders::VideoDecodeSupport) -> VideoDecodeSupport {
    let program = |support| match support {
        decoders::DecodeSupport::Yes => DecodeSupport::Yes,
        decoders::DecodeSupport::No => DecodeSupport::No,
        decoders::DecodeSupport::Unknown => DecodeSupport::Unknown,
    };
    VideoDecodeSupport {
        hardware: program(support.hardware),
        software: program(support.software),
    }
}

pub(crate) fn program_kind(kind: decoders::CodecKind) -> CodecKind {
    match kind {
        decoders::CodecKind::Hardware => CodecKind::Hardware,
        decoders::CodecKind::Software => CodecKind::Software,
    }
}

pub(crate) fn program_error(error: decoders::DecoderError) -> DecoderError {
    match error {
        decoders::DecoderError::Unsupported(why) => DecoderError::Unsupported(why),
        decoders::DecoderError::Busy => DecoderError::Busy,
        decoders::DecoderError::BadData(why) => DecoderError::BadData(why),
        decoders::DecoderError::Lost => DecoderError::Lost,
        decoders::DecoderError::Failed(why) => DecoderError::Failed(why),
    }
}

/// Waft's `frame` as the engine takes a decoder's: the platform's picture behind it, with what
/// its decoder said of it; none, said so, for a frame of a kind the engine does not take.
pub(crate) fn decoded_frame(frame: &Arc<dyn ui::VideoFrame>) -> Option<DecodedFrame> {
    let visible = frame.visible_rect();
    let aspect = frame.pixel_aspect();
    Some(DecodedFrame {
        frame: engine_frame(frame.as_ref())?,
        // Every frame of a decoder carries its chunk's time.
        timestamp: frame.timestamp().unwrap_or_default(),
        duration: frame
            .duration()
            .map(|duration| duration.as_nanos().try_into().unwrap_or(u64::MAX)),
        visible_rect: PixelRect {
            x: visible.x,
            y: visible.y,
            width: visible.width,
            height: visible.height,
        },
        pixel_aspect: PixelAspect {
            width: aspect.width,
            height: aspect.height,
        },
        color: program_color(frame.color()),
        hdr_metadata: frame.hdr_metadata().map(program_hdr_metadata),
    })
}

fn program_color(color: ui::VideoColor) -> VideoColor {
    VideoColor {
        primaries: color.primaries,
        transfer: color.transfer,
        matrix: color.matrix,
        range: color.range.map(|range| match range {
            ui::ColorRange::Limited => ColorRange::Limited,
            ui::ColorRange::Full => ColorRange::Full,
        }),
        chroma_location: color.chroma_location.map(|location| ChromaLocation {
            horizontal: program_alignment(location.horizontal),
            vertical: program_alignment(location.vertical),
        }),
    }
}

fn program_alignment(alignment: ui::ChromaAlignment) -> ChromaAlignment {
    match alignment {
        ui::ChromaAlignment::Cosited => ChromaAlignment::Cosited,
        ui::ChromaAlignment::Centered => ChromaAlignment::Centered,
    }
}

fn program_hdr_metadata(metadata: ui::HdrMetadata) -> HdrMetadata {
    let point = |point: ui::Chromaticity| Chromaticity {
        x: point.x,
        y: point.y,
    };
    HdrMetadata {
        mastering_display: metadata.mastering_display.map(|display| MasteringDisplay {
            red: point(display.red),
            green: point(display.green),
            blue: point(display.blue),
            white: point(display.white),
            max_luminance: display.max_luminance,
            min_luminance: display.min_luminance,
        }),
        content_light: metadata.content_light.map(|light| ContentLight {
            max: light.max,
            max_frame_average: light.max_frame_average,
        }),
    }
}
