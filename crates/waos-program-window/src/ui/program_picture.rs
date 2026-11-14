//! The picture a program's view last showed, with none of the program's input: the view's
//! composition layer alone, filling its box. What shows a view whose program ended, as a tab of
//! WaOS's deck keeps the last picture of a program that ended on its own.
//!
//! The invariant: nothing here reaches the program: no pointer, no key, no focus and no text.

use waft::{
    App, BuildContext, CompositionLayerView, Entity, IntoWidget, StatelessWidget, WidgetRef,
};

use crate::state::shown_view::ShownView;

/// What `view` last showed, filling the box it is given.
#[derive(Clone, Debug)]
pub struct ProgramPicture {
    view: Entity<ShownView>,
}

impl ProgramPicture {
    pub fn new(view: Entity<ShownView>) -> ProgramPicture {
        ProgramPicture { view }
    }
}

impl StatelessWidget for ProgramPicture {
    fn build(&self, app: &mut App, _context: BuildContext) -> WidgetRef {
        CompositionLayerView::new(self.view.read(app).layer()).into_widget()
    }
}
