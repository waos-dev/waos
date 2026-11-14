//! The broadcast a program's streams of changes are read from.
//!
//! Each stream a program opens is a subscriber of its own, and hears every item sent from then on,
//! in the order it was sent. The invariant: nothing is dropped, however far behind the program is,
//! since a change among its views is a view it would otherwise never hear of. What is sent while
//! no stream is open goes nowhere, and a stream the program dropped is forgotten.

use std::sync::Mutex;

use futures::channel::mpsc;

use crate::sync::locked;

/// A broadcast with one unbounded queue per stream open.
pub(crate) struct Channel<T> {
    /// One per stream the program opened; one it dropped is forgotten at the next send.
    streams: Mutex<Vec<mpsc::UnboundedSender<T>>>,
}

impl<T> Default for Channel<T> {
    fn default() -> Channel<T> {
        Channel {
            streams: Mutex::default(),
        }
    }
}

impl<T: Clone> Channel<T> {
    /// Sends `item` to every stream open.
    pub(crate) fn send(&self, item: T) {
        let mut streams = locked(&self.streams);
        streams.retain(|stream| !stream.is_closed());
        for stream in streams.iter() {
            // A stream that closed since the retain is forgotten at the next send.
            let _ = stream.unbounded_send(item.clone());
        }
    }

    /// A stream of its own for the items from now on.
    pub(crate) fn subscribe(&self) -> mpsc::UnboundedReceiver<T> {
        let (stream, items) = mpsc::unbounded();
        locked(&self.streams).push(stream);
        items
    }

    /// Whether a stream is open to hear what is sent now.
    pub(crate) fn is_heard(&self) -> bool {
        locked(&self.streams)
            .iter()
            .any(|stream| !stream.is_closed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stream_hears_every_item_in_order() {
        let channel = Channel::default();
        let (mut first, mut second) = (channel.subscribe(), channel.subscribe());
        channel.send(1);
        channel.send(2);
        for stream in [&mut first, &mut second] {
            assert_eq!(stream.try_recv().unwrap(), 1);
            assert_eq!(stream.try_recv().unwrap(), 2);
            assert!(stream.try_recv().is_err(), "nothing more yet");
        }
    }

    #[test]
    fn a_stream_hears_only_what_is_sent_after_it_opened() {
        let channel = Channel::default();
        channel.send(1);
        let mut stream = channel.subscribe();
        channel.send(2);
        assert_eq!(stream.try_recv().unwrap(), 2);
    }

    #[test]
    fn a_channel_is_heard_while_a_stream_is_open() {
        let channel = Channel::<u32>::default();
        assert!(!channel.is_heard());
        let stream = channel.subscribe();
        assert!(channel.is_heard());
        drop(stream);
        assert!(!channel.is_heard());
    }

    #[test]
    fn a_stream_dropped_is_forgotten() {
        let channel = Channel::default();
        drop(channel.subscribe());
        channel.send(1);
        assert!(locked(&channel.streams).is_empty());
    }
}
