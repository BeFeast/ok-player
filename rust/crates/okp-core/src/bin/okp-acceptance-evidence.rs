use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use okp_core::acceptance_evidence::{
    ArtifactKind, EvidenceManifest, PackageArtifact, PackageIdentity,
};
use okp_core::sha256sums::sha256_hex;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("identity") => write_identity(&args[1..]),
        Some("template") => write_template(&args[1..]),
        Some("validate") => validate(&args[1..]),
        _ => Err(usage()),
    }
}

fn write_identity(args: &[String]) -> Result<(), String> {
    let parsed = PackageArgs::parse(args)?;
    let identity = parsed.identity()?;
    print_json(&identity)
}

fn write_template(args: &[String]) -> Result<(), String> {
    let parsed = PackageArgs::parse(args)?;
    print_json(&EvidenceManifest::template(parsed.identity()?))
}

fn validate(args: &[String]) -> Result<(), String> {
    let manifest_path = value(args, "--manifest")?;
    let identity_path = value(args, "--identity")?;
    let manifest: EvidenceManifest = read_json(manifest_path)?;
    let identity: PackageIdentity = read_json(identity_path)?;
    match manifest.validate_release_ready(&identity) {
        Ok(()) => {
            println!("Linux release acceptance evidence is complete and matches the candidate.");
            Ok(())
        }
        Err(errors) => Err(errors.join("\n")),
    }
}

fn print_json(value: &impl serde::Serialize) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    println!("{json}");
    Ok(())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
    serde_json::from_str(&text).map_err(|error| format!("{path}: {error}"))
}

struct PackageArgs {
    version: String,
    commit_sha: String,
    deb: PathBuf,
    appimage: PathBuf,
}

impl PackageArgs {
    fn parse(args: &[String]) -> Result<Self, String> {
        Ok(Self {
            version: value(args, "--version")?.to_owned(),
            commit_sha: value(args, "--commit")?.to_owned(),
            deb: PathBuf::from(value(args, "--deb")?),
            appimage: PathBuf::from(value(args, "--appimage")?),
        })
    }

    fn identity(&self) -> Result<PackageIdentity, String> {
        Ok(PackageIdentity {
            version: self.version.clone(),
            commit_sha: self.commit_sha.clone(),
            artifacts: vec![
                artifact(ArtifactKind::Debian, &self.deb)?,
                artifact(ArtifactKind::AppImage, &self.appimage)?,
            ],
        })
    }
}

fn artifact(kind: ArtifactKind, path: &Path) -> Result<PackageArtifact, String> {
    let payload = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("{} has no UTF-8 file name", path.display()))?;
    Ok(PackageArtifact {
        kind,
        file_name: file_name.to_owned(),
        sha256: sha256_hex(&payload),
    })
}

fn value<'a>(args: &'a [String], name: &str) -> Result<&'a str, String> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
        .ok_or_else(|| format!("missing {name}\n{}", usage()))
}

fn usage() -> String {
    "usage:\n  okp-acceptance-evidence identity --version V --commit SHA --deb PATH --appimage PATH\n  okp-acceptance-evidence template --version V --commit SHA --deb PATH --appimage PATH\n  okp-acceptance-evidence validate --manifest PATH --identity PATH".to_owned()
}
