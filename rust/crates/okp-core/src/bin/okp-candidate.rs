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
//!   okp-candidate publish-decision --requested-sha SHA --build-sha SHA \
//!       --current-sha SHA --build-number N --allocated-build N \
//!       [--published-feed PATH]
//!   okp-candidate classify --phase idle|building --age-seconds N \
//!       [--stall-after N]
//!   okp-candidate stage-velopack --output-dir DIR --channel CHANNEL \
//!       --package-id ID --version VERSION --versioned-appimage FILE
//!   okp-candidate lock-run --lock PATH --owner PATH --phase PHASE \
//!       [--coalesce] -- COMMAND [ARG ...]
//!   okp-candidate project-health --snapshot PATH

use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

use okp_core::acceptance_evidence::{ArtifactKind, PackageArtifact, PackageIdentity};
use okp_core::candidate_build::{
    BuildDecision, BuildPhase, CandidateBuild, DEFAULT_STALL_AFTER_SECONDS, GateResult, GateStatus,
    assemble_candidate_feed, candidate_prune_plan, candidate_version, classify_activity,
    verify_candidate_bundle,
};
use okp_core::candidate_channel::{AcceptanceStatus, CandidateFeed, decide_candidate_publish};
use okp_core::candidate_lock::{
    CandidateLock, CandidateLockError, CandidateLockOwner, CandidateLockPhase,
};
use okp_core::project_health::ProjectHealthSnapshot;
use okp_core::sha256sums::sha256_hex;
use okp_core::velopack_artifacts::stage_versioned_appimage;

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
        Some("verify-bundle") => verify_bundle(&args[1..]),
        Some("feed") => feed(&args[1..]),
        Some("publish-decision") => publish_decision(&args[1..]),
        Some("prune-plan") => prune_plan(&args[1..]),
        Some("version") => version(&args[1..]),
        Some("classify") => classify(&args[1..]),
        Some("stage-velopack") => stage_velopack(&args[1..]),
        Some("lock-run") => lock_run(&args[1..]),
        Some("project-health") => project_health(&args[1..]),
        _ => Err(usage()),
    }
}

fn project_health(args: &[String]) -> Result<(), String> {
    let snapshot_path = match value(args, "--snapshot") {
        Ok(path) => path,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let snapshot = match read_json::<ProjectHealthSnapshot>(Path::new(snapshot_path)) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let outcome = snapshot.evaluate();
    print_json(&outcome)?;
    if outcome.healthy {
        Ok(())
    } else {
        std::process::exit(1);
    }
}

fn lock_run(args: &[String]) -> Result<(), String> {
    let lock_path = Path::new(value(args, "--lock")?);
    let owner_path = Path::new(value(args, "--owner")?);
    let phase = parse_lock_phase(value(args, "--phase")?)?;
    let separator = args
        .iter()
        .position(|arg| arg == "--")
        .ok_or_else(|| format!("lock-run requires -- COMMAND\n{}", usage()))?;
    let command = args
        .get(separator + 1)
        .ok_or_else(|| format!("lock-run requires a command after --\n{}", usage()))?;
    let command_args = &args[separator + 2..];
    let owner = CandidateLockOwner::current(
        phase,
        env::var("GITHUB_RUN_ID").ok(),
        optional_value(args, "--source-sha").map(str::to_owned),
    );
    let lock = match CandidateLock::try_acquire(lock_path, owner_path, owner) {
        Ok(lock) => lock,
        Err(CandidateLockError::Busy(owner)) if args.iter().any(|arg| arg == "--coalesce") => {
            eprintln!("{}", CandidateLockError::Busy(owner));
            return Ok(());
        }
        Err(error) => return Err(error.to_string()),
    };
    eprintln!("candidate lock acquired ({})", lock.owner().diagnostic());

    let status = Command::new(command)
        .args(command_args)
        .env("OKP_CANDIDATE_LOCK_HELD", "1")
        .status()
        .map_err(|error| format!("launch {command}: {error}"))?;
    if !status.success() {
        return Err(format!("{command} exited with {status}"));
    }
    Ok(())
}

fn parse_lock_phase(value: &str) -> Result<CandidateLockPhase, String> {
    match value {
        "build" => Ok(CandidateLockPhase::Build),
        "publish" => Ok(CandidateLockPhase::Publish),
        "promote" => Ok(CandidateLockPhase::Promote),
        "build-and-publish" => Ok(CandidateLockPhase::BuildAndPublish),
        other => Err(format!(
            "invalid --phase {other:?}; expected build|publish|promote|build-and-publish"
        )),
    }
}

fn stage_velopack(args: &[String]) -> Result<(), String> {
    let identity = stage_versioned_appimage(
        Path::new(value(args, "--output-dir")?),
        value(args, "--channel")?,
        value(args, "--package-id")?,
        value(args, "--version")?,
        value(args, "--versioned-appimage")?,
    )?;
    print_json(&identity)
}

fn verify_bundle(args: &[String]) -> Result<(), String> {
    let bundle = Path::new(value(args, "--bundle")?);
    let verified = verify_candidate_bundle(bundle).map_err(|errors| errors.join("\n"))?;
    println!(
        "Candidate bundle {} (source {}) is verified.",
        verified.record.version, verified.record.source_sha
    );
    Ok(())
}

fn feed(args: &[String]) -> Result<(), String> {
    let bundle = Path::new(value(args, "--bundle")?);
    let base_url = value(args, "--base-url")?;
    let output = value(args, "--output")?;
    let acceptance = match value(args, "--acceptance")? {
        "pending" => AcceptanceStatus::Pending,
        "accepted" => AcceptanceStatus::Accepted,
        "rejected" => AcceptanceStatus::Rejected,
        other => return Err(format!("invalid --acceptance {other:?}")),
    };
    let previous = optional_value(args, "--previous")
        .map(|path| read_json::<CandidateFeed>(Path::new(path)))
        .transpose()?;
    let verified = verify_candidate_bundle(bundle).map_err(|errors| errors.join("\n"))?;
    let feed = assemble_candidate_feed(&verified, base_url, acceptance, previous.as_ref())?;
    let output_path = Path::new(output);
    let temp_path = output_path.with_extension(format!("tmp-{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(&feed).map_err(|error| error.to_string())?;
    fs::write(&temp_path, bytes).map_err(|error| format!("{}: {error}", temp_path.display()))?;
    if let Err(error) = fs::rename(&temp_path, output_path) {
        let _ = fs::remove_file(&temp_path);
        return Err(format!("{output}: {error}"));
    }
    Ok(())
}

fn publish_decision(args: &[String]) -> Result<(), String> {
    let build_number = value(args, "--build-number")?
        .parse::<u64>()
        .map_err(|error| format!("invalid --build-number: {error}"))?;
    let allocated_build = value(args, "--allocated-build")?
        .parse::<u64>()
        .map_err(|error| format!("invalid --allocated-build: {error}"))?;
    let published = optional_value(args, "--published-feed")
        .map(|path| read_json::<CandidateFeed>(Path::new(path)))
        .transpose()?;
    let decision = decide_candidate_publish(
        value(args, "--requested-sha")?,
        value(args, "--build-sha")?,
        value(args, "--current-sha")?,
        build_number,
        allocated_build,
        published.as_ref(),
    )?;
    print_json(&decision)
}

fn prune_plan(args: &[String]) -> Result<(), String> {
    let feed = read_json::<CandidateFeed>(Path::new(value(args, "--feed")?))?;
    let assets = read_json::<Vec<String>>(Path::new(value(args, "--assets")?))?;
    for asset in candidate_prune_plan(&feed, &assets) {
        println!("{asset}");
    }
    Ok(())
}

fn version(args: &[String]) -> Result<(), String> {
    let build = value(args, "--build")?
        .parse::<u64>()
        .map_err(|error| format!("invalid --build: {error}"))?;
    println!("{}", candidate_version(value(args, "--base")?, build)?);
    Ok(())
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

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))
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
    "usage:\n  okp-candidate decide --head SHA [--last SHA]\n  okp-candidate record --source-sha SHA --build-number N --version V --started-at TS --finished-at TS [--require-native-hardware] --deb PATH --appimage PATH --gate name:status[:detail] ...\n  okp-candidate promotable --record PATH\n  okp-candidate verify-bundle --bundle DIR\n  okp-candidate feed --bundle DIR --base-url URL --acceptance pending|accepted|rejected [--previous PATH] --output PATH\n  okp-candidate publish-decision --requested-sha SHA --build-sha SHA --current-sha SHA --build-number N --allocated-build N [--published-feed PATH]\n  okp-candidate prune-plan --feed PATH --assets PATH\n  okp-candidate version --base VERSION --build N\n  okp-candidate classify --phase idle|building --age-seconds N [--stall-after N]\n  okp-candidate stage-velopack --output-dir DIR --channel CHANNEL --package-id ID --version VERSION --versioned-appimage FILE\n  okp-candidate lock-run --lock PATH --owner PATH --phase build|publish|promote|build-and-publish [--source-sha SHA] [--coalesce] -- COMMAND [ARG ...]\n  okp-candidate project-health --snapshot PATH".to_owned()
}
