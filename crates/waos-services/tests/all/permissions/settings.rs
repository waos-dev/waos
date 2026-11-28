//! What the user lets apps use: the global settings, which start as the design's defaults, each
//! app's own, both kept across a reopen in WaOS's files where they always were; an app's grants,
//! read live, keeping its answers and the machines and files picked for it, and answering its
//! pickers and dialogs as its user set them; and an app's settings refused while it is removed,
//! and forgotten once it is.

use std::fs;
use std::sync::Arc;

use super::support::{Removals, TestPrompts, heard, triangle};
use futures::FutureExt;
use waos_services::permissions::{
    Answer, AppSettings, DomainRule, FileDialog, GlobalSettings, Mount, NetworkGrant, NoPrompts,
    NothingInstalled, Permissions, Question, Setting, SettingsError, SettingsFiles,
    SettingsInMemory,
};

fn network_off() -> NetworkGrant {
    NetworkGrant {
        on: false,
        domains: DomainRule::Any,
    }
}

/// Permissions over WaOS's files in `directory`, with nobody to ask.
fn over_files(directory: &std::path::Path) -> Permissions {
    let files = Arc::new(SettingsFiles::new(directory));
    Permissions::open(files, Arc::new(NoPrompts), Arc::new(NothingInstalled))
}

/// Permissions in memory, asking the user `prompts`.
fn asking(prompts: &Arc<TestPrompts>) -> Permissions {
    let prompts = Arc::clone(prompts) as Arc<dyn waos_services::permissions::Prompts>;
    let installed = Arc::new(NothingInstalled);
    Permissions::open(Arc::new(SettingsInMemory::default()), prompts, installed)
}

fn in_memory() -> Permissions {
    asking(&Arc::default())
}

#[test]
fn the_settings_are_kept_in_the_files_they_always_were_in() {
    let scratch = tempfile::tempdir().unwrap();
    let permissions = over_files(scratch.path());
    let defaults = permissions.global_settings().unwrap();
    assert_eq!(defaults, GlobalSettings::default());
    assert!(!defaults.local_network, "off until the user turns it on");
    assert_eq!(
        permissions.app_settings(&triangle()).unwrap(),
        AppSettings::default()
    );
    let folder = scratch.path().join("programs").join(triangle().as_str());
    assert!(!folder.exists(), "reading keeps nothing");

    permissions
        .change_global_settings(|settings| settings.clipboard_read = false)
        .unwrap();
    permissions
        .change_app_settings(&triangle(), |settings| {
            settings.network = Setting::Own(network_off());
        })
        .unwrap();
    assert!(scratch.path().join("settings.toml").is_file());
    assert!(
        folder.join("settings.toml").is_file(),
        "in the app's folder, set before its install"
    );

    let reopened = over_files(scratch.path());
    assert!(!reopened.global_settings().unwrap().clipboard_read);
    assert_eq!(
        reopened.app_settings(&triangle()).unwrap().network,
        Setting::Own(network_off())
    );
}

#[test]
fn an_apps_settings_take_no_write_while_it_is_removed_and_are_forgotten_once_it_is() {
    let removals = Arc::new(Removals::default());
    let prompts = Arc::new(TestPrompts::default());
    let permissions = Permissions::open(
        Arc::new(SettingsInMemory::default()),
        Arc::clone(&prompts) as Arc<dyn waos_services::permissions::Prompts>,
        Arc::clone(&removals) as Arc<dyn waos_services::permissions::InstalledApps>,
    );
    let own_network = |permissions: &Permissions| {
        permissions
            .change_app_settings(&triangle(), |settings| {
                settings.network = Setting::Own(network_off());
            })
            .map(drop)
    };
    own_network(&permissions).unwrap();

    removals.begin(&triangle());
    let refused = own_network(&permissions);
    assert!(
        matches!(refused, Err(SettingsError::Removing(_))),
        "{refused:?}"
    );
    let grants = permissions.of(&triangle());
    let mut asked = grants.ask(Question::Camera);
    assert_eq!(asked.as_mut().now_or_never(), None, "asked of the user");
    prompts.answer(Some(Answer::Granted));
    assert_eq!(
        asked.as_mut().now_or_never(),
        Some(Some(Answer::Granted)),
        "the run has its answer"
    );
    assert_eq!(grants.answer(Question::Camera), None, "but it is not kept");

    let changed = permissions.changed();
    removals.end(&triangle());
    assert_eq!(
        permissions.app_settings(&triangle()).unwrap(),
        AppSettings::default(),
        "forgotten: an install of it again starts afresh"
    );
    assert!(heard(changed), "its grants hear it");
}

#[test]
fn an_apps_grants_follow_the_global_settings_until_it_has_its_own() {
    let permissions = in_memory();
    let grants = permissions.of(&triangle());
    assert!(grants.network().on);
    permissions
        .change_global_settings(|settings| settings.network = network_off())
        .unwrap();
    assert!(!grants.network().on, "read live");
    let only_example = DomainRule::Only(vec!["example.com".parse().unwrap()]);
    permissions
        .change_app_settings(&triangle(), |settings| {
            settings.network = Setting::Own(NetworkGrant {
                on: true,
                domains: only_example.clone(),
            });
            settings.local_network = Setting::Own(true);
        })
        .unwrap();
    assert_eq!(grants.network().domains, only_example, "its own");
    assert!(grants.network().on && grants.local_network());
}

#[test]
fn a_pick_is_kept_among_an_apps_own_mounts_beside_the_global_ones() {
    let permissions = in_memory();
    let shared = Mount {
        host: "/shared".into(),
        at: "/shared".into(),
        writable: false,
    };
    permissions
        .change_global_settings(|settings| settings.file_system.mounts.push(shared.clone()))
        .unwrap();
    let picked = Mount {
        host: "/Users/me/notes.txt".into(),
        at: "/Users/me/notes.txt".into(),
        writable: false,
    };
    let grants = permissions.of(&triangle());
    grants.keep_picked(picked.clone());
    let writable = Mount {
        writable: true,
        ..picked
    };
    grants.keep_picked(writable.clone());
    assert_eq!(
        grants.file_system().mounts,
        [shared, writable],
        "the global ones, then the pick, picked again writable"
    );
}

#[test]
fn a_machine_picked_is_kept_once_however_often_it_is_picked() {
    let permissions = in_memory();
    let grants = permissions.of(&triangle());
    assert!(grants.machines().is_empty());
    grants.keep_machine("this-computer");
    grants.keep_machine("this-computer");
    assert_eq!(grants.machines(), ["this-computer"]);
}

#[test]
fn an_apps_grants_answer_its_pickers_and_dialogs_as_its_user_set_them() {
    let permissions = in_memory();
    let grants = permissions.of(&triangle());
    assert_eq!(grants.machine_answer(), None, "to be asked");
    permissions
        .change_app_settings(&triangle(), |settings| {
            settings.answer_with.machine = Some("this-computer".to_owned());
            settings.answer_with.audio_output = Some("speakers".to_owned());
            settings.answer_with.save_file = Some("/tmp/out.pdf".into());
        })
        .unwrap();
    assert_eq!(grants.machine_answer().as_deref(), Some("this-computer"));
    assert_eq!(grants.audio_output_answer().as_deref(), Some("speakers"));
    assert_eq!(
        grants.file_dialog_answer(FileDialog::SaveFile),
        Some(vec!["/tmp/out.pdf".into()])
    );
    assert_eq!(grants.file_dialog_answer(FileDialog::OpenFile), None);
}

#[test]
fn settings_that_cannot_be_read_grant_nothing() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = scratch.path().join("programs").join(triangle().as_str());
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("settings.toml"),
        "clipboard-read = \"perhaps\"\n",
    )
    .unwrap();
    let grants = over_files(scratch.path()).of(&triangle());
    assert!(!grants.network().on && !grants.clipboard_read() && !grants.clipboard_write());
    assert_eq!(grants.answer(Question::Notifications), Some(Answer::Denied));
    assert_eq!(grants.link_scheme_answer("zoommtg"), Some(Answer::Denied));
    assert!(grants.machines().is_empty());
}
