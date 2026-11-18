//! Forward-compatible deserialization for enums inside messages (`#[serde(remote = "Self",
//! tag = "kind", content = "data")]` with `#[serde(skip)] Unknown`).
//!
//! `serde`'s derived `#[serde(other)]` rejects an unknown variant that carries a `"data"` field,
//! and only one `#[serde(other)]` variant can sit on a unit. Instead, incoming CBOR is read as
//! [`ciborium::Value`], its `"kind"` tag is probed directly against the derived deserializer via
//! [`MapDeserializer`], and an unknown tag resolves to `Unknown` while known tags decode through
//! the derive as usual — with no hand-written tag lists.

use std::fmt;
use std::marker::PhantomData;

use ciborium::Value;
use serde::de::value::{MapAccessDeserializer, MapDeserializer, SeqDeserializer};
use serde::de::{self, Deserialize, Deserializer, IntoDeserializer, Visitor};

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

/// Deserializes the incoming CBOR tree and extracts its `"kind"` tag if present as text.
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

/// Deserializer over `value` for decoding a known variant through its derived `Deserialize`.
pub(crate) fn decode<'a, E>(value: &'a Value) -> ValueDeserializer<'a, E> {
    ValueDeserializer::new(value)
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
                $name::deserialize(crate::adjacent::decode(&value))
            }
        }
    };
}

pub(crate) use adjacent_unknown;

/// A `serde::Deserializer` over a borrowed [`ciborium::Value`], preserving byte strings for
/// `serde_bytes` and delegating sequences and maps to serde's own value deserializers.
pub(crate) struct ValueDeserializer<'a, E> {
    value: &'a Value,
    _error: PhantomData<fn() -> E>,
}

impl<'a, E> ValueDeserializer<'a, E> {
    fn new(value: &'a Value) -> Self {
        Self {
            value,
            _error: PhantomData,
        }
    }
}

impl<'a, E> Clone for ValueDeserializer<'a, E> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'a, E> Copy for ValueDeserializer<'a, E> {}

impl<'a, 'de, E: de::Error> IntoDeserializer<'de, E> for ValueDeserializer<'a, E> {
    type Deserializer = Self;

    fn into_deserializer(self) -> Self {
        self
    }
}

impl<'a, 'de, E: de::Error> Deserializer<'de> for ValueDeserializer<'a, E> {
    type Error = E;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, E> {
        match self.value {
            Value::Null => visitor.visit_unit(),
            Value::Bool(b) => visitor.visit_bool(*b),
            Value::Integer(int) => {
                if let Ok(u) = u64::try_from(*int) {
                    visitor.visit_u64(u)
                } else if let Ok(i) = i64::try_from(*int) {
                    visitor.visit_i64(i)
                } else {
                    visitor.visit_i128(i128::from(*int))
                }
            }
            Value::Float(f) => visitor.visit_f64(*f),
            Value::Bytes(bytes) => visitor.visit_bytes(bytes),
            Value::Text(text) => visitor.visit_str(text),
            Value::Array(items) => {
                visitor.visit_seq(SeqDeserializer::new(items.iter().map(Self::new)))
            }
            Value::Map(entries) => visitor.visit_map(MapDeserializer::new(
                entries.iter().map(|(k, v)| (Self::new(k), Self::new(v))),
            )),
            Value::Tag(_, inner) => Self::new(inner).deserialize_any(visitor),
            _ => Err(de::Error::custom("unsupported CBOR value")),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, E> {
        match self.value {
            Value::Null => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, E> {
        match self.value {
            Value::Text(variant) => visitor.visit_enum(variant.as_str().into_deserializer()),
            Value::Map(entries) if entries.len() == 1 => {
                visitor.visit_enum(MapAccessDeserializer::new(MapDeserializer::new(
                    entries.iter().map(|(k, v)| (Self::new(k), Self::new(v))),
                )))
            }
            _ => self.deserialize_any(visitor),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, E> {
        visitor.visit_newtype_struct(self)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct seq tuple tuple_struct
        map struct identifier ignored_any
    }
}
