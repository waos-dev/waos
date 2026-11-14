//! A secret WaOS keeps in the user's Keychain, as Chrome keeps its "Chrome Safe Storage" key there
//! for OSCrypt: a generic password item of WaOS's own, made of random bytes the first time it is
//! asked for. The Keychain lets the app that made the item read it without asking, and asks the
//! user before any other code reads it; a development build, whose signature changes with each
//! build, is asked once per build.
//!
//! The invariant: the secret is made once and only read after, so what it vouched for stays
//! vouched for; a Keychain that cannot be read is a failure, never a new secret.

use std::io;

use security_framework::passwords::{generic_password, set_generic_password};
use security_framework::passwords_options::PasswordOptions;

/// The Keychain item's service and account, which the user sees it under.
const SERVICE: &str = "WaOS Safe Storage";
const ACCOUNT: &str = "WaOS";

/// Security's `errSecItemNotFound` (`SecBase.h`): no such item.
const ITEM_NOT_FOUND: i32 = -25300;

/// How many random bytes a secret is.
const SECRET_BYTES: usize = 32;

/// WaOS's secret, made and kept in the Keychain if it has none yet.
pub(crate) fn safe_storage_key() -> io::Result<Vec<u8>> {
    match generic_password(PasswordOptions::new_generic_password(SERVICE, ACCOUNT)) {
        Ok(secret) => Ok(secret),
        Err(error) if error.code() == ITEM_NOT_FOUND => make_secret(),
        Err(error) => Err(io::Error::other(format!(
            "reading WaOS's key from the Keychain: {error}"
        ))),
    }
}

/// A new secret, kept in the Keychain.
fn make_secret() -> io::Result<Vec<u8>> {
    let mut secret = vec![0; SECRET_BYTES];
    getrandom::fill(&mut secret).map_err(io::Error::other)?;
    set_generic_password(SERVICE, ACCOUNT, &secret).map_err(|error| {
        io::Error::other(format!("keeping WaOS's key in the Keychain: {error}"))
    })?;
    tracing::info!("WaOS made its key and keeps it in the Keychain");
    Ok(secret)
}
