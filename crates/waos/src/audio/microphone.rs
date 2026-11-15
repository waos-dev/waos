//! The microphone as a source of a program's graph: opened once the user allows it, recording
//! while the program holds it, which keeps the program thawed and which the shell shows
//! (`recordings`); let go, the device is closed.
//!
//! The invariant: a microphone holds the program thawed and shown recording for exactly as long as
//! it is open, and closes its device as it goes.

use waos_capabilities::web_audio_api::media_streams::MediaStream;
use waos_capabilities::web_audio_api::node::MediaStreamAudioSourceNode;

use super::nodes::{AnyNode, Node};
use crate::access::Held;
use crate::lifecycle::Sounding;
use crate::recordings::Recording;

/// A microphone open: the value behind a program's `microphone-node`.
pub struct Microphone {
    node: Node<MediaStreamAudioSourceNode>,
    stream: MediaStream,
    _recording: Recording,
    _sounding: Sounding,
    /// The microphone's use, held until the program lets go of it.
    _open: Held,
}

impl Microphone {
    /// `stream`, the microphone open, as `source`, its source on a context, recording as
    /// `recording` holds, thawed as `sounding` holds, its use held by `open`.
    pub(super) fn new(
        source: MediaStreamAudioSourceNode,
        stream: MediaStream,
        recording: Recording,
        sounding: Sounding,
        open: Held,
    ) -> Microphone {
        Microphone {
            node: Node::new(source),
            stream,
            _recording: recording,
            _sounding: sounding,
            _open: open,
        }
    }

    pub(super) fn any(&self) -> AnyNode {
        self.node.any()
    }
}

impl Drop for Microphone {
    /// Closes the device: its track ends, and with it the source.
    fn drop(&mut self) {
        for track in self.stream.get_tracks() {
            track.close();
        }
    }
}
