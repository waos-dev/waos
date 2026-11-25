//! A paired computer's token: what `waos` sends in each hello across the network, which the device
//! gave it when the two paired, and recognizes by its digest.
//!
//! The invariant a later editor will break: a device keeps the digest, never the token, so its
//! files hold nothing to connect with; and neither side prints a token.

use std::fmt;

use aws_lc_rs::digest;
use serde::{Deserialize, Serialize};

/// A paired computer's token.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Token(String);

impl Token {
    /// The token as a hello carries it, and as the computer keeps it.
    pub fn new(text: impl Into<String>) -> Token {
        Token(text.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// What the device keeps of it: its SHA-256, in hex.
    pub fn digest(&self) -> String {
        hex::encode(digest::digest(&digest::SHA256, self.0.as_bytes()))
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Token(..)")
    }
}
