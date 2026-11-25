//! `waos ui screenshot`: the last picture of each window of a program running, the package's
//! development install by default, asked of the WaOS running it, written as `window-<id>.png` under
//! `target/waos/screenshots/<package>`, each printed with its window's id, title and size, so
//! an agent building the program sees it as its user does (the design's decision 60). `--out`
//! writes them in a folder instead, one named as a folder or one that exists, all the windows, or
//! one window's picture at a file's path; `--window` asks WaOS for that window alone. A window's
//! id is the one it was given as it opened, and no run gives one twice, so a window keeps its id
//! however others close; a point in a picture's pixels, from its top-left, is where `tap` and
//! `scroll` reach.
//!
//! What WaOS asks the program's user over its windows is the program's prompt, not its picture:
//! the screenshot says on stderr what is asked.
//!
//! The invariant: a folder the pictures go in holds this screenshot's pictures alone. The window
//! pictures an earlier screenshot left there go first, so a window that closed since leaves no
//! stale file behind.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use waos_dev_protocol::{Request, ScreenshotAnswer, WindowId, WindowPicture};
use waos_package::ProgramId;

use crate::asked;
use crate::asking::ask;
use crate::destination::Destination;

/// The prefix and the extension of a window's picture's file.
const PREFIX: &str = "window-";
const EXTENSION: &str = "png";

/// Where a screenshot's pictures go: each window's as `window-<id>.png` in a folder, or one
/// window's alone at a file's path.
#[derive(Debug, PartialEq, Eq)]
pub enum Where {
    Folder(PathBuf),
    File(PathBuf),
}

impl Where {
    /// Where `out` asks for, or the folder `default` when it names none: a folder for a path that
    /// is one or is written as one, else a file.
    pub fn of(out: Option<&Path>, default: &Path) -> Where {
        match out {
            None => Where::Folder(default.to_path_buf()),
            Some(path) if names_folder(path) => Where::Folder(path.to_path_buf()),
            Some(path) => Where::File(path.to_path_buf()),
        }
    }
}

/// Whether `path` names a folder: one that is there, or one written with a separator at its end.
fn names_folder(path: &Path) -> bool {
    if path.is_dir() {
        return true;
    }
    let written = path.as_os_str().to_string_lossy();
    written.ends_with('/') || written.ends_with('\\')
}

/// Asks for the pictures of the windows of the program `id`, or of `window` alone, writes them
/// where `out` asks, or in `default` without it, and prints each.
pub async fn take(
    destination: &Destination,
    id: ProgramId,
    out: Option<&Path>,
    window: Option<WindowId>,
    default: &Path,
) -> Result<ExitCode> {
    let answer = ask(
        destination,
        &Request::Screenshot {
            id: id.clone(),
            window,
        },
    )
    .await?;
    let pictures = match answer {
        ScreenshotAnswer::Pictures(pictures) => pictures,
        ScreenshotAnswer::Refused(refusal) => {
            eprintln!("waos: {}", refusal.message);
            return Ok(ExitCode::FAILURE);
        }
        ScreenshotAnswer::Unknown => bail!(
            "WaOS answered the screenshot in a way this waos does not know: update waos with `curl -fsSL https://waos.app/install.sh | sh`"
        ),
    };
    if pictures.is_empty() {
        eprintln!("waos: {id} has no window open");
    }
    match Where::of(out, default) {
        Where::Folder(folder) => {
            for (picture, path) in pictures.iter().zip(write(&folder, &pictures)?) {
                say(picture, path.as_deref());
            }
        }
        Where::File(path) => {
            let picture = match window {
                Some(window) => pictures.iter().find(|each| each.window == window),
                None => pictures.first(),
            };
            if let Some(picture) = picture {
                say(picture, write_one(&path, picture)?.as_deref());
            }
        }
    }
    if let Some(asking) = pictures.iter().find_map(|picture| picture.asking.as_ref()) {
        eprintln!(
            "waos: WaOS is asking over {id}'s windows, which the pictures do not show: {}",
            asked::said(asking, id.as_str())
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// Writes each of `pictures` that has a picture as `window-<id>.png` in `folder`, made where it
/// is missing, after taking away the window pictures already there; answers where each went, none
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

/// Writes `picture`'s picture at `path`, made where it is missing; answers where it went, none for
/// a window that has shown no picture yet.
fn write_one(path: &Path, picture: &WindowPicture) -> Result<Option<PathBuf>> {
    let Some(png) = &picture.png else {
        return Ok(None);
    };
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).with_context(|| format!("making {}", parent.display()))?;
    }
    fs::write(path, png).with_context(|| format!("writing {}", path.display()))?;
    Ok(Some(path.to_path_buf()))
}

/// The file the picture of the window `window` is written to.
fn file_name(window: WindowId) -> String {
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
    number.is_some_and(|number| number.parse::<u64>().is_ok())
}

/// Prints where `picture` went with its window's id, title and size, and says on stderr what a
/// reader of it needs to know: that the window showed no picture yet, or is out of sight.
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
        None => {
            eprintln!("waos: window {window} {title} {width}x{height} has shown no picture yet")
        }
    }
    if !picture.in_sight {
        eprintln!(
            "waos: window {window} is out of sight, minimized or hidden: it shows its last picture, and draws nothing new until it is in sight"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(window: u64, png: Option<&[u8]>) -> WindowPicture {
        WindowPicture {
            window: WindowId(window),
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

    #[test]
    fn out_names_a_folder_when_it_is_one_or_ends_as_one_and_a_file_otherwise() {
        let folder = tempfile::tempdir().unwrap();
        let default = folder.path().join("default");
        let existing = folder.path().join("existing");
        fs::create_dir(&existing).unwrap();
        assert_eq!(Where::of(None, &default), Where::Folder(default.clone()));
        assert_eq!(
            Where::of(Some(&existing), &default),
            Where::Folder(existing)
        );
        assert_eq!(
            Where::of(Some(Path::new("later/")), &default),
            Where::Folder(PathBuf::from("later/"))
        );
        assert_eq!(
            Where::of(Some(Path::new("notes.png")), &default),
            Where::File(PathBuf::from("notes.png"))
        );
    }

    #[test]
    fn a_picture_named_as_a_file_is_written_there_alone() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("pictures/notes.png");
        let written = write_one(&path, &picture(1, Some(b"png"))).unwrap();
        assert_eq!(written.as_deref(), Some(path.as_path()));
        assert_eq!(fs::read(&path).unwrap(), b"png");
        assert_eq!(write_one(&path, &picture(1, None)).unwrap(), None);
    }
}
