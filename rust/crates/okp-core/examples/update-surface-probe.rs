//! The update surface's own verdict for a machine, asked the way the shell asks it (#725/#726).
//!
//! `scripts/verify-apt-source-instructions.sh` drives this against a real `apt-cache policy`
//! and real apt source files captured inside a clean container, so the acceptance is measured
//! on what a machine actually reports rather than on text a test wrote for itself. The chain it
//! exercises is the shell's: run the command, read the configuration, hand both to
//! [`okp_core::apt_policy`] and [`okp_core::apt_sources`], and let the lifecycle decide.
//!
//! ```text
//! update-surface-probe setup-commands <suite>
//!     Print the repository instructions the app shows a machine on that channel, so the
//!     script can prove they are the README's and that following them works.
//!
//! update-surface-probe describe <running> <announced> [--packaged-suite <suite>]
//!                               [--sources <file>...] < apt-cache-policy-output
//!     Print what the update surface would say for a machine in that state. `--sources` names
//!     apt source files exactly as the shell reads them, which is what separates "no source"
//!     from "a source apt has not read yet".
//! ```

use std::io::Read;

use okp_core::apt_policy::package_source_from_policy;
use okp_core::apt_sources;
use okp_core::update_lifecycle::{
    InstallKind, PackageSourceEvidence, UpdateLifecycle, apt_repository_setup,
};

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("setup-commands") => {
            let Some(suite) = args.next() else {
                eprintln!("usage: update-surface-probe setup-commands <suite>");
                std::process::exit(2);
            };
            match apt_repository_setup(&suite) {
                Some(setup) => print!("{}", setup.commands),
                None => {
                    eprintln!("the archive publishes no suite named {suite}");
                    std::process::exit(2);
                }
            }
        }
        Some("describe") => {
            let (Some(running), Some(announced)) = (args.next(), args.next()) else {
                eprintln!("usage: update-surface-probe describe <running> <announced> ...");
                std::process::exit(2);
            };

            let mut packaged_suite = None;
            let mut source_files: Vec<String> = Vec::new();
            while let Some(flag) = args.next() {
                match flag.as_str() {
                    "--packaged-suite" => packaged_suite = args.next(),
                    "--sources" => source_files.extend(args.by_ref()),
                    other => {
                        eprintln!("unknown argument: {other}");
                        std::process::exit(2);
                    }
                }
            }

            let mut policy = String::new();
            if let Err(error) = std::io::stdin().read_to_string(&mut policy) {
                eprintln!("could not read the apt-cache policy output: {error}");
                std::process::exit(2);
            }

            // Exactly the shell's rule: the policy answers what apt can install, and the
            // configuration answers whether a source exists at all. They differ for the whole
            // window between a .deb's postinst and the next `apt update`.
            let mut evidence = package_source_from_policy(&policy);
            if matches!(evidence, PackageSourceEvidence::NoSource) {
                let contents: Vec<String> = source_files
                    .iter()
                    .map(|path| std::fs::read_to_string(path).unwrap_or_default())
                    .collect();
                if let Some(source) =
                    apt_sources::configured_source(contents.iter().map(String::as_str))
                {
                    evidence = PackageSourceEvidence::ConfiguredButUnread {
                        suite: source.suite,
                    };
                }
            }

            let mut lifecycle = UpdateLifecycle::new(InstallKind::Deb, running);
            lifecycle.package_source_observed(evidence);
            lifecycle.packaged_suite_observed(packaged_suite);
            lifecycle
                .start_check()
                .expect("a .deb install is allowed to check");
            lifecycle
                .check_found(announced)
                .expect("the feed announced a build");

            let presentation = lifecycle.describe();
            println!("capability: {:?}", presentation.capability);
            println!(
                "system_updater_offered: {}",
                presentation.system_updater_offered
            );
            println!(
                "repository_setup: {}",
                match &presentation.repository_setup {
                    Some(setup) => format!("present ({})", setup.suite),
                    None => "absent".to_owned(),
                }
            );
            println!(
                "refresh_command: {}",
                presentation.refresh_command.unwrap_or("absent")
            );
            println!("action: {:?}", presentation.action);
            println!("message: {}", presentation.updates_message);
        }
        other => {
            eprintln!("unknown mode: {other:?}");
            std::process::exit(2);
        }
    }
}
