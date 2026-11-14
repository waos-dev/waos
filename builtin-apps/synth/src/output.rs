//! The instrument's sound on the platform's output: a task that owns the synth, takes what the
//! instrument asks of it, and writes its sound ahead of the platform's graph through a stream
//! input, a short lead at a time.
//!
//! The platform runs none of a program's code in time with its output, so the samples are made
//! here, on the program's one thread, between its events and its frames, and the lead is what
//! covers the time the thread spends on those ([`Pacer`]). The task looks every couple of
//! milliseconds while there is sound; while there is none it holds no stream input, which would
//! keep the platform from freezing the program, and waits for the next key alone.
//!
//! The invariant: a write never waits. The lead stays well below the tenth of a second the
//! platform lets a program write ahead, so `write` returns at once and the task is back at the
//! instrument's commands.

use std::cell::RefCell;
use std::pin::pin;
use std::rc::Rc;

use futures::StreamExt;
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use futures::future::{self, Either};
use wasdk::audio::{self, AudioContext, AudioContextOptions, AudioError, StreamInputNode};
use wasdk::monotonic_clock;

use crate::pacing::{BLOCK, Pacer};
use crate::pitch::Note;
use crate::presets::Preset;
use crate::synth::{CHANNELS, Synth};
use crate::tone::Tone;
use crate::trace::Trace;
use crate::voice::CHUNK;

/// How long the task waits between two looks at the graph's clock while sound plays: two
/// milliseconds, in nanoseconds, a little under a block's length.
const LOOK_EVERY: u64 = 2_000_000;

/// The most blocks made before they are written. A run of sound starts with a whole lead due:
/// written a few blocks at a time, its first note is heard after those few are made.
const MOST_BLOCKS_AT_ONCE: usize = 4;

/// What the instrument asks of its sound.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Command {
    Press(Note),
    LetGo(Note),
    SetTone(Tone),
    ChangePreset(Preset, Tone),
}

/// The instrument's end of its sound: where it asks, and where it reads what was played.
#[derive(Clone)]
pub(crate) struct Output {
    commands: UnboundedSender<Command>,
    trace: Rc<RefCell<Trace>>,
}

impl Output {
    /// Opens the platform's default output and starts the task that plays `preset` with `tone`
    /// on it; none where the platform has no sound, or its output cannot be opened.
    pub(crate) fn start(preset: Preset, tone: Tone) -> Option<Output> {
        let context = audio::get_audio()?
            .new_context(AudioContextOptions::new())
            .ok()?;
        let sample_rate = f64::from(context.base().sample_rate());
        let (commands, asked) = mpsc::unbounded();
        let trace = Rc::new(RefCell::new(Trace::new()));
        let playing = Playing {
            context,
            synth: Synth::new(sample_rate, preset, tone),
            pacer: Pacer::new(sample_rate),
            trace: Rc::clone(&trace),
        };
        wasdk::spawn(play(playing, asked));
        Some(Output { commands, trace })
    }

    /// Asks `command` of the sound, which takes it before the next block it writes.
    pub(crate) fn ask(&self, command: Command) {
        if matches!(command, Command::Press(_)) {
            self.trace.borrow_mut().wake();
        }
        // The task ends only with the program.
        let _ = self.commands.unbounded_send(command);
    }

    /// What was played last, for the scope.
    pub(crate) fn trace(&self) -> &Rc<RefCell<Trace>> {
        &self.trace
    }
}

/// What the task plays with: the platform's context, the synth it plays on it, how far ahead it
/// writes, and where it keeps what it played for the scope.
struct Playing {
    context: AudioContext,
    synth: Synth,
    pacer: Pacer,
    trace: Rc<RefCell<Trace>>,
}

/// Plays as `asked` says, for as long as the instrument lives.
async fn play(mut playing: Playing, mut asked: UnboundedReceiver<Command>) {
    while let Some(command) = asked.next().await {
        playing.take(command);
        if playing.synth.is_quiet() {
            continue;
        }
        let played = playing.play_until_quiet(&mut asked).await;
        playing.pacer.rest();
        playing.trace.borrow_mut().rest();
        if let Err(error) = played {
            eprintln!("the sound stopped: {error:?}");
            return;
        }
    }
}

impl Playing {
    /// Writes the synth's sound ahead of the graph until it has died away, taking what is
    /// asked meanwhile. The stream input lives as long: once it is dropped the graph plays what
    /// was written, and the program holds nothing that plays.
    async fn play_until_quiet(
        &mut self,
        asked: &mut UnboundedReceiver<Command>,
    ) -> Result<(), AudioError> {
        let graph = self.context.base();
        let input = self.context.create_stream_input(CHANNELS as u32);
        input.node().connect(&graph.destination().node());
        while !self.synth.is_quiet() {
            while let Ok(command) = asked.try_recv() {
                self.take(command);
            }
            let due = self.pacer.blocks_due(graph.current_time());
            let blocks = due.min(MOST_BLOCKS_AT_ONCE);
            self.write(&input, blocks).await?;
            self.pacer.wrote(blocks, graph.current_time());
            if blocks < due {
                continue;
            }
            match future::select(asked.next(), pin!(monotonic_clock::wait_for(LOOK_EVERY))).await {
                Either::Left((Some(command), _)) => self.take(command),
                Either::Left((None, _)) => break,
                Either::Right(_) => {}
            }
        }
        Ok(())
    }

    /// Writes the synth's next `blocks` blocks to `input`, keeping them for the scope.
    async fn write(&mut self, input: &StreamInputNode, blocks: usize) -> Result<(), AudioError> {
        if blocks == 0 {
            return Ok(());
        }
        let frames = self.synth.play(blocks * BLOCK / CHUNK);
        self.trace.borrow_mut().take(&frames);
        input.write(frames).await
    }

    fn take(&mut self, command: Command) {
        match command {
            Command::Press(note) => self.synth.press(note),
            Command::LetGo(note) => self.synth.let_go(note),
            Command::SetTone(tone) => self.synth.set_tone(tone),
            Command::ChangePreset(preset, tone) => self.synth.change_preset(preset, tone),
        }
    }
}
