//! Developer access: whether computers on the network may reach this WaOS's dev server, as `waos
//! --device` does, and what lets one in: the device's identity, the certificate it answers
//! `wss://` with; the computers paired with it, each recognized by its token's digest; and the
//! code the device shows while a computer pairs (`dev_pairing`). It is kept in a folder of WaOS's
//! data, the user's alone: `identity.der` and `key.der`, made the first time access is turned on,
//! and `paired.toml`.
//!
//! The invariants a later editor will break:
//! - The identity is made once and kept: a computer pinned its certificate, and a new one would
//!   make every paired computer refuse the device.
//! - The files keep the tokens' digests, never the tokens, so reading them reaches nothing.
//! - Access withdrawn is withdrawn from connections already open too: [`DeveloperAccess::withdrawn`]
//!   ends once access is off or the computer is removed, and the dev server ends the connection.
//! - A code is shown for [`CODE_LIFE`] and [`CODE_TRIES`] pairings at most, each pairing that
//!   takes it counting whether or not it finishes, and it ends once a computer pairs with it: each
//!   pairing is one guess at most, and a code few.

use std::fs::{self, OpenOptions};
use std::io::{self, ErrorKind, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use waos_dev_protocol::{Identity, PairingCode, Token};

/// How long a code is shown before it ends.
pub(crate) const CODE_LIFE: Duration = Duration::from_secs(120);

/// How many pairings a code is taken by before it ends.
pub(crate) const CODE_TRIES: u32 = 3;

const CERTIFICATE_FILE: &str = "identity.der";
const KEY_FILE: &str = "key.der";
const PAIRED_FILE: &str = "paired.toml";

/// Developer access, one for WaOS, shared by its dev server and its settings: every clone is the
/// same.
#[derive(Clone)]
pub(crate) struct DeveloperAccess(Arc<Inner>);

struct Inner {
    folder: PathBuf,
    place: ListeningPlace,
    kept: Mutex<Kept>,
    /// Whether access is on; the dev server follows it.
    on: watch::Sender<bool>,
    /// Whether the dev server listens for the network, which the settings show.
    listening: watch::Sender<Listening>,
    /// Bumped at each change the settings show: a code shown or ended, a computer paired or
    /// removed.
    changes: watch::Sender<u64>,
}

/// Where the dev server listens for the network.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ListeningPlace {
    /// Every address, on `waos_dev_protocol::PORT` or the first free port after it.
    EveryAddress,
    /// This machine's loopback, on any free port, as a test listens.
    #[cfg(test)]
    Loopback,
}

/// Whether the dev server listens for the network.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Listening {
    Not,
    On {
        port: u16,
    },
    /// Access is on, but the dev server could not listen, for the reason given.
    Failed(String),
}

/// What the folder keeps, as read, and the code shown now.
struct Kept {
    identity: Option<Identity>,
    paired: Vec<PairedComputer>,
    code: Option<ShownCode>,
}

/// The code shown, until when, and how many pairings took it.
struct ShownCode {
    code: PairingCode,
    until: Instant,
    tries: u32,
}

/// A computer paired with this device.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PairedComputer {
    /// The computer's name, as it said it.
    pub(crate) name: String,
    /// The SHA-256 of its token.
    token_digest: String,
    /// When it paired, in seconds since 1970.
    pub(crate) paired_at: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct PairedFile {
    #[serde(default)]
    computers: Vec<PairedComputer>,
}

impl DeveloperAccess {
    /// Developer access kept in `folder`, off until turned on; the computers paired are read now,
    /// and none where the file cannot be read, which is said.
    pub(crate) fn at(folder: PathBuf) -> DeveloperAccess {
        DeveloperAccess::listening_at(folder, ListeningPlace::EveryAddress)
    }

    /// Developer access kept in `folder`, whose dev server listens on this machine's loopback.
    #[cfg(test)]
    pub(crate) fn on_loopback(folder: PathBuf) -> DeveloperAccess {
        DeveloperAccess::listening_at(folder, ListeningPlace::Loopback)
    }

    fn listening_at(folder: PathBuf, place: ListeningPlace) -> DeveloperAccess {
        let paired = read_paired(&folder.join(PAIRED_FILE));
        DeveloperAccess(Arc::new(Inner {
            folder,
            place,
            kept: Mutex::new(Kept {
                identity: None,
                paired,
                code: None,
            }),
            on: watch::Sender::new(false),
            listening: watch::Sender::new(Listening::Not),
            changes: watch::Sender::new(0),
        }))
    }

    /// Turns access on or off; the dev server follows. Turned off, it ends the code shown.
    pub(crate) fn set_on(&self, on: bool) {
        self.0
            .on
            .send_if_modified(|was| std::mem::replace(was, on) != on);
        if !on {
            self.end_code();
        }
    }

    /// Whether access is on, as it changes.
    pub(crate) fn turned(&self) -> watch::Receiver<bool> {
        self.0.on.subscribe()
    }

    pub(crate) fn place(&self) -> ListeningPlace {
        self.0.place
    }

    /// Whether the dev server listens for the network, as it changes.
    pub(crate) fn listening(&self) -> watch::Receiver<Listening> {
        self.0.listening.subscribe()
    }

    /// The dev server says whether it listens now.
    pub(crate) fn listens(&self, listening: Listening) {
        self.0.listening.send_replace(listening);
    }

    /// The device's identity: the one kept, or one made and kept now.
    pub(crate) fn identity(&self) -> io::Result<Identity> {
        let mut kept = self.kept();
        if let Some(identity) = &kept.identity {
            return Ok(identity.clone());
        }
        let identity = match read_identity(&self.0.folder)? {
            Some(identity) => identity,
            None => {
                let identity = Identity::make()?;
                keep_identity(&self.0.folder, &identity)?;
                identity
            }
        };
        kept.identity = Some(identity.clone());
        Ok(identity)
    }

    /// The computers paired, in the order they paired.
    pub(crate) fn paired(&self) -> Vec<PairedComputer> {
        self.kept().paired.clone()
    }

    /// Whether `token` is a paired computer's.
    pub(crate) fn knows(&self, token: &Token) -> bool {
        let digest = token.digest();
        self.kept()
            .paired
            .iter()
            .any(|computer| computer.token_digest == digest)
    }

    /// Keeps the computer `name` as paired, its token `token`, in place of one of the same name
    /// paired before; the code shown ends.
    pub(crate) fn pair(&self, name: &str, token: &Token) -> io::Result<()> {
        let mut kept = self.kept();
        let mut paired = kept.paired.clone();
        paired.retain(|computer| computer.name != name);
        paired.push(PairedComputer {
            name: name.to_owned(),
            token_digest: token.digest(),
            paired_at: now_in_seconds(),
        });
        write_paired(&self.0.folder, &paired)?;
        kept.paired = paired;
        kept.code = None;
        drop(kept);
        self.changed();
        Ok(())
    }

    /// Forgets the computer `name`: its token is refused from now on.
    pub(crate) fn remove(&self, name: &str) -> io::Result<()> {
        let mut kept = self.kept();
        let mut paired = kept.paired.clone();
        paired.retain(|computer| computer.name != name);
        write_paired(&self.0.folder, &paired)?;
        kept.paired = paired;
        drop(kept);
        self.changed();
        Ok(())
    }

    /// Shows a new code, in place of any shown, for [`CODE_LIFE`]; answers it.
    pub(crate) fn show_code(&self) -> io::Result<PairingCode> {
        let code = PairingCode::make()?;
        self.kept().code = Some(ShownCode {
            code: code.clone(),
            until: Instant::now() + CODE_LIFE,
            tries: 0,
        });
        self.changed();
        Ok(code)
    }

    /// The code shown now; none once it ended.
    pub(crate) fn shown_code(&self) -> Option<PairingCode> {
        let kept = self.kept();
        let shown = kept.code.as_ref()?;
        (shown.until > Instant::now()).then(|| shown.code.clone())
    }

    /// The code shown, for a pairing to try, counted as one of its [`CODE_TRIES`]; none while
    /// none is shown, or once it ended.
    pub(crate) fn take_code(&self) -> Option<PairingCode> {
        let mut kept = self.kept();
        let shown = kept.code.as_mut()?;
        if shown.until <= Instant::now() {
            return None;
        }
        shown.tries += 1;
        let code = shown.code.clone();
        if shown.tries >= CODE_TRIES {
            kept.code = None;
            drop(kept);
            self.changed();
        }
        Some(code)
    }

    /// Ends the code shown, if any.
    pub(crate) fn end_code(&self) {
        if self.kept().code.take().is_some() {
            self.changed();
        }
    }

    /// Ends the code shown once its time is up: what a timer calls as the code's life ends.
    pub(crate) fn end_expired_code(&self) {
        let mut kept = self.kept();
        if kept
            .code
            .as_ref()
            .is_some_and(|shown| shown.until <= Instant::now())
        {
            kept.code = None;
            drop(kept);
            self.changed();
        }
    }

    /// Resolves once a connection from the network whose hello carried `token`, none for a
    /// pairing, may be answered no longer: access turned off, or its computer removed.
    pub(crate) async fn withdrawn(&self, token: Option<&Token>) {
        let mut turned = self.turned();
        let mut changes = self.changes();
        loop {
            let on = *turned.borrow_and_update();
            changes.borrow_and_update();
            let removed = token.is_some_and(|token| !self.knows(token));
            if !on || removed {
                return;
            }
            // The access outlives every connection, so its senders never go.
            tokio::select! {
                _ = turned.changed() => {}
                _ = changes.changed() => {}
            }
        }
    }

    /// Each change the settings show, from now on.
    pub(crate) fn changes(&self) -> watch::Receiver<u64> {
        self.0.changes.subscribe()
    }

    fn changed(&self) {
        self.0.changes.send_modify(|count| *count += 1);
    }

    fn kept(&self) -> MutexGuard<'_, Kept> {
        self.0.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn now_in_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default()
}

/// The computers `path` keeps; none where it is missing or does not read, which is said.
fn read_paired(path: &Path) -> Vec<PairedComputer> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "the paired computers cannot be read: none is paired");
            return Vec::new();
        }
    };
    match toml::from_str::<PairedFile>(&text) {
        Ok(file) => file.computers,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "the paired computers do not read: none is paired");
            Vec::new()
        }
    }
}

fn write_paired(folder: &Path, computers: &[PairedComputer]) -> io::Result<()> {
    let file = PairedFile {
        computers: computers.to_vec(),
    };
    let text = toml::to_string(&file).map_err(io::Error::other)?;
    write_private(&folder.join(PAIRED_FILE), text.as_bytes())
}

/// The identity `folder` keeps; none where it keeps none yet.
fn read_identity(folder: &Path) -> io::Result<Option<Identity>> {
    let certificate = match fs::read(folder.join(CERTIFICATE_FILE)) {
        Ok(certificate) => certificate,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let key = fs::read(folder.join(KEY_FILE))?;
    Ok(Some(Identity::new(certificate, key)))
}

/// Keeps `identity` in `folder`, the key first, so a certificate kept always has its key.
fn keep_identity(folder: &Path, identity: &Identity) -> io::Result<()> {
    write_private(&folder.join(KEY_FILE), identity.key())?;
    write_private(&folder.join(CERTIFICATE_FILE), identity.certificate())
}

/// Writes `contents` to `path` for the user alone, its folder made for the user alone: by mode on
/// Unix; on Windows, WaOS's data folder is the user's own already.
fn write_private(path: &Path, contents: &[u8]) -> io::Result<()> {
    let folder = path.parent().expect("a file in a folder");
    make_private_folder(folder)?;
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(contents)?;
    file.sync_all()
}

#[cfg(unix)]
fn make_private_folder(folder: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(folder)
}

#[cfg(not(unix))]
fn make_private_folder(folder: &Path) -> io::Result<()> {
    fs::create_dir_all(folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn access() -> (tempfile::TempDir, DeveloperAccess) {
        let folder = tempfile::tempdir().unwrap();
        let access = DeveloperAccess::at(folder.path().join("developer"));
        (folder, access)
    }

    #[test]
    fn the_identity_is_made_once_and_kept() {
        let (folder, access) = access();
        let made = access.identity().unwrap().fingerprint();
        let again = DeveloperAccess::at(folder.path().join("developer"));
        assert_eq!(again.identity().unwrap().fingerprint(), made);
    }

    #[cfg(unix)]
    #[test]
    fn the_key_is_the_users_alone() {
        use std::os::unix::fs::PermissionsExt;
        let (folder, access) = access();
        access.identity().unwrap();
        let mode = fs::metadata(folder.path().join("developer").join(KEY_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn a_paired_computer_is_known_by_its_token_across_a_restart() {
        let (folder, access) = access();
        let token = Token::new("a1b2");
        assert!(!access.knows(&token));
        access.pair("mac", &token).unwrap();
        assert!(access.knows(&token));
        let again = DeveloperAccess::at(folder.path().join("developer"));
        assert!(again.knows(&token));
        assert!(!again.knows(&Token::new("other")));
    }

    #[test]
    fn a_computer_removed_is_known_no_longer() {
        let (folder, access) = access();
        let token = Token::new("a1b2");
        access.pair("mac", &token).unwrap();
        access.remove("mac").unwrap();
        assert!(!access.knows(&token));
        let again = DeveloperAccess::at(folder.path().join("developer"));
        assert!(!again.knows(&token), "removed across a restart");
    }

    #[test]
    fn a_code_ends_after_its_tries_and_once_a_computer_pairs() {
        let (_folder, access) = access();
        let shown = access.show_code().unwrap();
        for _ in 0..CODE_TRIES {
            assert_eq!(access.take_code(), Some(shown.clone()));
        }
        assert_eq!(access.take_code(), None);
        assert_eq!(access.shown_code(), None);
        access.show_code().unwrap();
        access.pair("mac", &Token::new("t")).unwrap();
        assert_eq!(access.shown_code(), None);
    }

    #[test]
    fn turning_access_off_ends_the_code() {
        let (_folder, access) = access();
        access.set_on(true);
        access.show_code().unwrap();
        access.set_on(false);
        assert_eq!(access.shown_code(), None);
    }

    #[test]
    fn the_file_keeps_the_tokens_digest_not_the_token() {
        let (folder, access) = access();
        access.pair("mac", &Token::new("secret-token")).unwrap();
        let text = fs::read_to_string(folder.path().join("developer").join(PAIRED_FILE)).unwrap();
        assert!(!text.contains("secret-token"), "{text}");
    }
}
