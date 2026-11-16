//! The microphone as a source of a program's graph: opened once the user allows it, recording
//! while the program holds it, which keeps the program thawed and is held through the program's
//! access, whose record the shell's indicator shows; let go, the device is closed.
//!
//! The invariant: a microphone holds the program thawed and shown recording for exactly as long as
//! it is open, and closes its device as it goes.

use waos_capabilities::web_audio_api::media_streams::MediaStream;
use waos_capabilities::web_audio_api::node::MediaStreamAudioSourceNode;

use super::nodes::{AnyNode, Node};
use crate::access::ActiveUse;
use crate::lifecycle::Sounding;

/// A microphone open: the value behind a program's `microphone-node`.
pub struct Microphone {
    node: Node<MediaStreamAudioSourceNode>,
    stream: MediaStream,
    /// The microphone's use, held while it records.
    _recording: ActiveUse,
    _sounding: Sounding,
}

impl Microphone {
    /// `stream`, the microphone open, as `source`, its source on a context, recording as
    /// `recording` holds, thawed as `sounding` holds.
    pub(super) fn new(
        source: MediaStreamAudioSourceNode,
        stream: MediaStream,
        recording: ActiveUse,
        sounding: Sounding,
    ) -> Microphone {
        Microphone {
            node: Node::new(source),
            stream,
            _recording: recording,
            _sounding: sounding,
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
