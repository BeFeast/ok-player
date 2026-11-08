//! The bundle build calls `okp-macos-document-types`; it must emit exactly the generated
//! property list, to a file or to standard output, and reject anything else.

use std::fs;
use std::process::Command;

use okp_core::macos_document_types::info_plist_fragment;

const BIN: &str = env!("CARGO_BIN_EXE_okp-macos-document-types");

#[test]
fn writes_the_generated_property_list_to_the_named_file() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let output = dir.path().join("DocumentTypes.plist");

    let status = Command::new(BIN)
        .arg(&output)
        .status()
        .expect("generator runs");

    assert!(status.success());
    assert_eq!(
        fs::read_to_string(&output).expect("output exists"),
        info_plist_fragment()
    );
}

#[test]
fn prints_the_generated_property_list_without_an_argument() {
    let output = Command::new(BIN).output().expect("generator runs");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 output"),
        info_plist_fragment()
    );
}

#[test]
fn rejects_extra_arguments_and_flags() {
    for args in [vec!["a.plist", "b.plist"], vec!["--help"]] {
        let output = Command::new(BIN)
            .args(&args)
            .output()
            .expect("generator runs");

        assert!(!output.status.success(), "{args:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("usage:"),
            "{args:?}"
        );
    }
}
