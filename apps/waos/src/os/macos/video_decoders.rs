//! Waft's video decoders for macOS, VideoToolbox's, taken from Waft's Apple crate itself: what a
//! WaOS that is no Waft app serves, which has no platform of Waft's to ask.

use std::rc::Rc;

use waft::ui::video_decoders::VideoDecoders;

/// The system's video decoders, to stay on the thread that asks for them.
pub(crate) fn video_decoders() -> Option<Rc<dyn VideoDecoders>> {
    waft_embedder_apple::video_decoders::video_decoders()
}
