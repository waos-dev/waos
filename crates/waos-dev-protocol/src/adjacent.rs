//! Deserializing adjacently tagged enums (`#[serde(tag = "...", content = "...")]`) whose
//! `Unknown` variant catches any variant a newer build added, even when that variant carries a
//! payload under the content key.
//!
//! `#[derive(Deserialize)]` with `#[serde(other)]` rejects any unknown variant that carries a
//! content field (`invalid type: ..., expected unit variant`), because serde passes the content to
//! `unit_variant()`. Here the tag is checked first so an unknown tag's content is skipped
//! unread, while known variants deserialize straight from the stream without buffering.

use std::fmt;
use std::marker::PhantomData;

use ciborium::Value;
use serde::Deserializer;
use serde::de::{self, DeserializeOwned, DeserializeSeed, IgnoredAny, MapAccess, Visitor};

/// An adjacently tagged enum whose unknown variants deserialize to [`Adjacent::unknown`].
pub(crate) trait Adjacent: Sized + DeserializeOwned {
    const TAG: &'static str;
    const CONTENT: &'static str;

    fn unknown() -> Self;
    fn is_known(tag: &str) -> bool;
    fn unit(tag: &str) -> Option<Self>;
    fn payload<'de, D: Deserializer<'de>>(tag: &str, deserializer: D) -> Result<Self, D::Error>;
}

/// Deserializes `T` from a CBOR map of `T::TAG` and optionally `T::CONTENT`, reading any unknown
/// variant tag as `T::unknown()`.
pub(crate) fn deserialize<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Adjacent,
{
    deserializer.deserialize_map(AdjacentVisitor::<T>(PhantomData))
}

struct AdjacentVisitor<T>(PhantomData<T>);

impl<'de, T: Adjacent> Visitor<'de> for AdjacentVisitor<T> {
    type Value = T;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a map with {:?}", T::TAG)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<T, A::Error> {
        let mut tag: Option<String> = None;
        let mut value: Option<T> = None;
        let mut buffered: Option<Value> = None;

        while let Some(key) = map.next_key::<String>()? {
            if key == T::TAG {
                if tag.is_some() {
                    return Err(de::Error::duplicate_field(T::TAG));
                }
                tag = Some(map.next_value()?);
            } else if key == T::CONTENT {
                if value.is_some() || buffered.is_some() {
                    return Err(de::Error::duplicate_field(T::CONTENT));
                }
                match tag.as_deref() {
                    Some(known) if T::is_known(known) => {
                        value = Some(map.next_value_seed(PayloadSeed::<T> {
                            tag: known,
                            _marker: PhantomData,
                        })?);
                    }
                    Some(_unknown) => {
                        let _ = map.next_value::<IgnoredAny>()?;
                    }
                    None => {
                        buffered = Some(map.next_value::<Value>()?);
                    }
                }
            } else {
                let _ = map.next_value::<IgnoredAny>()?;
            }
        }

        let Some(tag) = tag else {
            return Err(de::Error::missing_field(T::TAG));
        };
        if !T::is_known(&tag) {
            return Ok(T::unknown());
        }
        if let Some(value) = value {
            return Ok(value);
        }
        if let Some(buffered) = buffered {
            // Re-run with `TAG` first so the known variant's payload deserializes directly.
            let ordered = Value::Map(vec![
                (Value::Text(T::TAG.to_owned()), Value::Text(tag)),
                (Value::Text(T::CONTENT.to_owned()), buffered),
            ]);
            return ordered.deserialized().map_err(de::Error::custom);
        }
        T::unit(&tag).ok_or_else(|| de::Error::missing_field(T::CONTENT))
    }
}

struct PayloadSeed<'a, T> {
    tag: &'a str,
    _marker: PhantomData<T>,
}

impl<'de, T: Adjacent> DeserializeSeed<'de> for PayloadSeed<'_, T> {
    type Value = T;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<T, D::Error> {
        if let Some(unit) = T::unit(self.tag) {
            // Accept `null` or `()` if a writer emits an empty content field for a unit variant.
            let _: Option<()> = de::Deserialize::deserialize(deserializer)?;
            return Ok(unit);
        }
        T::payload(self.tag, deserializer)
    }
}
