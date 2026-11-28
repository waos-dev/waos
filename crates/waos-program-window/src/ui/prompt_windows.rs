//! WaOS's prompts over its own windows: over each of an app's windows, the prompt shown for the
//! app, in the middle of the window over a dimmed backdrop that keeps the app's canvas from the
//! pointer ([`PromptOver`]), rebuilt when a prompt comes or goes. A prompt's window of its own is
//! the prompt alone (`PromptWindows::content_of`).

use std::fmt::{self, Debug};

use crate::state::prompt_windows::ShownPrompts;
use waft::{
    App, BorderRadius, BuildContext, Center, ClipRRect, Color, ColoredBox, Entity, IntoWidget,
    SizedBox, StatelessWidget, WidgetRef,
};

/// Over the program's window, behind the prompt: dims it.
const BACKDROP: Color = Color::from_argb(96, 0, 0, 0);

/// How wide a prompt is over a program's window, and how round its corners are.
const WIDTH: f64 = 380.0;
const CORNER_RADIUS: f64 = 8.0;

/// What is shown over a window of the app `app`, by the name its instance runs under: its prompt
/// shown, over the backdrop, or nothing.
pub(crate) struct PromptOver {
    pub(crate) shown: Entity<ShownPrompts>,
    pub(crate) app: String,
}

impl Debug for PromptOver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PromptOver")
            .field("app", &self.app)
            .finish_non_exhaustive()
    }
}

impl StatelessWidget for PromptOver {
    fn build(&self, app: &mut App, _context: BuildContext) -> WidgetRef {
        self.shown
            .read(app)
            .prompts()
            .iter()
            .find(|shown| shown.app.as_str() == self.app)
            .map_or_else(
                || SizedBox::shrink().into_widget(),
                |shown| over(&shown.prompt),
            )
    }
}

/// `prompt` in the middle of the window it is shown over, at a prompt's width with round corners,
/// over the backdrop.
fn over(prompt: &WidgetRef) -> WidgetRef {
    let card = ClipRRect::new()
        .border_radius(BorderRadius::circular(CORNER_RADIUS).into())
        .child(prompt.clone());
    let centred = Center::new().child(SizedBox::new().width(WIDTH).child(card));
    ColoredBox::new(BACKDROP).child(centred).into_widget()
}
