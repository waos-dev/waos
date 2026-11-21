//! `wasdk:platform/views`: a program's views, from both sides. One view, as the shell reports
//! into it and the engine reads it, is [`ProgramView`]; a program's views, as the shell adds and
//! removes them and the program asks for more, are [`ProgramViews`]; what the program reads of a
//! view is its stream of events, each with the engine's value for what it holds, a key its
//! [`KeyReply`], which the shell waits on; the modifier keys the keyboard holds, which the shell
//! reports with a pointer event so the program hears those it missed, are [`HeldModifiers`]. How
//! a view stands on screen, which its program's state follows, is the lifecycle's
//! [`Presence`](crate::lifecycle::Presence), and what a view arrives with is the shell's
//! [`PlacedView`](crate::shell::PlacedView). The screens' refreshes every program's views wait on
//! for their frames are [`ScreenRefreshes`]; the layers each view holds beneath its picture, as its
//! program set them, are [`ViewLayers`]; a composition layer's presents of the program's own
//! hardware buffers are `LayerPresents`; the picture a view shows, with the layers beneath it,
//! kept for a tool that looks at the program's window and takes its screenshot, is `ShownPicture`;
//! a view's text input, with the engine's copy of what is edited, which a tool types into, is
//! `ViewTextInput`; a tool's watch of a program, which keeps its views in sight though
//! covered, is [`Watching`]; and a screen the shell showed before the program asked, kept for its
//! first request, is what the shell waits on as a [`ReservedView`]. The interface is served to a program by [`host`].

mod event_queue;
mod frames_in_flight;
pub(crate) mod host;
mod key_reply;
mod layer_presents;
mod modifiers;
mod program_view;
mod program_views;
mod reserved_view;
mod screen_refreshes;
mod shown_picture;
mod view_layers;
mod view_text_input;

pub(crate) use event_queue::QueuedEvent;
pub(crate) use frames_in_flight::FrameInFlight;
pub use key_reply::{KeyHandledFuture, KeyReply};
pub(crate) use layer_presents::LayerPresents;
pub use modifiers::HeldModifiers;
pub use program_view::ProgramView;
pub use program_views::Watching;
pub(crate) use program_views::{ProgramViews, ViewChange};
pub use reserved_view::ReservedView;
pub(crate) use screen_refreshes::{Refresh, ScreenRefreshes};
pub(crate) use view_layers::{PlacedShellLayer, ViewLayers};
