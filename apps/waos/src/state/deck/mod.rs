//! The deck: on a phone, WaOS's one screen as tabs, each the home, the library's programs to open
//! one from, or a program opened from it. A tab gives its program a view as the program starts
//! (`tab_view`), with no window and answering no request, and the tabs keep what each tab is, how
//! far its program got, the tab the deck is on and whether its overview is open (`tabs`, over
//! `logic::tab_list`); the library, which the home's tab shows, is kept with the home (`home`).
//!
//! The invariant: a tab's program hears its view's presence from the tabs alone, by the rule
//! `tabs` states.

pub(crate) mod home;
pub(crate) mod tab_view;
pub(crate) mod tabs;
#[cfg(test)]
mod task_tests;
#[cfg(test)]
pub(crate) mod test_phone;
#[cfg(test)]
mod tests;
