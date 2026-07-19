use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use okp_core::acceptance_evidence::{
    ArtifactKind, CandidateUpgradeEvidence, EvidenceManifest, PackageArtifact, PackageIdentity,
};
use okp_core::fedora_acceptance::{
    AcceptanceVerdict, FedoraAcceptanceManifest, FedoraArtifact, FedoraArtifactKind,
};
use okp_core::presentation_evidence::{
    PresentationRecord, PresentationThresholds, exercise_errors, summarize_window,
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
        Some("candidate-upgrade-validate") => candidate_upgrade_validate(&args[1..]),
        Some("presentation") => presentation(&args[1..]),
        Some("fedora-artifact") => fedora_artifact(&args[1..]),
        Some("fedora-validate") => fedora_validate(&args[1..]),
        _ => Err(usage()),
    }
}

fn candidate_upgrade_validate(args: &[String]) -> Result<(), String> {
    let manifest_path = value(args, "--manifest")?;
    let manifest: CandidateUpgradeEvidence = read_json(manifest_path)?;
    match manifest.validate_cleanup_ready() {
        Ok(()) => {
            println!(
                "Linux candidate upgrade evidence is complete; migration-anchor cleanup is unblocked."
            );
            Ok(())
        }
        Err(errors) => Err(errors.join("\n")),
    }
}

/// Hash a Fedora package file into a `FedoraArtifact` JSON fragment so the
/// collector never re-implements SHA-256 or the artifact shape.
fn fedora_artifact(args: &[String]) -> Result<(), String> {
    let kind = match value(args, "--kind")? {
        "flatpak" => FedoraArtifactKind::Flatpak,
        "rpm" => FedoraArtifactKind::Rpm,
        "copr" => FedoraArtifactKind::Copr,
        other => return Err(format!("unknown --kind {other}; expected flatpak|rpm|copr")),
    };
    let path = PathBuf::from(value(args, "--file")?);
    let payload = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("{} has no UTF-8 file name", path.display()))?;
    print_json(&FedoraArtifact {
        kind,
        file_name: file_name.to_owned(),
        sha256: sha256_hex(&payload),
    })
}

/// Evaluate a collected Fedora acceptance manifest. Pass exits 0, a blocked
/// precondition exits 3 (distinct from a real failure), and a failure exits 1.
/// Blocked is never silently treated as a pass.
fn fedora_validate(args: &[String]) -> Result<(), String> {
    let manifest_path = value(args, "--manifest")?;
    let manifest: FedoraAcceptanceManifest = read_json(manifest_path)?;
    let outcome = manifest.evaluate();
    print_json(&outcome)?;
    match outcome.verdict {
        AcceptanceVerdict::Pass => {
            // The banner goes to stderr so stdout stays a parseable outcome JSON
            // when the harness redirects it to fedora-acceptance-outcome.json.
            eprintln!("Fedora acceptance: PASS");
            Ok(())
        }
        AcceptanceVerdict::Blocked => {
            eprintln!("Fedora acceptance: BLOCKED (precondition unmet, not a pass)");
            std::process::exit(3);
        }
        AcceptanceVerdict::Fail => Err(format!(
            "Fedora acceptance: FAIL\n{}",
            outcome.failures.join("\n")
        )),
    }
}

fn presentation(args: &[String]) -> Result<(), String> {
    let log_path = value(args, "--log")?;
    let warmup_seconds = optional_value(args, "--warmup-seconds")
        .unwrap_or("3")
        .parse::<f64>()
        .map_err(|error| format!("invalid --warmup-seconds: {error}"))?;
    if !warmup_seconds.is_finite() || warmup_seconds < 0.0 {
        return Err("--warmup-seconds must be a finite non-negative number".to_owned());
    }
    let report_only = args.iter().any(|arg| arg == "--report-only");
    let text = fs::read_to_string(log_path).map_err(|error| format!("{log_path}: {error}"))?;
    let records = text
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            serde_json::from_str::<PresentationRecord>(line)
                .map_err(|error| format!("{log_path}:{}: {error}", index + 1))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let first_present_ns = first_final_boundary_ns(&records)
        .ok_or_else(|| "presentation log contains no presentation records".to_owned())?;
    let warmup_ns = (warmup_seconds * 1_000_000_000.0).round() as u64;
    let thresholds = PresentationThresholds::default();
    let mut summary = summarize_window(
        &records,
        first_present_ns.saturating_add(warmup_ns),
        thresholds,
    );
    if records
        .iter()
        .any(|record| matches!(record, PresentationRecord::Action { .. }))
    {
        summary.errors.extend(exercise_errors(&records, thresholds));
    }
    print_json(&summary)?;
    if summary.passed() || report_only {
        Ok(())
    } else {
        Err(summary.errors.join("\n"))
    }
}

fn first_final_boundary_ns(records: &[PresentationRecord]) -> Option<u64> {
    records.iter().find_map(|record| match record {
        PresentationRecord::Present { monotonic_ns, .. }
        | PresentationRecord::CompositorPresented { monotonic_ns, .. } => Some(*monotonic_ns),
        _ => None,
    })
}

fn write_identity(args: &[String]) -> Result<(), String> {
    let parsed = PackageArgs::parse(args)?;
    let identity = parsed.identity()?;
    print_json(&identity)
}

fn write_template(args: &[String]) -> Result<(), String> {
    let parsed = PackageArgs::parse(args)?;
    let build_environment_sha256 = value(args, "--build-environment-sha256")?.to_owned();
    print_json(&EvidenceManifest::template(
        parsed.identity()?,
        build_environment_sha256,
    ))
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

fn optional_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
}

fn usage() -> String {
    "usage:\n  okp-acceptance-evidence identity --version V --commit SHA --deb PATH --appimage PATH\n  okp-acceptance-evidence template --version V --commit SHA --deb PATH --appimage PATH --build-environment-sha256 SHA256\n  okp-acceptance-evidence validate --manifest PATH --identity PATH\n  okp-acceptance-evidence candidate-upgrade-validate --manifest PATH\n  okp-acceptance-evidence presentation --log PATH [--warmup-seconds N] [--report-only]\n  okp-acceptance-evidence fedora-artifact --kind flatpak|rpm|copr --file PATH\n  okp-acceptance-evidence fedora-validate --manifest PATH".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use okp_core::presentation_evidence::PresentationBackend;

    #[test]
    fn compositor_only_log_has_a_final_boundary_start() {
        let records = vec![PresentationRecord::CompositorPresented {
            monotonic_ns: 42,
            backend: PresentationBackend::NativeWaylandDmabuf,
            presented_ns: 84,
            sequence: 1,
            refresh_ns: 16_666_667,
            flags: 0,
            width: 3840,
            height: 2160,
        }];

        assert_eq!(first_final_boundary_ns(&records), Some(42));
    }
}
