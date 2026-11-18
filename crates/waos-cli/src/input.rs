//! `cargo waos tap`, `press`, `drag`, `type`, `key` and `scroll`: input to a window of the program
//! a run of `cargo waos run` runs, sent to the WaOS running it, which gives it to the program as
//! its user would give it and answers once the window shows the picture after it, or once it
//! knows the window was out of sight or drew nothing new, so a screenshot taken next shows what
//! it did (the design's decision 60). A point is in the window's pixels, a screenshot's, from its
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
        InputAnswer::Done(InputDone {
            in_sight,
            new_picture,
        }) => {
            if let Some(said) = said(in_sight, new_picture, window) {
                eprintln!("cargo waos: {said}");
            }
            Ok(ExitCode::SUCCESS)
        }
        InputAnswer::Refused(refusal) => {
            eprintln!("cargo waos: {}", refusal.message);
            Ok(ExitCode::FAILURE)
        }
        InputAnswer::Unknown => bail!(
            "WaOS answered the input in a way this cargo waos does not know: update cargo waos with `cargo install cargo-waos`"
        ),
    }
}

/// What the answer to an input says, in words: none where the window drew its new picture; else
/// whether the window was in sight and why no new picture came.
fn said(in_sight: bool, new_picture: bool, window: Option<WindowId>) -> Option<String> {
    let named = match window {
        Some(window) => format!("window {window}"),
        None => "the window".to_owned(),
    };
    match (in_sight, new_picture) {
        (true, true) => None,
        (true, false) => Some(format!(
            "{named} was in sight, and the program drew nothing new after the input"
        )),
        (false, true) => Some(format!(
            "{named} was out of sight when the input was given, and showed the program's new picture after it"
        )),
        (false, false) => Some(format!(
            "{named} was out of sight when the input was given, so it showed no new picture; bring it into sight to see what it drew"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_input_says_whether_its_window_was_in_sight_and_why_no_new_picture_came() {
        assert_eq!(
            said(true, true, Some(WindowId(1))),
            None,
            "its new picture says enough"
        );
        assert_eq!(
            said(true, false, Some(WindowId(2))).unwrap(),
            "window 2 was in sight, and the program drew nothing new after the input"
        );
        assert_eq!(
            said(false, false, Some(WindowId(3))).unwrap(),
            "window 3 was out of sight when the input was given, so it showed no new picture; bring it into sight to see what it drew"
        );
        assert_eq!(
            said(false, true, Some(WindowId(4))).unwrap(),
            "window 4 was out of sight when the input was given, and showed the program's new picture after it"
        );
        assert_eq!(
            said(true, false, None).unwrap(),
            "the window was in sight, and the program drew nothing new after the input"
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
