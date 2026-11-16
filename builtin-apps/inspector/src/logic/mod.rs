//! The Inspector's logic, as plain Rust: what a program did over the days kept (`activity`,
//! `report`), what each live panel makes of what it hears (`panels`), and what the detail pane
//! says of a row (`detail`). Nothing here names Waft's `App` or an entity, so it is tested alone.

pub(crate) mod activity;
pub(crate) mod detail;
pub(crate) mod panels;
pub(crate) mod report;
