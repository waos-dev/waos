//! Forward-compatible deserialization for enums inside messages (`#[serde(remote = "Self",
//! tag = "kind", content = "data")]` with `#[serde(skip)] Unknown`).
//!
//! `serde`'s derived `#[serde(other)]` rejects an unknown variant that carries a `"data"` field,
//! and only one `#[serde(other)]` variant can sit on a unit. Instead, incoming CBOR is read as
//! [`ciborium::Value`], its `"kind"` tag is probed directly against the derived deserializer via
//! [`MapDeserializer`], and an unknown tag resolves to `Unknown` while known tags decode through
//! the derive via [`ciborium::Value::deserialized`] — with no hand-written tag lists.

use std::fmt;

use ciborium::Value;
use serde::de::value::MapDeserializer;
use serde::de::{self, Deserialize, Deserializer};

/// The tag key every enum in the protocol uses.
const TAG: &str = "kind";

/// Error type used solely to probe whether a variant tag is known to a derived deserializer.
#[derive(Debug)]
pub(crate) enum ProbeError {
    UnknownVariant,
    Other,
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("tag probe")
    }
}

impl std::error::Error for ProbeError {}

impl de::Error for ProbeError {
    fn custom<T: fmt::Display>(_msg: T) -> Self {
        ProbeError::Other
    }

    fn unknown_variant(_variant: &str, _expected: &'static [&'static str]) -> Self {
        ProbeError::UnknownVariant
    }
}

/// Deserializes the incoming CBOR tree as a [`Value`].
pub(crate) fn read_value<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Value, D::Error> {
    Value::deserialize(deserializer)
}

/// The `"kind"` tag in `value` when `value` is a map carrying one as text.
pub(crate) fn tag_of(value: &Value) -> Option<&str> {
    let Value::Map(entries) = value else {
        return None;
    };
    entries.iter().find_map(|(key, val)| match (key, val) {
        (Value::Text(k), Value::Text(v)) if k == TAG => Some(v.as_str()),
        _ => None,
    })
}

/// Single-entry map deserializer feeding `"kind": tag` to a derived `Deserialize` to probe
/// whether `tag` is known.
pub(crate) fn probe(tag: &str) -> MapDeserializer<'_, std::iter::Once<(&str, &str)>, ProbeError> {
    MapDeserializer::new(std::iter::once((TAG, tag)))
}

/// Implements `Serialize` and forward-compatible `Deserialize` for a `#[serde(remote = "Self",
/// tag = "kind", content = "data")]` enum with a `#[serde(skip)] Unknown` variant.
macro_rules! adjacent_unknown {
    ($name:ident) => {
        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                $name::serialize(self, serializer)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = crate::adjacent::read_value(deserializer)?;
                if let Some(tag) = crate::adjacent::tag_of(&value)
                    && matches!(
                        $name::deserialize(crate::adjacent::probe(tag)),
                        Err(crate::adjacent::ProbeError::UnknownVariant)
                    )
                {
                    return Ok($name::Unknown);
                }
                struct Remote($name);
                impl<'de> serde::Deserialize<'de> for Remote {
                    fn deserialize<R: serde::Deserializer<'de>>(
                        deserializer: R,
                    ) -> Result<Self, R::Error> {
                        $name::deserialize(deserializer).map(Remote)
                    }
                }
                value
                    .deserialized::<Remote>()
                    .map(|remote| remote.0)
                    .map_err(serde::de::Error::custom)
            }
        }
    };
}

pub(crate) use adjacent_unknown;
