//! How a value crosses between `wasdk:gpu/types` and wgpu-types: each enum, flag set and record
//! that holds no GPU object, field by field, the way each crate names it. A descriptor that holds
//! an object is converted where the object is looked up in the program's table, by the module
//! that serves the call.
//!
//! The invariant: an enum is converted by an exhaustive `match` in each direction it crosses
//! (`cases`), and the features by one table checked against all of wgpu's (`features`), so a case
//! or a feature wgpu adds fails to compile, or fails the table's test, until it is decided on.

mod cases;
mod features;
mod flags;
mod records;

pub(crate) use features::{feature_set, features_of};
pub(crate) use records::*;

/// Calls `$callback!` with `$args` and each row of a table as `(wit wgpu)`: a row written `A` is
/// `(A A)`, one written `A = B` is `(A B)`.
macro_rules! rows {
    ($callback:ident! $args:tt [$($done:tt)*]) => {
        $callback!($args [$($done)*]);
    };
    ($callback:ident! $args:tt [$($done:tt)*] $a:ident = $b:ident $(, $($rest:tt)*)?) => {
        rows!($callback! $args [$($done)* ($a $b)] $($($rest)*)?);
    };
    ($callback:ident! $args:tt [$($done:tt)*] $a:ident $(, $($rest:tt)*)?) => {
        rows!($callback! $args [$($done)* ($a $a)] $($($rest)*)?);
    };
}
pub(crate) use rows;
