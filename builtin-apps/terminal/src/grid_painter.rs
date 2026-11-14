//! Draws a session's screen: the terminal's background, each row's fills and text at their
//! cells, the cursor, and what an input method is composing at the cursor.
//!
//! The invariant a later editor will break: a run of text is laid out with the letter spacing
//! that brings the font's advance to a cell's width, and placed at its first cell, so every
//! character lands on its own cell whatever the run's length.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use waft_embedder::{
    Color, FontStyle, FontWeight, Offset, Rect, Size, TextBaseline, TextDecoration, TextDirection,
};
use waft_foundation::{App, Entity};
use waft_rendering::CustomPainter;
use waft_widgets::{Canvas, Paint, PaintingBinding, TextPainter, TextSpan, TextStyle};

use crate::cell_metrics::CellMetrics;
use crate::frame_picture::{Cursor, CursorShape, FramePicture, frame_picture};
use crate::palette::{Palette, Rgb};
use crate::row_picture::{RowPicture, RunStyle, TextRun};
use crate::run_cache::{RunCache, RunKey};
use crate::session::Session;

/// The thickness of a cursor drawn as a line, in logical pixels.
const CURSOR_LINE: f64 = 2.0;
/// The thickness of the outline of a cursor in a window without the keyboard.
const CURSOR_OUTLINE: f64 = 1.0;

/// The font the grid's text is drawn in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextSettings {
    pub(crate) family: &'static str,
    /// The font's size in logical pixels.
    pub(crate) size: f64,
}

impl TextSettings {
    /// The style of text drawn as `run` says, its characters `letter_spacing` further apart than
    /// the font sets them.
    fn style(&self, run: RunStyle, letter_spacing: f64) -> TextStyle {
        let style = TextStyle::new()
            .font_family(self.family)
            .font_size(self.size)
            .letter_spacing(letter_spacing)
            .color(color(run.color))
            .font_weight(if run.bold {
                FontWeight::BOLD
            } else {
                FontWeight::NORMAL
            })
            .font_style(if run.italic {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            });
        if run.underline {
            style.decoration(TextDecoration::UNDERLINE)
        } else {
            style
        }
    }
}

/// The cells of text drawn as `text` says, on a screen of `pixel_ratio` physical pixels to a
/// logical one: measured from a line of the font's own characters.
pub(crate) fn measure_cells(app: &mut App, text: &TextSettings, pixel_ratio: f64) -> CellMetrics {
    const SAMPLE: &str = "MMMMMMMMMM";
    let plain = RunStyle {
        color: Rgb(0, 0, 0),
        bold: false,
        italic: false,
        underline: false,
    };
    let mut painter = laid_out(app, SAMPLE, text.style(plain, 0.0));
    let advance = painter.width() / SAMPLE.len() as f64;
    let baseline = painter.compute_distance_to_actual_baseline(TextBaseline::Alphabetic);
    let metrics = CellMetrics::of_font(advance, painter.height(), baseline, pixel_ratio);
    painter.dispose();
    metrics
}

/// `text` in `style`, laid out on one line.
fn laid_out(app: &mut App, text: &str, style: TextStyle) -> TextPainter {
    let fonts = PaintingBinding::instance(app).fonts(app);
    let mut painter = TextPainter::new();
    painter.set_text(Some(Rc::new(
        TextSpan::new().text(text.to_owned()).style(style),
    )));
    painter.set_text_direction(Some(TextDirection::Ltr));
    painter.layout(app.get_mut(fonts), 0.0, f64::INFINITY);
    painter
}

/// `rgb` as the framework's colour, opaque.
pub(crate) fn color(rgb: Rgb) -> Color {
    Color::from_argb(255, i32::from(rgb.0), i32::from(rgb.1), i32::from(rgb.2))
}

fn paint_of(rgb: Rgb) -> Paint {
    Paint::from_color(color(rgb).into())
}

/// Paints what `session` shows, in cells of `metrics`.
pub(crate) struct GridPainter {
    pub(crate) session: Entity<Session>,
    pub(crate) metrics: CellMetrics,
    pub(crate) text: TextSettings,
    pub(crate) palette: Palette,
    /// Whether the window has the keyboard, which the cursor's shape says.
    pub(crate) focused: bool,
    /// What an input method is composing, drawn at the cursor until it is committed.
    pub(crate) composing: String,
    /// The runs the view's last frame laid out, which this one draws again where it can.
    pub(crate) runs: Rc<RefCell<RunCache>>,
}

impl GridPainter {
    fn cell_rect(&self, row: u16, column: u16, cells: u16) -> Rect {
        Rect::from_ltwh(
            f64::from(column) * self.metrics.width,
            f64::from(row) * self.metrics.height,
            f64::from(cells) * self.metrics.width,
            self.metrics.height,
        )
    }

    fn paint_fills(&self, canvas: &mut Canvas, row: u16, picture: &RowPicture) {
        for fill in &picture.fills {
            canvas.draw_rect(
                self.cell_rect(row, fill.column, fill.cells),
                &paint_of(fill.color),
            );
        }
    }

    /// Draws `run` from its first cell, its baseline on the row's, laid out anew only where the
    /// last frame did not draw the same text in the same style.
    fn paint_run(
        &self,
        app: &mut App,
        canvas: &mut Canvas,
        runs: &mut RunCache,
        row: u16,
        run: &TextRun,
    ) {
        let key = RunKey {
            text: run.text.clone(),
            style: run.style,
            family: self.text.family,
            size_bits: self.text.size.to_bits(),
            spacing_bits: self.metrics.letter_spacing.to_bits(),
        };
        let laid_out = runs.run(key, |key| {
            let style = self.text.style(key.style, self.metrics.letter_spacing);
            let painter = laid_out(app, &key.text, style);
            let baseline = painter.compute_distance_to_actual_baseline(TextBaseline::Alphabetic);
            (painter, baseline)
        });
        let origin = Offset::new(
            f64::from(run.column) * self.metrics.width,
            f64::from(row) * self.metrics.height + self.metrics.baseline - laid_out.baseline,
        );
        let fonts = PaintingBinding::instance(app).fonts(app);
        laid_out.painter.paint(app.get_mut(fonts), canvas, origin);
    }

    /// Draws a cursor that is not a block, which its row's picture holds already.
    fn paint_cursor(&self, canvas: &mut Canvas, cursor: Cursor) {
        let cell = self.cell_rect(cursor.row, cursor.column, 1);
        let paint = paint_of(self.palette.cursor);
        let (left, top, width, height) = (cell.left, cell.top, cell.width(), cell.height());
        let lines = match cursor.shape {
            CursorShape::Block => vec![],
            CursorShape::Bar => vec![Rect::from_ltwh(left, top, CURSOR_LINE, height)],
            CursorShape::Underline => vec![Rect::from_ltwh(
                left,
                top + height - CURSOR_LINE,
                width,
                CURSOR_LINE,
            )],
            CursorShape::Outline => vec![
                Rect::from_ltwh(left, top, width, CURSOR_OUTLINE),
                Rect::from_ltwh(left, top + height - CURSOR_OUTLINE, width, CURSOR_OUTLINE),
                Rect::from_ltwh(left, top, CURSOR_OUTLINE, height),
                Rect::from_ltwh(left + width - CURSOR_OUTLINE, top, CURSOR_OUTLINE, height),
            ],
        };
        for line in lines {
            canvas.draw_rect(line, &paint);
        }
    }

    /// Draws what is being composed from the cursor's cell on, underlined, over what was there.
    fn paint_composing(&self, app: &mut App, canvas: &mut Canvas, cursor: Cursor) {
        let style = RunStyle {
            color: self.palette.foreground,
            bold: false,
            italic: false,
            underline: true,
        };
        let mut painter = laid_out(app, &self.composing, self.text.style(style, 0.0));
        let cell = self.cell_rect(cursor.row, cursor.column, 1);
        let behind = Rect::from_ltwh(cell.left, cell.top, painter.width(), cell.height());
        canvas.draw_rect(behind, &paint_of(self.palette.background));
        let baseline = painter.compute_distance_to_actual_baseline(TextBaseline::Alphabetic);
        let origin = Offset::new(cell.left, cell.top + self.metrics.baseline - baseline);
        let fonts = PaintingBinding::instance(app).fonts(app);
        painter.paint(app.get_mut(fonts), canvas, origin);
        painter.dispose();
    }

    fn paint_frame(&self, app: &mut App, canvas: &mut Canvas, frame: &FramePicture) {
        for (row, picture) in (0u16..).zip(&frame.rows) {
            self.paint_fills(canvas, row, picture);
        }
        let mut runs = self.runs.borrow_mut();
        runs.begin_frame();
        for (row, picture) in (0u16..).zip(&frame.rows) {
            for run in &picture.runs {
                self.paint_run(app, canvas, &mut runs, row, run);
            }
        }
        runs.end_frame();
        drop(runs);
        let Some(cursor) = frame.cursor else {
            return;
        };
        if self.composing.is_empty() {
            self.paint_cursor(canvas, cursor);
        } else {
            self.paint_composing(app, canvas, cursor);
        }
    }
}

impl CustomPainter for GridPainter {
    fn paint(&self, app: &mut App, canvas: &mut Canvas, size: Size) {
        let whole = Rect::from_ltwh(0.0, 0.0, size.width(), size.height());
        canvas.draw_rect(whole, &paint_of(self.palette.background));
        let frame = frame_picture(self.session.read(app), &self.palette, self.focused);
        self.paint_frame(app, canvas, &frame);
    }

    /// A painter is made anew only when what it draws changed: the session said so, or the
    /// window's size, look or focus did.
    fn should_repaint(&self, _app: &App, _old_delegate: &dyn CustomPainter) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
