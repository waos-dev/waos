//! What a source's listing of a program offers, against the programs installed: to install it, to
//! update the installed one to the version listed, or nothing, for a version not newer than the one
//! installed, by semver's precedence as the store compares them. An update is any listing of an
//! installed id at a newer version; whether the package is signed with the installed program's key
//! is the store's to check when it installs it. A development install is never offered an update,
//! since no source updates it.

use std::cmp::Ordering;

use waos_package::ListedProgram;
use waos_store::InstalledProgram;

/// What the library offers to do with a listed program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Offer {
    /// Its id is not installed.
    Install,
    /// The version listed is newer than the one installed, which is `installed`.
    Update { installed: String },
    /// The version installed is the one listed, or newer; or the id is a development install's,
    /// which no source updates.
    Installed,
}

/// What `listed` offers, with `installed` the programs installed.
pub(crate) fn offer_of(listed: &ListedProgram, installed: &[InstalledProgram]) -> Offer {
    let Some(program) = installed
        .iter()
        .find(|program| program.manifest.id() == &listed.id)
    else {
        return Offer::Install;
    };
    if program.is_development() {
        return Offer::Installed;
    }
    let version = program.manifest.version();
    if listed.version.cmp_precedence(version) == Ordering::Greater {
        Offer::Update {
            installed: version.to_string(),
        }
    } else {
        Offer::Installed
    }
}

#[cfg(test)]
mod tests {
    use waos_package::{Manifest, SigningKey};
    use waos_store::Origin;

    use super::*;

    fn installed(id: &str, version: &str) -> InstalledProgram {
        InstalledProgram {
            manifest: Manifest::new(id.parse().unwrap(), "Triangle", version.parse().unwrap())
                .unwrap(),
            origin: Origin::Package {
                publisher: SigningKey::generate().publisher(),
            },
        }
    }

    fn listed(id: &str, version: &str) -> ListedProgram {
        ListedProgram::new(
            id.parse().unwrap(),
            "Triangle",
            version.parse().unwrap(),
            format!("{id}-{version}.wapk"),
            [0; 32],
        )
    }

    #[test]
    fn a_program_not_installed_is_offered_to_install() {
        let offer = offer_of(
            &listed("dev.example.triangle", "1.0.0"),
            &[installed("dev.example.other", "1.0.0")],
        );
        assert_eq!(offer, Offer::Install);
    }

    #[test]
    fn a_newer_version_of_an_installed_program_is_offered_as_an_update() {
        let offer = offer_of(
            &listed("dev.example.triangle", "1.1.0"),
            &[installed("dev.example.triangle", "1.0.0")],
        );
        assert_eq!(
            offer,
            Offer::Update {
                installed: "1.0.0".to_owned()
            }
        );
    }

    #[test]
    fn a_version_not_newer_than_the_installed_one_offers_nothing() {
        let installed = [installed("dev.example.triangle", "1.1.0")];
        assert_eq!(
            offer_of(&listed("dev.example.triangle", "1.1.0"), &installed),
            Offer::Installed
        );
        assert_eq!(
            offer_of(&listed("dev.example.triangle", "1.0.0"), &installed),
            Offer::Installed,
            "an older version is not offered"
        );
    }

    #[test]
    fn a_development_install_is_offered_no_update() {
        let development = InstalledProgram {
            origin: Origin::Development {
                component_sha256: [0; 32],
            },
            ..installed("dev.example.triangle.development", "0.1.0")
        };
        let offer = offer_of(
            &listed("dev.example.triangle.development", "9.0.0"),
            &[development],
        );
        assert_eq!(offer, Offer::Installed);
    }
}
