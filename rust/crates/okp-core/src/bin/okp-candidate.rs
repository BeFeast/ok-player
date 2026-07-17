//! CLI seam for the coalesced Linux candidate builder (issue #340).
//!
//! The shell entry point (`scripts/build-linux-candidate.sh`) owns process
//! orchestration — the single-run lock, `git fetch`, the clean checkout, and
//! running the bounded gates — but every *decision* is delegated here so no
//! build/promotion state machine lives in bash:
//!
//!   okp-candidate decide --head SHA [--last SHA]
//!   okp-candidate record --source-sha SHA --build-number N --version V \
//!       --started-at TS --finished-at TS [--require-native-hardware] \
//!       --deb PATH --appimage PATH --gate name:status[:detail] ...
//!   okp-candidate promotable --record PATH
//!   okp-candidate classify --phase idle|building --age-seconds N \
//!       [--stall-after N]

use std::env;
use std::fs;
use std::path::Path;

use okp_core::acceptance_evidence::{ArtifactKind, PackageArtifact, PackageIdentity};
use okp_core::candidate_build::{
    BuildDecision, BuildPhase, CandidateBuild, DEFAULT_STALL_AFTER_SECONDS, GateResult, GateStatus,
    classify_activity,
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
        Some("decide") => decide(&args[1..]),
        Some("record") => record(&args[1..]),
        Some("promotable") => promotable(&args[1..]),
        Some("classify") => classify(&args[1..]),
        _ => Err(usage()),
    }
}

fn decide(args: &[String]) -> Result<(), String> {
    let head = value(args, "--head")?;
    let last = optional_value(args, "--last");
    let decision = BuildDecision::resolve(head, last)?;
    print_json(&decision)
}

fn record(args: &[String]) -> Result<(), String> {
    let source_sha = value(args, "--source-sha")?.to_owned();
    let build_number = value(args, "--build-number")?
        .parse::<u64>()
        .map_err(|error| format!("invalid --build-number: {error}"))?;
    let version = value(args, "--version")?.to_owned();
    let started_at = value(args, "--started-at")?.to_owned();
    let finished_at = value(args, "--finished-at")?.to_owned();
    let require_native_hardware = args.iter().any(|arg| arg == "--require-native-hardware");
    let deb = value(args, "--deb")?;
    let appimage = value(args, "--appimage")?;

    let gates = parse_gates(args)?;
    let package = PackageIdentity {
        version: version.clone(),
        commit_sha: source_sha.clone(),
        artifacts: vec![
            artifact(ArtifactKind::Debian, Path::new(deb))?,
            artifact(ArtifactKind::AppImage, Path::new(appimage))?,
        ],
    };

    let build = CandidateBuild::new(
        source_sha,
        build_number,
        version,
        started_at,
        finished_at,
        require_native_hardware,
        gates,
        package,
    );
    print_json(&build)
}

fn promotable(args: &[String]) -> Result<(), String> {
    let record_path = value(args, "--record")?;
    let text =
        fs::read_to_string(record_path).map_err(|error| format!("{record_path}: {error}"))?;
    let build: CandidateBuild =
        serde_json::from_str(&text).map_err(|error| format!("{record_path}: {error}"))?;
    match build.promotable() {
        Ok(()) => {
            println!(
                "Candidate build {} (source {}) is promotable.",
                build.version, build.source_sha
            );
            Ok(())
        }
        Err(errors) => Err(errors.join("\n")),
    }
}

fn classify(args: &[String]) -> Result<(), String> {
    let phase = match value(args, "--phase")? {
        "idle" => BuildPhase::Idle,
        "building" => BuildPhase::Building,
        other => {
            return Err(format!(
                "invalid --phase {other:?}; expected idle or building"
            ));
        }
    };
    let age = value(args, "--age-seconds")?
        .parse::<u64>()
        .map_err(|error| format!("invalid --age-seconds: {error}"))?;
    let stall_after = match optional_value(args, "--stall-after") {
        Some(raw) => raw
            .parse::<u64>()
            .map_err(|error| format!("invalid --stall-after: {error}"))?,
        None => DEFAULT_STALL_AFTER_SECONDS,
    };
    let activity = classify_activity(phase, age, stall_after);
    print_json(&activity)
}

/// Parse repeated `--gate name:status[:detail]` flags.
fn parse_gates(args: &[String]) -> Result<Vec<GateResult>, String> {
    let mut gates = Vec::new();
    let mut index = 0;
    while index + 1 < args.len() {
        if args[index] == "--gate" {
            gates.push(parse_gate(&args[index + 1])?);
            index += 2;
        } else {
            index += 1;
        }
    }
    if gates.is_empty() {
        return Err("at least one --gate name:status is required".to_owned());
    }
    Ok(gates)
}

fn parse_gate(spec: &str) -> Result<GateResult, String> {
    let mut parts = spec.splitn(3, ':');
    let name = parts
        .next()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| format!("gate {spec:?} has no name"))?;
    let status = match parts.next() {
        Some("passed") => GateStatus::Passed,
        Some("failed") => GateStatus::Failed,
        Some("skipped") => GateStatus::Skipped,
        other => {
            return Err(format!(
                "gate {name} has invalid status {other:?}; expected passed|failed|skipped"
            ));
        }
    };
    let detail = parts.next().unwrap_or("").to_owned();
    Ok(GateResult {
        name: name.to_owned(),
        status,
        detail,
    })
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

fn print_json(value: &impl serde::Serialize) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    println!("{json}");
    Ok(())
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
    "usage:\n  okp-candidate decide --head SHA [--last SHA]\n  okp-candidate record --source-sha SHA --build-number N --version V --started-at TS --finished-at TS [--require-native-hardware] --deb PATH --appimage PATH --gate name:status[:detail] ...\n  okp-candidate promotable --record PATH\n  okp-candidate classify --phase idle|building --age-seconds N [--stall-after N]".to_owned()
}
