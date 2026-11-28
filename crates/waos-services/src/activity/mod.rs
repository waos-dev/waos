//! The activity record: what each app did, one day at a time, as the engine counts it
//! ([`ActivityRecord`], the engine's `AuditLog`), on this device only, its last seven days. The days
//! are kept in an [`ActivityStore`], in the record's own values: WaOS's files
//! ([`ActivityFiles`]), or memory in a test ([`ActivityInMemory`]). The record follows the user's
//! settings, which say whether it records an app, and the package manager's record, which says
//! which apps there are: what it kept of an app goes when either says so.
//!
//! The invariant a later editor will break: the record's rules (which apps, how many days, whether
//! at all) are kept here, never by a caller, and nothing but the record writes or removes a day.

mod activity_files;
mod activity_store;
mod record;

pub use activity_files::ActivityFiles;
pub use activity_store::{ActivityInMemory, ActivityStore};
pub use record::ActivityRecord;
