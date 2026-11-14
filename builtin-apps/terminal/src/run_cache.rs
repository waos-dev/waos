//! The runs of text laid out for the last frame, kept for the next: most of a screen's text is
//! the same from one frame to the next, and laying a run out costs far more than drawing it
//! again.
//!
//! The invariant a later editor will break: a run is kept only while frames draw it. Each frame
//! marks the runs it draws, and what the frame did not draw is let go at its end, so the cache
//! never holds more than a screen of text.

use std::collections::HashMap;

use waft_widgets::TextPainter;

use crate::row_picture::RunStyle;

/// What makes two runs the same laid-out text: their characters, their style, and the font.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RunKey {
    pub(crate) text: String,
    pub(crate) style: RunStyle,
    pub(crate) family: &'static str,
    /// The font's size and the spacing added between characters, as their bits, which compare
    /// exactly.
    pub(crate) size_bits: u64,
    pub(crate) spacing_bits: u64,
}

/// A run laid out, with how far its baseline lies below its top.
pub(crate) struct LaidOutRun {
    pub(crate) painter: TextPainter,
    pub(crate) baseline: f64,
    /// The frame that drew it last.
    drawn_in: u64,
}

/// The runs the last frame drew.
#[derive(Default)]
pub(crate) struct RunCache {
    runs: HashMap<RunKey, LaidOutRun>,
    frame: u64,
}

impl RunCache {
    /// Starts a frame: the runs asked for until [`end_frame`](Self::end_frame) are kept.
    pub(crate) fn begin_frame(&mut self) {
        self.frame += 1;
    }

    /// The run of `key`, laid out by `lay_out` where the last frame did not draw it.
    pub(crate) fn run(
        &mut self,
        key: RunKey,
        lay_out: impl FnOnce(&RunKey) -> (TextPainter, f64),
    ) -> &mut LaidOutRun {
        let frame = self.frame;
        let run = self.runs.entry(key).or_insert_with_key(|key| {
            let (painter, baseline) = lay_out(key);
            LaidOutRun {
                painter,
                baseline,
                drawn_in: frame,
            }
        });
        run.drawn_in = frame;
        run
    }

    /// Ends the frame: lets go of every run it did not draw.
    pub(crate) fn end_frame(&mut self) {
        let frame = self.frame;
        self.runs.retain(|_key, run| {
            let kept = run.drawn_in == frame;
            if !kept {
                run.painter.dispose();
            }
            kept
        });
    }

    /// Lets go of every run, as the view that draws them leaves.
    pub(crate) fn clear(&mut self) {
        for (_key, mut run) in self.runs.drain() {
            run.painter.dispose();
        }
    }
}
