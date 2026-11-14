//! What WaOS and a program's window process say of WaOS's prompts for the program, each shown in a
//! dialog window of its own in the program's app (decision 9 of
//! `docs/proposals/2026-10-29-macos-apps.md`): WaOS draws the prompt into an Waft view of its own,
//! on a composition layer in a layer context the dialog hosts, and the dialog sends back what
//! Waft's host in the window process reports of the view it is: its metrics, whether it is in
//! sight, its pointers, its keys and its focus ([`PromptReport`]), as Waft's own values, which
//! its `serde` feature lets cross. WaOS's view asks the dialog for a cursor and for the keyboard's
//! focus ([`PromptCall`]).
//!
//! The invariant a later editor will break: a dialog's reports are what Waft's host in the window
//! process reported of the dialog's view, naming that view and each pointing device by the window
//! process's own numbers, which WaOS turns into its own view and numbers of its own as it forwards
//! them.

use std::fmt::{self, Debug};

use serde::{Deserialize, Serialize};
use waft_embedder::{
    KeyData, PointerReport, SystemMouseCursorKind, ViewFocusDirection, ViewFocusEvent,
    ViewFocusState, ViewMetrics,
};

/// Names one prompt WaOS shows in a dialog, from [`ToWindow::ShowPrompt`](crate::ToWindow) until
/// [`ToWindow::HidePrompt`](crate::ToWindow).
pub type PromptId = u64;

/// What WaOS's view of a prompt asks of the dialog it is shown in, as a view asks it of its host.
#[derive(Debug, Serialize, Deserialize)]
pub enum PromptCall {
    /// Show `kind` while a pointer is over the dialog.
    SetCursor(SystemMouseCursorKind),
    /// Move the keyboard's focus to the dialog, or away from it.
    ChangeFocus {
        state: ViewFocusState,
        direction: ViewFocusDirection,
    },
}

/// What happened to a prompt's dialog.
#[derive(Serialize, Deserialize)]
pub enum PromptReport {
    /// The dialog is shown, its view as the metrics say.
    Shown(ViewMetrics),
    /// The view's size, scale or screen changed.
    MetricsChanged(ViewMetrics),
    /// Whether the dialog is in sight: not while it is minimized or covered.
    InSight(bool),
    Pointer(PointerReport),
    Key(KeyData),
    Focus(ViewFocusEvent),
    /// The dialog went otherwise than as WaOS asked: the user closed it, or no window could be made
    /// for it. The user put the prompt away unanswered.
    Closed,
}

/// Written without the character a key typed, as every message is: messages are traced.
impl Debug for PromptReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PromptReport::Shown(metrics) => f.debug_tuple("Shown").field(metrics).finish(),
            PromptReport::MetricsChanged(metrics) => {
                f.debug_tuple("MetricsChanged").field(metrics).finish()
            }
            PromptReport::InSight(in_sight) => f.debug_tuple("InSight").field(in_sight).finish(),
            PromptReport::Pointer(pointer) => f
                .debug_struct("Pointer")
                .field("device", &pointer.device)
                .field("change", &pointer.change)
                .finish_non_exhaustive(),
            PromptReport::Key(key) => f
                .debug_struct("Key")
                .field("event_type", &key.event_type)
                .finish_non_exhaustive(),
            PromptReport::Focus(event) => f.debug_tuple("Focus").field(event).finish(),
            PromptReport::Closed => f.write_str("Closed"),
        }
    }
}
