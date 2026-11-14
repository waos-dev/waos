//! How much sound to write ahead of the platform's graph, and when.
//!
//! The platform plays what a program writes to a stream input a block at a time, on its own
//! thread, and silence when nothing is written; it lets a program write up to a tenth of a second
//! ahead, which is that much delay between a key and its sound. A [`Pacer`] keeps the program
//! only a lead ahead instead, reckoned from the graph's clock: the delay it adds is its lead.
//!
//! The program writes from its one thread, between its events and its frames, so it gets to
//! look at the clock only as often as those let it. The lead is the longest it lately waited
//! between two looks, and a margin: long enough that the graph has sound through such a wait
//! again, and no longer.

/// The frames the graph takes at once: Web Audio's render quantum. Sound is written in whole
/// blocks, so the graph never waits on half of one.
pub(crate) const BLOCK: usize = 128;

/// The wait between two looks taken for the longest until a span of them has been seen, in
/// seconds: a frame of a screen at 60 Hz, which a program drawing every frame waits through.
const ASSUMED_WAIT: f64 = 0.016;

/// How many blocks the lead is beyond the longest wait: what a wait may outlast the longest
/// seen before the graph runs dry.
const MARGIN_BLOCKS: f64 = 2.0;

/// How much of a block may be missing before one more is due: less is the rounding of the
/// clock's arithmetic.
const ROUNDING: f64 = 1e-6;

/// The longest the lead is, in seconds: well short of the tenth of a second the platform lets a
/// program write ahead. A write that reaches that bound waits for the graph, and a program
/// waiting there looks at the clock late, which would keep the lead long.
const LONGEST_LEAD: f64 = 0.06;

/// How long a span of looks is, in seconds. The longest wait is the longest of this span and
/// of the one before, so a long wait is remembered for one span at least and two at most.
const SPAN: f64 = 5.0;

pub(crate) struct Pacer {
    sample_rate: f64,
    /// What was written since the graph last had nothing to play, and the clock then.
    run: Option<Run>,
    /// The graph's clock at the last look, while sound is being written.
    last_look: Option<f64>,
    waits: RecentWaits,
}

/// An unbroken stretch of sound: when on the graph's clock its first frames were handed to
/// the graph, which plays them from then, and its frames written so far.
#[derive(Clone, Copy)]
struct Run {
    began: f64,
    written: u64,
}

/// The longest wait between two looks, lately: in the span of looks going on, which began at
/// its first, and in the one before it.
struct RecentWaits {
    span_began: Option<f64>,
    longest_this_span: f64,
    longest_last_span: f64,
}

impl Pacer {
    /// A pacer for a graph of `sample_rate` frames a second, nothing written yet.
    pub(crate) fn new(sample_rate: f64) -> Pacer {
        Pacer {
            sample_rate,
            run: None,
            last_look: None,
            waits: RecentWaits {
                span_began: None,
                longest_this_span: 0.0,
                longest_last_span: ASSUMED_WAIT,
            },
        }
    }

    /// How far ahead of the graph the program writes now, in seconds: the delay the writing
    /// adds between a key and its sound.
    pub(crate) fn lead(&self) -> f64 {
        let margin = MARGIN_BLOCKS * BLOCK as f64 / self.sample_rate;
        (self.waits.longest() + margin).min(LONGEST_LEAD)
    }

    /// The blocks to write now, the graph's clock reading `clock` seconds, to be a lead ahead
    /// of it. A clock past everything written means the graph ran dry: what is written next
    /// starts a run of its own.
    pub(crate) fn blocks_due(&mut self, clock: f64) -> usize {
        if let Some(last_look) = self.last_look.replace(clock) {
            self.waits.saw(clock - last_look, clock);
        }
        let ahead = match self.run {
            Some(run) => run.written as f64 - (clock - run.began) * self.sample_rate,
            None => 0.0,
        };
        if ahead <= 0.0 {
            self.run = None;
        }
        let missing = self.lead() * self.sample_rate - ahead.max(0.0);
        (missing / BLOCK as f64 - ROUNDING).ceil().max(0.0) as usize
    }

    /// `blocks` blocks were written, the graph's clock reading `clock` seconds once they were:
    /// the first of a run play from then, however long they took to make.
    pub(crate) fn wrote(&mut self, blocks: usize, clock: f64) {
        let run = self.run.get_or_insert(Run {
            began: clock,
            written: 0,
        });
        run.written += (blocks * BLOCK) as u64;
    }

    /// The sound has ended, and nothing is written until there is more: the time until then is
    /// no wait between two looks.
    pub(crate) fn rest(&mut self) {
        self.run = None;
        self.last_look = None;
    }
}

impl RecentWaits {
    fn longest(&self) -> f64 {
        self.longest_this_span.max(self.longest_last_span)
    }

    /// A look at `clock` came `wait` seconds after the one before.
    fn saw(&mut self, wait: f64, clock: f64) {
        let span_age = clock - *self.span_began.get_or_insert(clock);
        if span_age >= SPAN {
            self.longest_last_span = if span_age < 2.0 * SPAN {
                self.longest_this_span
            } else {
                ASSUMED_WAIT
            };
            self.longest_this_span = 0.0;
            self.span_began = Some(clock);
        }
        self.longest_this_span = self.longest_this_span.max(wait);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: f64 = 48_000.0;

    /// The seconds `blocks` blocks last.
    fn seconds(blocks: usize) -> f64 {
        (blocks * BLOCK) as f64 / SAMPLE_RATE
    }

    /// Looks every `wait_blocks` blocks from `clock` for `looks` looks, writing what is due.
    /// Answers the clock after the last, and the fewest frames the graph was left ahead with.
    fn play(pacer: &mut Pacer, mut clock: f64, wait_blocks: usize, looks: usize) -> (f64, i64) {
        let mut written = 0i64;
        let mut least_ahead = i64::MAX;
        for look in 0..looks {
            let played = (look * wait_blocks * BLOCK) as i64;
            if look > 0 {
                least_ahead = least_ahead.min(written - played);
            }
            let due = pacer.blocks_due(clock);
            pacer.wrote(due, clock);
            written += (due * BLOCK) as i64;
            clock += seconds(wait_blocks);
        }
        (clock, least_ahead)
    }

    #[test]
    fn the_first_write_covers_a_frame_of_drawing_and_a_margin() {
        let mut pacer = Pacer::new(SAMPLE_RATE);
        assert_eq!(
            pacer.blocks_due(5.0),
            8,
            "16 ms and two blocks: 1024 frames"
        );
    }

    #[test]
    fn a_program_in_time_writes_what_the_graph_played() {
        let mut pacer = Pacer::new(SAMPLE_RATE);
        let first = pacer.blocks_due(5.0);
        pacer.wrote(first, 5.0);
        assert_eq!(pacer.blocks_due(5.0), 0, "a lead ahead already");
        assert_eq!(
            pacer.blocks_due(5.0 + seconds(2)),
            2,
            "the graph played two blocks"
        );
    }

    #[test]
    fn a_run_plays_from_when_its_first_blocks_were_written_not_from_when_they_were_due() {
        let mut pacer = Pacer::new(SAMPLE_RATE);
        let first = pacer.blocks_due(5.0);
        let made_at = 5.0 + seconds(6);
        pacer.wrote(first, made_at);
        assert_eq!(pacer.blocks_due(made_at), 0, "nothing of it has played yet");
        assert_eq!(pacer.blocks_due(made_at + seconds(1)), 1);
    }

    #[test]
    fn the_lead_shortens_to_the_waits_seen_once_two_spans_of_them_were_short() {
        let mut pacer = Pacer::new(SAMPLE_RATE);
        let looks_in_a_span = (SPAN / seconds(1)).ceil() as usize + 1;
        let (clock, _) = play(&mut pacer, 5.0, 1, looks_in_a_span);
        assert_eq!(
            pacer.lead(),
            ASSUMED_WAIT + seconds(2),
            "the first span is assumed"
        );
        play(&mut pacer, clock, 1, looks_in_a_span);
        assert!(
            (pacer.lead() - seconds(3)).abs() < 1e-9,
            "a block's wait and the margin"
        );
    }

    #[test]
    fn a_long_wait_lengthens_the_lead_at_once_and_is_forgotten_two_spans_on() {
        let mut pacer = Pacer::new(SAMPLE_RATE);
        let looks_in_a_span = (SPAN / seconds(1)).ceil() as usize + 1;
        let (clock, _) = play(&mut pacer, 5.0, 1, 2 * looks_in_a_span);
        let late = clock + seconds(14);
        let due = pacer.blocks_due(late);
        assert!(
            (pacer.lead() - seconds(17)).abs() < 1e-9,
            "15 blocks' wait and the margin"
        );
        assert_eq!(due, 17, "the graph ran dry: a whole lead is due");
        pacer.wrote(due, late);
        play(&mut pacer, late + seconds(1), 1, 2 * looks_in_a_span);
        assert!((pacer.lead() - seconds(3)).abs() < 1e-9);
    }

    #[test]
    fn waits_no_longer_than_the_longest_seen_never_run_the_graph_dry() {
        let mut pacer = Pacer::new(SAMPLE_RATE);
        let (_, least_ahead) = play(&mut pacer, 5.0, 6, 2_000);
        assert!(
            least_ahead >= (2 * BLOCK) as i64,
            "left {least_ahead} frames ahead"
        );
    }

    #[test]
    fn the_lead_is_never_longer_than_the_platform_holds() {
        let mut pacer = Pacer::new(SAMPLE_RATE);
        pacer.blocks_due(1.0);
        pacer.blocks_due(4.0);
        assert_eq!(pacer.lead(), LONGEST_LEAD);
    }

    #[test]
    fn the_silence_between_two_notes_is_no_wait() {
        let mut pacer = Pacer::new(SAMPLE_RATE);
        let first = pacer.blocks_due(5.0);
        pacer.wrote(first, 5.0);
        pacer.rest();
        assert_eq!(
            pacer.blocks_due(8.0),
            first,
            "a new run, with the lead as it was"
        );
    }
}
