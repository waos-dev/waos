//! Watches of a folder of this machine, as the system reports its changes: what a file's creation,
//! write, rename and removal come as, a folder beneath the watched one watched only where the
//! watch is recursive, the changes of a burst in one batch, a burst past what a batch holds a
//! rescan, and a folder no path leads to refused.

use std::fs;
use std::path::Path;
use std::time::Duration;

use futures::StreamExt;
use tempfile::TempDir;

use super::batch::LIMIT;
use super::{Batches, watch};
use crate::access::{ActiveUseKind, ProgramAccess};
use crate::bindings::wasdk::platform::file_watching::Change;
use crate::files::{ErrorCode, HeldFolder, folder_of_machine};

/// How long a test waits for a change it expects.
const PATIENCE: Duration = Duration::from_secs(10);

/// A folder to watch, and the folder it is in.
fn scratch() -> (TempDir, std::path::PathBuf) {
    let scratch = tempfile::tempdir().unwrap();
    let folder = scratch.path().join("watched");
    fs::create_dir(&folder).unwrap();
    (scratch, folder)
}

fn watching(folder: &Path, recursive: bool, latency: Duration) -> Batches {
    let held = HeldFolder::of(&folder_of_machine(folder, &access()).unwrap()).unwrap();
    watch(&held, recursive, latency, held_watch(&access(), folder)).unwrap()
}

fn access() -> ProgramAccess {
    ProgramAccess::new(std::sync::Arc::new(crate::permissions::NothingGranted))
}

/// The use of a watch of `folder`, held through `access`.
fn held_watch(access: &ProgramAccess, folder: &Path) -> crate::access::ActiveUse {
    access.hold(ActiveUseKind::FolderWatch {
        folder: folder.to_path_buf(),
        recursive: false,
    })
}

/// The changes the watch reports until they hold each of `expected`, or the test's patience runs
/// out.
async fn changes_until(batches: &mut Batches, expected: &[Change]) -> Vec<Change> {
    let mut seen = Vec::new();
    while !expected.iter().all(|change| seen.contains(change)) {
        match tokio::time::timeout(PATIENCE, batches.next()).await {
            Ok(Some(batch)) => seen.extend(batch),
            _ => panic!("not all of {expected:?} among {seen:?}"),
        }
    }
    seen
}

fn created(path: &str) -> Change {
    Change::Created(path.to_owned())
}

fn modified(path: &str) -> Change {
    Change::Modified(path.to_owned())
}

fn removed(path: &str) -> Change {
    Change::Removed(path.to_owned())
}

/// What `from` renamed to `to`, both in the folder, comes as: one rename where the system pairs
/// its ends, each end apart where it does not, as FSEvents.
fn renames(from: &str, to: &str) -> Vec<Change> {
    if cfg!(target_os = "macos") {
        vec![Change::Other(from.to_owned()), Change::Other(to.to_owned())]
    } else {
        vec![Change::Renamed((from.to_owned(), to.to_owned()))]
    }
}

#[tokio::test]
async fn a_file_made_written_renamed_and_removed_is_told_by_its_path_in_the_folder() {
    let (_scratch, folder) = scratch();
    let mut batches = watching(&folder, false, Duration::ZERO);

    fs::write(folder.join("note.txt"), "one").unwrap();
    changes_until(&mut batches, &[created("note.txt")]).await;

    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(folder.join("note.txt"))
        .unwrap();
    std::io::Write::write_all(&mut file, b" two").unwrap();
    drop(file);
    changes_until(&mut batches, &[modified("note.txt")]).await;

    fs::rename(folder.join("note.txt"), folder.join("kept.txt")).unwrap();
    changes_until(&mut batches, &renames("note.txt", "kept.txt")).await;

    fs::remove_file(folder.join("kept.txt")).unwrap();
    changes_until(&mut batches, &[removed("kept.txt")]).await;
}

#[tokio::test]
async fn a_folder_beneath_is_watched_only_where_the_watch_is_recursive() {
    let (_scratch, folder) = scratch();
    fs::create_dir(folder.join("sub")).unwrap();
    let mut recursive = watching(&folder, true, Duration::ZERO);
    let mut own_entries = watching(&folder, false, Duration::ZERO);

    fs::write(folder.join("sub/deep.txt"), "deep").unwrap();
    fs::write(folder.join("marker"), "").unwrap();

    changes_until(&mut recursive, &[created("sub/deep.txt")]).await;
    let told = changes_until(&mut own_entries, &[created("marker")]).await;
    assert!(
        !told
            .iter()
            .any(|change| matches!(change, Change::Created(path) | Change::Modified(path) if path == "sub/deep.txt")),
        "a watch of its own entries told {told:?}"
    );
}

#[tokio::test]
async fn a_burst_of_changes_comes_in_one_batch_after_the_latency() {
    let (_scratch, folder) = scratch();
    let mut batches = watching(&folder, false, Duration::from_millis(300));
    for name in ["a", "b", "c"] {
        fs::write(folder.join(name), "").unwrap();
    }
    let batch = tokio::time::timeout(PATIENCE, batches.next())
        .await
        .unwrap()
        .unwrap();
    for name in ["a", "b", "c"] {
        assert!(batch.contains(&created(name)), "{name} in {batch:?}");
    }
}

/// More changes than WaOS's batch holds, made faster than the batch is taken: past its limit, or
/// past what FSEvents' queue holds on macOS, which then asks for a rescan itself.
#[tokio::test]
async fn a_burst_past_what_a_batch_holds_is_a_rescan_of_everything() {
    let (_scratch, folder) = scratch();
    let mut batches = watching(&folder, false, Duration::from_secs(2));
    for index in 0..=LIMIT {
        fs::write(folder.join(index.to_string()), "").unwrap();
    }
    let batch = tokio::time::timeout(PATIENCE, batches.next())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(batch, [Change::Rescan(None)]);
}

#[test]
fn a_folder_no_path_leads_to_is_not_watched() {
    let (scratch, folder) = scratch();
    let descriptor = folder_of_machine(&folder, &access()).unwrap();
    let held = HeldFolder::of(&descriptor).unwrap();
    fs::rename(&folder, scratch.path().join("moved")).unwrap();
    fs::create_dir(&folder).unwrap();
    let access = access();
    assert!(matches!(
        watch(&held, false, Duration::ZERO, held_watch(&access, &folder)),
        Err(ErrorCode::NoEntry)
    ));
    assert!(access.uses_but_open_files().is_empty(), "no watch is held");
}

#[test]
fn a_watch_is_held_while_its_stream_is() {
    let (_scratch, folder) = scratch();
    let access = access();
    let held = HeldFolder::of(&folder_of_machine(&folder, &access).unwrap()).unwrap();
    let batches = watch(&held, false, Duration::ZERO, held_watch(&access, &folder)).unwrap();
    assert_eq!(access.uses_but_open_files().len(), 1);
    drop(batches);
    assert!(access.uses_but_open_files().is_empty());
}
