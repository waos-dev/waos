//! What `CreateProcessW` takes of a command, which the standard library's `Command` makes for a
//! command it starts but WaOS's pseudoconsole start makes itself: the program found on the
//! command's own `PATH`, as `Command` finds it, its command line quoted as Microsoft's C runtime
//! reads one back into arguments, and its environment as one block.
//!
//! The invariant: an argument reads back exactly as given, whatever spaces, tabs, quotes and
//! backslashes it holds.

use std::ffi::OsString;
use std::os::windows::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use crate::login_environment::Variables;

/// The extensions a program is found by where `PATHEXT` names none.
const DEFAULT_EXTENSIONS: &str = ".COM;.EXE;.BAT;.CMD";

/// Where `program` is: itself where it names a folder or has an extension and is there, and
/// otherwise the first file of its name, with each of `PATHEXT`'s extensions in turn, in the folders
/// `variables`' `PATH` lists; `program` as given where none is, which starting then refuses.
pub(super) fn resolved_program(program: &str, variables: &Variables) -> PathBuf {
    let given = Path::new(program);
    if given.components().count() > 1 || given.is_absolute() {
        return given.to_path_buf();
    }
    let extensions = variable(variables, "PATHEXT")
        .map(|extensions| extensions.to_string_lossy().into_owned())
        .unwrap_or_else(|| DEFAULT_EXTENSIONS.to_owned());
    let folders = variable(variables, "PATH").unwrap_or_default();
    std::env::split_paths(&folders)
        .flat_map(|folder| {
            let named = folder.join(program);
            let with_extensions: Vec<PathBuf> = extensions
                .split(';')
                .filter(|extension| !extension.is_empty())
                .map(|extension| {
                    let mut file = named.clone().into_os_string();
                    file.push(extension);
                    PathBuf::from(file)
                })
                .collect();
            std::iter::once(named).chain(with_extensions)
        })
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| given.to_path_buf())
}

/// The value of `name` among `variables`, whose names Windows reads ignoring case.
fn variable(variables: &Variables, name: &str) -> Option<OsString> {
    variables
        .iter()
        .find(|(kept, _)| kept.to_string_lossy().eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
}

/// The command line of `program` with `arguments`, NUL-terminated, each part quoted where it needs
/// to be.
pub(super) fn command_line(program: &Path, arguments: &[String]) -> Vec<u16> {
    let mut line: Vec<u16> = Vec::new();
    quote_into(&mut line, &program.as_os_str().to_string_lossy());
    for argument in arguments {
        line.push(u16::from(b' '));
        quote_into(&mut line, argument);
    }
    line.push(0);
    line
}

/// Appends `argument` to `line`, quoted where it is empty or holds a space, a tab or a quote: a
/// quote escaped with a backslash, and each run of backslashes before a quote, or before the
/// closing quote, doubled, as the C runtime halves them when it reads them back.
fn quote_into(line: &mut Vec<u16>, argument: &str) {
    let needs_quotes = argument.is_empty() || argument.contains([' ', '\t', '"']);
    if !needs_quotes {
        line.extend(argument.encode_utf16());
        return;
    }
    let mut quoted = String::from('"');
    let mut backslashes = 0;
    for letter in argument.chars() {
        match letter {
            '\\' => backslashes += 1,
            '"' => {
                quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.push_str(&"\\".repeat(backslashes));
                quoted.push(letter);
                backslashes = 0;
            }
        }
    }
    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    line.extend(quoted.encode_utf16());
}

/// `variables` as `CreateProcessW` takes them with `CREATE_UNICODE_ENVIRONMENT`: each `name=value`
/// NUL-terminated, sorted by name ignoring case, as Windows keeps them, and a NUL after the last.
pub(super) fn environment_block(variables: &Variables) -> Vec<u16> {
    let mut sorted: Vec<&(OsString, OsString)> = variables.iter().collect();
    sorted.sort_by_key(|(name, _)| name.to_string_lossy().to_uppercase());
    let mut block = Vec::new();
    for (name, value) in sorted {
        block.extend(name.encode_wide());
        block.push(u16::from(b'='));
        block.extend(value.encode_wide());
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    block
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: &[u16]) -> String {
        String::from_utf16(line.strip_suffix(&[0]).unwrap()).unwrap()
    }

    #[test]
    fn an_argument_is_quoted_only_where_it_needs_to_be_and_its_backslashes_kept() {
        let arguments = [
            "plain",
            "two words",
            "",
            r#"say "hi""#,
            r"C:\folder\",
            r"C:\with space\",
            r#"a\"b"#,
        ]
        .map(str::to_owned);
        let line = command_line(Path::new(r"C:\Windows\cmd.exe"), &arguments);
        assert_eq!(
            text(&line),
            r#"C:\Windows\cmd.exe plain "two words" "" "say \"hi\"" C:\folder\ "C:\with space\\" "a\\\"b""#
        );
    }

    #[test]
    fn the_environment_is_sorted_by_name_and_ends_with_two_nuls() {
        let variables: Variables = vec![("b".into(), "2".into()), ("A".into(), "1".into())];
        let block = environment_block(&variables);
        let expected: Vec<u16> = "A=1\0b=2\0\0".encode_utf16().collect();
        assert_eq!(block, expected);
    }

    #[test]
    fn a_program_is_found_on_the_command_s_path_with_an_extension() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("tool.exe"), b"").unwrap();
        let variables: Variables = vec![
            ("Path".into(), folder.path().as_os_str().to_owned()),
            ("PATHEXT".into(), ".com;.exe".into()),
        ];
        assert_eq!(
            resolved_program("tool", &variables),
            folder.path().join("tool.exe")
        );
        assert_eq!(
            resolved_program("missing", &variables),
            PathBuf::from("missing")
        );
    }
}
