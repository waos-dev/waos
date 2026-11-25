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
    ClientConnection, Install, InstallAnswer, Request, Upload, component_digest,
};
use waos_package::{Category, FileAssociation, ProgramId, ProgramKind, Version};

use crate::asking;
use crate::connecting::{WaOSStarter, connect_or_start};
use crate::destination::Destination;

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
    /// What it is for, where its metadata says.
    pub category: Option<Category>,
    /// The kinds of file it opens; none where its metadata declares none.
    pub file_associations: Vec<FileAssociation>,
    pub component: Vec<u8>,
}

/// Installs `build` as its program's development install, in the WaOS running or the one started
/// when none answers, and says what it is installed as.
pub async fn install(
    destination: &Destination,
    build: &InstallBuild,
    starter: &dyn WaOSStarter,
) -> Result<()> {
    let connection = connect_or_start(destination, starter).await?;
    let id = install_on(connection, destination, build).await?;
    eprintln!(
        "waos: installed {} as {id}; `waos launch` starts it",
        build.name
    );
    println!("{id}");
    Ok(())
}

/// Installs `build` over `connection` to `destination` as its program's development install,
/// after saying its hello; answers the id it is installed under.
pub async fn install_on(
    connection: ClientConnection,
    destination: &Destination,
    build: &InstallBuild,
) -> Result<ProgramId> {
    let request = Request::Install(Install {
        id: build.id.clone(),
        kind: build.kind,
        name: build.name.clone(),
        version: build.version.clone(),
        icon: build.icon.clone(),
        category: build.category.map(|category| category.as_str().to_owned()),
        file_associations: build.file_associations.clone(),
        length: build.component.len() as u64,
        digest: component_digest(&build.component),
    });
    let mut connection = asking::greeted(connection, destination, &request).await?;
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
async fn upload(connection: &mut ClientConnection, name: &str, component: &[u8]) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use waos_dev_protocol::{Component, Hello, Listener, TestPlace};

    use super::*;

    fn build() -> InstallBuild {
        InstallBuild {
            id: "dev.example.notes".parse().unwrap(),
            kind: ProgramKind::App,
            name: "Notes".to_owned(),
            version: "0.1.0".parse().unwrap(),
            icon: Some(b"png".to_vec()),
            category: Some(Category::Productivity),
            file_associations: Vec::new(),
            component: b"component bytes".to_vec(),
        }
    }

    /// Installs `build()` against a WaOS of the test's own at `place`, which answers the upload
    /// with `upload`, and answers what it received: the install and the component, if one came.
    async fn installed_with(place: &str, upload: Upload) -> (Install, Option<Vec<u8>>) {
        let place = TestPlace::new(place);
        let socket = place.path().to_owned();
        let mut listener = Listener::bind(&socket).await.unwrap();
        let served = tokio::spawn(async move {
            let mut server = listener.accept().await.unwrap();
            server
                .accept_hello(&Hello::waos("WaOS 0.3.0"))
                .await
                .unwrap();
            let Some(Request::Install(install)) = server.receive::<Request>().await.unwrap() else {
                panic!("an install");
            };
            let send = upload == Upload::Send;
            server.send(&upload).await.unwrap();
            let component = if send {
                Some(
                    server
                        .receive_component(install.length, &install.digest)
                        .await
                        .unwrap(),
                )
            } else {
                None
            };
            server
                .send(&InstallAnswer::Installed {
                    id: install.id.development(),
                })
                .await
                .unwrap();
            // Nothing more comes after the answer.
            assert_eq!(server.receive::<Component>().await.unwrap(), None);
            (install, component)
        });
        let client = ClientConnection::connect(&socket).await.unwrap();
        let destination = Destination::ThisMachine { socket };
        let id = install_on(client, &destination, &build()).await.unwrap();
        assert_eq!(id.as_str(), "dev.example.notes.development");
        served.await.unwrap()
    }

    #[tokio::test]
    async fn an_install_sends_the_build_and_its_component_when_waos_asks_for_it() {
        let (install, component) = installed_with("install-send", Upload::Send).await;
        assert_eq!(
            (install.name.as_str(), install.version.to_string()),
            ("Notes", "0.1.0".to_owned())
        );
        assert_eq!(install.icon.as_deref(), Some(&b"png"[..]));
        assert_eq!(component.as_deref(), Some(&b"component bytes"[..]));
    }

    #[tokio::test]
    async fn have_skips_the_upload() {
        let (install, component) = installed_with("install-have", Upload::Have).await;
        assert_eq!(install.digest, component_digest(b"component bytes"));
        assert_eq!(component, None, "no component frame sent");
    }
}
