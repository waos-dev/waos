//! `waos inspect`: what the WaOS on this machine knows of what a program does with what it
//! was allowed (the inspector proposal's protocol), printed as JSON lines of `wasdk:inspect`'s
//! values, one value a line, for a terminal or a coding agent to read: the program's audit summary,
//! its days newest first; what it uses now, then each change to it; or its live trace of the
//! capabilities named, each event as it comes; the last two until Ctrl-C or the program's end. Only a program a run of `waos run`
//! on this machine runs is inspected live, and one it runs or has run has its summary read.
//!
//! The invariant: what WaOS answers is printed as it came, so the command needs no types of its
//! own: the WIT is the one schema.

use std::io::Write as _;
use std::process::ExitCode;

use anyhow::{Context, Result};
use waos_dev_protocol::{InspectAnswer, Inspection, Request, TracedCapability};
use waos_package::ProgramId;

use crate::asking::{self, connect};

/// What `what` asks of a program, with a summary's `days`: `summary`, `active-uses`, `all`, or the
/// capabilities to trace by name.
pub fn inspection_of(what: &[String], days: u32) -> Result<Inspection> {
    match what {
        [one] if one == "summary" => return Ok(Inspection::Summary { days }),
        [one] if one == "active-uses" => return Ok(Inspection::ActiveUses),
        [one] if one == "all" => return Ok(Inspection::Trace(TracedCapability::ALL.to_vec())),
        _ => {}
    }
    let traced = what
        .iter()
        .map(|name| {
            TracedCapability::ALL.into_iter().find(|traced| traced.key() == name).with_context(|| {
                let names: Vec<&str> = TracedCapability::ALL.iter().map(|traced| traced.key()).collect();
                format!(
                    "nothing to inspect is named {name:?}: summary, active-uses, all, or any of {}",
                    names.join(", ")
                )
            })
        })
        .collect::<Result<Vec<TracedCapability>>>()?;
    Ok(Inspection::Trace(traced))
}

/// Prints what WaOS answers `asked` of the program `id`, a line a value, until it has answered.
pub async fn inspect(id: ProgramId, asked: Inspection) -> Result<ExitCode> {
    let summary = matches!(asked, Inspection::Summary { .. });
    let inspect = Request::Inspect {
        id: id.clone(),
        asked,
    };
    let mut connection = asking::greeted(connect().await?, &inspect).await?;
    connection.send(&inspect).await.context("sending to WaOS")?;
    let mut printed = false;
    let mut stdout = std::io::stdout().lock();
    while let Some(answer) = connection
        .receive::<InspectAnswer>()
        .await
        .context("reading from WaOS")?
    {
        match answer {
            InspectAnswer::Inspected(lines) => {
                for line in lines {
                    writeln!(stdout, "{line}")?;
                    printed = true;
                }
                stdout.flush()?;
            }
            InspectAnswer::Refused(refusal) => {
                eprintln!("waos: {}", refusal.message);
                return Ok(ExitCode::FAILURE);
            }
            // A line of a newer WaOS's this build does not know: skipped, as the protocol
            // promises, never breaking the trace over it.
            InspectAnswer::Unknown => {}
        }
    }
    if summary && !printed {
        eprintln!("waos: {id} used nothing WaOS guards on any day kept");
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn what_is_inspected_is_named_by_its_words() {
        assert_eq!(
            inspection_of(&words(&["summary"]), 3).unwrap(),
            Inspection::Summary { days: 3 }
        );
        assert_eq!(
            inspection_of(&words(&["active-uses"]), 7).unwrap(),
            Inspection::ActiveUses
        );
        assert_eq!(
            inspection_of(&words(&["network", "files"]), 7).unwrap(),
            Inspection::Trace(vec![TracedCapability::Network, TracedCapability::Files])
        );
        assert_eq!(
            inspection_of(&words(&["all"]), 7).unwrap(),
            Inspection::Trace(TracedCapability::ALL.to_vec())
        );
        let error = inspection_of(&words(&["network", "summary"]), 7).unwrap_err();
        assert!(
            format!("{error}").contains("summary, active-uses, all"),
            "{error}"
        );
    }
}
