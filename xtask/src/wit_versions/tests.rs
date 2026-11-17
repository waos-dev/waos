use super::*;

fn file(path: &str, text: &str) -> WitFile {
    WitFile {
        path: path.to_owned(),
        text: text.to_owned(),
    }
}

const VIEWS: &str = "package a:p@0.1.0;\n\n/// A view.\ninterface views {\n  open: func();\n}\n";

#[test]
fn a_doc_comment_or_spacing_changes_no_contract() {
    let released = contracts(&[file("wit/deps/a-p-0.1.0/views.wit", VIEWS)]);
    let current = contracts(&[file(
        "wit/deps/a-p-0.1.0/views.wit",
        "package a:p@0.1.0;\n/// A view, said again.\ninterface views { open: func(); }\n",
    )]);
    assert_eq!(
        changed_under_same_version(&released, &current),
        Vec::<String>::new()
    );
}

#[test]
fn an_added_function_under_the_same_version_is_named() {
    let released = contracts(&[file("wit/deps/a-p-0.1.0/views.wit", VIEWS)]);
    let current = contracts(&[file(
        "wit/deps/a-p-0.1.0/views.wit",
        "package a:p@0.1.0;\ninterface views {\n  open: func();\n  close: func();\n}\n",
    )]);
    assert_eq!(
        changed_under_same_version(&released, &current),
        ["a:p@0.1.0"]
    );
}

#[test]
fn a_change_under_a_new_version_is_not_named() {
    let released = contracts(&[file("wit/deps/a-p-0.1.0/views.wit", VIEWS)]);
    let current = contracts(&[file(
        "wit/deps/a-p-0.2.0/views.wit",
        "package a:p@0.2.0;\ninterface views {\n  close: func();\n}\n",
    )]);
    assert_eq!(
        changed_under_same_version(&released, &current),
        Vec::<String>::new()
    );
}

#[test]
fn a_file_without_a_declaration_belongs_to_its_folders_package() {
    let released = contracts(&[
        file("wit/deps/a-p-0.1.0/views.wit", VIEWS),
        file("wit/deps/a-p-0.1.0/screens.wit", "interface screens {}\n"),
    ]);
    let current = contracts(&[
        file("wit/deps/a-p-0.1.0/views.wit", VIEWS),
        file(
            "wit/deps/a-p-0.1.0/screens.wit",
            "interface screens { count: func() -> u32; }\n",
        ),
    ]);
    assert_eq!(
        changed_under_same_version(&released, &current),
        ["a:p@0.1.0"]
    );
}
