//! `cargo waos inspect`: what the WaOS on this machine knows of what a program does with what it
//! was allowed (the inspector proposal's protocol), printed as JSON lines of `wasdk:inspect`'s
//! values, one value a line, for a terminal or a coding agent to read: the program's audit summary,
//! its days newest first; what it holds now; or its live trace of the capabilities named, each
//! event as it comes, until Ctrl-C or the program's end. Only a program a run of `cargo waos run`
//! on this machine runs is inspected live, and one it runs or has run has its summary read.
//!
//! The invariant: what WaOS answers is printed as it came, so the command needs no types of its
//! own: the WIT is the one schema.

use std::io::Write as _;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use waos_dev_protocol::{FromWaOS, Inspection, ToWaOS, TracedCapability};
use waos_package::ProgramId;

use crate::asking::connect;
use crate::unreached;

/// What `what` asks of a program, with a summary's `days`: `summary`, `holdings`, `all`, or the
/// capabilities to trace by name.
pub fn inspection_of(what: &[String], days: u32) -> Result<Inspection> {
    match what {
        [one] if one == "summary" => return Ok(Inspection::Summary { days }),
        [one] if one == "holdings" => return Ok(Inspection::Holdings),
        [one] if one == "all" => return Ok(Inspection::Trace(TracedCapability::ALL.to_vec())),
        _ => {}
    }
    let traced = what
        .iter()
        .map(|name| {
            TracedCapability::ALL.into_iter().find(|traced| traced.key() == name).with_context(|| {
                let names: Vec<&str> = TracedCapability::ALL.iter().map(|traced| traced.key()).collect();
                format!(
                    "nothing to inspect is named {name:?}: summary, holdings, all, or any of {}",
                    names.join(", ")
                )
            })
        })
        .collect::<Result<Vec<TracedCapability>>>()?;
    Ok(Inspection::Trace(traced))
}

/// Prints what WaOS answers `asked` of the program `id`, a line a value, until it has answered.
pub async fn inspect(id: ProgramId, asked: Inspection) -> Result<ExitCode> {
    let mut connection = connect().await?;
    connection
        .agree_on_protocol()
        .await
        .context("the WaOS that answered")?;
    let summary = matches!(asked, Inspection::Summary { .. });
    let inspect = ToWaOS::Inspect {
        id: id.clone(),
        asked,
    };
    connection.send(&inspect).await.context("sending to WaOS")?;
    let mut printed = false;
    let mut stdout = std::io::stdout().lock();
    while let Some(answer) = connection
        .receive::<FromWaOS>()
        .await
        .context("reading from WaOS")?
    {
        match answer {
            FromWaOS::Inspected(lines) => {
                for line in lines {
                    writeln!(stdout, "{line}")?;
                    printed = true;
                }
                stdout.flush()?;
            }
            FromWaOS::Unreached(why) => {
                eprintln!("cargo waos: {}", unreached::said(&why, &id, 1));
                return Ok(ExitCode::FAILURE);
            }
            FromWaOS::Refused(why) => bail!("WaOS refused the inspection: {why}"),
            other => bail!("WaOS answered {other:?}, not an inspection"),
        }
    }
    if summary && !printed {
        eprintln!("cargo waos: {id} used nothing WaOS guards on any day kept");
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
            inspection_of(&words(&["holdings"]), 7).unwrap(),
            Inspection::Holdings
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
            format!("{error}").contains("summary, holdings, all"),
            "{error}"
        );
    }
}
