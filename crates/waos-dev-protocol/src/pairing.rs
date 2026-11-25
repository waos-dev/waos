//! Pairing a computer with a device's WaOS, as Android pairs a computer for wireless debugging:
//! the device shows a six-digit code, the computer's user types it into `waos pair`, and the two
//! agree a key from it with SPAKE2 without the code crossing the network. From the key each side
//! derives the computer's token, which the computer sends in every later hello and the device
//! keeps a digest of; and the device proves, with the key, which certificate it answered with,
//! so the computer pins that one.
//!
//! The messages: the computer's `Pair(PairStart)`; the device's [`PairAnswer::Spake`], with its
//! half of SPAKE2 and its proof of the certificate; the computer's [`PairConfirm`], its own
//! proof of the key, which a wrong code fails; and the device's [`PairAnswer::Paired`].
//!
//! The invariants a later editor will break:
//! - The code never leaves either side, and the token never crosses the network: each side
//!   derives it from the key.
//! - The computer keeps the certificate's fingerprint only once the device's proof of it holds,
//!   so one that answered in the device's place is never pinned.
//! - Each proof names its side, so one side's proof never passes for the other's.

use std::fmt;
use std::io;

use aws_lc_rs::{constant_time, hmac, rand};
use serde::{Deserialize, Serialize};
use spake2::{Ed25519Group, Identity, Password, Spake2};

use crate::adjacent::adjacent_unknown;
use crate::network::Fingerprint;
use crate::refusal::Refusal;
use crate::token::Token;

/// What both sides name the exchange: one pairing of WaOS developer access.
const EXCHANGE: &[u8] = b"waos developer access pairing";

/// How many digits a code has.
const CODE_DIGITS: u32 = 6;

/// The computer's first word: who it is, as the device lists it, and its half of SPAKE2.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairStart {
    /// The computer's name, as the device's list of paired computers shows it.
    pub computer: String,
    #[serde(with = "serde_bytes")]
    pub spake: Vec<u8>,
}

/// The computer's proof that it holds the key the code made.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairConfirm {
    #[serde(with = "serde_bytes")]
    pub confirmation: Vec<u8>,
}

/// What a pairing answers: the device's half of SPAKE2 and its proof of its certificate, then,
/// once the computer proved the key, that the two are paired; or why not.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(remote = "Self", tag = "kind", content = "data")]
pub enum PairAnswer {
    Spake {
        #[serde(with = "serde_bytes")]
        spake: Vec<u8>,
        #[serde(with = "serde_bytes")]
        confirmation: Vec<u8>,
    },
    Paired,
    Refused(Refusal),
    #[serde(skip)]
    Unknown,
}

adjacent_unknown!(PairAnswer);

/// The code a device shows for a pairing: six digits.
#[derive(Clone, PartialEq, Eq)]
pub struct PairingCode(String);

impl PairingCode {
    /// A new code, each of its million values as likely.
    pub fn make() -> io::Result<PairingCode> {
        let bound = 10u32.pow(CODE_DIGITS);
        // The largest multiple of `bound` a u32 holds, below which a draw is fair.
        let fair = u32::MAX - u32::MAX % bound;
        loop {
            let mut bytes = [0; 4];
            rand::fill(&mut bytes).map_err(|_| io::Error::other("no randomness"))?;
            let drawn = u32::from_le_bytes(bytes);
            if drawn < fair {
                return Ok(PairingCode(format!("{:06}", drawn % bound)));
            }
        }
    }

    /// The code as typed: six digits, spaces ignored.
    pub fn parse(typed: &str) -> Result<PairingCode, String> {
        let digits: String = typed.chars().filter(|c| !c.is_whitespace()).collect();
        if digits.len() == CODE_DIGITS as usize && digits.chars().all(|c| c.is_ascii_digit()) {
            Ok(PairingCode(digits))
        } else {
            Err(format!(
                "{typed:?} is not a code: it is the six digits the device shows"
            ))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for PairingCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PairingCode(..)")
    }
}

/// One side's half of a pairing, its SPAKE2 begun with the code.
pub struct Pairing(Spake2<Ed25519Group>);

impl Pairing {
    /// Begins a pairing with `code`; answers this side's half of SPAKE2, to send.
    pub fn start(code: &PairingCode) -> (Pairing, Vec<u8>) {
        let (spake, outgoing) = Spake2::<Ed25519Group>::start_symmetric(
            &Password::new(code.as_str()),
            &Identity::new(EXCHANGE),
        );
        (Pairing(spake), outgoing)
    }

    /// The key the two halves agree, once the other's is here. A wrong code still makes one, but
    /// not the other side's: the proofs tell.
    pub fn finish(self, theirs: &[u8]) -> io::Result<PairingKey> {
        let key = self
            .0
            .finish(theirs)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, format!("{error:?}")))?;
        Ok(PairingKey(hmac::Key::new(hmac::HMAC_SHA256, &key)))
    }
}

/// The key a pairing agreed, from which each side derives the token and the proofs.
pub struct PairingKey(hmac::Key);

impl fmt::Debug for PairingKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PairingKey(..)")
    }
}

impl PairingKey {
    /// The computer's token: what it sends in each hello from now on.
    pub fn token(&self) -> Token {
        Token::new(hex::encode(self.mac(b"token", &[])))
    }

    /// The device's proof that it holds the key and answered with the certificate `fingerprint`
    /// names.
    pub fn device_proof(&self, fingerprint: Fingerprint) -> Vec<u8> {
        self.mac(b"device", &fingerprint.0)
    }

    /// The computer's proof that it holds the key.
    pub fn computer_proof(&self) -> Vec<u8> {
        self.mac(b"computer", &[])
    }

    /// Whether `proof` is the device's for the certificate `fingerprint` names.
    pub fn holds_for_device(&self, fingerprint: Fingerprint, proof: &[u8]) -> bool {
        constant_time::verify_slices_are_equal(&self.device_proof(fingerprint), proof).is_ok()
    }

    /// Whether `proof` is the computer's.
    pub fn holds_for_computer(&self, proof: &[u8]) -> bool {
        constant_time::verify_slices_are_equal(&self.computer_proof(), proof).is_ok()
    }

    fn mac(&self, side: &[u8], subject: &[u8]) -> Vec<u8> {
        let mut context = hmac::Context::with_key(&self.0);
        context.update(side);
        context.update(subject);
        context.sign().as_ref().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paired(device_code: &str, computer_code: &str) -> (PairingKey, PairingKey) {
        let (device, from_device) = Pairing::start(&PairingCode::parse(device_code).unwrap());
        let (computer, from_computer) = Pairing::start(&PairingCode::parse(computer_code).unwrap());
        (
            device.finish(&from_computer).unwrap(),
            computer.finish(&from_device).unwrap(),
        )
    }

    #[test]
    fn the_same_code_on_both_sides_makes_the_same_token_and_proofs_that_hold() {
        let (device, computer) = paired("123456", "123 456");
        let fingerprint = Fingerprint([7; 32]);
        assert!(computer.holds_for_device(fingerprint, &device.device_proof(fingerprint)));
        assert!(device.holds_for_computer(&computer.computer_proof()));
        assert!(device.token() == computer.token());
    }

    #[test]
    fn a_wrong_code_makes_proofs_that_do_not_hold() {
        let (device, computer) = paired("123456", "654321");
        let fingerprint = Fingerprint([7; 32]);
        assert!(!computer.holds_for_device(fingerprint, &device.device_proof(fingerprint)));
        assert!(!device.holds_for_computer(&computer.computer_proof()));
    }

    #[test]
    fn the_device_proof_names_its_certificate_and_is_not_the_computers() {
        let (device, computer) = paired("123456", "123456");
        let proof = device.device_proof(Fingerprint([7; 32]));
        assert!(!computer.holds_for_device(Fingerprint([8; 32]), &proof));
        assert!(!device.holds_for_computer(&proof));
    }

    #[test]
    fn a_code_is_six_digits() {
        let code = PairingCode::make().unwrap();
        assert_eq!(code.as_str().len(), 6);
        assert!(PairingCode::parse("12345").is_err());
        assert!(PairingCode::parse("12345a").is_err());
    }
}
