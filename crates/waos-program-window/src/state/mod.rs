//! What ties the windows to Waft's application: each view a program asks for in a window of its
//! own (`view_windows`), each view shown, which knows nothing of a window (`shown_view`), and
//! WaOS's prompts placed in its own windows (`prompt_windows`).
//!
//! The invariant: nothing here is touched off the main thread. What reaches it from the program
//! comes through a queue a task of the main thread's empties: the window requests, and each view's
//! calls.

pub(crate) mod prompt_windows;
pub(crate) mod shown_view;
pub(crate) mod view_windows;
