//! The devices this computer is paired with, kept in `devices.toml` beside the signing key, which
//! the user alone may read: each device's name, its address, the fingerprint of the certificate
//! it answered with when the two paired, and the token it gave this computer.
//!
//! The invariant a later editor will break: a device is reached only by the fingerprint kept
//! here, and the file holds the tokens, so it is written for the user alone, as the signing key
//! is, and never printed.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use waos_dev_protocol::{DeviceAddress, Fingerprint, Token};

use crate::configuration;

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
    fn no_file_is_no_device() {
        let folder = tempfile::tempdir().unwrap();
        let devices = Devices::at(folder.path().join(DEVICES_FILE));
        assert!(devices.paired().unwrap().is_empty());
    }
}
