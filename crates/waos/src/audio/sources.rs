//! The graph's scheduled sources, oscillators, constant sources and buffer sources: each with the
//! moment it ended, which the program awaits as the spec's `ended` event, and, on a context that
//! plays, the sound it plays from the start the program asks for until then, which keeps its
//! program thawed while the context's clock is past that start.
//!
//! The invariant: web-audio-api keeps one `ended` handler a node, so a source's is set once, when
//! it is made, and every wait on it reads what that one handler tells; a source is scheduled at
//! most once, before web-audio-api is told to start it, and never after that handler ran, so
//! whichever comes first, a source that ends is silent by its end.

use std::future::Future;
use std::mem;
use std::sync::{Arc, Mutex};

use tokio::sync::watch;
use waos_capabilities::web_audio_api::node::AudioScheduledSourceNode;

use super::mistakes::trapping;
use super::nodes::{AnyNode, Node};
use super::sound::{ContextSound, Playing};
use crate::sync::locked;

/// One scheduled source of kind `N`: the value behind that kind's handle.
pub struct Source<N> {
    node: Node<N>,
    end: Arc<SourceEnd>,
    /// The sound of its context; none for one that plays nothing aloud.
    sound: Option<Arc<ContextSound>>,
}

/// When a source ended, and its sound until then.
struct SourceEnd {
    ended: watch::Sender<bool>,
    sound: Mutex<SourceSound>,
}

/// A source's sound, as its start and its end move it.
enum SourceSound {
    /// Not started yet.
    Waiting,
    /// Started: it plays from the start asked for until it ends, aloud where its context plays.
    Started { _playing: Option<Playing> },
    /// Ended, started or not.
    Ended,
}

impl SourceEnd {
    /// The source ended: its sound is over, and then whoever waits hears it, so a program that
    /// heard its source end is silent by it.
    fn end(&self) {
        let sound = mem::replace(&mut *locked(&self.sound), SourceSound::Ended);
        drop(sound);
        self.ended.send_replace(true);
    }

    /// The source plays from `when`, on `sound` where its context plays, unless it was started
    /// before or ended already.
    fn start(&self, sound: Option<&Arc<ContextSound>>, when: f64) {
        let mut source = locked(&self.sound);
        if let SourceSound::Waiting = *source {
            *source = SourceSound::Started {
                _playing: sound.map(|sound| sound.play_from(when)),
            };
        }
    }
}

impl<N: AudioScheduledSourceNode + Send + 'static> Source<N> {
    /// `node`, a source made just now on the context whose sound is `sound`, none for one that
    /// plays nothing aloud.
    pub(super) fn new(node: N, sound: Option<&Arc<ContextSound>>) -> Source<N> {
        let end = Arc::new(SourceEnd {
            ended: watch::Sender::new(false),
            sound: Mutex::new(SourceSound::Waiting),
        });
        let on_end = Arc::clone(&end);
        node.set_onended(move |_| on_end.end());
        Source {
            node: Node::new(node),
            end,
            sound: sound.cloned(),
        }
    }

    pub(super) fn node(&self) -> &Node<N> {
        &self.node
    }

    pub(super) fn any(&self) -> AnyNode {
        self.node.any()
    }

    /// Starts the source at `when`, on the context's clock, as `start` does it: it plays from
    /// then until it ends.
    pub(super) fn start(&self, when: f64, start: impl FnOnce(&mut N)) -> wasmtime::Result<()> {
        self.end.start(self.sound.as_ref(), when);
        trapping(|| self.node.with(start))
    }

    /// Resolves once the source has ended; never when its context is closed first.
    pub(super) fn ended(&self) -> impl Future<Output = ()> + Send + 'static + use<N> {
        let mut ended = self.end.ended.subscribe();
        async move {
            // The sender is held by the source's handler, as long as its context.
            let _ = ended.wait_for(|ended| *ended).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::{Duration, Instant};

    use futures::executor::block_on;
    use waos_capabilities::web_audio_api::context::{
        AudioContext, AudioContextOptions, BaseAudioContext,
    };

    use super::*;
    use crate::audio::sound::tests::PausedProgram;
    use crate::epoch::TICK;

    /// How far ahead of the context's clock the source is scheduled.
    const AHEAD: f64 = 0.1;

    /// A context on no output, which renders in real time, as one on a device does.
    fn silent_context() -> AudioContext {
        AudioContext::new(AudioContextOptions {
            sink_id: "none".to_owned(),
            ..AudioContextOptions::default()
        })
    }

    /// Waits until `context`'s clock reaches `time`, as it does in real time.
    fn until_clock_at(context: &AudioContext, time: f64) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while context.current_time() < time {
            assert!(Instant::now() < deadline, "the context's clock stands");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_source_sounds_from_the_tick_that_finds_it_started_until_it_ends() {
        let PausedProgram {
            time,
            clock,
            lifecycle,
            ..
        } = &PausedProgram::new();
        let context = silent_context();
        let base = context.base().clone();
        let sound = ContextSound::new(lifecycle.clone(), clock, move || base.current_time());
        let source = Source::new(context.create_constant_source(), Some(&sound));
        let start = context.current_time() + AHEAD;
        source.start(start, |node| node.start_at(start)).unwrap();
        assert!(lifecycle.is_frozen(), "scheduled ahead, it is silent");

        until_clock_at(&context, start);
        time.elapse(TICK);
        assert!(!lifecycle.is_frozen(), "it plays now");

        source.node().with(|node| node.stop());
        block_on(source.ended());
        assert!(lifecycle.is_frozen(), "it ended");
        context.close_sync();
    }
}
