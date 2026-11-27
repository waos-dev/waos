//! What the user lets programs use: the global settings, which start as the design's defaults,
//! each program's own in its folder, both kept across a reopen; a program's grants, read live
//! from them, keeping its answers and the machines picked for it, installed or run in development,
//! and answering its pickers and dialogs as its user set them;
//! and a program's settings and its own folders going with it when it is removed.

mod support;

use std::fs;
use std::path::Path;
use std::time::Duration;

use futures::FutureExt;
use support::{block_on, install, packages, remove, write_package};
use waos_capabilities::{Answer, DomainRule, FileDialog, Mount, NetworkGrant, Question, Setting};
use waos_package::{ProgramId, SigningKey};
use waos_package_manager::PackageManager;
use waos_store::{GlobalSettings, ProgramSettings, SourceProxy, Store, StoreError};

const TRIANGLE: &str = "dev.example.triangle";

fn triangle() -> ProgramId {
    TRIANGLE.parse().unwrap()
}

/// A store in `folder` with the triangle installed, and the package manager it was installed with.
fn store_with_triangle(folder: &Path) -> (Store, PackageManager) {
    let store = Store::open(&folder.join("store"), SourceProxy::None).unwrap();
    let packages = packages(&folder.join("store"));
    install_triangle(folder, &packages);
    (store, packages)
}

/// Installs the triangle with `packages`, from a package written in `folder`.
fn install_triangle(folder: &Path, packages: &PackageManager) {
    let package = write_package(folder, TRIANGLE, "1.0.0", &SigningKey::generate());
    block_on(install(packages, &package)).unwrap();
}

fn network_off() -> NetworkGrant {
    NetworkGrant {
        on: false,
        domains: DomainRule::Any,
    }
}

#[test]
fn the_global_settings_start_as_the_defaults_and_are_kept() {
    let scratch = tempfile::tempdir().unwrap();
    let directory = scratch.path().join("store");
    let store = Store::open(&directory, SourceProxy::None).unwrap();
    let defaults = store.global_settings().unwrap();
    assert_eq!(defaults, GlobalSettings::default());
    assert!(defaults.network.on && defaults.network.domains == DomainRule::Any);
    assert!(defaults.clipboard_read && defaults.clipboard_write);
    assert!(
        !defaults.local_network,
        "the local network is off until the user turns it on"
    );
    assert!(defaults.file_system.mounts.is_empty());
    assert_eq!(*defaults.answers.get(Question::Notifications), None);

    store
        .change_global_settings(|settings| settings.clipboard_read = false)
        .unwrap();
    let reopened = Store::open(&directory, SourceProxy::None).unwrap();
    assert!(!reopened.global_settings().unwrap().clipboard_read);
}

#[test]
fn a_programs_settings_are_its_own_kept_beside_its_install_and_go_with_it() {
    let scratch = tempfile::tempdir().unwrap();
    let (store, packages) = store_with_triangle(scratch.path());
    assert_eq!(
        store.program_settings(&triangle()).unwrap(),
        ProgramSettings::default()
    );
    store
        .change_program_settings(&triangle(), |settings| {
            settings.network = Setting::Own(network_off());
        })
        .unwrap();
    let reopened = Store::open(&scratch.path().join("store"), SourceProxy::None).unwrap();
    assert_eq!(
        reopened.program_settings(&triangle()).unwrap().network,
        Setting::Own(network_off()),
        "kept across a reopen, which clears what an install left"
    );

    block_on(remove(&reopened, &packages, &triangle()));
    assert!(matches!(
        reopened.program_settings(&triangle()),
        Err(StoreError::NotInstalled(_))
    ));
    install_triangle(scratch.path(), &packages);
    assert_eq!(
        reopened.program_settings(&triangle()).unwrap(),
        ProgramSettings::default(),
        "a program installed again starts afresh"
    );
}

#[test]
fn a_read_of_a_program_not_installed_keeps_nothing_and_a_setting_keeps_its_folder() {
    let scratch = tempfile::tempdir().unwrap();
    let store = Store::open(&scratch.path().join("store"), SourceProxy::None).unwrap();
    let folder = scratch.path().join("store/programs").join(TRIANGLE);
    let read = store.program_settings(&triangle());
    assert!(matches!(read, Err(StoreError::NotInstalled(_))));
    assert!(!folder.exists(), "reading keeps nothing");

    store
        .change_program_settings(&triangle(), |settings| {
            settings.clipboard_read = Setting::Own(false);
        })
        .unwrap();
    assert!(
        folder.is_dir(),
        "a setting set before its first run keeps one"
    );
    assert_eq!(
        store.program_settings(&triangle()).unwrap().clipboard_read,
        Setting::Own(false)
    );
}

#[test]
fn a_programs_own_folders_are_made_at_its_first_run_and_kept_until_it_is_removed() {
    let scratch = tempfile::tempdir().unwrap();
    let directory = scratch.path().join("store");
    let store = Store::open(&directory, SourceProxy::None).unwrap();
    let folders = store.folders(&triangle()).unwrap();
    assert_eq!(
        folders.data,
        directory.join("programs").join(TRIANGLE).join("data")
    );
    assert_eq!(
        folders.cache,
        directory.join("programs").join(TRIANGLE).join("cache")
    );
    fs::write(folders.data.join("notes.txt"), "kept").unwrap();

    let reopened = Store::open(&directory, SourceProxy::None).unwrap();
    let packages = packages(&directory);
    assert!(packages.installed().is_empty(), "run, not installed");
    assert_eq!(fs::read(folders.data.join("notes.txt")).unwrap(), b"kept");

    install_triangle(scratch.path(), &packages);
    assert_eq!(reopened.folders(&triangle()).unwrap(), folders);
    assert_eq!(
        fs::read(folders.data.join("notes.txt")).unwrap(),
        b"kept",
        "the installed program is the one its development runs were"
    );

    block_on(remove(&reopened, &packages, &triangle()));
    assert!(!folders.data.exists() && !folders.cache.exists());
}

#[test]
fn a_program_run_in_development_has_its_own_settings_installed_or_not() {
    let scratch = tempfile::tempdir().unwrap();
    let directory = scratch.path().join("store");
    let store = Store::open(&directory, SourceProxy::None).unwrap();
    store.folders(&triangle()).unwrap();
    let grants = store.grants(&triangle());
    assert!(grants.clipboard_read(), "the global setting");
    store
        .change_program_settings(&triangle(), |settings| {
            settings.clipboard_read = Setting::Own(false);
        })
        .unwrap();
    assert!(!grants.clipboard_read(), "its own, read live");

    let changed = grants.changed();
    grants.set_answer(Question::Microphone, Answer::Granted);
    assert!(heard(changed));
    let reopened = Store::open(&directory, SourceProxy::None).unwrap();
    assert_eq!(
        reopened.grants(&triangle()).answer(Question::Microphone),
        Some(Answer::Granted),
        "kept for its next run"
    );
}

#[test]
fn a_programs_grants_follow_the_global_settings_until_it_has_its_own() {
    let scratch = tempfile::tempdir().unwrap();
    let (store, _packages) = store_with_triangle(scratch.path());
    let grants = store.grants(&triangle());
    assert!(grants.network().on);
    store
        .change_global_settings(|settings| settings.network = network_off())
        .unwrap();
    assert!(!grants.network().on, "read live");
    store
        .change_program_settings(&triangle(), |settings| {
            settings.network = Setting::Own(NetworkGrant {
                on: true,
                domains: DomainRule::Only(vec!["example.com".parse().unwrap()]),
            });
        })
        .unwrap();
    let network = grants.network();
    assert!(network.on, "its own");
    assert_eq!(
        network.domains,
        DomainRule::Only(vec!["example.com".parse().unwrap()])
    );

    assert!(!grants.local_network());
    store
        .change_program_settings(&triangle(), |settings| {
            settings.local_network = Setting::Own(true);
        })
        .unwrap();
    assert!(grants.local_network(), "its own local network");
}

#[test]
fn a_programs_answer_is_kept_in_its_settings() {
    let scratch = tempfile::tempdir().unwrap();
    let (store, _packages) = store_with_triangle(scratch.path());
    let grants = store.grants(&triangle());
    assert_eq!(grants.answer(Question::Camera), None, "to be asked");
    grants.set_answer(Question::Camera, Answer::Granted);
    let reopened = Store::open(&scratch.path().join("store"), SourceProxy::None).unwrap();
    assert_eq!(
        reopened.grants(&triangle()).answer(Question::Camera),
        Some(Answer::Granted)
    );
    assert_eq!(
        *reopened
            .program_settings(&triangle())
            .unwrap()
            .answers
            .get(Question::Camera),
        Setting::Own(Answer::Granted)
    );
}

#[test]
fn a_programs_answer_to_a_link_scheme_is_kept_in_its_settings() {
    let scratch = tempfile::tempdir().unwrap();
    let (store, _packages) = store_with_triangle(scratch.path());
    let grants = store.grants(&triangle());
    assert_eq!(grants.link_scheme_answer("zoommtg"), None, "to be asked");
    grants.set_link_scheme_answer("zoommtg", Answer::Granted);
    let reopened = Store::open(&scratch.path().join("store"), SourceProxy::None).unwrap();
    let grants = reopened.grants(&triangle());
    assert_eq!(grants.link_scheme_answer("zoommtg"), Some(Answer::Granted));
    assert_eq!(
        grants.link_scheme_answer("tel"),
        None,
        "each scheme its own"
    );
}

#[test]
fn a_pick_is_kept_among_a_programs_own_mounts_beside_the_global_ones() {
    let scratch = tempfile::tempdir().unwrap();
    let (store, _packages) = store_with_triangle(scratch.path());
    let shared = Mount {
        host: scratch.path().join("shared"),
        at: "/shared".into(),
        writable: false,
    };
    store
        .change_global_settings(|settings| settings.file_system.mounts.push(shared.clone()))
        .unwrap();
    let picked = Mount {
        host: scratch.path().join("notes.txt"),
        at: scratch.path().join("notes.txt"),
        writable: false,
    };
    let grants = store.grants(&triangle());
    grants.keep_picked(picked.clone());
    let writable = Mount {
        writable: true,
        ..picked
    };
    grants.keep_picked(writable.clone());

    let reopened = Store::open(&scratch.path().join("store"), SourceProxy::None).unwrap();
    assert_eq!(
        reopened.grants(&triangle()).file_system().mounts,
        [shared, writable],
        "the global ones, then the pick, picked again writable"
    );
}

#[test]
fn a_machine_picked_is_kept_among_a_programs_own_once_however_often_it_is_picked() {
    let scratch = tempfile::tempdir().unwrap();
    let (store, _packages) = store_with_triangle(scratch.path());
    let grants = store.grants(&triangle());
    assert!(
        grants.machines().is_empty(),
        "none until the user picks one"
    );
    grants.keep_machine("this-computer");
    grants.keep_machine("this-computer");

    let reopened = Store::open(&scratch.path().join("store"), SourceProxy::None).unwrap();
    assert_eq!(reopened.grants(&triangle()).machines(), ["this-computer"]);
    assert_eq!(
        reopened.program_settings(&triangle()).unwrap().machines,
        ["this-computer"]
    );
}

#[test]
fn a_programs_grants_answer_its_pickers_and_dialogs_as_its_user_set_them_across_a_reopen() {
    let scratch = tempfile::tempdir().unwrap();
    let (store, _packages) = store_with_triangle(scratch.path());
    let grants = store.grants(&triangle());
    assert_eq!(grants.machine_answer(), None, "to be asked");
    assert_eq!(grants.file_dialog_answer(FileDialog::SaveFile), None);
    store
        .change_program_settings(&triangle(), |settings| {
            settings.answer_with.machine = Some("this-computer".to_owned());
            settings.answer_with.audio_output = Some("speakers".to_owned());
            settings.answer_with.save_file = Some("/tmp/out.pdf".into());
        })
        .unwrap();

    let reopened = Store::open(&scratch.path().join("store"), SourceProxy::None).unwrap();
    let grants = reopened.grants(&triangle());
    assert_eq!(grants.machine_answer().as_deref(), Some("this-computer"));
    assert_eq!(grants.audio_output_answer().as_deref(), Some("speakers"));
    assert_eq!(
        grants.file_dialog_answer(FileDialog::SaveFile),
        Some(vec!["/tmp/out.pdf".into()])
    );
    assert_eq!(grants.file_dialog_answer(FileDialog::OpenFile), None);
    assert!(grants.machines().is_empty(), "answered, not yet picked");
}

#[test]
fn a_change_of_settings_is_heard_by_a_programs_grants() {
    let scratch = tempfile::tempdir().unwrap();
    let (store, _packages) = store_with_triangle(scratch.path());
    let grants = store.grants(&triangle());

    let changed = grants.changed();
    store
        .change_global_settings(|settings| settings.clipboard_write = false)
        .unwrap();
    assert!(heard(changed), "a global change");

    let changed = grants.changed();
    grants.set_answer(Question::Notifications, Answer::Denied);
    assert!(heard(changed), "an answer kept");

    let changed = grants.changed();
    assert!(!heard(changed), "nothing changed since");
}

#[test]
fn settings_that_cannot_be_read_grant_nothing() {
    let scratch = tempfile::tempdir().unwrap();
    store_with_triangle(scratch.path());
    let settings = scratch
        .path()
        .join("store/programs")
        .join(TRIANGLE)
        .join("settings.toml");
    fs::write(&settings, "clipboard-read = \"perhaps\"\n").unwrap();
    let grants = Store::open(&scratch.path().join("store"), SourceProxy::None)
        .unwrap()
        .grants(&triangle());
    assert!(!grants.network().on && !grants.clipboard_read() && !grants.clipboard_write());
    assert_eq!(grants.answer(Question::Notifications), Some(Answer::Denied));
    assert_eq!(grants.link_scheme_answer("zoommtg"), Some(Answer::Denied));
    assert!(grants.machines().is_empty());
}

/// Whether `changed` resolves within a moment.
fn heard(changed: waos_capabilities::ChangedFuture) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_millis(200);
    let mut changed = changed;
    while std::time::Instant::now() < deadline {
        if changed.as_mut().now_or_never().is_some() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}
