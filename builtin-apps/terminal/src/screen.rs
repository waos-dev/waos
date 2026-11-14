//! A terminal's screen: the grid of cells a command's output makes, its scrollback, and what the
//! command asked of the terminal on the way (a title, an answer to a question). `vt100` keeps the
//! grid and reads the escape sequences; this module names the lines so that a selection stays on
//! its text while output scrolls, and answers the questions `vt100` leaves to its host.
//!
//! The invariant a later editor will break: a [`BufferPoint`]'s line is counted from the first
//! line the screen ever showed, so it names the same text until that text leaves the scrollback;
//! a row is counted from the top of what is shown now, and names other text after every scroll.

use std::num::NonZeroU16;

use crate::keys::KeyModes;

/// The lines kept above the screen once they scroll off it.
const SCROLLBACK_LINES: usize = 10_000;

/// A screen's size in character cells, at least one each way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GridSize {
    pub(crate) columns: u16,
    pub(crate) rows: u16,
}

impl GridSize {
    /// The size nearest `columns` by `rows` that a screen can have.
    pub(crate) fn new(columns: u16, rows: u16) -> GridSize {
        GridSize {
            columns: columns.max(1),
            rows: rows.max(1),
        }
    }

    fn non_zero(self) -> (NonZeroU16, NonZeroU16) {
        (
            NonZeroU16::new(self.rows).unwrap_or(NonZeroU16::MIN),
            NonZeroU16::new(self.columns).unwrap_or(NonZeroU16::MIN),
        )
    }
}

/// A cell of the buffer: its line, counted from the first line the screen ever showed, and its
/// column. Ordered as text reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct BufferPoint {
    pub(crate) line: u64,
    pub(crate) column: u16,
}

/// How a program asked to hear of the mouse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MouseReporting {
    pub(crate) mode: vt100::MouseProtocolMode,
    pub(crate) encoding: vt100::MouseProtocolEncoding,
}

impl MouseReporting {
    /// Whether the program asked for the mouse at all.
    pub(crate) fn is_on(self) -> bool {
        self.mode != vt100::MouseProtocolMode::None
    }
}

/// What a command's output asked of the terminal besides drawing.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Asked {
    /// Bytes to type back: the answers to the command's questions about the terminal.
    pub(crate) replies: Vec<u8>,
    /// Whether the title changed.
    pub(crate) title_changed: bool,
    /// Whether the screen changed between the main one and the alternate one, which full-screen
    /// programs draw on.
    pub(crate) screen_switched: bool,
}

/// The grid, its scrollback and its title.
pub(crate) struct Screen {
    parser: vt100::Parser<Requests>,
}

impl Screen {
    pub(crate) fn new(size: GridSize) -> Screen {
        let (rows, columns) = size.non_zero();
        Screen {
            parser: vt100::Parser::new_with_callbacks(
                rows,
                columns,
                SCROLLBACK_LINES,
                Requests::default(),
            ),
        }
    }

    /// Draws `output`, what the command wrote, and answers what it asked on the way. While the
    /// scrollback is scrolled into view, the lines being read stay where they are as output
    /// arrives beneath them.
    pub(crate) fn feed(&mut self, output: &[u8]) -> Asked {
        let was_alternate = self.is_alternate();
        let read_from = (self.scroll_offset() > 0).then(|| self.first_shown_line());
        self.parser.process(output);
        if let Some(line) = read_from {
            self.scroll_to_show(line);
        }
        let requests = self.parser.callbacks_mut();
        Asked {
            replies: std::mem::take(&mut requests.replies),
            title_changed: std::mem::take(&mut requests.title_changed),
            screen_switched: was_alternate != self.is_alternate(),
        }
    }

    pub(crate) fn size(&self) -> GridSize {
        let (rows, columns) = self.parser.screen().size();
        GridSize {
            columns: columns.get(),
            rows: rows.get(),
        }
    }

    pub(crate) fn resize(&mut self, size: GridSize) {
        let (rows, columns) = size.non_zero();
        self.parser.set_size(rows, columns);
    }

    /// The title the command gave the terminal, if it gave one.
    pub(crate) fn title(&self) -> Option<&str> {
        let title = self.parser.callbacks().title.as_str();
        (!title.is_empty()).then_some(title)
    }

    /// The cell shown at `row` and `column`, the scrollback's while it is scrolled into view.
    pub(crate) fn cell(&self, row: u16, column: u16) -> Option<&vt100::Cell> {
        self.parser.screen().cell(row, column)
    }

    /// Where the cursor is among the rows shown, as a row and a column; none while the command
    /// hides it or the scrollback covers it.
    pub(crate) fn cursor(&self) -> Option<(u16, u16)> {
        let screen = self.parser.screen();
        if screen.hide_cursor() {
            return None;
        }
        let (row, column) = screen.cursor_position();
        let shown_row = usize::from(row) + screen.scrollback();
        (shown_row < usize::from(self.size().rows)).then_some((shown_row as u16, column))
    }

    pub(crate) fn cursor_style(&self) -> vt100::CursorStyle {
        self.parser.screen().cursor_style()
    }

    /// How many lines of scrollback are scrolled into view; 0 shows the screen itself.
    pub(crate) fn scroll_offset(&self) -> usize {
        self.parser.screen().scrollback()
    }

    /// How many lines the scrollback holds.
    pub(crate) fn scrollback_len(&self) -> usize {
        self.parser.screen().scrollback_len()
    }

    /// Scrolls so that `offset` lines of scrollback are in view, or as many as there are.
    pub(crate) fn set_scroll_offset(&mut self, offset: usize) {
        self.parser.screen_mut().set_scrollback(offset);
    }

    /// Scrolls by `lines`, up into the scrollback where positive.
    pub(crate) fn scroll_by(&mut self, lines: isize) {
        let offset = self.scroll_offset().saturating_add_signed(lines);
        self.set_scroll_offset(offset);
    }

    /// Forgets the scrollback.
    pub(crate) fn clear_scrollback(&mut self) {
        self.parser.screen_mut().clear_scrollback();
    }

    /// Whether the alternate screen is shown, which full-screen programs draw on and which has
    /// no scrollback.
    pub(crate) fn is_alternate(&self) -> bool {
        self.parser.screen().alternate_screen()
    }

    /// Whether the command asked for pasted text to be marked as pasted.
    pub(crate) fn brackets_paste(&self) -> bool {
        self.parser.screen().bracketed_paste()
    }

    pub(crate) fn application_cursor(&self) -> bool {
        self.parser.screen().application_cursor()
    }

    pub(crate) fn mouse_reporting(&self) -> MouseReporting {
        let screen = self.parser.screen();
        MouseReporting {
            mode: screen.mouse_protocol_mode(),
            encoding: screen.mouse_protocol_encoding(),
        }
    }

    /// How keys are sent now, where Alt sends Escape or not as `alt_sends_escape` says.
    pub(crate) fn key_modes(&self, alt_sends_escape: bool) -> KeyModes {
        KeyModes {
            application_cursor: self.application_cursor(),
            alt_sends_escape,
        }
    }

    /// The line shown in the top row.
    pub(crate) fn first_shown_line(&self) -> u64 {
        let above = self.scrollback_len() - self.scroll_offset();
        self.parser.callbacks().lines_forgotten + above as u64
    }

    /// The buffer's cell shown at `row` and `column`.
    pub(crate) fn point_at(&self, row: u16, column: u16) -> BufferPoint {
        BufferPoint {
            line: self.first_shown_line() + u64::from(row),
            column,
        }
    }

    /// The row `line` is shown in; none while it is out of view.
    pub(crate) fn row_of(&self, line: u64) -> Option<u16> {
        let row = line.checked_sub(self.first_shown_line())?;
        (row < u64::from(self.size().rows)).then_some(row as u16)
    }

    /// The first and the last column of the word at `row` and `column`: the cells around it
    /// that hold neither a blank nor a character that sets words apart. A cell that holds one
    /// is a word by itself.
    pub(crate) fn word_at(&self, row: u16, column: u16) -> (u16, u16) {
        let in_word = |column: u16| {
            self.cell(row, column)
                .is_some_and(|cell| !cell.contents().chars().all(separates_words))
        };
        if !in_word(column) {
            return (column, column);
        }
        let first = (0..column)
            .rev()
            .take_while(|&before| in_word(before))
            .last()
            .unwrap_or(column);
        let last = (column + 1..self.size().columns)
            .take_while(|&after| in_word(after))
            .last()
            .unwrap_or(column);
        (first, last)
    }

    /// The last line the buffer holds: the screen's bottom row.
    pub(crate) fn last_line(&self) -> u64 {
        let screen_top = self.parser.callbacks().lines_forgotten + self.scrollback_len() as u64;
        screen_top + u64::from(self.size().rows) - 1
    }

    /// The first line the scrollback still holds.
    pub(crate) fn first_line(&self) -> u64 {
        self.parser.callbacks().lines_forgotten
    }

    /// The text from `start` to `end`, both included, lines the terminal wrapped joined again
    /// and the others ended with a newline. Lines out of view are read by scrolling to them and
    /// back.
    pub(crate) fn text_between(&mut self, start: BufferPoint, end: BufferPoint) -> String {
        let offset = self.scroll_offset();
        let mut text = String::new();
        let mut line = start.line.max(self.parser.callbacks().lines_forgotten);
        while line <= end.line {
            self.scroll_to_show(line);
            let Some(first_row) = self.row_of(line) else {
                break;
            };
            let last_shown = self.first_shown_line() + u64::from(self.size().rows) - 1;
            let last_line = end.line.min(last_shown);
            let last_row = first_row + (last_line - line) as u16;
            let first_column = if line == start.line { start.column } else { 0 };
            let ends_here = last_line == end.line;
            let end_column = if ends_here {
                end.column.saturating_add(1)
            } else {
                self.size().columns
            };
            let screen = self.parser.screen();
            text.push_str(&screen.contents_between(first_row, first_column, last_row, end_column));
            if !ends_here && !screen.row_wrapped(last_row) {
                text.push('\n');
            }
            line = last_line + 1;
        }
        self.set_scroll_offset(offset);
        text
    }

    /// Scrolls so that `line` is in view, at the top where the scrollback reaches that far.
    fn scroll_to_show(&mut self, line: u64) {
        let screen_top = self.parser.callbacks().lines_forgotten + self.scrollback_len() as u64;
        let offset = screen_top.saturating_sub(line);
        self.set_scroll_offset(usize::try_from(offset).unwrap_or(usize::MAX));
    }
}

/// Whether `character` sets words apart, for a double click's selection: a blank, a quote, a
/// bracket or a separator, so that a path or an address is one word. An empty cell's text holds
/// none, and so is no word either.
fn separates_words(character: char) -> bool {
    character.is_whitespace() || ",|`:\"'()[]{}<>\u{2502}".contains(character)
}

/// What `vt100` hands its host: the title, the questions a command asks of the terminal, and each
/// line the scrollback forgets.
#[derive(Default)]
struct Requests {
    title: String,
    title_changed: bool,
    replies: Vec<u8>,
    /// How many lines left the top of the main screen's scrollback for good.
    lines_forgotten: u64,
}

impl vt100::Callbacks for Requests {
    fn set_window_title(&mut self, _screen: &mut vt100::Screen, title: &[u8]) {
        let title = String::from_utf8_lossy(title).into_owned();
        self.title_changed |= title != self.title;
        self.title = title;
    }

    /// Answers the two questions programs wait on: where the cursor is (`CSI 6 n`), and what
    /// kind of terminal this is (`CSI c` and `CSI > c`), as a VT220 and an xterm answer.
    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        intermediate: Option<u8>,
        _second_intermediate: Option<u8>,
        parameters: &[&[u16]],
        action: char,
    ) {
        let parameter = parameters
            .first()
            .and_then(|values| values.first())
            .copied();
        match (intermediate, action, parameter.unwrap_or(0)) {
            (None, 'n', 5) => self.replies.extend_from_slice(b"\x1b[0n"),
            (None, 'n', 6) => {
                let (row, column) = screen.cursor_position();
                let report = format!("\x1b[{};{}R", row + 1, column + 1);
                self.replies.extend_from_slice(report.as_bytes());
            }
            (None, 'c', 0) => self.replies.extend_from_slice(b"\x1b[?62;22c"),
            (Some(b'>'), 'c', 0) => self.replies.extend_from_slice(b"\x1b[>1;10;0c"),
            _ => {}
        }
    }

    fn on_scroll(&mut self, _contents: vt100::capture::RowContents<'_>, alternate_screen: bool) {
        if !alternate_screen {
            self.lines_forgotten += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(columns: u16, rows: u16) -> Screen {
        Screen::new(GridSize::new(columns, rows))
    }

    fn row_text(screen: &Screen, row: u16) -> String {
        (0..screen.size().columns)
            .filter_map(|column| screen.cell(row, column))
            .map(vt100::Cell::contents)
            .collect::<String>()
            .trim_end()
            .to_owned()
    }

    #[test]
    fn output_is_drawn_where_the_cursor_is() {
        let mut screen = screen(20, 4);
        screen.feed(b"hello\r\nworld");
        assert_eq!(row_text(&screen, 0), "hello");
        assert_eq!(row_text(&screen, 1), "world");
        assert_eq!(screen.cursor(), Some((1, 5)));
    }

    #[test]
    fn a_title_is_kept_and_said_once() {
        let mut screen = screen(20, 4);
        assert_eq!(screen.title(), None);
        assert!(screen.feed(b"\x1b]2;build\x07").title_changed);
        assert_eq!(screen.title(), Some("build"));
        assert!(!screen.feed(b"\x1b]2;build\x07").title_changed);
    }

    #[test]
    fn a_question_about_the_cursor_is_answered() {
        let mut screen = screen(20, 4);
        let asked = screen.feed(b"ab\r\ncd\x1b[6n");
        assert_eq!(asked.replies, b"\x1b[2;3R");
    }

    #[test]
    fn a_question_about_the_terminal_is_answered() {
        let mut screen = screen(20, 4);
        assert_eq!(screen.feed(b"\x1b[c").replies, b"\x1b[?62;22c");
        assert_eq!(screen.feed(b"\x1b[>c").replies, b"\x1b[>1;10;0c");
    }

    #[test]
    fn a_point_stays_on_its_text_while_output_scrolls() {
        let mut screen = screen(10, 3);
        screen.feed(b"one\r\ntwo\r\nthree");
        let two = screen.point_at(1, 0);
        screen.feed(b"\r\nfour\r\nfive");
        assert_eq!(screen.row_of(two.line), None);
        screen.scroll_by(2);
        let row = screen.row_of(two.line).expect("scrolled back into view");
        assert_eq!(row_text(&screen, row), "two");
    }

    #[test]
    fn the_lines_being_read_stay_in_view_as_output_arrives() {
        let mut screen = screen(10, 3);
        screen.feed(b"one\r\ntwo\r\nthree\r\nfour\r\nfive");
        screen.scroll_by(2);
        assert_eq!(row_text(&screen, 0), "one");
        screen.feed(b"\r\nsix\r\nseven");
        assert_eq!(row_text(&screen, 0), "one");
        screen.set_scroll_offset(0);
        assert_eq!(row_text(&screen, 2), "seven");
    }

    #[test]
    fn selected_text_is_read_across_lines_out_of_view() {
        let mut screen = screen(10, 3);
        screen.feed(b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix");
        let start = BufferPoint { line: 0, column: 1 };
        let end = BufferPoint { line: 4, column: 1 };
        assert_eq!(screen.text_between(start, end), "ne\ntwo\nthree\nfour\nfi");
        assert_eq!(screen.scroll_offset(), 0);
    }

    #[test]
    fn a_wrapped_line_is_read_as_one() {
        let mut screen = screen(4, 3);
        screen.feed(b"abcdefgh");
        let start = BufferPoint { line: 0, column: 0 };
        let end = BufferPoint { line: 1, column: 3 };
        assert_eq!(screen.text_between(start, end), "abcdefgh");
    }

    #[test]
    fn a_word_is_a_path_whole_and_a_blank_is_itself() {
        let mut screen = screen(30, 2);
        screen.feed(b"cat ~/notes/to-do.txt (old)");
        assert_eq!(screen.word_at(0, 8), (4, 20));
        assert_eq!(screen.word_at(0, 3), (3, 3));
        assert_eq!(screen.word_at(0, 24), (23, 25));
        assert_eq!(screen.word_at(0, 29), (29, 29));
    }

    #[test]
    fn the_alternate_screen_is_noticed() {
        let mut screen = screen(10, 3);
        assert!(screen.feed(b"\x1b[?1049h").screen_switched);
        assert!(screen.is_alternate());
        assert!(!screen.feed(b"x").screen_switched);
        assert!(screen.feed(b"\x1b[?1049l").screen_switched);
    }
}
