//! What `waos install` and `waos uninstall` ask (the design's decision 70). An install is a
//! program built from its source, installed as its development install, under its id followed by
//! `.development`, through the package manager (`Installable::Development`): WaOS answers
//! `Upload::Have` when that install holds the component already, by its length and digest, and
//! `Upload::Send` when it must receive it; a build installed already as it is, manifest, component
//! and icon, changes nothing, so its code compiled before stays; any other is compiled, then
//! replaces the build before, its run stopped first, ending `Replaced`, and the install is
//! answered once that is done. An uninstall removes any installed program, as the library does,
//! its run stopped first.
//!
//! The invariant: what is installed changes only through the package manager, which takes the
//! program's turn, so an install never interleaves with another of the same program, an
//! uninstall, or an open.

use std::sync::Arc;

use anyhow::Context as _;
use waos_dev_protocol::{
    Install, InstallAnswer, Refusal, RefusalKind, ServerConnection, UninstallAnswer, Upload,
};
use waos_package::{Manifest, ProgramId};
use waos_store::{Origin, Store};

use super::dev_refusal;
use super::package_manager::{Installable, PackageManager};

/// What installing and uninstalling need of the app.
#[derive(Clone)]
pub(crate) struct InstallContext {
    pub(crate) packages: Arc<PackageManager>,
    pub(crate) store: Store,
}

/// Serves a connection's `Install` to its end: the upload WaOS answers, the component's parts,
/// then the install, answered with the development id it is installed under, or why not.
pub(crate) async fn install(
    mut connection: ServerConnection,
    install: Install,
    context: &InstallContext,
) {
    let manifest = Manifest::new(install.id.clone(), &install.name, install.version.clone())
        .and_then(|manifest| {
            // A category this WaOS does not know is a later `waos`'s: none, as a manifest's is.
            let category = install
                .category
                .as_deref()
                .and_then(|category| category.parse().ok());
            manifest
                .with_kind(install.kind)
                .with_category(category)
                .with_file_associations(install.file_associations.clone())
        });
    let manifest = match manifest {
        Ok(manifest) => manifest,
        Err(error) => {
            let refusal = Refusal::new(RefusalKind::BadInput, format!("{}: {error}", install.id));
            let _ = connection.send(&Upload::Refused(refusal)).await;
            return;
        }
    };
    let development = install.id.development();
    let held = context
        .store
        .installed_program(&development)
        .filter(|held| holds_component(&context.store, held.origin, &development, &install));
    let component = if held.is_some() {
        if connection.send(&Upload::Have).await.is_err() {
            return;
        }
        None
    } else {
        if connection.send(&Upload::Send).await.is_err() {
            return;
        }
        match connection
            .receive_component(install.length, &install.digest)
            .await
        {
            Ok(component) => Some(component),
            // A wrong upload was whole frames, so the connection still speaks: the install is
            // refused on it.
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
                tracing::warn!(%error, "an install's component did not arrive as announced");
                let refusal = Refusal::new(RefusalKind::CannotLoad, error.to_string());
                let _ = connection.send(&InstallAnswer::Refused(refusal)).await;
                return;
            }
            Err(error) => {
                tracing::warn!(%error, "an install's component did not arrive");
                return;
            }
        }
    };
    let answer = match put_in_place(context, &manifest, &install, component).await {
        Ok(()) => InstallAnswer::Installed { id: development },
        Err(error) => InstallAnswer::Refused(Refusal::new(
            RefusalKind::Failed,
            format!("{} could not be installed: {error:#}", install.id),
        )),
    };
    // A client gone meanwhile has nobody to tell.
    let _ = connection.send(&answer).await;
}

/// Whether the development install `development`, from `origin`, holds the component `install`
/// announces, by its length and digest.
fn holds_component(
    store: &Store,
    origin: Origin,
    development: &ProgramId,
    install: &Install,
) -> bool {
    let same_digest = origin
        == Origin::Development {
            component_sha256: install.digest,
        };
    let same_length = store
        .installed_version(development)
        .map(|version| version.component())
        .and_then(|component| std::fs::metadata(component).ok())
        .is_some_and(|metadata| metadata.len() == install.length);
    same_digest && same_length
}

/// Installs the build `manifest` and `install` describe, its component `component`, or the one
/// installed already where WaOS held it, unless it is installed as it is.
async fn put_in_place(
    context: &InstallContext,
    manifest: &Manifest,
    install: &Install,
    component: Option<Vec<u8>>,
) -> anyhow::Result<()> {
    let store = &context.store;
    let icon = install.icon.as_deref();
    if store.holds_development(manifest, &install.digest, icon) {
        tracing::info!(id = %install.id, "its development build is installed as it is");
        return Ok(());
    }
    let component = match component {
        Some(component) => component,
        None => {
            let held = store
                .installed_version(&manifest.id().development())
                .context("the development install went meanwhile")?
                .component();
            tokio::task::spawn_blocking(move || std::fs::read(held))
                .await
                .context("the read's thread")??
        }
    };
    let installed = context
        .packages
        .install(Installable::Development {
            manifest: manifest.clone(),
            component,
            icon: install.icon.clone(),
        })
        .await?;
    tracing::info!(id = %installed.manifest.id(), "installed from its source");
    Ok(())
}

/// Serves a connection's `Uninstall`: the installed program `id` removed, with its settings and
/// its own folders, its run stopped first, or why not.
pub(crate) async fn uninstall(
    mut connection: ServerConnection,
    id: &ProgramId,
    context: &InstallContext,
) {
    let answer = match remove(context, id).await {
        Ok(()) => UninstallAnswer::Uninstalled,
        Err(refusal) => UninstallAnswer::Refused(refusal),
    };
    // A client gone meanwhile has nobody to tell.
    let _ = connection.send(&answer).await;
}

/// Removes the installed program `id`, its run stopped first.
async fn remove(context: &InstallContext, id: &ProgramId) -> Result<(), Refusal> {
    if context.store.installed_program(id).is_none() {
        return Err(dev_refusal::not_installed(id));
    }
    context
        .packages
        .remove(id)
        .await
        .map_err(|error| Refusal::new(RefusalKind::Failed, format!("{error:#}")))?;
    tracing::info!(%id, "uninstalled by waos");
    Ok(())
}
