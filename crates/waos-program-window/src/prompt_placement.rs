//! Where WaOS shows what it asks an app's user, apart from what it asks: a prompt is one Waft
//! widget WaOS draws on every platform, and a [`PromptPlacement`] shows it for the app, by the
//! app's id, over its window or in a window of the prompt's own, wherever the app's windows are,
//! whether or not an instance of it runs (after Chrome's permission bubbles, which the browser
//! draws for an app shim too). Where an app's windows are WaOS's own, the placement is
//! `PromptWindows`.
//!
//! The invariant a later editor will break: a prompt is shown from `show` until its
//! [`ShownPrompt`] is dropped, and its placement calls `dismissed` only for what the user did
//! outside the prompt, as closing a window it is shown in.

use std::rc::Rc;

use waft::{App, WidgetRef};
use waos_package::ProgramId;

/// How large a prompt is where it has a window of its own, in logical pixels.
pub const PROMPT_SIZE: [f64; 2] = [440.0, 320.0];

/// Where WaOS's prompts for an app are shown.
pub trait PromptPlacement {
    /// Shows `prompt` for the app `id`, until the answer is dropped. `dismissed` is called when
    /// the user puts the prompt away otherwise than through it, as by closing a window it is
    /// shown in.
    fn show(
        &self,
        app: &mut App,
        id: &ProgramId,
        prompt: WidgetRef,
        dismissed: Rc<dyn Fn(&mut App)>,
    ) -> ShownPrompt;
}

/// A prompt shown, until this is dropped.
pub struct ShownPrompt {
    hide: Option<Box<dyn FnOnce()>>,
}

impl ShownPrompt {
    /// A prompt shown, which `hide` takes away once this is dropped.
    pub fn new(hide: impl FnOnce() + 'static) -> ShownPrompt {
        ShownPrompt {
            hide: Some(Box::new(hide)),
        }
    }
}

impl Drop for ShownPrompt {
    fn drop(&mut self) {
        if let Some(hide) = self.hide.take() {
            hide();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn a_prompt_is_taken_away_once_when_its_answer_is_dropped() {
        let hidden = Rc::new(Cell::new(0));
        let counted = Rc::clone(&hidden);
        let shown = ShownPrompt::new(move || counted.set(counted.get() + 1));
        assert_eq!(hidden.get(), 0, "shown until dropped");
        drop(shown);
        assert_eq!(hidden.get(), 1);
    }
}
