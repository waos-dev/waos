//! Conversions between wgpu's values and `wasdk:gpu`'s, which names each case and field as
//! wgpu-types does.
//!
//! Most are rows of a table the macros below expand into both directions: an enum's into two
//! `match`es the compiler checks are exhaustive, so a case either side adds fails to build here,
//! and a set of flags into a fold over its rows. What wgpu has and the WIT leaves out (the
//! experimental features, ray tracing's usages and stages) panics naming it (`left_out`); nothing
//! is silently dropped or substituted.

mod binding;
mod capabilities;
mod device;
mod format;
mod render;
mod sampler;
mod surface;

pub use binding::*;
pub use capabilities::*;
pub use device::*;
pub use format::*;
pub use render::*;
pub use sampler::*;
pub use surface::*;

/// A wgpu label as the WIT carries it.
pub fn label(label: wgpu::Label<'_>) -> Option<String> {
    label.map(str::to_owned)
}

/// Both directions between a wgpu enum and the WIT's (`wasdk::gpu_types`), case for case. A case
/// is named the same on both sides unless written `Wgpu => Wit`; cases that carry values are
/// converted by the arms under `to_wit` and `from_wit`.
macro_rules! enum_conversions {
    (
        $(#[$doc:meta])*
        $to_wit:ident, $from_wit:ident: $wgpu:ident <=> $wit:ident {
            $($wgpu_case:ident $(=> $wit_case:ident)?),* $(,)?
        }
        $(to_wit { $($to_pattern:pat => $to_value:expr),* $(,)? }
        from_wit { $($from_pattern:pat => $from_value:expr),* $(,)? })?
    ) => {
        $(#[$doc])*
        pub fn $to_wit(value: ::wgpu::$wgpu) -> ::wasdk::gpu_types::$wit {
            match value {
                $(::wgpu::$wgpu::$wgpu_case => $crate::convert::wit_case!($wit, $wgpu_case $(, $wit_case)?),)*
                $($($to_pattern => $to_value,)*)?
            }
        }

        $(#[$doc])*
        pub fn $from_wit(value: ::wasdk::gpu_types::$wit) -> ::wgpu::$wgpu {
            match value {
                $($crate::convert::wit_case!($wit, $wgpu_case $(, $wit_case)?) => ::wgpu::$wgpu::$wgpu_case,)*
                $($($from_pattern => $from_value,)*)?
            }
        }
    };
}

/// The WIT's case for a wgpu case: the same name, or the one written after `=>`.
macro_rules! wit_case {
    ($wit:ident, $same:ident) => {
        ::wasdk::gpu_types::$wit::$same
    };
    ($wit:ident, $wgpu_case:ident, $wit_case:ident) => {
        ::wasdk::gpu_types::$wit::$wit_case
    };
}

/// Both directions between a set of wgpu flags and the WIT's, flag for flag, each named the same
/// on both sides unless written `WGPU => WIT`. A wgpu flag with no row is left out of the WIT.
macro_rules! flag_conversions {
    (
        $(#[$doc:meta])*
        $to_wit:ident, $from_wit:ident: $wgpu:ident <=> $wit:ident {
            $($wgpu_flag:ident $(=> $wit_flag:ident)?),* $(,)?
        }
    ) => {
        $(#[$doc])*
        pub fn $to_wit(value: ::wgpu::$wgpu) -> ::wasdk::gpu_types::$wit {
            let mut wit = ::wasdk::gpu_types::$wit::empty();
            let mut rest = value;
            $(
                if value.contains(::wgpu::$wgpu::$wgpu_flag) {
                    wit |= $crate::convert::wit_flag!($wit, $wgpu_flag $(, $wit_flag)?);
                    rest.remove(::wgpu::$wgpu::$wgpu_flag);
                }
            )*
            if !rest.is_empty() {
                $crate::left_out::left_out(format_args!("{rest:?}"));
            }
            wit
        }

        $(#[$doc])*
        pub fn $from_wit(value: ::wasdk::gpu_types::$wit) -> ::wgpu::$wgpu {
            let mut wgpu = ::wgpu::$wgpu::empty();
            $(
                if value.contains($crate::convert::wit_flag!($wit, $wgpu_flag $(, $wit_flag)?)) {
                    wgpu |= ::wgpu::$wgpu::$wgpu_flag;
                }
            )*
            wgpu
        }
    };
}

/// The WIT's flag for a wgpu flag: the same name, or the one written after `=>`.
macro_rules! wit_flag {
    ($wit:ident, $same:ident) => {
        ::wasdk::gpu_types::$wit::$same
    };
    ($wit:ident, $wgpu_flag:ident, $wit_flag:ident) => {
        ::wasdk::gpu_types::$wit::$wit_flag
    };
}

use {enum_conversions, flag_conversions, wit_case, wit_flag};
