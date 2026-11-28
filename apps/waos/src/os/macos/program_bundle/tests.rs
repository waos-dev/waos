use std::process::Command;

use super::*;

/// WaOS's own files, as a bundle refers to them: a stand-in loader, any executable that signs.
fn waos_files(folder: &Path) -> WaosFiles {
    let loader = folder.join("loader");
    fs::copy("/usr/bin/true", &loader).unwrap();
    WaosFiles {
        loader,
        window_library: PathBuf::from(
            "/Applications/WaOS.app/Contents/Frameworks/libwaos_window.dylib",
        ),
        waos: PathBuf::from("/Applications/WaOS.app"),
        data_folder: PathBuf::from("/Users/someone/Library/Application Support/WaOS"),
    }
}

/// `key`'s value in the `Info.plist` at `plist`, as the system reads it.
fn plist_value(plist: &Path, key: &str) -> String {
    let output = Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(plist)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{key}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn a_bundle_says_which_program_it_is_and_where_waos_is() {
    let folder = tempfile::tempdir().unwrap();
    let program = ProgramId::try_from("dev.example.notes".to_owned()).unwrap();
    let waos = waos_files(folder.path());
    let bundle = folder.path().join("Notes & Things.app");
    let contents = BundleContents {
        program: &program,
        name: "Notes & Things",
        version: "1.2.0",
        icon: None,
        category: Some(Category::Productivity),
        file_associations: &[],
        waos: &waos,
    };
    write_files(&bundle, &contents).unwrap();
    let plist = bundle.join("Contents/Info.plist");
    assert_eq!(
        plist_value(&plist, "CFBundleIdentifier"),
        "dev.waos.program.dev.example.notes"
    );
    assert_eq!(plist_value(&plist, "CFBundleName"), "Notes & Things");
    assert_eq!(
        plist_value(&plist, "LSApplicationCategoryType"),
        "public.app-category.productivity"
    );
    assert_eq!(plist_value(&plist, "CFBundleShortVersionString"), "1.2.0");
    assert_eq!(plist_value(&plist, "CFBundleExecutable"), LOADER);
    assert_eq!(plist_value(&plist, PROGRAM_ID_KEY), "dev.example.notes");
    assert_eq!(plist_value(&plist, WAOS_KEY), "/Applications/WaOS.app");
    assert_eq!(
        plist_value(&plist, DATA_FOLDER_KEY),
        "/Users/someone/Library/Application Support/WaOS"
    );
    assert!(bundle.join("Contents/MacOS").join(LOADER).is_file());
}

#[test]
fn a_bundle_carries_the_packages_icon_at_every_size() {
    let folder = tempfile::tempdir().unwrap();
    let program = ProgramId::try_from("dev.example.notes".to_owned()).unwrap();
    let waos = waos_files(folder.path());
    let bundle = folder.path().join("Notes.app");
    let icon = fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icon.png")).unwrap();
    let contents = BundleContents {
        program: &program,
        name: "Notes",
        version: "1.0.0",
        icon: Some(&icon),
        category: None,
        file_associations: &[],
        waos: &waos,
    };
    write_files(&bundle, &contents).unwrap();
    let plist = bundle.join("Contents/Info.plist");
    assert_eq!(plist_value(&plist, "CFBundleIconFile"), ICON);
    let icns = fs::read(bundle.join("Contents/Resources/program.icns")).unwrap();
    assert_eq!(&icns[..4], b"icns");
}

#[test]
fn a_signed_bundle_has_a_code_hash_that_changes_with_it() {
    let folder = tempfile::tempdir().unwrap();
    let program = ProgramId::try_from("dev.example.notes".to_owned()).unwrap();
    let waos = waos_files(folder.path());
    let bundle = folder.path().join("Notes.app");
    let mut contents = BundleContents {
        program: &program,
        name: "Notes",
        version: "1.0.0",
        icon: None,
        category: None,
        file_associations: &[],
        waos: &waos,
    };
    write_files(&bundle, &contents).unwrap();
    sign(&bundle).unwrap();
    let first = code_signature::code_hash_of_bundle(&bundle).unwrap();
    assert_eq!(first.len(), 20, "a code directory hash");
    contents.version = "1.1.0";
    write_files(&bundle, &contents).unwrap();
    sign(&bundle).unwrap();
    let second = code_signature::code_hash_of_bundle(&bundle).unwrap();
    assert_ne!(first, second, "Info.plist is sealed by the signature");
}

#[test]
fn an_app_is_named_apart_from_the_apps_it_could_pose_as() {
    let program = ProgramId::try_from("com.apple.safari".to_owned()).unwrap();
    assert_eq!(bundle_id(&program), "dev.waos.program.com.apple.safari");
}

#[test]
fn a_bundle_offers_its_app_for_the_kinds_of_file_its_program_opens_never_as_their_default() {
    let folder = tempfile::tempdir().unwrap();
    let program = ProgramId::try_from("dev.example.ruffle".to_owned()).unwrap();
    let waos = waos_files(folder.path());
    let bundle = folder.path().join("Ruffle.app");
    let flash = FileAssociation {
        extensions: vec!["swf".to_owned(), "spl".to_owned()],
        name: "Flash movie".to_owned(),
        mime_type: Some("application/x-shockwave-flash".to_owned()),
    };
    let contents = BundleContents {
        program: &program,
        name: "Ruffle",
        version: "1.0.0",
        icon: None,
        category: None,
        file_associations: std::slice::from_ref(&flash),
        waos: &waos,
    };
    write_files(&bundle, &contents).unwrap();
    let plist = bundle.join("Contents/Info.plist");
    assert_eq!(
        plist_value(&plist, "CFBundleDocumentTypes.0.CFBundleTypeName"),
        "Flash movie"
    );
    assert_eq!(
        plist_value(&plist, "CFBundleDocumentTypes.0.CFBundleTypeExtensions.1"),
        "spl"
    );
    assert_eq!(
        plist_value(&plist, "CFBundleDocumentTypes.0.LSHandlerRank"),
        "Alternate"
    );
    assert_eq!(
        plist_value(&plist, "CFBundleDocumentTypes.0.CFBundleTypeRole"),
        "Viewer"
    );
    let plain = folder.path().join("Notes.app");
    let contents = BundleContents {
        file_associations: &[],
        ..contents
    };
    write_files(&plain, &contents).unwrap();
    let text = fs::read_to_string(plain.join("Contents/Info.plist")).unwrap();
    assert!(!text.contains("CFBundleDocumentTypes"), "{text}");
}
