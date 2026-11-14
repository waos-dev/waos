//! What the user chose for the app, kept from one run to the next.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Settings {
    /// Whether a notification says so when a refresh the user did not ask for finds new items.
    #[serde(default)]
    pub(crate) notify_of_new_items: bool,
}
