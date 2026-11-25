//! What WaOS asks of Windows itself: the user's local application data folder and the Start
//! menu's folder of programs, as the shell names them (`known_folders`), a program's Start menu
//! shortcut, written through the shell (`shortcuts`), and the registry settings that offer a
//! program for the kinds of file it opens (`registry`).

mod known_folders;
mod registry;
mod shortcuts;

pub(crate) use known_folders::{local_application_data, start_menu_programs};
pub(crate) use registry::{delete_registry_settings, set_registry_values};
pub(crate) use shortcuts::write_shortcut;
