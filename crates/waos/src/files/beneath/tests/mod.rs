#[macro_use]
mod sys_common;

mod dir_calls;

mod cap_basics;
#[cfg(windows)]
mod changes_at_folder;
mod file_type_ext;
mod fs;
mod fs_additional;
mod metadata_ext;
mod names_ending_in_a_slash;
mod paths_containing_nul;
mod rename;
mod rename_directory;
mod reopendir;
#[cfg(unix)]
mod routes;
mod set_times;
mod symlinks;
mod windows_open;
mod windows_symlinks;
