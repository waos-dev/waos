//! An app installed on Linux, found as the desktop finds it: by its desktop entry,
//! `<identifier>.desktop` in the `applications` folder of the XDG data folders, the user's first,
//! and started with the command line the entry's `Exec` gives, read as the Desktop Entry
//! specification reads it.
//!
//! The invariant: the command is the entry's own with nothing to open, so every field code is
//! dropped from it; what the caller adds comes after it.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// The desktop entry of the app `identifier`, in the first data folder that has one.
pub fn find(identifier: &str) -> Option<PathBuf> {
    let name = format!("{identifier}.desktop");
    data_folders()
        .into_iter()
        .map(|folder| folder.join("applications").join(&name))
        .find(|entry| entry.is_file())
}

/// The program and arguments the entry at `path` starts its app with.
pub fn command(path: &Path) -> Result<Vec<String>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let exec = exec_value(&text)
        .with_context(|| format!("{} has no Exec in its [Desktop Entry]", path.display()))?;
    let command = exec_arguments(&unescaped(exec));
    if command.is_empty() {
        bail!("{}'s Exec names no program", path.display());
    }
    Ok(command)
}

/// The XDG data folders, the user's first: `$XDG_DATA_HOME`, or `~/.local/share`, then each of
/// `$XDG_DATA_DIRS`, or `/usr/local/share` and `/usr/share`. A variable that is empty or not an
/// absolute path counts as unset.
fn data_folders() -> Vec<PathBuf> {
    let variable = |name| std::env::var_os(name).filter(|value| !value.is_empty());
    let home = variable("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|folder| folder.is_absolute())
        .or_else(|| std::env::home_dir().map(|home| home.join(".local/share")));
    let system = variable("XDG_DATA_DIRS")
        .map(|folders| std::env::split_paths(&folders).collect::<Vec<_>>())
        .unwrap_or_else(|| vec!["/usr/local/share".into(), "/usr/share".into()]);
    home.into_iter()
        .chain(system.into_iter().filter(|folder| folder.is_absolute()))
        .collect()
}

/// The value of `Exec` in the `[Desktop Entry]` group of `entry`, as written.
fn exec_value(entry: &str) -> Option<&str> {
    let mut in_main_group = false;
    for line in entry.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main_group = line == "[Desktop Entry]";
        } else if in_main_group
            && let Some((key, value)) = line.split_once('=')
            && key.trim_end() == "Exec"
        {
            return Some(value.trim_start());
        }
    }
    None
}

/// A string value with the escapes every value may hold read: `\s`, `\n`, `\t`, `\r` and `\\`.
fn unescaped(value: &str) -> String {
    let mut text = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            text.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => text.push(' '),
            Some('n') => text.push('\n'),
            Some('t') => text.push('\t'),
            Some('r') => text.push('\r'),
            Some('\\') => text.push('\\'),
            Some(other) => {
                text.push('\\');
                text.push(other);
            }
            None => text.push('\\'),
        }
    }
    text
}

/// `exec` split into its arguments: separated by spaces, an argument in double quotes taking
/// `"`, `` ` ``, `$` and `\` escaped by a backslash; with its field codes, which stand for what is
/// opened and what the entry says of itself, dropped, and `%%` written `%`.
fn exec_arguments(exec: &str) -> Vec<String> {
    let mut arguments = Vec::new();
    let mut argument = String::new();
    let mut started = false;
    let mut quoted = false;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '\\' if quoted => {
                if let Some(escaped) = chars.next() {
                    argument.push(escaped);
                }
            }
            ' ' if !quoted => {
                if started {
                    arguments.push(std::mem::take(&mut argument));
                    started = false;
                }
            }
            '%' if !quoted => {
                started = true;
                if chars.next() == Some('%') {
                    argument.push('%');
                }
            }
            other => {
                argument.push(other);
                started = true;
            }
        }
    }
    if started {
        arguments.push(argument);
    }
    arguments.retain(|argument| !argument.is_empty());
    arguments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_exec_of_the_main_group_is_read() {
        let entry =
            "[Desktop Entry]\nName=WaOS\nExec = waos %F\n\n[Desktop Action New]\nExec=other\n";
        assert_eq!(exec_value(entry), Some("waos %F"));
        let other_group_only = "[Desktop Action New]\nExec=other\n";
        assert_eq!(exec_value(other_group_only), None);
    }

    #[test]
    fn the_field_codes_are_dropped() {
        assert_eq!(exec_arguments("waos %F"), ["waos"]);
        assert_eq!(exec_arguments("waos %U --flag %i"), ["waos", "--flag"]);
        assert_eq!(exec_arguments("/usr/bin/waos"), ["/usr/bin/waos"]);
    }

    #[test]
    fn a_quoted_argument_keeps_its_spaces_and_escapes() {
        assert_eq!(
            exec_arguments(r#""/opt/Wa OS/waos" "a \"b\" \$c" 100%%"#),
            ["/opt/Wa OS/waos", "a \"b\" $c", "100%"]
        );
    }

    #[test]
    fn a_value_is_unescaped_before_it_is_split() {
        let exec = unescaped(r#""/opt/my\sapp/run" \\x"#);
        assert_eq!(exec, r#""/opt/my app/run" \x"#);
        assert_eq!(exec_arguments(&exec), ["/opt/my app/run", "\\x"]);
    }
}
