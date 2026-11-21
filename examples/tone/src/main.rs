//! A program that plays a tone while it is out of sight: what the platform keeps from freezing
//! while it is audible, and only then. It asks for a view, which it draws nothing in, starts a
//! tone, plays it while it is
//! hidden, stops it once it is shown again and waits for its end, and schedules another an hour
//! ahead, which leaves it silent until then; and once it is hidden and shown again, starts the
//! first tone again, which the spec forbids, so it traps with the spec's name for the mistake.
//!
//! It prints each step as it takes it.
//!
//! ```text
//! cargo build -p tone --target wasm32-wasip3
//! ```

use wasdk::StreamReader;
use wasdk::audio::{self, AudioContextOptions};
use wasdk::lifecycle::{self, LifecycleState};
use wasdk::views;
use wasdk::windows::WindowConfig;

/// The tone's pitch: A above middle C, in hertz.
const PITCH: f32 = 440.0;

/// How far ahead the second tone is scheduled: an hour, in seconds.
const AN_HOUR: f64 = 3600.0;

#[wasdk::main]
async fn main() {
    let mut states = lifecycle::state_changes();
    if let Err(error) = views::request_view(&WindowConfig::new(), None) {
        println!("no view: {error}");
        return;
    }
    let Some(audio) = audio::get_audio() else {
        println!("no sound");
        return;
    };
    let context = audio
        .new_context(AudioContextOptions::new())
        .expect("a context on the default output")
        .base();
    let tone = context.create_oscillator();
    tone.frequency().set_value(PITCH);
    let gain = context.create_gain();
    gain.gain().set_value(0.1);
    tone.node().connect(&gain.node());
    gain.node().connect(&context.destination().node());
    tone.start(0.0);
    println!("playing");

    until(&mut states, hidden).await;
    println!("hidden, playing");
    until(&mut states, shown).await;
    tone.stop(0.0);
    tone.ended().await;
    let later = context.create_oscillator();
    later.node().connect(&gain.node());
    later.start(context.current_time() + AN_HOUR);
    println!("shown, ended, another an hour ahead");

    until(&mut states, hidden).await;
    println!("hidden, silent until then");
    until(&mut states, shown).await;
    println!("shown, starting again");
    tone.start(0.0);
    println!("started twice");
}

/// Waits until the program's state changes to one that is `wanted`.
async fn until(states: &mut StreamReader<LifecycleState>, wanted: fn(LifecycleState) -> bool) {
    while let Some(changed) = states.next().await {
        if wanted(changed) {
            return;
        }
    }
}

/// Whether the user cannot see the program.
fn hidden(state: LifecycleState) -> bool {
    matches!(state, LifecycleState::Hidden | LifecycleState::Paused)
}

/// Whether the user sees the program.
fn shown(state: LifecycleState) -> bool {
    matches!(state, LifecycleState::Resumed | LifecycleState::Inactive)
}
