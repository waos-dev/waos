//! The signing key, the private key that signs every package this user makes, as Android names
//! it (the design's decision 47): the text of `WAOS_SIGNING_KEY` when it is set, as on CI, and
//! otherwise the file `signing.key` in the user's configuration folder, made on first use, which
//! the user alone may read: by its mode on Unix, and on Windows by its access list, which names the
//! user alone. Both hold the same text, the key's PKCS#8 PEM, so a CI secret is set to the file's
//! content and a backup restores on any machine. The public half, which each package carries, is
//! the publisher's key.
//!
//! The invariant a later editor will break: a key is never replaced, and never made where it
//! would be thrown away. Every update of a program must be signed with the key that signed it
//! first, so a file that cannot be read, or a variable set to anything but a key, is an error,
//! never a reason to make another; and on CI, whose machine goes when the run ends, a key is
//! never made at all.

use std::env::{self, VarError};
use std::fs;
#[cfg(unix)]
use std::fs::{DirBuilder, File, OpenOptions};
#[cfg(unix)]
use std::io;
use std::io::{ErrorKind, Write};
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use waos_package::SigningKey;

#[cfg(windows)]
use crate::os::windows::{create_private_file, make_private_folder};

/// The variable the key is read from on CI, holding what the key file holds.
const VARIABLE: &str = "WAOS_SIGNING_KEY";

/// The key file's name, in the user's configuration folder.
const KEY_FILE: &str = "signing.key";

/// The variable CI services set, GitHub's and GitLab's among them.
const CI: &str = "CI";

/// The user's signing key: `WAOS_SIGNING_KEY`'s when it is set, else the key file's, made when
/// there is none, except on CI.
pub fn signing_key() -> Result<SigningKey> {
    if let Some(key) = key_in_variable(env::var(VARIABLE))? {
        return Ok(key);
    }
    key_in_file(&key_path()?, env::var_os(CI).is_some())
}

/// The key in `WAOS_SIGNING_KEY`, whose value is `value`; none when it is not set.
fn key_in_variable(value: Result<String, VarError>) -> Result<Option<SigningKey>> {
    let text = match value {
        Ok(text) => text,
        Err(VarError::NotPresent) => return Ok(None),
        Err(VarError::NotUnicode(_)) => {
            bail!("{VARIABLE} is not UTF-8 text; it holds the PEM text of your signing key")
        }
    };
    // CI gives a secret it does not have as an empty value.
    if text.trim().is_empty() {
        bail!(
            "{VARIABLE} is set but empty: set it to the text of your signing key, the content of {KEY_FILE}, or unset it to use the key file"
        );
    }
    let key =
        SigningKey::from_pem(&text).with_context(|| format!("the signing key in {VARIABLE}"))?;
    Ok(Some(key))
}

/// The key in the file at `path`, made when there is none, unless `on_ci`: a key made on CI
/// would go with its machine, and no update of what it signed could be published.
fn key_in_file(path: &Path, on_ci: bool) -> Result<SigningKey> {
    if let Some(key) = read_key(path)? {
        return Ok(key);
    }
    if on_ci {
        bail!(
            "{CI} is set and {VARIABLE} is not: set {VARIABLE} to the text of your signing key, the content of {KEY_FILE}, rather than sign with a key made here and thrown away with this machine"
        );
    }
    let key = make_key(path)?;
    say_made(&key, path);
    Ok(key)
}

/// `signing.key` in the user's configuration folder: `$XDG_CONFIG_HOME/waos`, or else the
/// system's own, where command-line tools keep theirs ([`system_configuration`]). It is apart from
/// the WaOS app's own folder, so removing the app and its data never takes the key with it.
fn key_path() -> Result<PathBuf> {
    let configuration = match env::var_os("XDG_CONFIG_HOME") {
        Some(folder) if Path::new(&folder).is_absolute() => PathBuf::from(folder),
        _ => system_configuration()?,
    };
    Ok(configuration.join("waos").join(KEY_FILE))
}

/// `~/.config` on macOS and Linux.
#[cfg(not(windows))]
fn system_configuration() -> Result<PathBuf> {
    Ok(env::home_dir()
        .context("this system names no home folder to keep the signing key in")?
        .join(".config"))
}

/// `%APPDATA%` on Windows, the user's roaming application data, which follows the user to every
/// machine of their domain, as a key that must not be lost should.
#[cfg(windows)]
fn system_configuration() -> Result<PathBuf> {
    env::var_os("APPDATA")
        .map(PathBuf::from)
        .filter(|folder| folder.is_absolute())
        .context("Windows names no APPDATA folder to keep the signing key in")
}

/// The key in the file at `path`; none when there is no file.
fn read_key(path: &Path) -> Result<Option<SigningKey>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("reading the signing key at {}", path.display()));
        }
    };
    let key = SigningKey::from_pem(&text)
        .with_context(|| format!("the signing key at {}", path.display()))?;
    Ok(Some(key))
}

/// Makes a key and writes it to a new file at `path`, which only the user may read, in a folder
/// only the user may open. A file already there is left as it is, and the making fails.
fn make_key(path: &Path) -> Result<SigningKey> {
    let folder = path.parent().expect("the key file is in a folder");
    make_private_folder(folder).with_context(|| format!("making {}", folder.display()))?;
    let key = SigningKey::generate();
    let write = || {
        let mut file = create_private_file(path)?;
        file.write_all(key.to_pem().as_bytes())?;
        file.sync_all()
    };
    write().with_context(|| format!("writing a signing key at {}", path.display()))?;
    Ok(key)
}

/// Makes `folder` and its parents, the folder for the user alone (`0700`); one already there is
/// left as it is.
#[cfg(unix)]
fn make_private_folder(folder: &Path) -> io::Result<()> {
    DirBuilder::new().recursive(true).mode(0o700).create(folder)
}

/// A new file at `path`, to write, which the user alone may read (`0600`); `AlreadyExists` when
/// there is one.
#[cfg(unix)]
fn create_private_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

/// Tells the user a key was made, where, the publisher key its packages carry, and that it must
/// be kept.
fn say_made(key: &SigningKey, path: &Path) {
    eprintln!(
        "cargo waos: made your signing key at {}; the packages it signs carry the publisher key {}",
        path.display(),
        key.publisher().fingerprint()
    );
    eprintln!(
        "cargo waos: back it up: every update of a program you publish must be signed with it, and a program whose key is lost can be published again only under a new id"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_file_in(folder: &Path) -> PathBuf {
        folder.join("waos").join(KEY_FILE)
    }

    #[test]
    fn a_key_is_made_once_and_then_read() {
        let scratch = tempfile::tempdir().unwrap();
        let path = key_file_in(scratch.path());
        assert!(read_key(&path).unwrap().is_none());
        let made = key_in_file(&path, false).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(
            key_in_file(&path, false).unwrap().publisher(),
            made.publisher()
        );
        assert!(make_key(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
    }

    #[cfg(unix)]
    #[test]
    fn only_the_user_may_read_the_key_file() {
        use std::os::unix::fs::PermissionsExt;

        let scratch = tempfile::tempdir().unwrap();
        let path = key_file_in(scratch.path());
        make_key(&path).unwrap();
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
    }

    #[cfg(windows)]
    #[test]
    fn only_the_user_may_read_the_key_file() {
        use crate::os::windows::{as_the_system_writes_it, owner_and_access_text, this_users_sid};

        let scratch = tempfile::tempdir().unwrap();
        let path = key_file_in(scratch.path());
        make_key(&path).unwrap();
        let user = this_users_sid();
        // `FA`, every right a file has, for the user alone, through a list that inherits nothing.
        let given = |inheritance: &str| {
            as_the_system_writes_it(&format!("O:{user}D:P(A;{inheritance};FA;;;{user})")).unwrap()
        };
        assert_eq!(owner_and_access_text(&path).unwrap(), given(""));
        assert_eq!(
            owner_and_access_text(path.parent().unwrap()).unwrap(),
            given("OICI")
        );
    }

    #[test]
    fn on_ci_a_key_is_read_but_never_made() {
        let scratch = tempfile::tempdir().unwrap();
        let path = key_file_in(scratch.path());
        let error = key_in_file(&path, true).unwrap_err();
        assert!(error.to_string().contains(VARIABLE), "{error}");
        assert!(!path.exists());
        let made = make_key(&path).unwrap();
        assert_eq!(
            key_in_file(&path, true).unwrap().publisher(),
            made.publisher()
        );
    }

    #[test]
    fn a_key_file_that_holds_no_key_is_an_error_not_a_reason_to_make_one() {
        let scratch = tempfile::tempdir().unwrap();
        let path = key_file_in(scratch.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "not a key").unwrap();
        let error = key_in_file(&path, false).unwrap_err();
        assert!(format!("{error:#}").contains("PKCS#8"), "{error:#}");
        assert_eq!(fs::read_to_string(&path).unwrap(), "not a key");
    }

    #[test]
    fn the_variable_holds_what_the_key_file_holds() {
        let scratch = tempfile::tempdir().unwrap();
        let path = key_file_in(scratch.path());
        let made = make_key(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        let read = key_in_variable(Ok(text)).unwrap().unwrap();
        assert_eq!(read.publisher(), made.publisher());
    }

    #[test]
    fn an_unset_variable_leaves_the_key_to_the_file() {
        assert!(
            key_in_variable(Err(VarError::NotPresent))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn an_empty_variable_is_refused_rather_than_making_a_key() {
        for empty in ["", "\n"] {
            let error = key_in_variable(Ok(empty.to_owned())).unwrap_err();
            assert!(error.to_string().contains("empty"), "{error}");
        }
    }

    #[test]
    fn a_variable_that_holds_no_key_is_refused() {
        let error = key_in_variable(Ok("a key".to_owned())).unwrap_err();
        assert!(format!("{error:#}").contains(VARIABLE), "{error:#}");
    }
}
