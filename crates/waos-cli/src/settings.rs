//! `waos settings` and `waos settings set`: a program's settings, as its settings page in WaOS
//! shows them, one row each, read or set by the WaOS on this machine, which keeps them; a set
//! answer is the program's setting, used by every run of it. Any program is reached, however it was
//! installed, the package's development install by default, and one the store does not have
//! installed, which may be set before its install. WaOS is started when none answers, since the
//! settings are there to be set before the run that needs them.
//!
//! The invariant: a path set is whole before it is sent, made so against this directory, as
//! `waos open` makes the files it opens; WaOS resolves it as it was written before keeping it,
//! since a program names each mount by its parts and one holding `..` would name no mount of it.

use std::path::{self, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use waos_dev_protocol::{ASK, Request, Row, SettingRow, SettingsAnswer};
use waos_package::ProgramId;

use crate::asking::ask_starting;
use crate::connecting::WaOSStarter;
use crate::destination::Destination;

/// Prints the rows of the program `id`'s settings.
pub async fn show(
    destination: &Destination,
    id: ProgramId,
    starter: &dyn WaOSStarter,
) -> Result<ExitCode> {
    let request = Request::Settings { id: id.clone() };
    let answer = ask_starting(destination, &request, starter).await?;
    answered(answer, None)
}

/// Sets the row `words` name among the program `id`'s settings, and prints the rows after it.
pub async fn set(
    destination: &Destination,
    id: ProgramId,
    words: &[String],
    starter: &dyn WaOSStarter,
) -> Result<ExitCode> {
    let row = whole_paths(Row::parse(words)?)?;
    let answer = ask_starting(
        destination,
        &Request::Set {
            id: id.clone(),
            row: row.clone(),
        },
        starter,
    )
    .await?;
    answered(answer, Some(&row))
}

/// Prints what WaOS answered of the program `id`'s settings, after `set` where one was set.
fn answered(answer: SettingsAnswer, set: Option<&Row>) -> Result<ExitCode> {
    let rows = match answer {
        SettingsAnswer::Rows(rows) => rows,
        SettingsAnswer::Refused(refusal) => {
            eprintln!("waos: {}", refusal.message);
            return Ok(ExitCode::FAILURE);
        }
        SettingsAnswer::Unknown => bail!(
            "WaOS answered the settings in a way this waos does not know: update waos with `curl -fsSL https://waos.app/install.sh | sh`"
        ),
    };
    for row in &rows {
        println!("{}", printed(row));
    }
    if let Some(Row::Question(question, None)) = set
        && let Some(row) = rows.iter().find(|row| row.key == question.key())
        && row.values != [ASK]
    {
        eprintln!(
            "waos: {} follows the answer for every program, {}, set on WaOS's settings page for every program; it is asked only once that is Ask",
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
