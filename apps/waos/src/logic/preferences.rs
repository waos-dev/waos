//! WaOS's own preferences, kept in a small file of its own in its data folder, apart from the
//! store's settings, which are what programs are granted: whether programs are shown inside WaOS,
//! in the deck's tabs, rather than each as a task of its own, which Android alone offers a choice
//! of, and whether WaOS is kept in a desktop's Dock or taskbar, or else only has an icon in the
//! menu bar or the tray, and whether computers on the network may reach WaOS's dev server
//! (`logic::developer_access`); and the preferences as chosen, each kept in the file before it
//! holds (`KeptPreferences`).
//!
//! The invariant: a file that is missing or cannot be read gives the defaults, which is said, so
//! WaOS always starts; a preference the file does not name takes its default, so a file written by
//! an older WaOS still reads; a choice holds only once the file keeps it, so a WaOS started again
//! behaves as the user last saw it.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// What the user chose of WaOS's own behaviour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Preferences {
    /// Whether programs opened from now on show inside WaOS, in the deck's tabs; off by default,
    /// each then a task of its own.
    pub(crate) programs_inside_waos: bool,
    /// Whether WaOS is kept in a desktop's Dock or taskbar; on by default, and off, WaOS has only
    /// an icon in the menu bar or the tray.
    pub(crate) kept_in_dock: bool,
    /// Whether computers on the network may reach WaOS's dev server, once paired; off by
    /// default.
    pub(crate) developer_access: bool,
}

impl Default for Preferences {
    fn default() -> Preferences {
        Preferences {
            programs_inside_waos: false,
            kept_in_dock: true,
            developer_access: false,
        }
    }
}

/// The preferences as the user last chose them, and the file that keeps them.
#[derive(Clone, Debug)]
pub(crate) struct KeptPreferences {
    file: PreferencesFile,
    chosen: Preferences,
}

impl KeptPreferences {
    /// The preferences `file` keeps.
    pub(crate) fn of(file: PreferencesFile) -> KeptPreferences {
        let chosen = file.read();
        KeptPreferences { file, chosen }
    }

    pub(crate) fn chosen(&self) -> Preferences {
        self.chosen
    }

    /// The preferences `change` makes of those chosen, once the file keeps them; where it cannot,
    /// they stay as they were, and why is answered.
    pub(crate) fn choose(&mut self, change: impl FnOnce(&mut Preferences)) -> Result<()> {
        let mut chosen = self.chosen;
        change(&mut chosen);
        self.file.write(&chosen)?;
        self.chosen = chosen;
        Ok(())
    }
}

/// The file the preferences are kept in.
#[derive(Clone, Debug)]
pub(crate) struct PreferencesFile {
    path: PathBuf,
}

impl PreferencesFile {
    pub(crate) fn at(path: PathBuf) -> PreferencesFile {
        PreferencesFile { path }
    }

    /// The preferences the file holds; the defaults where it holds none, or none that reads.
    pub(crate) fn read(&self) -> Preferences {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Preferences::default();
            }
            Err(error) => {
                tracing::warn!(path = %self.path.display(), %error, "WaOS's preferences cannot be read: the defaults hold");
                return Preferences::default();
            }
        };
        toml::from_str(&text).unwrap_or_else(|error| {
            tracing::warn!(path = %self.path.display(), %error, "WaOS's preferences do not read: the defaults hold");
            Preferences::default()
        })
    }

    /// Keeps `preferences` in the file, its folder made where it is missing.
    pub(crate) fn write(&self, preferences: &Preferences) -> Result<()> {
        if let Some(folder) = self.path.parent() {
            std::fs::create_dir_all(folder)
                .with_context(|| format!("making {}", folder.display()))?;
        }
        let text = toml::to_string(preferences).context("writing the preferences as TOML")?;
        std::fs::write(&self.path, text).with_context(|| format!("writing {}", self.path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programs_are_tasks_until_the_user_says_otherwise() {
        let folder = tempfile::tempdir().unwrap();
        let file = PreferencesFile::at(folder.path().join("preferences.toml"));
        assert!(!file.read().programs_inside_waos);
    }

    #[test]
    fn waos_is_kept_in_the_dock_until_the_user_says_otherwise() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("preferences.toml");
        assert!(PreferencesFile::at(path.clone()).read().kept_in_dock);
        std::fs::write(&path, "programs_inside_waos = true").unwrap();
        assert!(
            PreferencesFile::at(path).read().kept_in_dock,
            "a file from before the choice"
        );
    }

    #[test]
    fn computers_on_the_network_are_kept_out_until_the_user_says_otherwise() {
        let folder = tempfile::tempdir().unwrap();
        let file = PreferencesFile::at(folder.path().join("preferences.toml"));
        assert!(!file.read().developer_access);
        std::fs::write(
            folder.path().join("preferences.toml"),
            "kept_in_dock = false\n",
        )
        .unwrap();
        assert!(
            !file.read().developer_access,
            "a file from before the choice"
        );
    }

    #[test]
    fn what_is_written_reads_back() {
        let folder = tempfile::tempdir().unwrap();
        let file = PreferencesFile::at(folder.path().join("waos").join("preferences.toml"));
        let inside = Preferences {
            programs_inside_waos: true,
            kept_in_dock: false,
            developer_access: true,
        };
        file.write(&inside).unwrap();
        assert_eq!(file.read(), inside);
    }

    #[test]
    fn a_choice_holds_once_the_file_keeps_it() {
        let folder = tempfile::tempdir().unwrap();
        let file = PreferencesFile::at(folder.path().join("preferences.toml"));
        let mut kept = KeptPreferences::of(file.clone());
        kept.choose(|chosen| chosen.programs_inside_waos = true)
            .unwrap();
        assert!(kept.chosen().programs_inside_waos);
        assert_eq!(file.read(), kept.chosen());
    }

    #[test]
    fn a_choice_the_file_cannot_keep_does_not_hold() {
        let folder = tempfile::tempdir().unwrap();
        let blocked = folder.path().join("a file");
        std::fs::write(&blocked, "").unwrap();
        let mut kept = KeptPreferences::of(PreferencesFile::at(blocked.join("preferences.toml")));
        assert!(
            kept.choose(|chosen| chosen.programs_inside_waos = true)
                .is_err()
        );
        assert_eq!(kept.chosen(), Preferences::default());
    }

    #[test]
    fn a_file_that_does_not_read_gives_the_defaults() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("preferences.toml");
        std::fs::write(&path, "programs_inside_waos = \"perhaps\"").unwrap();
        assert_eq!(PreferencesFile::at(path).read(), Preferences::default());
    }
}
