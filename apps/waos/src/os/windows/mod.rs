//! What WaOS asks of Windows itself: the user's local application data folder and the Start
//! menu's folder of programs, as the shell names them (`known_folders`), and a program's Start
//! menu shortcut, written through the shell (`shortcuts`).

mod known_folders;
mod shortcuts;

pub(crate) use known_folders::{local_application_data, start_menu_programs};
pub(crate) use shortcuts::write_shortcut;
