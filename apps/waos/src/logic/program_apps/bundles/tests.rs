use super::*;

/// WaOS's key, as the Keychain would give it.
const KEY: &[u8] = b"a key of the tests' own, 32 bytes";

fn bundles(folder: &Path, system: &str) -> Bundles {
    bundles_made_from(folder, system, "loader 1")
}

/// The bundles in `folder`, on the system at `system`, made from WaOS's files as `made_from` says.
fn bundles_made_from(folder: &Path, system: &str, made_from: &str) -> Bundles {
    let folders = BundleFolders {
        installed: folder.join("Applications/WaOS Programs"),
    };
    let waos = WaosFiles {
        loader: folder.join("waos-program-loader"),
        window_library: folder.join("libwaos_window.dylib"),
        waos: folder.join("WaOS.app"),
        data_folder: folder.join("data"),
    };
    Bundles::open(
        folders,
        (waos, made_from.to_owned()),
        &folder.join("data"),
        system.to_owned(),
        KEY.to_vec(),
    )
}

fn installed(id: &str, name: &str) -> ProgramDescription {
    ProgramDescription {
        id: id.parse().unwrap(),
        name: name.to_owned(),
        version: "1.0.0".to_owned(),
        icon: None,
        category: None,
        file_associations: Vec::new(),
    }
}

/// Records a bundle for `program` at `path` as if it had been written and signed with the code
/// hash `00ff`.
fn record(bundles: &Bundles, program: &ProgramDescription, path: PathBuf) {
    let code_hash_hmac = hex::encode(bundles.hmac(&[0x00, 0xff]).finalize().into_bytes());
    bundles
        .change_record(|record| {
            record.system.clone_from(&bundles.system);
            record.apps.insert(
                program.id.to_string(),
                KeptBundle {
                    path,
                    code_hash: "00ff".to_owned(),
                    code_hash_hmac,
                    version: program.version.clone(),
                    file_associations: program.file_associations.clone(),
                    made_from: bundles.made_from.clone(),
                },
            );
        })
        .unwrap();
}

#[test]
fn an_installed_programs_app_is_named_after_it_a_development_installs_as_any() {
    let folder = tempfile::tempdir().unwrap();
    let bundles = bundles(folder.path(), "27.0.1");
    let notes = installed("dev.example.notes", "Notes");
    assert_eq!(
        bundles.place_of(&notes),
        folder.path().join("Applications/WaOS Programs/Notes.app")
    );
    let development = installed("dev.example.notes.development", "Notes (Development)");
    assert_eq!(
        bundles.place_of(&development),
        folder
            .path()
            .join("Applications/WaOS Programs/Notes (Development).app")
    );
}

#[test]
fn a_name_another_programs_app_has_is_told_apart_by_the_id() {
    let folder = tempfile::tempdir().unwrap();
    let bundles = bundles(folder.path(), "27.0.1");
    let first = installed("dev.example.notes", "Notes");
    record(&bundles, &first, bundles.place_of(&first));
    let second = installed("dev.other.notes", "Notes");
    assert_eq!(
        bundles.place_of(&second),
        folder
            .path()
            .join("Applications/WaOS Programs/Notes (dev.other.notes).app")
    );
    assert_eq!(
        bundles.place_of(&first),
        bundles.kept(&first.id).unwrap().path
    );
}

#[test]
fn the_record_outlives_waos_and_says_the_hash_each_bundle_was_signed_with() {
    let folder = tempfile::tempdir().unwrap();
    let notes = installed("dev.example.notes", "Notes");
    record(
        &bundles(folder.path(), "27.0.1"),
        &notes,
        folder.path().join("Notes.app"),
    );
    let reopened = bundles(folder.path(), "27.0.1");
    assert_eq!(
        reopened.code_hash(&notes.id),
        KeptCodeHash::Signed(vec![0x00, 0xff])
    );
    assert_eq!(reopened.programs(), std::slice::from_ref(&notes.id));
    assert!(!reopened.signed_on_another_system());
    assert_eq!(
        reopened.code_hash(&"dev.example.other".parse().unwrap()),
        KeptCodeHash::None
    );
}

#[test]
fn bundles_signed_on_another_system_are_known_to_be() {
    let folder = tempfile::tempdir().unwrap();
    let notes = installed("dev.example.notes", "Notes");
    record(
        &bundles(folder.path(), "27.0.1"),
        &notes,
        folder.path().join("Notes.app"),
    );
    assert!(bundles(folder.path(), "27.1").signed_on_another_system());
    assert!(!bundles(tempfile::tempdir().unwrap().path(), "27.1").signed_on_another_system());
}

#[test]
fn a_bundle_of_the_programs_version_that_is_there_is_kept_as_it_is() {
    let folder = tempfile::tempdir().unwrap();
    let bundles = bundles(folder.path(), "27.0.1");
    let notes = installed("dev.example.notes", "Notes");
    let path = folder.path().join("Notes.app");
    fs::create_dir_all(&path).unwrap();
    record(&bundles, &notes, path.clone());
    assert_eq!(bundles.ensure(&notes).unwrap().path, path);
}

#[test]
fn an_unreadable_record_is_an_empty_one() {
    let folder = tempfile::tempdir().unwrap();
    fs::create_dir_all(folder.path().join("data")).unwrap();
    fs::write(folder.path().join("data").join(RECORD), "not toml [").unwrap();
    assert!(bundles(folder.path(), "27.0.1").programs().is_empty());
}

#[test]
fn a_code_hash_written_by_anyone_but_waos_is_not_vouched_for() {
    let folder = tempfile::tempdir().unwrap();
    let bundles = bundles(folder.path(), "27.0.1");
    let notes = installed("dev.example.notes", "Notes");
    record(&bundles, &notes, folder.path().join("Notes.app"));
    bundles
        .change_record(|record| {
            let kept = record.apps.get_mut(notes.id.as_str()).unwrap();
            kept.code_hash = "0bad".to_owned();
        })
        .unwrap();
    assert_eq!(bundles.code_hash(&notes.id), KeptCodeHash::Unvouched);
}

#[test]
fn a_record_vouched_for_under_another_key_is_not_vouched_for() {
    let folder = tempfile::tempdir().unwrap();
    let notes = installed("dev.example.notes", "Notes");
    record(
        &bundles(folder.path(), "27.0.1"),
        &notes,
        folder.path().join("Notes.app"),
    );
    let mut under_another_key = bundles(folder.path(), "27.0.1");
    under_another_key.key = b"another key".to_vec();
    assert_eq!(
        under_another_key.code_hash(&notes.id),
        KeptCodeHash::Unvouched
    );
}

#[test]
fn a_bundle_made_from_another_waos_is_to_be_written_again_and_one_from_this_is_not() {
    let folder = tempfile::tempdir().unwrap();
    let notes = installed("dev.example.notes", "Notes");
    let reader = installed("dev.example.reader", "Reader");
    let before = bundles_made_from(folder.path(), "27.0.1", "loader 1");
    record(&before, &notes, folder.path().join("Notes.app"));
    let now = bundles_made_from(folder.path(), "27.0.1", "loader 2");
    record(&now, &reader, folder.path().join("Reader.app"));
    assert_eq!(
        now.stale(),
        std::slice::from_ref(&notes.id),
        "made from the loader before"
    );
    assert!(
        bundles_made_from(folder.path(), "27.1", "loader 2")
            .stale()
            .len()
            == 2
    );
}

#[test]
fn a_bundle_recorded_before_waos_kept_what_it_was_made_from_is_to_be_written_again() {
    let folder = tempfile::tempdir().unwrap();
    let notes = installed("dev.example.notes", "Notes");
    record(
        &bundles(folder.path(), "27.0.1"),
        &notes,
        folder.path().join("Notes.app"),
    );
    let text = fs::read_to_string(folder.path().join("data").join(RECORD)).unwrap();
    let older: String = text
        .lines()
        .filter(|line| !line.starts_with("made_from"))
        .map(|line| format!("{line}\n"))
        .collect();
    fs::write(folder.path().join("data").join(RECORD), older).unwrap();
    assert_eq!(bundles(folder.path(), "27.0.1").stale(), [notes.id]);
}

#[test]
fn what_a_bundle_is_made_from_changes_with_the_loader_and_the_paths_it_names() {
    let folder = tempfile::tempdir().unwrap();
    let loader = folder.path().join("waos-program-loader");
    let files = |data: &str| WaosFiles {
        loader: loader.clone(),
        window_library: folder.path().join("libwaos_window.dylib"),
        waos: folder.path().join("WaOS.app"),
        data_folder: folder.path().join(data),
    };
    fs::write(&loader, b"a loader").unwrap();
    let first = made_from(&files("data")).unwrap();
    assert_eq!(
        made_from(&files("data")).unwrap(),
        first,
        "the same files, the same"
    );
    fs::write(&loader, b"a loader, rebuilt").unwrap();
    let rebuilt = made_from(&files("data")).unwrap();
    assert_ne!(rebuilt, first, "another loader");
    assert_ne!(
        made_from(&files("elsewhere")).unwrap(),
        rebuilt,
        "another path"
    );
}

#[test]
fn a_bundle_opening_other_kinds_of_file_than_its_program_does_now_is_written_again() {
    let folder = tempfile::tempdir().unwrap();
    let bundles = bundles(folder.path(), "27.0.1");
    let notes = installed("dev.example.notes", "Notes");
    let path = folder.path().join("Applications/WaOS Programs/Notes.app");
    fs::create_dir_all(&path).unwrap();
    record(&bundles, &notes, path);
    let kept = bundles.kept(&notes.id).unwrap();
    assert!(bundles.is_current(&kept, &notes));
    let opening = ProgramDescription {
        file_associations: vec![FileAssociation {
            extensions: vec!["note".to_owned()],
            name: "Note".to_owned(),
            mime_type: None,
        }],
        ..notes
    };
    assert!(
        !bundles.is_current(&kept, &opening),
        "the same version, opening a kind of file it did not"
    );
}
