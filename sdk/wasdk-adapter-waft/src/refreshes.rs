//! A refresh's frames gathered from the views' streams. WaOS hands every view a refresh serves the
//! same frame, each on its own stream after that view's earlier events, naming every view it
//! serves; Waft draws a refresh of several views as one call. So a refresh is drawn once the
//! frame of every view it names that the program still has has arrived, and a view that goes
//! meanwhile is not waited for.
//!
//! The invariant: each refresh is drawn once, after the frame of every view it draws has come off
//! that view's stream, and never waits for a view the program no longer has.

use std::time::Duration;

use waft_embedder::ViewId;
use wasdk::views;

/// The refreshes some of whose views' frames have arrived and some not yet.
#[derive(Default)]
pub(crate) struct Refreshes {
    gathering: Vec<Gathering>,
}

/// One refresh being gathered.
struct Gathering {
    /// When the refresh began, as the frame stamps it, and the views it names: what tells one
    /// refresh's frames from another's.
    time: u64,
    named: Vec<u64>,
    /// When the refresh's picture is expected on screen, on the program's clock.
    display_time: u64,
    /// The views it draws, in the order the frame names them: those the program had when its
    /// first frame arrived, less any gone since.
    views: Vec<ViewId>,
    /// Those of `views` whose frame has not arrived yet.
    waiting: Vec<ViewId>,
}

/// A refresh whose every view has had its frame: the client draws `views` for the refresh that
/// `began`, their picture expected on screen at `display_time`, on the program's clock.
#[derive(Debug, PartialEq)]
pub(crate) struct Refresh {
    pub(crate) views: Vec<ViewId>,
    pub(crate) began: Duration,
    pub(crate) display_time: u64,
}

impl Refreshes {
    /// `frame` arrived on the stream of `view`, which the program has; `has_view` says which of
    /// the others it has. Answers the refresh once the frames of all its views have arrived.
    pub(crate) fn frame_arrived(
        &mut self,
        view: ViewId,
        frame: &views::Frame,
        has_view: impl Fn(ViewId) -> bool,
    ) -> Option<Refresh> {
        let index = match self.gathering_of(frame) {
            Some(index) => index,
            None => self.begin(frame, view, has_view),
        };
        self.gathering[index]
            .waiting
            .retain(|waiting| *waiting != view);
        self.take_if_complete(index)
    }

    /// The program no longer has `view`: no refresh waits for its frame. Answers the refreshes
    /// that waited for it alone.
    pub(crate) fn view_removed(&mut self, view: ViewId) -> Vec<Refresh> {
        for gathering in &mut self.gathering {
            gathering.views.retain(|drawn| *drawn != view);
            gathering.waiting.retain(|waiting| *waiting != view);
        }
        let (complete, gathering) = std::mem::take(&mut self.gathering)
            .into_iter()
            .partition(Gathering::is_complete);
        self.gathering = gathering;
        complete
            .into_iter()
            .map(Gathering::into_refresh)
            .filter(|refresh| !refresh.views.is_empty())
            .collect()
    }

    fn gathering_of(&self, frame: &views::Frame) -> Option<usize> {
        self.gathering
            .iter()
            .position(|gathering| gathering.time == frame.time && gathering.named == frame.views)
    }

    /// Begins gathering the refresh `frame` is of, whose first frame arrived on `view`'s stream:
    /// it draws the views it names that the program has.
    fn begin(
        &mut self,
        frame: &views::Frame,
        view: ViewId,
        has_view: impl Fn(ViewId) -> bool,
    ) -> usize {
        let views: Vec<ViewId> = frame
            .views
            .iter()
            .map(|&id| ViewId(id))
            .filter(|&named| named == view || has_view(named))
            .collect();
        self.gathering.push(Gathering {
            time: frame.time,
            named: frame.views.clone(),
            display_time: frame.display_time,
            waiting: views.clone(),
            views,
        });
        self.gathering.len() - 1
    }

    fn take_if_complete(&mut self, index: usize) -> Option<Refresh> {
        if !self.gathering[index].is_complete() {
            return None;
        }
        Some(self.gathering.remove(index).into_refresh())
    }
}

impl Gathering {
    fn is_complete(&self) -> bool {
        self.waiting.is_empty()
    }

    fn into_refresh(self) -> Refresh {
        Refresh {
            views: self.views,
            began: Duration::from_nanos(self.time),
            display_time: self.display_time,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(time: u64, views: &[u64]) -> views::Frame {
        views::Frame {
            time,
            display_time: time + 16,
            views: views.to_vec(),
        }
    }

    fn every_view(_view: ViewId) -> bool {
        true
    }

    #[test]
    fn a_refresh_of_one_view_is_drawn_at_its_frame() {
        let mut refreshes = Refreshes::default();
        let drawn = refreshes.frame_arrived(ViewId(1), &frame(5, &[1]), every_view);
        assert_eq!(
            drawn,
            Some(Refresh {
                views: vec![ViewId(1)],
                began: Duration::from_nanos(5),
                display_time: 21,
            })
        );
    }

    #[test]
    fn a_refresh_of_two_views_is_drawn_once_both_frames_have_arrived() {
        let mut refreshes = Refreshes::default();
        let both = frame(5, &[1, 2]);
        assert_eq!(refreshes.frame_arrived(ViewId(2), &both, every_view), None);
        let drawn = refreshes.frame_arrived(ViewId(1), &both, every_view);
        assert_eq!(
            drawn.map(|refresh| refresh.views),
            Some(vec![ViewId(1), ViewId(2)]),
            "once, in the order the frame names them"
        );
    }

    #[test]
    fn a_view_gone_before_its_frame_is_not_waited_for() {
        let mut refreshes = Refreshes::default();
        let both = frame(5, &[1, 2]);
        assert_eq!(refreshes.frame_arrived(ViewId(1), &both, every_view), None);
        let drawn = refreshes.view_removed(ViewId(2));
        assert_eq!(
            drawn
                .into_iter()
                .map(|refresh| refresh.views)
                .collect::<Vec<_>>(),
            [vec![ViewId(1)]]
        );
    }

    #[test]
    fn a_view_the_program_no_longer_has_is_left_out_from_the_start() {
        let mut refreshes = Refreshes::default();
        let drawn =
            refreshes.frame_arrived(ViewId(1), &frame(5, &[1, 2]), |view| view == ViewId(1));
        assert_eq!(drawn.map(|refresh| refresh.views), Some(vec![ViewId(1)]));
    }

    #[test]
    fn two_refreshes_are_gathered_apart() {
        let mut refreshes = Refreshes::default();
        let (first, second) = (frame(5, &[1, 2]), frame(9, &[3]));
        assert_eq!(refreshes.frame_arrived(ViewId(1), &first, every_view), None);
        let other = refreshes.frame_arrived(ViewId(3), &second, every_view);
        assert_eq!(other.map(|refresh| refresh.views), Some(vec![ViewId(3)]));
        let drawn = refreshes.frame_arrived(ViewId(2), &first, every_view);
        assert_eq!(
            drawn.map(|refresh| refresh.began),
            Some(Duration::from_nanos(5))
        );
    }
}
