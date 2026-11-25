//! The dev server announced on the local network as Linux announces its services: through Avahi,
//! over D-Bus, as an entry group of the connection's own, named as Avahi names this computer; and
//! where no Avahi answers, through an mDNS responder of WaOS's own (mdns-sd).
//!
//! The invariant a later editor will break: Avahi keeps an entry group as long as the D-Bus
//! connection that made it, so the connection lives in the announcement and goes with it.

use std::io;

use mdns_sd::{ServiceDaemon, ServiceInfo};
use zbus::zvariant::OwnedObjectPath;

/// Avahi's "any interface" and "any protocol".
const ANY: i32 = -1;

#[zbus::proxy(
    interface = "org.freedesktop.Avahi.Server",
    default_service = "org.freedesktop.Avahi",
    default_path = "/"
)]
trait AvahiServer {
    fn entry_group_new(&self) -> zbus::Result<OwnedObjectPath>;
    fn get_host_name(&self) -> zbus::Result<String>;
}

#[zbus::proxy(
    interface = "org.freedesktop.Avahi.EntryGroup",
    default_service = "org.freedesktop.Avahi"
)]
trait AvahiEntryGroup {
    #[allow(clippy::too_many_arguments)]
    fn add_service(
        &self,
        interface: i32,
        protocol: i32,
        flags: u32,
        name: &str,
        service_type: &str,
        domain: &str,
        host: &str,
        port: u16,
        txt: Vec<Vec<u8>>,
    ) -> zbus::Result<()>;
    fn commit(&self) -> zbus::Result<()>;
}

/// The dev server's service on the network, there until dropped.
pub(crate) enum DevServerAnnouncement {
    /// Avahi's entry group, freed with the connection that made it.
    Avahi(zbus::Connection),
    /// WaOS's own responder, which unregisters the service as it shuts down.
    Responder(ServiceDaemon),
}

impl Drop for DevServerAnnouncement {
    fn drop(&mut self) {
        if let DevServerAnnouncement::Responder(daemon) = self {
            // The daemon says goodbye on the network for each service it shuts down with.
            let _ = daemon.shutdown();
        }
    }
}

/// Announces `service` on `port`, its TXT record `txt`, named as this computer is: through Avahi,
/// or WaOS's own responder where Avahi does not answer.
pub(crate) async fn announce_dev_server(
    service: &str,
    port: u16,
    txt: &[(&str, String)],
) -> io::Result<DevServerAnnouncement> {
    match through_avahi(service, port, txt).await {
        Ok(connection) => Ok(DevServerAnnouncement::Avahi(connection)),
        Err(error) => {
            tracing::info!(%error, "Avahi does not announce the dev server: WaOS announces it itself");
            through_own_responder(service, port, txt).map(DevServerAnnouncement::Responder)
        }
    }
}

async fn through_avahi(
    service: &str,
    port: u16,
    txt: &[(&str, String)],
) -> zbus::Result<zbus::Connection> {
    let connection = zbus::Connection::system().await?;
    let server = AvahiServerProxy::new(&connection).await?;
    let name = server.get_host_name().await?;
    let group = AvahiEntryGroupProxy::builder(&connection)
        .path(server.entry_group_new().await?)?
        .build()
        .await?;
    let txt = txt
        .iter()
        .map(|(key, value)| format!("{key}={value}").into_bytes())
        .collect();
    group
        .add_service(ANY, ANY, 0, &name, service, "", "", port, txt)
        .await?;
    group.commit().await?;
    Ok(connection)
}

fn through_own_responder(
    service: &str,
    port: u16,
    txt: &[(&str, String)],
) -> io::Result<ServiceDaemon> {
    let host = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|name| name.trim().to_owned())
        .unwrap_or_else(|_| "waos".to_owned());
    let daemon = ServiceDaemon::new().map_err(io::Error::other)?;
    let info = ServiceInfo::new(
        &format!("{service}.local."),
        &host,
        &format!("{host}.local."),
        "",
        port,
        txt,
    )
    .map_err(io::Error::other)?
    .enable_addr_auto();
    daemon.register(info).map_err(io::Error::other)?;
    Ok(daemon)
}
