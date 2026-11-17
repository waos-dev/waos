//! The Inspector's state, as entities over the logic: the programs on the device and the one
//! chosen (`programs`), the chosen program's page (`page`), and each live panel of it (`panel`),
//! one entity a feature. The widgets read them and call them; they keep only what a view keeps.

pub(crate) mod page;
pub(crate) mod panel;
pub(crate) mod programs;
pub(crate) mod storage;
