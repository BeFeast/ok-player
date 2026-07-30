//! The update surface's own verdict for a machine, asked the way the shell asks it (#725).
//!
//! `scripts/verify-apt-source-instructions.sh` drives this against a real `apt-cache policy`
//! captured inside a clean container, so the acceptance is measured on what a machine actually
//! reports rather than on text a test wrote for itself. The chain it exercises is the shell's:
//! run the command, hand the output to [`okp_core::apt_policy`], and let the lifecycle decide.
//!
//! ```text
//! update-surface-probe setup-commands
//!     Print the repository instructions the app shows, so the script can prove they are the
//!     README's and that following them works.
//!
//! update-surface-probe describe <running> <announced> < apt-cache-policy-output
//!     Print what the update surface would say for a machine in that state.
//! ```

use std::io::Read;

use okp_core::apt_policy::package_source_from_policy;
use okp_core::update_lifecycle::{APT_REPOSITORY_SETUP, InstallKind, UpdateLifecycle};

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("setup-commands") => print!("{}", APT_REPOSITORY_SETUP.commands),
        Some("describe") => {
            let (Some(running), Some(announced)) = (args.next(), args.next()) else {
                eprintln!("usage: update-surface-probe describe <running> <announced>");
                std::process::exit(2);
            };
            let mut policy = String::new();
            if let Err(error) = std::io::stdin().read_to_string(&mut policy) {
                eprintln!("could not read the apt-cache policy output: {error}");
                std::process::exit(2);
            }

            let mut lifecycle = UpdateLifecycle::new(InstallKind::Deb, running);
            lifecycle.package_source_observed(package_source_from_policy(&policy));
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
                match presentation.repository_setup {
                    Some(_) => "present",
                    None => "absent",
                }
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
