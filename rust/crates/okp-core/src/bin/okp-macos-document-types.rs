//! Write the macOS document types generated from `okp_core::media_formats` as a standalone
//! property list (`CFBundleDocumentTypes` and `UTImportedTypeDeclarations`), for the app
//! bundle build to put into `Info.plist`.
//!
//! Usage: `okp-macos-document-types [OUTPUT_PLIST]`. Without an argument the property list goes
//! to standard output.

use std::env;
use std::fs;
use std::io::{self, Write as _};

use okp_core::macos_document_types::info_plist_fragment;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let plist = info_plist_fragment();
    match args.as_slice() {
        [] => io::stdout()
            .write_all(plist.as_bytes())
            .map_err(|error| format!("could not write the property list: {error}")),
        [output] if !output.starts_with('-') => {
            fs::write(output, plist).map_err(|error| format!("could not write {output}: {error}"))
        }
        _ => Err("usage: okp-macos-document-types [OUTPUT_PLIST]".to_owned()),
    }
}
