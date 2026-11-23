//! `waos install`'s side of an install: a program built from its source sent to the WaOS on
//! this machine, which installs it as its development install, under its id followed by
//! `.development`, a program of its own beside the one published under the plain id (the design's
//! decision 70). The component goes over in parts when the development install does not hold it
//! already, so a component of any size installs; the install says how much it is sending when
//! that takes noticeable time.
//!
//! The invariant: what is installed is what was built: the component's length and SHA-256 go
//! first, and WaOS checks the parts against them.

use anyhow::{Context, Result, bail};
use waos_dev_protocol::{
    ClientStream, Connection, Install, InstallAnswer, Request, Upload, component_digest,
};
use waos_package::{ProgramId, ProgramKind, Version};

use crate::asking;

/// Over this many bytes, sending a component takes noticeable time, so the install says how much
/// it is sending. A debug build is hundreds of megabytes; a release one is under this.
const SAID_OVER_BYTES: usize = 64 << 20;

/// A program built from its source, as an install sends it.
pub struct InstallBuild {
    pub id: ProgramId,
    pub kind: ProgramKind,
    /// The name its user sees, which WaOS shows followed by " (Development)".
    pub name: String,
    pub version: Version,
    /// Its icon's PNG, where its metadata names one.
    pub icon: Option<Vec<u8>>,
    pub component: Vec<u8>,
}

/// Installs `build` over `connection` as its program's development install, after saying its
/// hello; answers the id it is installed under.
pub async fn install_on(
    connection: Connection<ClientStream>,
    build: &InstallBuild,
) -> Result<ProgramId> {
    let request = Request::Install(Install {
        id: build.id.clone(),
        kind: build.kind,
        name: build.name.clone(),
        version: build.version.clone(),
        icon: build.icon.clone(),
        length: build.component.len() as u64,
        digest: component_digest(&build.component),
    });
    let mut connection = asking::greeted(connection, &request).await?;
    let name = &build.name;
    connection
        .send(&request)
        .await
        .with_context(|| format!("sending {name} to WaOS"))?;
    upload(&mut connection, name, &build.component).await?;
    match connection
        .receive::<InstallAnswer>()
        .await
        .context("reading from WaOS")?
    {
        Some(InstallAnswer::Installed { id }) => Ok(id),
        Some(InstallAnswer::Refused(refusal)) => bail!("WaOS refused {name}: {}", refusal.message),
        Some(InstallAnswer::Unknown) => bail!(
            "WaOS answered the install in a way this waos does not know: update waos with `curl -fsSL https://waos.app/install.sh | sh`"
        ),
        None => bail!("WaOS closed the connection before {name} was installed"),
    }
}

/// Sends `component` unless WaOS answers that the development install holds it.
async fn upload(
    connection: &mut Connection<ClientStream>,
    name: &str,
    component: &[u8],
) -> Result<()> {
    match connection
        .receive::<Upload>()
        .await
        .context("reading from WaOS")?
    {
        Some(Upload::Send) => {
            if component.len() > SAID_OVER_BYTES {
                eprintln!(
                    "waos: sending {name} to WaOS, {} MiB",
                    component.len() >> 20
                );
            }
            connection
                .send_component(component)
                .await
                .with_context(|| format!("sending {name} to WaOS"))
        }
        // The development install holds this component already: nothing is sent.
        Some(Upload::Have) => Ok(()),
        Some(Upload::Refused(refusal)) => bail!("WaOS refused {name}: {}", refusal.message),
        Some(Upload::Unknown) => bail!(
            "WaOS answered the upload in a way this waos does not know: update waos with `curl -fsSL https://waos.app/install.sh | sh`"
        ),
        None => bail!("WaOS closed the connection before {name} was sent"),
    }
}
