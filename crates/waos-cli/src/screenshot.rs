//! `cargo waos screenshot`: the last picture of each window of the program a run of `cargo waos
//! run` runs, asked of the WaOS running it, written as `window-<n>.png` under
//! `target/waos/screenshots/<package>`, each printed with its window's number, title and size, so
//! an agent building the program sees it as its user does (the design's decision 60). The windows
//! are numbered from 1 in the order they opened, as `--window` names them, and a point in a
//! picture's pixels, from its top-left, is where `tap` and `scroll` reach.
//!
//! What WaOS asks the program's user over its windows is the program's prompt, not its picture:
//! the screenshot says on stderr what is asked.
//!
//! The invariant: the folder holds this screenshot's pictures alone. The window pictures an earlier
//! screenshot left there go first, so a window that closed since leaves no stale file behind.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use waos_dev_protocol::{FromWaOS, ToWaOS, WindowPicture};
use waos_package::ProgramId;

use crate::asked;
use crate::asking::ask;
use crate::unreached;

/// The prefix and the extension of a window's picture's file.
const PREFIX: &str = "window-";
const EXTENSION: &str = "png";

/// Asks for the pictures of the windows of the program `id`, writes them in `folder`, and prints
/// each.
pub async fn take(id: ProgramId, folder: &Path) -> Result<ExitCode> {
    let answer = ask(&ToWaOS::Screenshot { id: id.clone() }).await?;
    let pictures = match answer {
        FromWaOS::Screenshots(pictures) => pictures,
        FromWaOS::Unreached(why) => {
            eprintln!("cargo waos: {}", unreached::said(&why, &id, 1));
            return Ok(ExitCode::FAILURE);
        }
        FromWaOS::Refused(why) => bail!("WaOS refused the screenshot: {why}"),
        other => bail!("WaOS answered {other:?}, not a screenshot"),
    };
    if pictures.is_empty() {
        eprintln!("cargo waos: {id} has no window open");
    }
    let written = write(folder, &pictures)?;
    for (picture, path) in pictures.iter().zip(written) {
        say(picture, path.as_deref());
    }
    if let Some(asking) = pictures.iter().find_map(|picture| picture.asking.as_ref()) {
        eprintln!(
            "cargo waos: WaOS is asking over {id}'s windows, which the pictures do not show: {}",
            asked::said(asking, id.as_str())
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// Writes each of `pictures` that has a picture as `window-<n>.png` in `folder`, made where it is
/// missing, after taking away the window pictures already there; answers where each went, none
/// for a window that has shown no picture yet.
pub fn write(folder: &Path, pictures: &[WindowPicture]) -> Result<Vec<Option<PathBuf>>> {
    fs::create_dir_all(folder).with_context(|| format!("making {}", folder.display()))?;
    remove_window_pictures(folder)?;
    pictures
        .iter()
        .map(|picture| {
            let Some(png) = &picture.png else {
                return Ok(None);
            };
            let path = folder.join(file_name(picture.window));
            fs::write(&path, png).with_context(|| format!("writing {}", path.display()))?;
            Ok(Some(path))
        })
        .collect()
}

/// The file the picture of the window numbered `window` is written to.
fn file_name(window: u32) -> String {
    format!("{PREFIX}{window}.{EXTENSION}")
}

/// Takes away the window pictures an earlier screenshot left in `folder`, and nothing else.
fn remove_window_pictures(folder: &Path) -> Result<()> {
    let entries = fs::read_dir(folder).with_context(|| format!("reading {}", folder.display()))?;
    for entry in entries {
        let path = entry?.path();
        if is_window_picture(&path) {
            match fs::remove_file(&path) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => {
                    return Err(error).with_context(|| format!("removing {}", path.display()));
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// Whether `path` is named as a window's picture is.
fn is_window_picture(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let number = name
        .strip_prefix(PREFIX)
        .and_then(|rest| rest.strip_suffix(&format!(".{EXTENSION}")));
    number.is_some_and(|number| number.parse::<u32>().is_ok())
}

/// Prints where `picture` went with its window's number, title and size, and says on stderr what
/// a reader of it needs to know: that the window showed no picture yet, or is out of sight.
fn say(picture: &WindowPicture, path: Option<&Path>) {
    let window = picture.window;
    let title = if picture.title.is_empty() {
        "(untitled)".to_owned()
    } else {
        format!("{:?}", picture.title)
    };
    let (width, height) = (picture.width, picture.height);
    match path {
        Some(path) => println!(
            "{}  window {window} {title} {width}x{height}",
            path.display()
        ),
        None => eprintln!(
            "cargo waos: window {window} {title} {width}x{height} has shown no picture yet"
        ),
    }
    if !picture.in_sight {
        eprintln!(
            "cargo waos: window {window} is out of sight, minimized or hidden: it shows its last picture, and draws nothing new until it is in sight"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(window: u32, png: Option<&[u8]>) -> WindowPicture {
        WindowPicture {
            window,
            title: "Notes".into(),
            width: 4,
            height: 2,
            in_sight: true,
            png: png.map(<[u8]>::to_vec),
            asking: None,
        }
    }

    #[test]
    fn each_picture_is_written_by_its_windows_number_and_one_with_none_is_not() {
        let folder = tempfile::tempdir().unwrap();
        let screenshots = folder.path().join("screenshots/notes");
        let written = write(
            &screenshots,
            &[
                picture(1, Some(b"first")),
                picture(2, None),
                picture(3, Some(b"third")),
            ],
        )
        .unwrap();
        assert_eq!(
            written,
            [
                Some(screenshots.join("window-1.png")),
                None,
                Some(screenshots.join("window-3.png"))
            ]
        );
        assert_eq!(
            fs::read(screenshots.join("window-1.png")).unwrap(),
            b"first"
        );
        assert!(!screenshots.join("window-2.png").exists());
    }

    #[test]
    fn the_window_pictures_an_earlier_screenshot_left_go_and_nothing_else() {
        let folder = tempfile::tempdir().unwrap();
        for name in ["window-2.png", "window-7.png", "notes.txt", "window-a.png"] {
            fs::write(folder.path().join(name), b"earlier").unwrap();
        }
        write(folder.path(), &[picture(1, Some(b"now"))]).unwrap();
        let mut left: Vec<String> = fs::read_dir(folder.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(left, ["notes.txt", "window-1.png", "window-a.png"]);
    }
}
