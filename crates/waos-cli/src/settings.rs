//! `cargo waos settings` and `cargo waos set`: a program's settings, as its settings page in WaOS
//! shows them, one row each, read or set by the WaOS on this machine, which keeps them; a set
//! answer is the program's setting, used by every run, installed or not (decision 11 of the
//! permissions proposal). Only a program a run of `cargo waos run` on this machine runs or has run
//! is reached, or one the store does not have installed, which may be set before its first run.
//! WaOS is started when none answers, since the settings are there to be set before the run that
//! needs them.
//!
//! The invariant: a path set is whole before it is sent, made so against this directory, as
//! `waos open` makes the files it opens; WaOS resolves it as it was written before keeping it,
//! since a program names each mount by its parts and one holding `..` would name no mount of it.

use std::path::{self, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use waos_dev_protocol::{ASK, FromWaOS, Row, SettingRow, ToWaOS};
use waos_package::ProgramId;

use crate::asking::ask_on;
use crate::connecting::connect_or_start;
use crate::unreached;

/// Prints the rows of the program `id`'s settings.
pub async fn show(id: ProgramId) -> Result<ExitCode> {
    let answer = ask_on(
        connect_or_start().await?,
        &ToWaOS::Settings { id: id.clone() },
    )
    .await?;
    answered(&id, answer, None)
}

/// Sets the row `words` name among the program `id`'s settings, and prints the rows after it.
pub async fn set(id: ProgramId, words: &[String]) -> Result<ExitCode> {
    let row = whole_paths(Row::parse(words)?)?;
    let set = ToWaOS::Set {
        id: id.clone(),
        row: row.clone(),
    };
    let answer = ask_on(connect_or_start().await?, &set).await?;
    answered(&id, answer, Some(&row))
}

/// Prints what WaOS answered of the program `id`'s settings, after `set` where one was set.
fn answered(id: &ProgramId, answer: FromWaOS, set: Option<&Row>) -> Result<ExitCode> {
    let rows = match answer {
        FromWaOS::Settings(rows) => rows,
        FromWaOS::Unreached(why) => {
            eprintln!("cargo waos: {}", unreached::said(&why, id, 1));
            return Ok(ExitCode::FAILURE);
        }
        FromWaOS::Refused(why) => {
            eprintln!("cargo waos: WaOS refused it: {why}");
            return Ok(ExitCode::FAILURE);
        }
        other => bail!("WaOS answered {other:?}, not the program's settings"),
    };
    for row in &rows {
        println!("{}", printed(row));
    }
    if let Some(Row::Question(question, None)) = set
        && let Some(row) = rows.iter().find(|row| row.key == question.key())
        && row.values != [ASK]
    {
        eprintln!(
            "cargo waos: {} follows the answer for every program, {}, set on WaOS's settings page for every program; it is asked only once that is Ask",
            row.key,
            row.values.join(" ")
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// `row` with each of its paths whole, against this directory.
fn whole_paths(row: Row) -> Result<Row> {
    let Row::FileDialog(dialog, paths) = row else {
        return Ok(row);
    };
    let whole = paths
        .iter()
        .map(|path| {
            path::absolute(path).with_context(|| format!("making {} whole", path.display()))
        })
        .collect::<Result<Vec<PathBuf>>>()?;
    Ok(Row::FileDialog(dialog, whole))
}

/// One row as `settings` prints it: its key, what holds, whether it follows the setting for every
/// program, and what a picker's row can be set to now.
fn printed(row: &SettingRow) -> String {
    let values: Vec<String> = row.values.iter().map(|value| quoted(value)).collect();
    let mut line = format!("{:<16} {}", row.key, values.join(" "));
    if row.follows_global {
        line.push_str("  (follows every program's)");
    }
    if !row.offered.is_empty() {
        let offered: Vec<String> = row
            .offered
            .iter()
            .map(|offered| format!("{} ({})", quoted(&offered.id), offered.name))
            .collect();
        line.push_str(&format!("  offered: {}", offered.join(", ")));
    }
    line
}

/// `value` as a shell would take it back: quoted where it holds a space.
fn quoted(value: &str) -> String {
    if value.contains(char::is_whitespace) {
        format!("{value:?}")
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use waos_dev_protocol::{FileDialog, Offered};

    use super::*;

    #[test]
    fn a_row_prints_its_key_what_holds_and_what_it_can_be_set_to() {
        let machine = SettingRow {
            key: "machine".into(),
            values: vec![ASK.into()],
            follows_global: false,
            offered: vec![Offered {
                id: "this-computer".into(),
                name: "This computer".into(),
            }],
        };
        assert_eq!(
            printed(&machine),
            "machine          ask  offered: this-computer (This computer)"
        );
        let camera = SettingRow {
            key: "camera".into(),
            values: vec!["allow".into()],
            follows_global: true,
            offered: Vec::new(),
        };
        assert_eq!(
            printed(&camera),
            "camera           allow  (follows every program's)"
        );
        let files = SettingRow {
            key: "open-files".into(),
            values: vec!["/a.txt".into(), "/My Files/b.txt".into()],
            follows_global: false,
            offered: Vec::new(),
        };
        assert_eq!(
            printed(&files),
            "open-files       /a.txt \"/My Files/b.txt\""
        );
    }

    #[test]
    fn a_path_set_is_made_whole_against_this_directory() {
        let row = Row::FileDialog(FileDialog::SaveFile, vec!["out.pdf".into()]);
        let Row::FileDialog(_, paths) = whole_paths(row).unwrap() else {
            panic!("a dialog's row");
        };
        assert_eq!(paths, [std::env::current_dir().unwrap().join("out.pdf")]);
    }
}
