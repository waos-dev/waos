//! The latest of what the instrument played, for the scope to draw: the sound's own samples,
//! kept as they are written to the output.

/// How many samples are kept: enough for a window of the lowest note's wave and the stretch
/// before it a steady start is looked for in.
const KEPT: usize = 4096;

pub(crate) struct Trace {
    /// The latest samples, the two sides as one, the oldest at `oldest`.
    samples: Vec<f32>,
    oldest: usize,
    /// Whether sound is being played, so there is something new to draw.
    live: bool,
}

impl Trace {
    pub(crate) fn new() -> Trace {
        Trace {
            samples: vec![0.0; KEPT],
            oldest: 0,
            live: false,
        }
    }

    /// Keeps `frames`, each a left sample and a right, as the latest played.
    pub(crate) fn take(&mut self, frames: &[f32]) {
        let (frames, _) = frames.as_chunks::<2>();
        for [left, right] in frames {
            self.samples[self.oldest] = (left + right) / 2.0;
            self.oldest = (self.oldest + 1) % KEPT;
        }
    }

    pub(crate) fn is_live(&self) -> bool {
        self.live
    }

    /// Sound is about to play.
    pub(crate) fn wake(&mut self) {
        self.live = true;
    }

    /// The sound has died away: what is kept is silence.
    pub(crate) fn rest(&mut self) {
        self.live = false;
        self.samples.fill(0.0);
    }

    /// `length` samples from among the latest, begun where the wave rises through zero, so that
    /// a steady note's wave stands still from one look to the next; the latest `length` where it
    /// never does.
    pub(crate) fn steady(&self, length: usize) -> Vec<f32> {
        let length = length.min(KEPT / 2);
        let latest = |back: usize| self.samples[(self.oldest + KEPT - back) % KEPT];
        let rises = |back: usize| latest(back + 1) < 0.0 && latest(back) >= 0.0;
        let start = (length..KEPT - 1)
            .find(|back| rises(*back))
            .unwrap_or(length);
        (0..length).map(|index| latest(start - index)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frames of a sine `period` samples long, `count` of them, the same on both sides.
    fn sine(period: usize, count: usize) -> Vec<f32> {
        (0..count)
            .flat_map(|index| {
                let sample = (index as f32 / period as f32 * std::f32::consts::TAU).sin();
                [sample, sample]
            })
            .collect()
    }

    #[test]
    fn a_steady_wave_is_shown_from_where_it_rises_through_zero() {
        let mut trace = Trace::new();
        for count in [5_000, 5_037, 5_111] {
            trace.take(&sine(100, count));
            let shown = trace.steady(256);
            assert_eq!(shown.len(), 256);
            assert!(shown[0] >= 0.0 && shown[0] < 0.07, "begins at {}", shown[0]);
            assert!(shown[10] > shown[0], "and rises");
        }
    }

    #[test]
    fn silence_is_shown_as_the_latest_samples() {
        let trace = Trace::new();
        assert_eq!(trace.steady(64), vec![0.0; 64]);
    }

    #[test]
    fn a_trace_at_rest_keeps_silence() {
        let mut trace = Trace::new();
        trace.wake();
        trace.take(&sine(50, 300));
        assert!(trace.is_live());
        trace.rest();
        assert!(!trace.is_live());
        assert_eq!(trace.steady(128), vec![0.0; 128]);
    }
}
