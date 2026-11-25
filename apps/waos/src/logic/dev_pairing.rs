//! A pairing, the device's side: a computer on the network sends its half of SPAKE2 for the code
//! WaOS shows (`waos pair`); the device answers its own half, with its proof of the certificate it
//! answered with; the computer proves it holds the same key, which only the code shown makes, and
//! the device keeps it, by its token's digest, among the computers paired.
//!
//! The invariant a later editor will break: a pairing takes the code before it answers anything,
//! so each one is a try of the code's few whether or not it finishes, and the code never crosses
//! the network.

use std::time::Duration;

use waos_dev_protocol::{
    Locality, PairAnswer, PairConfirm, PairStart, Pairing, Refusal, RefusalKind, ServerConnection,
};

use super::developer_access::DeveloperAccess;

/// How long the computer may take to prove the key once the device answered.
const CONFIRM_WAIT: Duration = Duration::from_secs(10);

/// Pairs the computer `start` names with this device over `connection`, by the code `access`
/// shows, and says how it went.
pub(crate) async fn pair(
    mut connection: ServerConnection,
    start: PairStart,
    access: &DeveloperAccess,
) {
    let answer = match paired(&mut connection, &start, access).await {
        Ok(()) => {
            tracing::info!(computer = %start.computer, "a computer paired for developer access");
            PairAnswer::Paired
        }
        Err(refusal) => {
            tracing::info!(computer = %start.computer, %refusal, "a pairing was refused");
            PairAnswer::Refused(refusal)
        }
    };
    // A computer gone meanwhile has nobody to tell.
    let _ = connection.send(&answer).await;
}

/// The exchange to its end, the computer kept; or why it is refused.
async fn paired(
    connection: &mut ServerConnection,
    start: &PairStart,
    access: &DeveloperAccess,
) -> Result<(), Refusal> {
    if connection.locality() != Locality::Network {
        return Err(from_this_machine());
    }
    let code = access.take_code().ok_or_else(closed)?;
    let identity = access.identity().map_err(failed)?;
    let (pairing, ours) = Pairing::start(&code);
    let key = pairing.finish(&start.spake).map_err(failed)?;
    let spake = PairAnswer::Spake {
        spake: ours,
        confirmation: key.device_proof(identity.fingerprint()),
    };
    connection.send(&spake).await.map_err(failed)?;
    let confirm = tokio::time::timeout(CONFIRM_WAIT, connection.receive::<PairConfirm>())
        .await
        .map_err(|_| failed("the computer took too long to answer"))?
        .map_err(failed)?
        .ok_or_else(|| failed("the computer closed the connection"))?;
    if !key.holds_for_computer(&confirm.confirmation) {
        return Err(wrong_code());
    }
    access.pair(&start.computer, &key.token()).map_err(failed)
}

fn closed() -> Refusal {
    Refusal::new(
        RefusalKind::PairingClosed,
        "the device shows no code: in its WaOS's settings, under Developer access, choose Pair a computer, then type the code it shows",
    )
}

fn wrong_code() -> Refusal {
    Refusal::new(
        RefusalKind::WrongCode,
        "the code is not the one the device shows: type the six digits it shows now",
    )
}

fn from_this_machine() -> Refusal {
    Refusal::new(
        RefusalKind::Failed,
        "a pairing comes from another computer, across the network: `waos` on this machine reaches WaOS without one",
    )
}

fn failed(error: impl std::fmt::Display) -> Refusal {
    Refusal::new(RefusalKind::Failed, format!("the pairing failed: {error}"))
}
