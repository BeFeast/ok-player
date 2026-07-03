use std::env;
use std::path::PathBuf;

/// Generate the C header (`okp_core.h`) from the `#[repr(C)]` surface in `src/lib.rs`.
///
/// Reading the single source file (rather than walking the crate graph) keeps
/// generation off cargo's metadata/lock path, and emitting into `OUT_DIR` keeps the
/// source tree clean: the header is regenerated on every build and so can never drift
/// from the Rust declarations. A C consumer picks it up from the build output directory
/// (or by running cbindgen directly against this crate).
fn main() {
    let crate_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));

    let config = cbindgen::Config::from_file(crate_dir.join("cbindgen.toml"))
        .expect("cbindgen.toml must be readable and valid");

    let bindings = cbindgen::Builder::new()
        .with_config(config)
        .with_src(crate_dir.join("src/lib.rs"))
        .generate()
        .expect("cbindgen must generate the okp-core C header");
    bindings.write_to_file(out_dir.join("okp_core.h"));

    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=cbindgen.toml");
}
