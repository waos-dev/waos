//! What only Android has: the entry its activity calls, the app's folders, which only the
//! activity's Java context names, the system's certificate checker, which is reached through the
//! application, the system's log, where an app's traces go, since Android gives an app's process
//! no standard error, and the frames of WaOS's simulated camera, as Android's camera frames are
//! (`pattern_frames`).

mod certificates;
mod entry;
mod folders;
mod pattern_frames;
mod system_log;

pub(crate) use certificates::trust_the_system;
pub(crate) use folders::app_folders;
#[cfg(test)]
pub(crate) use pattern_frames::read_pattern_frame;
pub(crate) use pattern_frames::{CrossThreadFrame, pattern_frame, surface_of};
pub(crate) use system_log::SystemLog;
