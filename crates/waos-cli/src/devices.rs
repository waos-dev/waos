//! The devices this computer is paired with, kept in `devices.toml` beside the signing key, which
//! the user alone may read: each device's name, its address, the fingerprint of the certificate
//! it answered with when the two paired, and the token it gave this computer.
//!
//! The invariant a later editor will break: a device is reached only by the fingerprint kept
//! here, and the file holds the tokens, so it is written for the user alone, as the signing key
//! is, and never printed.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use waos_dev_protocol::{DeviceAddress, Fingerprint, Token};

use crate::configuration::{self, create_private_file, make_private_folder};

/// The file's name, in the user's configuration folder.
const DEVICES_FILE: &str = "devices.toml";

/// A device this computer is paired with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairedDevice {
    /// What `--device` names it by, beside its address.
    pub name: String,
    #[serde(with = "as_text")]
    pub address: DeviceAddress,
    #[serde(with = "as_text")]
    pub fingerprint: Fingerprint,
    pub token: Token,
}

impl PairedDevice {
    /// Whether `named`, as `--device` gives it, names this device: its name, or its address.
    fn is_named(&self, named: &str) -> bool {
        if self.name == named {
            return true;
        }
        named.parse::<DeviceAddress>().is_ok_and(|address| {
            address.host == self.address.host && address.port == self.address.port
        })
    }
}

#[derive(Default, Serialize, Deserialize)]
struct DevicesFile {
    #[serde(default)]
    devices: Vec<PairedDevice>,
}

/// The devices file: where it is, and what it holds.
pub struct Devices {
    path: PathBuf,
}

impl Devices {
    /// The file in the user's configuration folder.
    pub fn of_this_user() -> Result<Devices> {
        Ok(Devices::at(configuration::folder()?.join(DEVICES_FILE)))
    }

    pub fn at(path: PathBuf) -> Devices {
        Devices { path }
    }

    /// The devices paired, in the order they paired; none where there is no file.
    pub fn paired(&self) -> Result<Vec<PairedDevice>> {
        Ok(read(&self.path)?.devices)
    }

    /// The device `named` names, by its name or its address; none where no device paired is.
    pub fn find(&self, named: &str) -> Result<Option<PairedDevice>> {
        let paired = self.paired()?;
        Ok(paired.into_iter().find(|device| device.is_named(named)))
    }

    /// Keeps `device` as paired, in place of any paired before by its name or its address.
    pub fn keep(&self, device: PairedDevice) -> Result<()> {
        let mut file = read(&self.path)?;
        file.devices
            .retain(|kept| kept.name != device.name && !kept.is_named(&device.address.to_string()));
        file.devices.push(device);
        write(&self.path, &file)
    }

    /// Forgets the device `named` names; answers whether one was paired.
    pub fn forget(&self, named: &str) -> Result<bool> {
        let mut file = read(&self.path)?;
        let before = file.devices.len();
        file.devices.retain(|device| !device.is_named(named));
        if file.devices.len() == before {
            return Ok(false);
        }
        write(&self.path, &file)?;
        Ok(true)
    }
}

fn read(path: &Path) -> Result<DevicesFile> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(DevicesFile::default()),
        Err(error) => {
            return Err(error).with_context(|| format!("reading {}", path.display()));
        }
    };
    toml::from_str(&text).with_context(|| format!("reading {}", path.display()))
}

/// Writes `file` to `path` for the user alone, whole: to a new file beside it, then in its place.
fn write(path: &Path, file: &DevicesFile) -> Result<()> {
    let folder = path.parent().expect("the devices file is in a folder");
    make_private_folder(folder).with_context(|| format!("making {}", folder.display()))?;
    let text = toml::to_string(file).context("writing the devices as TOML")?;
    let new = path.with_extension("toml.new");
    // One a write that failed left.
    let _ = fs::remove_file(&new);
    let write = || {
        let mut written = create_private_file(&new)?;
        written.write_all(text.as_bytes())?;
        written.sync_all()?;
        // Windows refuses to move a file that is still open.
        drop(written);
        fs::rename(&new, path)
    };
    write().with_context(|| format!("writing {}", path.display()))
}

/// A value kept as its text: an address or a fingerprint, as a person reads and writes it.
mod as_text {
    use std::fmt::Display;
    use std::str::FromStr;

    use serde::{Deserialize, Deserializer, Serializer, de};

    pub fn serialize<T: Display, S: Serializer>(
        value: &T,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.collect_str(value)
    }

    pub fn deserialize<'de, T, D>(deserializer: D) -> Result<T, D::Error>
    where
        T: FromStr,
        T::Err: Display,
        D: Deserializer<'de>,
    {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device() -> PairedDevice {
        PairedDevice {
            name: "pixel".to_owned(),
            address: "192.168.1.20".parse().unwrap(),
            fingerprint: Fingerprint([7; 32]),
            token: Token::new("t"),
        }
    }

    #[test]
    fn a_device_is_found_by_its_name_or_its_address() {
        let folder = tempfile::tempdir().unwrap();
        let devices = Devices::at(folder.path().join(DEVICES_FILE));
        let file = DevicesFile {
            devices: vec![device()],
        };
        fs::write(
            folder.path().join(DEVICES_FILE),
            toml::to_string(&file).unwrap(),
        )
        .unwrap();
        assert_eq!(devices.find("pixel").unwrap(), Some(device()));
        assert_eq!(devices.find("192.168.1.20:7351").unwrap(), Some(device()));
        assert_eq!(devices.find("wss://192.168.1.20").unwrap(), Some(device()));
        assert_eq!(devices.find("192.168.1.21").unwrap(), None);
    }

    #[test]
    fn a_device_kept_again_replaces_the_one_kept_before_and_is_forgotten_whole() {
        let folder = tempfile::tempdir().unwrap();
        let devices = Devices::at(folder.path().join("waos").join(DEVICES_FILE));
        devices.keep(device()).unwrap();
        let again = PairedDevice {
            token: Token::new("new"),
            ..device()
        };
        devices.keep(again.clone()).unwrap();
        assert_eq!(devices.paired().unwrap(), [again]);
        assert!(devices.forget("pixel").unwrap());
        assert!(!devices.forget("pixel").unwrap());
        assert!(devices.paired().unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_the_users_alone() {
        use std::os::unix::fs::PermissionsExt;
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("waos").join(DEVICES_FILE);
        Devices::at(path.clone()).keep(device()).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn no_file_is_no_device() {
        let folder = tempfile::tempdir().unwrap();
        let devices = Devices::at(folder.path().join(DEVICES_FILE));
        assert!(devices.paired().unwrap().is_empty());
    }
}
