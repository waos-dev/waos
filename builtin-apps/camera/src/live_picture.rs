//! The camera's live picture, as the window shows it: each frame the camera hands out is drawn
//! through the look onto a sheet of the app's own, which a composition layer then shows beneath
//! the window's own drawing. A frame is sampled where the camera left it, through a texture
//! imported of its hardware buffer on the painter's device, so the one copy of a frame's pixels is
//! the look's drawing of them.
//!
//! The invariant a later editor will break: a sheet the layer shows is not drawn onto again until
//! the compositor says it is done with it, and a frame never waits for that. The system learns
//! that the compositor let go of a sheet only at the layer's next present, so a picture that
//! waited for a free sheet before presenting would wait for ever, five frames in. A frame that
//! finds no sheet free is drawn onto a new one, as Flutter's surface manager does, and the sheets
//! settle at a few.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use waft::ui::{CompositionLayer, HardwareBuffer, Platform};

use crate::look_painter::{LookPainter, Shown};
use crate::looks::Grade;
use crate::sheet::Sheet;

/// The most sheets kept. A frame that finds this many, none free, lets go of the one shown
/// longest ago before it makes its own, so the app's memory stays bounded even where the
/// compositor never says it is done with one.
const MOST_SHEETS: usize = 8;

pub(crate) struct LivePicture {
    layer: Arc<dyn CompositionLayer>,
    /// The sheets drawn onto so far, all of the size of the frames shown last.
    sheets: Vec<TurnedSheet>,
    /// How many frames were shown.
    shown: u64,
}

/// A sheet, whether the compositor is done with it, and which frame it showed last.
struct TurnedSheet {
    sheet: Sheet,
    free: Arc<AtomicBool>,
    turn: u64,
}

impl LivePicture {
    /// A live picture showing nothing yet; none where the system has no compositor.
    pub(crate) fn new(platform: &dyn Platform) -> Option<LivePicture> {
        let layer = platform.compositor()?.make_layer(true);
        Some(LivePicture {
            layer,
            sheets: Vec::new(),
            shown: 0,
        })
    }

    /// The layer the picture shows on, for the window to place.
    pub(crate) fn layer(&self) -> Arc<dyn CompositionLayer> {
        Arc::clone(&self.layer)
    }

    /// Shows `frame`, a frame's hardware buffer, drawn through `grade`, upright as `shown` says,
    /// over a sheet of `size`, the layer's in the screen's pixels, which the system shows the
    /// sheet at; why not, where the system gives the app no memory to draw it in.
    pub(crate) fn show(
        &mut self,
        platform: &dyn Platform,
        painter: &LookPainter,
        frame: &dyn HardwareBuffer,
        size: [u32; 2],
        grade: Grade,
        shown: Shown,
    ) -> Result<(), String> {
        let picture = platform
            .import_texture(frame, painter.device())
            .ok_or("the app's device cannot sample the camera's frames")?;
        let at = self.sheet_to_draw(platform, painter, size.map(|side| side.max(1)))?;
        self.shown += 1;
        let turned = &mut self.sheets[at];
        painter.paint(&picture, &turned.sheet.texture, grade, shown);
        turned.present_on(self.layer.as_ref(), self.shown);
        Ok(())
    }

    /// Where among the sheets one of `size` is free to draw onto: one the compositor is done
    /// with, or a new one.
    fn sheet_to_draw(
        &mut self,
        platform: &dyn Platform,
        painter: &LookPainter,
        size: [u32; 2],
    ) -> Result<usize, String> {
        self.sheets.retain(|turned| turned.is_of(size));
        if let Some(free) = self.sheets.iter().position(TurnedSheet::is_free) {
            return Ok(free);
        }
        if self.sheets.len() >= MOST_SHEETS {
            self.forget_oldest_sheet();
        }
        let sheet = Sheet::to_show(platform, painter.device(), size)?;
        self.sheets.push(TurnedSheet {
            sheet,
            free: Arc::new(AtomicBool::new(true)),
            turn: 0,
        });
        Ok(self.sheets.len() - 1)
    }

    /// Lets go of the sheet shown longest ago, which the layer holds for as long as it shows it.
    fn forget_oldest_sheet(&mut self) {
        let oldest = self
            .sheets
            .iter()
            .enumerate()
            .min_by_key(|(_, turned)| turned.turn);
        if let Some((oldest, _)) = oldest {
            self.sheets.swap_remove(oldest);
        }
    }
}

impl TurnedSheet {
    fn is_free(&self) -> bool {
        self.free.load(Ordering::Acquire)
    }

    fn is_of(&self, size: [u32; 2]) -> bool {
        [self.sheet.buffer.width(), self.sheet.buffer.height()] == size
    }

    /// Shows the sheet on `layer` as frame `turn`; it is free again once the compositor is done
    /// with it.
    fn present_on(&mut self, layer: &dyn CompositionLayer, turn: u64) {
        self.turn = turn;
        self.free.store(false, Ordering::Release);
        let free = Arc::clone(&self.free);
        layer.present(
            Arc::clone(&self.sheet.buffer),
            Vec::new(),
            Box::new(move || free.store(true, Ordering::Release)),
        );
    }
}
