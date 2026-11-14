//! `cargo waos tap`, `press`, `drag`, `type`, `key` and `scroll`: input to a window of the program
//! a run of `cargo waos run` runs, sent to the WaOS running it, which gives it to the program as
//! its user would give it and answers once the window shows the picture after it, so a screenshot
//! taken next shows what it did (the design's decision 60). A point is in the window's pixels, a
//! screenshot's, from its top-left, written `<x>,<y>`.
//!
//! The invariant: a command returns only once WaOS has answered, the input given or why not.

use std::process::ExitCode;
use std::str::FromStr;

use anyhow::{Result, bail};
use waos_dev_protocol::{FromWaOS, Input, ToWaOS};
use waos_package::ProgramId;

use crate::asking::ask;
use crate::unreached;

/// Two numbers written `<a>,<b>`: a point in a window's pixels, or how far to scroll.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pair(pub f64, pub f64);

impl FromStr for Pair {
    type Err = String;

    fn from_str(text: &str) -> Result<Pair, String> {
        let not_a_pair = || format!("{text:?} is not two numbers written <a>,<b>, such as 120,48");
        let (a, b) = text.split_once(',').ok_or_else(not_a_pair)?;
        let number = |part: &str| part.trim().parse::<f64>().ok().filter(|n| n.is_finite());
        match (number(a), number(b)) {
            (Some(a), Some(b)) => Ok(Pair(a, b)),
            _ => Err(not_a_pair()),
        }
    }
}

/// Gives `input` to the window numbered `window` of the program `id`, and waits for WaOS's answer.
pub async fn give(id: ProgramId, window: u32, input: Input) -> Result<ExitCode> {
    let answer = ask(&ToWaOS::Input {
        id: id.clone(),
        window,
        input,
    })
    .await?;
    match answer {
        FromWaOS::InputDone { new_picture: true } => Ok(ExitCode::SUCCESS),
        FromWaOS::InputDone { new_picture: false } => {
            eprintln!(
                "cargo waos: window {window} showed no new picture after the input: it changed nothing the program draws, or the window is out of sight"
            );
            Ok(ExitCode::SUCCESS)
        }
        FromWaOS::Unreached(why) => {
            eprintln!("cargo waos: {}", unreached::said(&why, &id, window));
            Ok(ExitCode::FAILURE)
        }
        FromWaOS::Refused(why) => bail!("WaOS refused the input: {why}"),
        other => bail!("WaOS answered {other:?}, not whether it gave the input"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pair_is_two_numbers_either_signed_with_a_comma_between() {
        assert_eq!("120,48".parse::<Pair>().unwrap(), Pair(120.0, 48.0));
        assert_eq!("-0.5, 200".parse::<Pair>().unwrap(), Pair(-0.5, 200.0));
        for text in ["120", "120,", ",48", "a,b", "1,2,3", "inf,1"] {
            assert!(text.parse::<Pair>().is_err(), "{text}");
        }
    }
}
