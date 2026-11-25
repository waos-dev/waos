//! `waos pair <device>`, `waos unpair <device>` and `waos devices`: this computer paired with a
//! device's WaOS, by the code the device shows under Developer access, and the devices found on
//! the local network and paired with. Pairing agrees a key with the device over the code (SPAKE2), so the code never
//! crosses the network; the device proves with the key which certificate it answered with, and
//! this computer keeps that certificate's fingerprint and the token the key makes in
//! `devices.toml`, which every later command to the device uses (`--device`).
//!
//! The invariant a later editor will break: the fingerprint is kept only once the device's proof
//! of it holds, so a machine that answered in the device's place, which saw no code, is never
//! trusted after.

use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use waos_dev_protocol::{
    ClientConnection, DeviceAddress, Fingerprint, Hello, PairAnswer, PairConfirm, PairStart,
    Pairing, PairingCode, PairingKey, Request, connect_across_network,
};

use crate::asking::{self, BUILD};
use crate::devices::{Devices, PairedDevice};
use crate::found::{self, BROWSE, FoundDevice};

/// Pairs this computer with the WaOS `named` names, a device found on the local network or an
/// address, by `code`, or the code typed when none is given, and keeps the device as `named`.
pub async fn pair(named: &str, code: Option<&str>) -> Result<ExitCode> {
    let address = address_of(named)?;
    if !address.secure {
        bail!(
            "a pairing goes over wss://, whose certificate it learns: pair with wss://{}:{}",
            address.host,
            address.port
        );
    }
    let code = match code {
        Some(code) => PairingCode::parse(code).map_err(anyhow::Error::msg)?,
        None => typed_code(named)?,
    };
    let (connection, seen) = connect_across_network(&address, None)
        .await
        .with_context(|| format!("connecting to WaOS at {address}"))?;
    let fingerprint = seen.context("the device answered with no certificate")?;
    let key = paired(connection, &code, fingerprint).await?;
    let device = PairedDevice {
        name: named.to_owned(),
        address,
        fingerprint,
        token: key.token(),
    };
    Devices::of_this_user()?.keep(device)?;
    eprintln!("waos: paired with {named}; `waos --device {named} run` runs a program there");
    Ok(ExitCode::SUCCESS)
}

/// The address of the device `named` names: the one found on the local network by that name, or
/// else `named` read as an address.
fn address_of(named: &str) -> Result<DeviceAddress> {
    let found = found::browse(BROWSE, |found| {
        found.iter().any(|device| device.name == named)
    })?;
    if let Some(device) = found.into_iter().find(|device| device.name == named) {
        return Ok(device.address);
    }
    named.parse().map_err(|why| {
        anyhow::anyhow!(
            "no device on the local network is named {named}, and it is no address: {why}"
        )
    })
}

/// The key the exchange over `connection` agrees with the device by `code`, once the device's
/// proof of the certificate `fingerprint` names holds and it says the two are paired.
async fn paired(
    connection: ClientConnection,
    code: &PairingCode,
    fingerprint: Fingerprint,
) -> Result<PairingKey> {
    let (pairing, ours) = Pairing::start(code);
    let request = Request::Pair(PairStart {
        computer: this_computer(),
        spake: ours,
    });
    let mut connection = asking::greeted_with(connection, &Hello::command(BUILD), &request).await?;
    connection.send(&request).await.context("sending to WaOS")?;
    let (spake, confirmation) = match answer(&mut connection).await? {
        PairAnswer::Spake {
            spake,
            confirmation,
        } => (spake, confirmation),
        other => return Err(unexpected(other)),
    };
    let key = pairing
        .finish(&spake)
        .context("the device's half of the pairing")?;
    if !key.holds_for_device(fingerprint, &confirmation) {
        bail!(
            "the code is not the one the device shows, or another machine answered in its place: type the six digits the device shows now"
        );
    }
    let confirm = PairConfirm {
        confirmation: key.computer_proof(),
    };
    connection.send(&confirm).await.context("sending to WaOS")?;
    match answer(&mut connection).await? {
        PairAnswer::Paired => Ok(key),
        other => Err(unexpected(other)),
    }
}

async fn answer(connection: &mut ClientConnection) -> Result<PairAnswer> {
    connection
        .receive::<PairAnswer>()
        .await
        .context("reading from WaOS")?
        .context("WaOS closed the connection before the two were paired")
}

/// What an answer out of turn comes to: the device's refusal, or a newer WaOS's word.
fn unexpected(answer: PairAnswer) -> anyhow::Error {
    match answer {
        PairAnswer::Refused(refusal) => anyhow::anyhow!("{refusal}"),
        _ => anyhow::anyhow!(
            "WaOS answered the pairing in a way this waos does not know: update waos with `curl -fsSL https://waos.app/install.sh | sh`"
        ),
    }
}

/// The code the user types, asked for on stderr.
fn typed_code(named: &str) -> Result<PairingCode> {
    eprint!(
        "waos: type the code {named} shows (in WaOS's settings, under Developer access, Pair a computer): "
    );
    io::stderr().flush().context("writing stderr")?;
    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .context("reading the code")?;
    PairingCode::parse(line.trim()).map_err(anyhow::Error::msg)
}

/// This computer's name, as the device lists it among the computers paired.
fn this_computer() -> String {
    let name = gethostname::gethostname().to_string_lossy().into_owned();
    name.strip_suffix(".local").unwrap_or(&name).to_owned()
}

/// Forgets the device `named`, by its name or address: this computer reaches it no longer.
pub fn unpair(named: &str) -> Result<ExitCode> {
    if !Devices::of_this_user()?.forget(named)? {
        bail!("this computer is not paired with {named}: `waos devices` lists the devices it is");
    }
    eprintln!(
        "waos: forgot {named}; its WaOS lists this computer until it is removed there, in its settings under Developer access"
    );
    Ok(ExitCode::SUCCESS)
}

/// Prints the devices found on the local network and those paired with, a line each: its name,
/// its address, and whether it is paired.
pub fn devices() -> Result<ExitCode> {
    let paired = Devices::of_this_user()?.paired()?;
    let found = found::browse(BROWSE, |_| false)?;
    let lines = device_lines(&found, &paired);
    for line in &lines {
        println!("{line}");
    }
    if lines.is_empty() {
        eprintln!(
            "waos: no device is found or paired: turn on Developer access in a device's WaOS settings, then `waos pair <its name>`"
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// The lines `devices` prints of `found` and `paired`: each device found, as paired where its
/// fingerprint is one paired with, then each paired device not found.
fn device_lines(found: &[FoundDevice], paired: &[PairedDevice]) -> Vec<String> {
    let paired_as = |device: &FoundDevice| {
        paired
            .iter()
            .find(|kept| kept.fingerprint == device.announced.fingerprint)
    };
    let found_lines = found.iter().map(|device| match paired_as(device) {
        Some(kept) => format!(
            "{}  {}  paired as {}",
            device.name, device.address, kept.name
        ),
        None => format!("{}  {}  not paired", device.name, device.address),
    });
    let unseen = paired.iter().filter(|kept| {
        !found
            .iter()
            .any(|device| device.announced.fingerprint == kept.fingerprint)
    });
    let paired_lines =
        unseen.map(|kept| format!("{}  {}  paired, not found now", kept.name, kept.address));
    found_lines.chain(paired_lines).collect()
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr};

    use waos_dev_protocol::{Identity, NetworkListener};

    use super::*;

    /// A device of the test's own on the loopback, showing `shown`, answering one pairing as a
    /// WaOS does: its address and fingerprint, and the token it kept, once the pairing ended.
    async fn device(
        shown: &'static str,
    ) -> (
        DeviceAddress,
        Fingerprint,
        tokio::task::JoinHandle<Option<String>>,
    ) {
        let identity = Identity::make().unwrap();
        let fingerprint = identity.fingerprint();
        let loopback = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let mut listener = NetworkListener::bind_at(&identity, loopback).await.unwrap();
        let address = format!("wss://127.0.0.1:{}", listener.port())
            .parse()
            .unwrap();
        let served = tokio::spawn(async move {
            let mut connection = listener.accept().await.unwrap().handshake().await.unwrap();
            connection
                .accept_hello(&Hello::waos("WaOS test"))
                .await
                .unwrap();
            let Some(Request::Pair(start)) = connection.receive::<Request>().await.unwrap() else {
                panic!("a pairing");
            };
            let (pairing, ours) = Pairing::start(&PairingCode::parse(shown).unwrap());
            let key = pairing.finish(&start.spake).unwrap();
            let spake = PairAnswer::Spake {
                spake: ours,
                confirmation: key.device_proof(fingerprint),
            };
            connection.send(&spake).await.unwrap();
            let confirm = connection.receive::<PairConfirm>().await.ok()??;
            if !key.holds_for_computer(&confirm.confirmation) {
                return None;
            }
            connection.send(&PairAnswer::Paired).await.unwrap();
            Some(key.token().as_str().to_owned())
        });
        (address, fingerprint, served)
    }

    async fn connected(address: &DeviceAddress) -> (ClientConnection, Fingerprint) {
        let (connection, seen) = connect_across_network(address, None).await.unwrap();
        (connection, seen.unwrap())
    }

    #[test]
    fn a_device_found_is_listed_as_paired_by_its_fingerprint_and_one_not_found_after() {
        let found = |name: &str, fingerprint: u8| FoundDevice {
            name: name.to_owned(),
            address: "192.168.1.20".parse().unwrap(),
            announced: waos_dev_protocol::Announced::new(Fingerprint([fingerprint; 32]), "macos"),
        };
        let kept = |name: &str, fingerprint: u8| PairedDevice {
            name: name.to_owned(),
            address: "192.168.1.9".parse().unwrap(),
            fingerprint: Fingerprint([fingerprint; 32]),
            token: waos_dev_protocol::Token::new("t"),
        };
        let lines = device_lines(
            &[found("Mac mini", 1), found("Pixel", 2)],
            &[kept("mini", 1), kept("laptop", 3)],
        );
        assert_eq!(
            lines,
            [
                "Mac mini  wss://192.168.1.20:7351  paired as mini",
                "Pixel  wss://192.168.1.20:7351  not paired",
                "laptop  wss://192.168.1.9:7351  paired, not found now",
            ]
        );
    }

    #[tokio::test]
    async fn the_code_shown_pairs_and_both_sides_hold_the_same_token() {
        let (address, fingerprint, served) = device("123456").await;
        let (connection, seen) = connected(&address).await;
        assert_eq!(seen, fingerprint);
        let code = PairingCode::parse("123 456").unwrap();
        let key = paired(connection, &code, seen).await.unwrap();
        assert_eq!(served.await.unwrap().as_deref(), Some(key.token().as_str()));
    }

    #[tokio::test]
    async fn another_code_pairs_nothing_and_says_to_type_the_one_shown() {
        let (address, _, served) = device("123456").await;
        let (connection, seen) = connected(&address).await;
        let code = PairingCode::parse("654321").unwrap();
        let refused = paired(connection, &code, seen).await.unwrap_err();
        assert!(format!("{refused:#}").contains("six digits"), "{refused:#}");
        assert_eq!(served.await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_device_whose_proof_names_another_certificate_is_not_trusted() {
        let (address, _, served) = device("123456").await;
        let (connection, _) = connected(&address).await;
        let code = PairingCode::parse("123456").unwrap();
        let refused = paired(connection, &code, Fingerprint([7; 32]))
            .await
            .unwrap_err();
        assert!(
            format!("{refused:#}").contains("another machine"),
            "{refused:#}"
        );
        assert_eq!(served.await.unwrap(), None);
    }
}
