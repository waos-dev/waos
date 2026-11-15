# Synth

An instrument to play at once. A keyboard along the bottom of the window plays under the mouse, under several fingers at once on a touch screen, and from the computer's keys laid out as a piano's (`A` to `'` the white keys from a C, `W E T Y U O P` the black ones, `Z` and `X` an octave down and up); a pointer held and slid plays each key it crosses. Six presets (Warm Pad, Electric Piano, Pluck, Bass, Lead, Organ), each a small graph of [fundsp](https://github.com/SamiPerttu/fundsp)'s oscillators, a lowpass filter under an envelope, an amplitude envelope, and a chorus, an echo or a reverb; five sliders (cutoff, resonance, attack, release, volume), which a preset sets where it sounds best; Sustain, which keeps notes sounding after their keys are let go, for a chord built a key at a time with the mouse and a hand free for the sliders; and a scope that draws the wave being played. Twelve notes sound at once, a thirteenth cutting the oldest; no note starts or stops with a click, and a preset changed lets the old one ring out.

- **Takes:** `wasdk`'s `audio`, for the platform's output: a context on the default output and a stream input the program writes its samples to; `wasdk-adapter-waft` and Waft with its WinUI kit, for the window, the controls, the pointers and the keys; `fundsp`, for the sound.
- **Provides:** the program `dev.waos.synth`.
- **Used by:** people; nothing depends on it.

```sh
cargo waos run -p synth --release
```

Run it `--release`: the sound is computed in the program, and a debug build computes it too slowly to keep up.

## How the sound reaches the output

The platform's audio is Web Audio's graph, rendered on a thread of the platform's own, and it runs none of a program's code there. Sound a program computes itself goes in through a stream input (`create_stream_input`), which the program writes ahead of what the graph plays. So `output` is a task on the program's one thread: it owns the synth, takes the keys and the controls as commands, and every two milliseconds writes as many blocks of 128 frames as keep it a lead ahead of the graph's clock (`pacing`). The lead is the delay the app adds between a key and its sound. The platform would let a program write a tenth of a second ahead and wait there; writing only a lead ahead is what makes the instrument playable.

The lead is the longest the task lately waited between two looks at the clock, and two blocks: the thread also handles every event and draws every frame, and what it spends on those the sound must have been written through. On the Mac this was written on, a frame drawn holds the thread about 13 ms, so the lead is 21 to 24 ms while the scope draws and 11 to 13 ms while nothing does, with 6 ms of output latency after it; a longer stall (a list opening, a screenshot taken) is heard as a gap, and lengthens the lead to as much as 60 ms for the next five to ten seconds.

For the same reason the scope draws thirty times a second, not at the screen's rate: every other refresh of a 60 Hz screen is left to the sound.

While nothing sounds the task holds no stream input, which would keep the platform from freezing the program, and the scope draws nothing. When the window loses the keyboard or is hidden, every held key is let go, since its release would never arrive; notes let go ring out, and sustained notes play on, which keeps the program awake while they do.

## Parts

Plain Rust, tested with `cargo test -p synth`: `pitch` (a note and its hertz), `tone` and `control` (what the sliders set, their ranges and how their values read), `key_envelope`, `presets`, `voice`, `layer` and `synth` (the sound, from one note's graph to the mixed frames), `pacing` (how much to write and when), `trace` (the latest samples, for the scope), `held_keys` (which note each finger and key holds, and which are sustained), `keyboard_layout` (where the keys are) and `typing` (the computer's keys as a piano's). Over them `instrument`, the state the widgets read, `output`, its sound, and `typed_keys`, which plays the computer's keys on it whatever has the focus. The widgets: `page`, `keyboard_view`, `scope_view`, `controls_view` and `window`.

## Left out

- **Velocity.** Every key plays at one strength: neither a mouse nor the computer's keys say how hard they were pressed.
- **The letters on the keys follow a US keyboard.** The piano row is by the keys' places, so it lies the same under the hand on any layout, but the letters drawn on the keys are a US layout's: the platform does not say what a key's place prints on this keyboard.
- **MIDI.** The platform has no interface for MIDI devices.
- **Keeping the last preset and tone** between runs.
- **A scope at the screen's rate,** as above.
