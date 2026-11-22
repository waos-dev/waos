//! `waos ui tap`, `press`, `drag`, `type`, `key` and `scroll`: input to a window of the program
//! a run of `waos run` runs, sent to the WaOS running it, which gives it to the program as
//! its user would give it and answers once it has handed it over, saying whether the window was in
//! sight (the design's decision 60). What the program draws in answer, a screenshot shows once it
//! has drawn it. A point is in the window's pixels, a screenshot's, from its
//! top-left, written `<x>,<y>`.
//!
//! The invariant: a command returns only once WaOS has answered, the input given or why not.

use std::process::ExitCode;
use std::str::FromStr;

use anyhow::{Result, bail};
use waos_dev_protocol::{Input, InputAnswer, InputDone, Request, WindowId};
use waos_package::ProgramId;

use crate::asking::ask;

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

/// Gives `input` to the window `window` of the program `id`, and waits for WaOS's answer.
pub async fn give(id: ProgramId, window: Option<WindowId>, input: Input) -> Result<ExitCode> {
    let answer = ask(&Request::Input { id, window, input }).await?;
    match answer {
        InputAnswer::Done(InputDone { in_sight }) => {
            if !in_sight {
                eprintln!("waos: {}", out_of_sight(window));
            }
            Ok(ExitCode::SUCCESS)
        }
        InputAnswer::Refused(refusal) => {
            eprintln!("waos: {}", refusal.message);
            Ok(ExitCode::FAILURE)
        }
        InputAnswer::Unknown => bail!(
            "WaOS answered the input in a way this waos does not know: update waos with `curl -fsSL https://waos.app/install.sh | sh`"
        ),
    }
}

/// What an input to a window out of sight comes to, in words.
fn out_of_sight(window: Option<WindowId>) -> String {
    let named = match window {
        Some(window) => format!("window {window}"),
        None => "the window".to_owned(),
    };
    format!(
        "{named} was out of sight when the input was given, so it draws nothing new until it is in sight"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_input_to_a_window_out_of_sight_says_so() {
        assert_eq!(
            out_of_sight(Some(WindowId(3))),
            "window 3 was out of sight when the input was given, so it draws nothing new until it is in sight"
        );
        assert_eq!(
            out_of_sight(None),
            "the window was out of sight when the input was given, so it draws nothing new until it is in sight"
        );
    }

    #[test]
    fn a_pair_is_two_numbers_either_signed_with_a_comma_between() {
        assert_eq!("120,48".parse::<Pair>().unwrap(), Pair(120.0, 48.0));
        assert_eq!("-0.5, 200".parse::<Pair>().unwrap(), Pair(-0.5, 200.0));
        for text in ["120", "120,", ",48", "a,b", "1,2,3", "inf,1"] {
            assert!(text.parse::<Pair>().is_err(), "{text}");
        }
    }
}
