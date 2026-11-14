//! Where the deck draws every tab, as one pure function of its animated values: the strip of
//! pages the bar swipes through, the overview's grid of cards, and a page in flight between the
//! two.
//!
//! A page is laid out once, at the page's size, in its place on the strip, and this says where it
//! is drawn: moved and scaled uniformly from where it is laid out, and clipped to a rectangle,
//! with rounded corners. Nothing is laid out again or re-parented between the strip and the grid,
//! so the page under the finger is the card it shrinks into, and a program is never told of a size
//! it does not have. A program's picture is a system
//! layer placed by a position, one scale and a rectangular crop, which is all this asks of it.
//!
//! Each tab is placed on its own, by its own values, as Safari places its pages. A tab's flight
//! says how far it is from its page on the strip to its card in the grid: 0 is the page, where
//! the strip puts it, and 1 the card, in its slot. A card in the grid stands still: nothing but
//! the grid's own scroll and the card's own slot and throw moves it, and it has one size. Between
//! the two, a page in flight is on the straight line from its page to its card, in size and in
//! place, and a finger that drags it carries it off that line to stay under the finger; a little
//! past its card it only shrinks, a little, where it is. No tab's place depends on another's.
//!
//! The invariant: a tab's placement is continuous in every one of its inputs. A jump here is a
//! jump on screen, and nothing downstream smooths it.

use waft::{Offset, Rect, Size};

use super::tab_page::BAR_HEIGHT;

/// Space between two pages on the strip, seen while swiping between tabs.
pub(crate) const PAGE_GAP: f64 = 24.0;
/// The overview's space at either side of the grid.
const GRID_MARGIN: f64 = 18.0;
/// Space between the overview's columns.
const COLUMN_SPACING: f64 = 16.0;
/// The width a card is given at least, which decides how many columns a view has beyond two.
const COLUMN_WIDTH: f64 = 200.0;
/// A card's height over its width, where the page is at least as tall: taller than a square,
/// shorter than the page, so the card shows the middle of its page.
const CARD_ASPECT: f64 = 1.36;
/// Room under each card for its title.
pub(crate) const TITLE_SPACE: f64 = 30.0;
/// Space between the overview's rows, below a title.
const ROW_SPACING: f64 = 12.0;
/// Space above the grid, below what the system covers at the top.
const GRID_TOP: f64 = 14.0;
/// A card's corner radius; a page on the strip has square corners.
pub(crate) const CARD_RADIUS: f64 = 14.0;
/// How far through its flight a page is before its card's title starts to show.
const CHROME_FROM: f64 = 0.6;
/// How quickly the page past the last tab fades as the overview opens: gone at a quarter open.
const PAST_THE_LAST_FADE: f64 = 4.0;
/// How far a finger travels upward to take a page all the way to its card's size, as a share of
/// the page's height.
const FULL_FLIGHT_TRAVEL: f64 = 0.5;
/// The furthest a flight goes past the card, which a finger reaches against resistance.
pub(crate) const FURTHEST_FLIGHT: f64 = 1.5;
/// How much smaller than its card a page is for each whole flight it is past it: at the furthest,
/// 0.85 of its card's size.
const SHRINK_PAST_THE_CARD: f64 = 0.3;

/// Where the deck's own animated values are at one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DeckPosition {
    /// Where the strip of pages is, in pages: 1.5 is halfway between the second and third.
    pub(crate) strip: f64,
    /// How far the overview's grid is scrolled.
    pub(crate) grid_scroll: f64,
}

/// Where one tab's animated values are at one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CardPosition {
    /// The grid slot of the tab's card, fractional while the card moves to a new one.
    pub(crate) slot: f64,
    /// How far the card has been dragged or thrown sideways.
    pub(crate) throw_offset: f64,
    /// How far the tab is from its page on the strip, 0, to its card in the grid, 1; past 1 the
    /// page is a little smaller than its card, which is what a drag past the card shows. Below 0
    /// is drawn as 0, and past [`FURTHEST_FLIGHT`] as that.
    pub(crate) flight: f64,
    /// How far the tab is carried on screen from where its flight puts it: what a finger dragging
    /// its page does, so the page stays under it.
    pub(crate) carried: Offset,
}

impl CardPosition {
    /// A tab that is its page on the strip, its card's slot being `slot`.
    pub(crate) fn page(slot: usize) -> CardPosition {
        CardPosition {
            slot: slot as f64,
            throw_offset: 0.0,
            flight: 0.0,
            carried: Offset::ZERO,
        }
    }

    /// A tab that is its card, standing in `slot`.
    pub(crate) fn card(slot: usize) -> CardPosition {
        CardPosition {
            flight: 1.0,
            ..CardPosition::page(slot)
        }
    }
}

/// The hold a finger has of a page it drags: the point of the page above it, and how far beneath
/// what shows of the page it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PageHold {
    /// How far across the page the finger is, as a share of the page's width as it is drawn: 0 at
    /// its left edge and 1 at its right.
    pub(crate) across: f64,
    /// How far below the bottom edge of what shows of the page the finger is; less than nothing
    /// for a finger on the page.
    pub(crate) beneath: f64,
}

/// Where one tab is laid out and where it is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TabPlacement {
    /// Where the tab's page is laid out, at the page's size: its place on the strip, where its
    /// flight lands when it heads to its page.
    pub(crate) laid_out: Rect,
    /// Where the tab's page is drawn: its layout moved and scaled uniformly into this.
    pub(crate) page: Rect,
    /// The part of the page that shows.
    pub(crate) clip: Rect,
    pub(crate) corner_radius: f64,
    /// How opaque the card's title and close button are.
    pub(crate) chrome_opacity: f64,
    /// How opaque the page is: 1 for every tab, and less only for the page past the last tab,
    /// which is never a program's.
    pub(crate) opacity: f64,
}

impl TabPlacement {
    /// How large the page is drawn, of its laid out size.
    pub(crate) fn scale(&self, layout: &DeckLayout) -> f64 {
        self.page.width() / layout.page_box().width()
    }

    /// How far the page is drawn from where it is laid out, by its top left corner.
    pub(crate) fn drawn_offset(&self) -> Offset {
        self.page.top_left() - self.laid_out.top_left()
    }

    /// The part of the page that shows, in the page's own points as it is laid out: the clip,
    /// taken back from where the page is drawn onto the page.
    pub(crate) fn clip_on_the_page(&self, layout: &DeckLayout) -> Rect {
        let scale = self.scale(layout);
        let on_the_page = |edge: f64, page_edge: f64| (edge - page_edge) / scale;
        Rect::from_ltrb(
            on_the_page(self.clip.left, self.page.left),
            on_the_page(self.clip.top, self.page.top),
            on_the_page(self.clip.right, self.page.left),
            on_the_page(self.clip.bottom, self.page.top),
        )
    }

    /// The radius of the clip's corners in the page's own points as it is laid out.
    pub(crate) fn corner_radius_on_the_page(&self, layout: &DeckLayout) -> f64 {
        self.corner_radius / self.scale(layout)
    }

    /// Whether the clip cuts anything of the page: the page reaches past it, or its corners are
    /// round. A page that fills its clip with square corners, as every page on the strip does, is
    /// drawn whole, and whoever draws it has nothing to clip.
    pub(crate) fn clips(&self) -> bool {
        self.corner_radius > 0.0
            || self.page.left < self.clip.left
            || self.page.top < self.clip.top
            || self.page.right > self.clip.right
            || self.page.bottom > self.clip.bottom
    }

    /// Whether any of the tab can be seen in `layout`'s view.
    pub(crate) fn is_visible(&self, layout: &DeckLayout) -> bool {
        self.opacity > 0.0 && self.clip.overlaps(Offset::ZERO & layout.view)
    }
}

/// The view the deck fills, and what the system and the bar take of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DeckLayout {
    view: Size,
    /// What the system covers at the view's top.
    top_padding: f64,
    /// What the bar sits clear of at the view's bottom, as `tab_page::beneath_the_bar` says.
    beneath_the_bar: f64,
}

impl DeckLayout {
    pub(crate) fn new(view: Size, top_padding: f64, beneath_the_bar: f64) -> DeckLayout {
        DeckLayout {
            view,
            top_padding,
            beneath_the_bar,
        }
    }

    /// How far up from the view's bottom the bar reaches, and the overview's toolbar in its place.
    pub(crate) fn bar_extent(&self) -> f64 {
        BAR_HEIGHT + self.beneath_the_bar
    }

    /// What the bar sits clear of at the view's bottom.
    pub(crate) fn beneath_the_bar(&self) -> f64 {
        self.beneath_the_bar
    }

    /// The box a page fills at rest: the view above the bar.
    pub(crate) fn page_box(&self) -> Rect {
        let height = (self.view.height() - self.bar_extent()).max(0.0);
        Rect::from_ltwh(0.0, 0.0, self.view.width(), height)
    }

    /// How far apart two neighbouring pages are on the strip.
    pub(crate) fn page_spacing(&self) -> f64 {
        self.view.width() + PAGE_GAP
    }

    /// How far sideways from its place in the bar the pill of the page at `index` is, with the
    /// strip at `strip`: as far as its page is from the page's box, so nothing while the strip
    /// rests on that page.
    pub(crate) fn pill_offset(&self, index: usize, strip: f64) -> f64 {
        (index as f64 - strip) * self.page_spacing()
    }

    /// How many columns the overview has: two on a phone held upright, more in a wider view.
    pub(crate) fn columns(&self) -> usize {
        ((self.view.width() / COLUMN_WIDTH).floor() as usize).max(2)
    }

    pub(crate) fn card_size(&self) -> Size {
        let columns = self.columns() as f64;
        let width =
            (self.view.width() - 2.0 * GRID_MARGIN - (columns - 1.0) * COLUMN_SPACING) / columns;
        let page = self.page_box();
        let page_aspect = page.height() / page.width().max(1.0);
        Size::new(
            width.max(1.0),
            width.max(1.0) * CARD_ASPECT.min(page_aspect),
        )
    }

    fn row_pitch(&self) -> f64 {
        self.card_size().height() + TITLE_SPACE + ROW_SPACING
    }

    /// A whole slot's card, in the grid before it is scrolled.
    fn slot_rect(&self, slot: usize) -> Rect {
        let card = self.card_size();
        let column = (slot % self.columns()) as f64;
        let row = (slot / self.columns()) as f64;
        Rect::from_ltwh(
            GRID_MARGIN + column * (card.width() + COLUMN_SPACING),
            self.top_padding + GRID_TOP + row * self.row_pitch(),
            card.width(),
            card.height(),
        )
    }

    /// The card at `slot` in the grid, scrolled by `grid_scroll`. Between two slots the card is
    /// on the straight line between them.
    pub(crate) fn card_rect(&self, slot: f64, grid_scroll: f64) -> Rect {
        let slot = slot.max(0.0);
        let before = self.slot_rect(slot.floor() as usize);
        let after = self.slot_rect(slot.ceil() as usize);
        lerp_rect(before, after, slot.fract()).translate(0.0, -grid_scroll)
    }

    /// The page a card shows the middle of: as wide as the card, in the page's shape, centred on
    /// it.
    fn page_of_card(&self, card: Rect) -> Rect {
        let page = self.page_box();
        let height = card.width() * page.height() / page.width().max(1.0);
        Rect::from_center(card.center(), card.width(), height)
    }

    /// How far the grid of `card_count` cards can scroll.
    pub(crate) fn max_grid_scroll(&self, card_count: usize) -> f64 {
        let rows = card_count.div_ceil(self.columns()) as f64;
        let content = self.top_padding + GRID_TOP + rows * self.row_pitch() + self.bar_extent();
        (content - self.view.height()).max(0.0)
    }

    /// The scroll that puts `slot`'s card in the middle of the space between what the system
    /// covers at the top and the toolbar, as far as a grid of `card_count` cards can scroll.
    pub(crate) fn grid_scroll_showing(&self, slot: usize, card_count: usize) -> f64 {
        let visible_middle = (self.top_padding + self.view.height() - self.bar_extent()) / 2.0;
        let scroll = self.slot_rect(slot).center().dy() - visible_middle;
        scroll.clamp(0.0, self.max_grid_scroll(card_count))
    }

    /// How far a finger travels upward to take a page from the strip all the way to its card's
    /// size: the flight a finger's travel makes is the travel's share of this.
    pub(crate) fn full_flight_travel(&self) -> f64 {
        (self.page_box().height() * FULL_FLIGHT_TRAVEL).max(1.0)
    }

    /// Where the tab at `index` is drawn, its own values being `card`.
    pub(crate) fn place_tab(
        &self,
        index: usize,
        card: &CardPosition,
        deck: &DeckPosition,
    ) -> TabPlacement {
        let flight = card.flight.clamp(0.0, FURTHEST_FLIGHT);
        let towards_card = flight.min(1.0);
        let on_strip = self
            .page_box()
            .translate(self.pill_offset(index, deck.strip), 0.0);
        let in_grid = self
            .card_rect(card.slot, deck.grid_scroll)
            .translate(card.throw_offset, 0.0);
        let page = lerp_rect(on_strip, self.page_of_card(in_grid), towards_card);
        let clip = lerp_rect(on_strip, in_grid, towards_card);
        let size = 1.0 - SHRINK_PAST_THE_CARD * (flight - 1.0).max(0.0);
        let about = clip.center();
        TabPlacement {
            laid_out: on_strip,
            page: scaled_about(page, about, size).shift(card.carried),
            clip: scaled_about(clip, about, size).shift(card.carried),
            corner_radius: CARD_RADIUS * towards_card,
            chrome_opacity: ((towards_card - CHROME_FROM) / (1.0 - CHROME_FROM)).clamp(0.0, 1.0),
            opacity: 1.0,
        }
    }

    /// Where each tab is drawn, in the tabs' order, `cards` being their values in that order.
    pub(crate) fn place_tabs(
        &self,
        cards: &[CardPosition],
        deck: &DeckPosition,
    ) -> Vec<TabPlacement> {
        cards
            .iter()
            .enumerate()
            .map(|(index, card)| self.place_tab(index, card, deck))
            .collect()
    }

    /// Where the page past the last of `tab_count` tabs is drawn, the home's while no home is
    /// open: on the strip alone, since it has no card, fading as the overview opens, `openness`
    /// being how far it has.
    pub(crate) fn place_page_past_the_last(
        &self,
        tab_count: usize,
        deck: &DeckPosition,
        openness: f64,
    ) -> TabPlacement {
        let page = self
            .page_box()
            .translate(self.pill_offset(tab_count, deck.strip), 0.0);
        TabPlacement {
            laid_out: page,
            page,
            clip: page,
            corner_radius: 0.0,
            chrome_opacity: 0.0,
            opacity: (1.0 - PAST_THE_LAST_FADE * openness.max(0.0)).clamp(0.0, 1.0),
        }
    }

    /// The hold a finger at `finger` has of the tab at `index` as it is drawn now.
    pub(crate) fn hold_of(
        &self,
        index: usize,
        finger: Offset,
        card: &CardPosition,
        deck: &DeckPosition,
    ) -> PageHold {
        let placed = self.place_tab(index, card, deck);
        PageHold {
            across: (finger.dx() - placed.page.left) / placed.page.width().max(f64::MIN_POSITIVE),
            beneath: finger.dy() - placed.clip.bottom,
        }
    }

    /// How far the tab at `index` is carried from where its flight puts it when a finger at
    /// `finger` holds it by `hold`, everything else as `card` and `deck` say: what keeps a page
    /// under the finger that drags it, both ways, but for a finger that goes lower than it came:
    /// what shows of a page never reaches below the page's box.
    pub(crate) fn carried_by(
        &self,
        index: usize,
        hold: PageHold,
        finger: Offset,
        card: &CardPosition,
        deck: &DeckPosition,
    ) -> Offset {
        let uncarried = CardPosition {
            carried: Offset::ZERO,
            ..*card
        };
        let placed = self.place_tab(index, &uncarried, deck);
        let bottom = (finger.dy() - hold.beneath).min(self.page_box().bottom);
        Offset::new(
            finger.dx() - (placed.page.left + hold.across * placed.page.width()),
            bottom - placed.clip.bottom,
        )
    }
}

/// `rect` at `size` of its own size, the point `about` staying where it is.
fn scaled_about(rect: Rect, about: Offset, size: f64) -> Rect {
    if size == 1.0 {
        return rect;
    }
    let scaled = |edge: f64, centre: f64| centre + (edge - centre) * size;
    Rect::from_ltrb(
        scaled(rect.left, about.dx()),
        scaled(rect.top, about.dy()),
        scaled(rect.right, about.dx()),
        scaled(rect.bottom, about.dy()),
    )
}

/// The rect `t` of the way from `a` to `b`.
fn lerp_rect(a: Rect, b: Rect, t: f64) -> Rect {
    let lerp = |from: f64, to: f64| from + (to - from) * t;
    Rect::from_ltrb(
        lerp(a.left, b.left),
        lerp(a.top, b.top),
        lerp(a.right, b.right),
        lerp(a.bottom, b.bottom),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A phone held upright: 390 by 844 points, 47 covered at the top and 34 at the bottom.
    fn phone() -> DeckLayout {
        DeckLayout::new(Size::new(390.0, 844.0), 47.0, 34.0)
    }

    fn at_rest_on(index: usize) -> DeckPosition {
        DeckPosition {
            strip: index as f64,
            grid_scroll: 0.0,
        }
    }

    /// The tab whose card's slot is `slot`, `flight` of the way from its page to its card.
    fn in_flight(slot: usize, flight: f64) -> CardPosition {
        CardPosition {
            flight,
            ..CardPosition::page(slot)
        }
    }

    fn assert_rect_near(actual: Rect, expected: Rect) {
        assert!(
            rect_distance(actual, expected) < 1e-6,
            "{actual:?} != {expected:?}"
        );
    }

    /// The furthest any edge of `a` is from the same edge of `b`.
    fn rect_distance(a: Rect, b: Rect) -> f64 {
        [
            a.left - b.left,
            a.top - b.top,
            a.right - b.right,
            a.bottom - b.bottom,
        ]
        .into_iter()
        .fold(0.0, |furthest: f64, edge| furthest.max(edge.abs()))
    }

    #[test]
    fn the_page_box_is_the_view_above_the_bar() {
        let layout = phone();
        assert_rect_near(
            layout.page_box(),
            Rect::from_ltwh(0.0, 0.0, 390.0, 844.0 - BAR_HEIGHT - 34.0),
        );
    }

    #[test]
    fn a_page_on_the_strip_fills_the_pages_box_where_the_strip_rests_on_it() {
        let layout = phone();
        let placed = layout.place_tab(1, &CardPosition::page(1), &at_rest_on(1));
        assert_eq!(placed.page, layout.page_box());
        assert_eq!(placed.clip, layout.page_box());
        assert_eq!(placed.corner_radius, 0.0);
        assert_eq!(placed.scale(&layout), 1.0);
        assert!(!placed.clips(), "drawn whole");
        for other in [0, 2, 3] {
            let placed = layout.place_tab(other, &CardPosition::page(other), &at_rest_on(1));
            assert!(!placed.is_visible(&layout), "tab {other}");
        }
    }

    #[test]
    fn the_strip_follows_its_position_one_to_one_with_a_gap_between_pages() {
        let layout = phone();
        for strip in [0.0, 0.25, 0.5, 0.99, 1.0, 1.7, -0.2] {
            let deck = DeckPosition {
                strip,
                ..at_rest_on(0)
            };
            for index in 0..3 {
                let placed = layout.place_tab(index, &CardPosition::page(index), &deck);
                let expected = layout
                    .page_box()
                    .translate((index as f64 - strip) * layout.page_spacing(), 0.0);
                assert_rect_near(placed.page, expected);
                assert_rect_near(placed.clip, expected);
                assert!(!placed.clips(), "a page on the strip is whole");
                assert_eq!(
                    placed.is_visible(&layout),
                    (index as f64 - strip).abs() < 0.94
                );
            }
        }
        let halfway = DeckPosition {
            strip: 0.5,
            ..at_rest_on(0)
        };
        let (left, right) = (
            layout.place_tab(0, &CardPosition::page(0), &halfway),
            layout.place_tab(1, &CardPosition::page(1), &halfway),
        );
        assert!((right.page.left - left.page.right - PAGE_GAP).abs() < 1e-9);
    }

    #[test]
    fn a_card_stands_in_its_slot_whatever_the_strip_does() {
        let layout = phone();
        for slot in 0..5 {
            for strip in [0.0, 1.3, 4.0] {
                let deck = DeckPosition {
                    strip,
                    grid_scroll: 30.0,
                };
                let placed = layout.place_tab(slot, &CardPosition::card(slot), &deck);
                assert_rect_near(placed.clip, layout.card_rect(slot as f64, 30.0));
                assert_eq!(placed.corner_radius, CARD_RADIUS);
                assert_eq!(placed.chrome_opacity, 1.0);
                assert!((placed.page.center() - placed.clip.center()).distance() < 1e-6);
                assert!((placed.page.width() - placed.clip.width()).abs() < 1e-6);
                assert!(placed.clips());
            }
        }
        let first = layout.card_rect(0.0, 0.0);
        assert!((layout.card_rect(1.0, 0.0).left - first.right - COLUMN_SPACING).abs() < 1e-9);
        assert!(layout.card_rect(2.0, 0.0).top > first.bottom + TITLE_SPACE);
    }

    #[test]
    fn a_page_in_flight_is_on_the_straight_line_from_its_page_to_its_card() {
        let layout = phone();
        let deck = at_rest_on(3);
        let (page, card) = (layout.page_box(), layout.card_rect(3.0, 0.0));
        let mut previous_width = f64::INFINITY;
        for step in 0..=20 {
            let flight = f64::from(step) / 20.0;
            let placed = layout.place_tab(3, &in_flight(3, flight), &deck);
            assert_rect_near(placed.clip, lerp_rect(page, card, flight));
            assert!(placed.clip.width() < previous_width, "it only shrinks");
            previous_width = placed.clip.width();
        }
    }

    #[test]
    fn a_page_keeps_its_shape_and_holds_all_of_its_clip_wherever_it_is_drawn() {
        let layout = phone();
        let aspect = layout.page_box().height() / layout.page_box().width();
        for flight in [0.0, 0.2, 0.5, 0.9, 1.0, 1.3, 1.5, 4.0] {
            for strip in [0.0, 0.4, 1.0] {
                let deck = DeckPosition {
                    strip,
                    ..at_rest_on(0)
                };
                let card = CardPosition {
                    carried: Offset::new(40.0, -25.0),
                    throw_offset: -12.0,
                    ..in_flight(2, flight)
                };
                let placed = layout.place_tab(1, &card, &deck);
                assert!((placed.page.height() / placed.page.width() - aspect).abs() < 1e-9);
                let inside = placed.page.inflate(1e-6);
                assert!(
                    inside.left <= placed.clip.left
                        && inside.top <= placed.clip.top
                        && inside.right >= placed.clip.right
                        && inside.bottom >= placed.clip.bottom,
                    "the clip is part of the page: {placed:?}"
                );
            }
        }
    }

    #[test]
    fn a_flight_is_continuous_at_both_of_its_ends_and_drawn_as_its_end_beyond_them() {
        let layout = phone();
        let deck = DeckPosition {
            strip: 1.3,
            grid_scroll: 20.0,
        };
        let place = |flight: f64| layout.place_tab(1, &in_flight(4, flight), &deck);
        for end in [0.0, 1.0, FURTHEST_FLIGHT] {
            for beside in [end - 1e-7, end + 1e-7] {
                let moved = rect_distance(place(end).clip, place(beside).clip)
                    .max(rect_distance(place(end).page, place(beside).page));
                assert!(moved < 1e-3, "flight {end}: {moved}");
            }
        }
        assert_eq!(place(-0.05), place(0.0));
        assert_eq!(place(9.0), place(FURTHEST_FLIGHT));
    }

    #[test]
    fn past_its_card_a_page_only_shrinks_a_little_where_it_is() {
        let layout = phone();
        let deck = at_rest_on(0);
        let card = layout.place_tab(0, &CardPosition::card(2), &deck).clip;
        let furthest = layout
            .place_tab(0, &in_flight(2, FURTHEST_FLIGHT), &deck)
            .clip;
        assert!((furthest.center() - card.center()).distance() < 1e-9);
        assert!((furthest.width() / card.width() - 0.85).abs() < 1e-9);
        assert!((furthest.height() / card.height() - 0.85).abs() < 1e-9);
    }

    #[test]
    fn a_page_is_laid_out_on_the_strip_and_drawn_moved_and_scaled_from_there() {
        let layout = phone();
        let deck = DeckPosition {
            strip: 0.7,
            grid_scroll: 30.0,
        };
        let on_strip = layout
            .page_box()
            .translate(layout.pill_offset(1, deck.strip), 0.0);
        for flight in [0.0, 0.3, 1.0, 1.4] {
            let card = CardPosition {
                carried: Offset::new(-15.0, 22.0),
                ..in_flight(3, flight)
            };
            let placed = layout.place_tab(1, &card, &deck);
            assert_rect_near(placed.laid_out, on_strip);
            let scale = placed.scale(&layout);
            let drawn = |on_the_page: Rect| {
                Rect::from_ltrb(
                    on_the_page.left * scale,
                    on_the_page.top * scale,
                    on_the_page.right * scale,
                    on_the_page.bottom * scale,
                )
                .shift(placed.laid_out.top_left() + placed.drawn_offset())
            };
            assert_rect_near(drawn(Offset::ZERO & layout.page_box().size()), placed.page);
            assert_rect_near(drawn(placed.clip_on_the_page(&layout)), placed.clip);
            assert!(
                (placed.corner_radius_on_the_page(&layout) * scale - placed.corner_radius).abs()
                    < 1e-9
            );
        }
    }

    #[test]
    fn a_page_is_clipped_only_once_it_leaves_the_strip() {
        let layout = phone();
        let deck = DeckPosition {
            strip: 1.4,
            ..at_rest_on(1)
        };
        let carried = CardPosition {
            carried: Offset::new(30.0, 0.0),
            ..CardPosition::page(1)
        };
        assert!(!layout.place_tab(1, &carried, &deck).clips());
        assert!(!layout.place_tab(1, &in_flight(1, -0.02), &deck).clips());
        assert!(
            layout.place_tab(1, &in_flight(1, 1e-6), &deck).clips(),
            "its corners round from the first of its flight"
        );
        assert!(
            !layout
                .place_page_past_the_last(3, &at_rest_on(2), 0.0)
                .clips()
        );
    }

    #[test]
    fn a_pill_is_in_its_place_exactly_while_the_strip_rests_on_its_page() {
        let layout = phone();
        assert_eq!(layout.pill_offset(2, 2.0), 0.0);
        assert_eq!(layout.pill_offset(2, 1.5), 0.5 * layout.page_spacing());
        assert!(layout.pill_offset(2, 2.0 + 1e-9) < 0.0);
    }

    #[test]
    fn the_grid_scroll_and_a_cards_slot_and_throw_move_it_continuously() {
        let layout = phone();
        let mut moved = CardPosition::card(3);
        moved.slot = 2.5;
        let between = lerp_rect(layout.card_rect(2.0, 0.0), layout.card_rect(3.0, 0.0), 0.5);
        assert_rect_near(layout.place_tab(3, &moved, &at_rest_on(0)).clip, between);
        moved.throw_offset = -120.0;
        let thrown = layout.place_tab(3, &moved, &at_rest_on(0));
        assert_rect_near(thrown.clip, between.translate(-120.0, 0.0));
        let scrolled = DeckPosition {
            grid_scroll: 55.0,
            ..at_rest_on(0)
        };
        assert_rect_near(
            layout.place_tab(3, &moved, &scrolled).clip,
            between.translate(-120.0, -55.0),
        );
    }

    #[test]
    fn a_tab_carried_keeps_its_size_and_moves_as_far_as_it_is_carried() {
        let layout = phone();
        let deck = at_rest_on(1);
        for flight in [0.0, 0.3, 1.0, 1.4] {
            let uncarried = in_flight(1, flight);
            let carried = CardPosition {
                carried: Offset::new(-75.0, 20.0),
                ..uncarried
            };
            let (before, after) = (
                layout.place_tab(1, &uncarried, &deck),
                layout.place_tab(1, &carried, &deck),
            );
            assert_rect_near(after.page, before.page.translate(-75.0, 20.0));
            assert_rect_near(after.clip, before.clip.translate(-75.0, 20.0));
        }
    }

    /// The finger holds one point of the page: wherever the finger is and however far the page
    /// has flown, the tab carried as answered has that point of its page above the finger, and
    /// its bottom edge as far above the finger as when it was taken hold of.
    #[test]
    fn a_tab_carried_by_a_finger_stays_under_it_both_ways() {
        let layout = phone();
        let deck = DeckPosition {
            strip: 2.8,
            grid_scroll: 40.0,
        };
        let mut card = CardPosition {
            carried: Offset::new(12.0, -7.0),
            ..in_flight(3, 0.1)
        };
        let grabbed_at = Offset::new(150.0, 700.0);
        let hold = layout.hold_of(3, grabbed_at, &card, &deck);
        assert_eq!(
            layout.carried_by(3, hold, grabbed_at, &card, &deck),
            card.carried
        );
        let fingers = [(0.1, 150.0, 700.0), (0.4, 90.0, 560.0), (1.0, 310.0, 380.0)];
        for (flight, x, y) in fingers.into_iter().chain([(1.4, 20.0, 150.0)]) {
            card.flight = flight;
            let finger = Offset::new(x, y);
            card.carried = layout.carried_by(3, hold, finger, &card, &deck);
            let placed = layout.place_tab(3, &card, &deck);
            let above = placed.page.left + hold.across * placed.page.width();
            assert!(
                (above - x).abs() < 1e-9,
                "flight {flight}: {placed:?} under {x}"
            );
            assert!((y - hold.beneath - placed.clip.bottom).abs() < 1e-9);
        }
    }

    #[test]
    fn a_finger_that_goes_lower_than_it_came_carries_the_page_no_lower_than_its_box() {
        let layout = phone();
        let deck = at_rest_on(0);
        let card = in_flight(0, 0.0);
        let grabbed_at = Offset::new(150.0, 780.0);
        let hold = layout.hold_of(0, grabbed_at, &card, &deck);
        let lower = Offset::new(200.0, 830.0);
        assert_eq!(
            layout.carried_by(0, hold, lower, &card, &deck),
            Offset::new(50.0, 0.0)
        );
    }

    #[test]
    fn a_fingers_upward_travel_of_half_the_page_is_a_whole_flight() {
        let layout = phone();
        assert_eq!(
            layout.full_flight_travel(),
            layout.page_box().height() / 2.0
        );
    }

    #[test]
    fn cards_fill_the_grid_row_by_row_as_tabs_come_and_go() {
        let layout = phone();
        assert_eq!(layout.columns(), 2);
        let first = layout.card_rect(0.0, 0.0);
        assert_eq!(first.left, GRID_MARGIN);
        assert_eq!(first.top, 47.0 + GRID_TOP);
        assert_eq!(layout.card_rect(1.0, 0.0).top, first.top);
        assert_eq!(layout.card_rect(2.0, 0.0).left, first.left);
        assert_eq!(
            layout.card_rect(2.0, 0.0).top,
            first.bottom + TITLE_SPACE + ROW_SPACING
        );
        assert!((first.height() / first.width() - CARD_ASPECT).abs() < 1e-9);
    }

    #[test]
    fn the_grid_scrolls_only_when_the_cards_do_not_fit() {
        let layout = phone();
        assert_eq!(layout.max_grid_scroll(2), 0.0);
        assert_eq!(layout.max_grid_scroll(4), 0.0);
        assert!(layout.max_grid_scroll(10) > 0.0);
        let scroll = layout.grid_scroll_showing(9, 10);
        assert!(scroll > 0.0 && scroll <= layout.max_grid_scroll(10));
        assert_eq!(layout.grid_scroll_showing(0, 10), 0.0);
    }

    #[test]
    fn the_page_past_the_last_tab_is_on_the_strip_and_fades_as_the_overview_opens() {
        let layout = phone();
        let resting = at_rest_on(1);
        assert!(
            !layout
                .place_page_past_the_last(2, &resting, 0.0)
                .is_visible(&layout)
        );
        let dragged = DeckPosition {
            strip: 1.6,
            ..resting
        };
        let placed = layout.place_page_past_the_last(2, &dragged, 0.0);
        assert!(placed.is_visible(&layout) && placed.opacity == 1.0);
        assert_rect_near(
            placed.page,
            layout
                .page_box()
                .translate(0.4 * layout.page_spacing(), 0.0),
        );
        let last = layout.place_tab(1, &CardPosition::page(1), &dragged);
        assert!((placed.page.left - last.page.right - PAGE_GAP).abs() < 1e-9);
        assert_eq!(
            layout.place_page_past_the_last(2, &dragged, 0.125).opacity,
            0.5
        );
    }

    #[test]
    fn a_view_held_sideways_has_more_columns_and_cards_no_taller_than_the_page() {
        let layout = DeckLayout::new(Size::new(844.0, 390.0), 0.0, 21.0);
        assert_eq!(layout.columns(), 4);
        let card = layout.card_size();
        let page = layout.page_box();
        assert!((card.height() / card.width() - page.height() / page.width()).abs() < 1e-9);
        let deck = at_rest_on(4);
        let (first, fifth) = (
            layout.place_tab(0, &CardPosition::card(0), &deck),
            layout.place_tab(4, &CardPosition::card(4), &deck),
        );
        assert!(fifth.clip.top > first.clip.bottom);
        assert_eq!(
            layout.place_tab(4, &CardPosition::page(4), &deck).page,
            layout.page_box()
        );
    }
}
