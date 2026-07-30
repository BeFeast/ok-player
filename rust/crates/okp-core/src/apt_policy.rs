//! What apt can actually deliver for this machine, read off `apt-cache policy`
//! (issue #725).
//!
//! The update surface used to tell every `.deb` install to "update with your
//! package manager (apt)" on the strength of the install kind alone. On the
//! machine that reported #725 there was no OK Player apt source at all — the
//! package had been downloaded and installed by hand — so apt could not deliver
//! the version the app was naming, and the software updater the app offered
//! correctly answered that everything was up to date. Every button led nowhere.
//!
//! The observable that settles it is apt's own answer. `apt-cache policy
//! ok-player` names what is installed, what apt would install next, and which
//! source each version comes from:
//!
//! ```text
//! ok-player:
//!   Installed: 0.11.0-beta.0.208
//!   Candidate: 0.11.0-beta.0.210
//!   Version table:
//!      0.11.0-beta.0.210 500
//!         500 https://befeast.github.io/ok-player/apt candidate/main amd64 Packages
//!  *** 0.11.0-beta.0.208 500
//!         500 https://befeast.github.io/ok-player/apt candidate/main amd64 Packages
//!         100 /var/lib/dpkg/status
//! ```
//!
//! This module is the pure half of that question: the shell runs the command,
//! this decides what the output means, and
//! [`crate::update_lifecycle::UpdateLifecycle`] decides what to say about it.
//! Nothing here touches a process or the filesystem, so the rules are testable
//! against captured output from a real machine.
//!
//! Three rules carry the whole answer, and each is apt's own judgement rather
//! than one reconstructed here:
//!
//! * **A source is a version-table origin that is not dpkg's status file.**
//!   dpkg's status is the record of what is installed; it delivers nothing. A
//!   package known only through it — the #725 machine exactly — has no source.
//! * **What apt would install is `Candidate:`.** It is by definition the version
//!   `apt-get install` resolves to, pins and all, so it is what the app may name.
//! * **A candidate equal to what is installed delivers nothing.** apt has
//!   already compared them with its own comparator, which is the one that
//!   decides whether the upgrade happens.
//!
//! Versions are reported in apt's language (the Debian encoding of the build,
//! `1:0.11.0~beta.0.210` since #709). [`crate::package_version`] translates one
//! back into the build version the rest of the app speaks; a version this
//! packaging never emits is passed through as apt printed it rather than
//! guessed at, because it is still exactly what apt would install.

use crate::package_version;
use crate::update_lifecycle::PackageSourceEvidence;

/// The origin apt prints for the dpkg database itself. It is the record of what
/// is already installed, so it can deliver nothing.
pub const DPKG_STATUS_ORIGIN: &str = "/var/lib/dpkg/status";

/// What `apt-cache policy <package>` establishes about this machine.
///
/// `output` is the command's standard output. A command that could not be run
/// at all is not this function's business: the shell reports
/// [`PackageSourceEvidence::Unestablished`] itself, because "apt-cache is
/// absent" and "apt-cache said nothing useful" are the same honest state.
pub fn package_source_from_policy(output: &str) -> PackageSourceEvidence {
    let policy = PackagePolicy::parse(output);
    let Some(suite) = policy.subscribed_suite() else {
        // Every version apt knows about comes from dpkg's own status file (or
        // it knows of none at all): nothing configured on this machine can
        // fetch this package.
        return PackageSourceEvidence::NoSource;
    };
    PackageSourceEvidence::Source {
        suite,
        deliverable: policy.deliverable(),
    }
}

/// One version row of the version table, with the origins that carry it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct VersionRow {
    version: String,
    /// Suites this version is served from, in the order apt printed them.
    /// Empty when dpkg's status file is the only origin.
    suites: Vec<String>,
}

/// The parts of `apt-cache policy <package>` this module reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct PackagePolicy {
    installed: Option<String>,
    candidate: Option<String>,
    versions: Vec<VersionRow>,
}

impl PackagePolicy {
    fn parse(output: &str) -> Self {
        let mut policy = Self::default();
        for line in output.lines() {
            let trimmed = line.trim();
            if let Some(value) = trimmed.strip_prefix("Installed:") {
                policy.installed = present_version(value);
                continue;
            }
            if let Some(value) = trimmed.strip_prefix("Candidate:") {
                policy.candidate = present_version(value);
                continue;
            }
            match parse_table_row(trimmed) {
                Some(TableRow::Version(version)) => policy.versions.push(VersionRow {
                    version,
                    suites: Vec::new(),
                }),
                // An origin belongs to the version row above it. A row with no
                // version above it is apt printing something this parser does
                // not model, and is dropped rather than attributed to a version
                // it is not about.
                Some(TableRow::Origin(suite)) => {
                    if let Some(row) = policy.versions.last_mut()
                        && let Some(suite) = suite
                    {
                        row.suites.push(suite);
                    }
                }
                None => {}
            }
        }
        policy
    }

    /// The suite this machine subscribes to for this package, or nothing when
    /// no configured source carries it.
    ///
    /// The candidate's own suite is preferred, because that is the source the
    /// next `apt-get install` would fetch from. A machine carrying both the
    /// `stable` and the `candidate` stanza — which the README warns against —
    /// is therefore described by the one that would actually deliver.
    fn subscribed_suite(&self) -> Option<String> {
        self.candidate
            .as_ref()
            .and_then(|candidate| self.row(candidate))
            .and_then(|row| row.suites.first())
            .or_else(|| self.versions.iter().find_map(|row| row.suites.first()))
            .cloned()
    }

    /// The version apt would install now, when that is something other than the
    /// version already installed and a source can actually fetch it.
    fn deliverable(&self) -> Option<String> {
        let candidate = self.candidate.as_ref()?;
        if self.installed.as_ref() == Some(candidate) {
            // apt has compared the two with its own comparator and chosen the
            // installed one. There is nothing to deliver, whatever any feed
            // says about builds published elsewhere.
            return None;
        }
        // A candidate no source carries cannot be fetched — it is the installed
        // version under another name, or a version held by dpkg alone.
        self.row(candidate)?.suites.first()?;
        Some(build_version_of(candidate))
    }

    fn row(&self, version: &str) -> Option<&VersionRow> {
        self.versions.iter().find(|row| row.version == version)
    }
}

/// The build version behind an apt version string, or the string itself when
/// this packaging did not emit it.
///
/// Naming apt's own string is honest — it is what `apt-get install` would
/// fetch — and it is what the archive still carries for packages published
/// before the #709 encoding existed.
fn build_version_of(apt_version: &str) -> String {
    package_version::build_version_from_debian_version(apt_version)
        .unwrap_or_else(|| apt_version.to_owned())
}

/// A row of the version table: either a version, or one origin of the version
/// above it.
enum TableRow {
    Version(String),
    /// The suite the origin serves, or nothing when it serves none — dpkg's
    /// status file, which delivers nothing.
    Origin(Option<String>),
}

/// Reads one line of the version table.
///
/// Both row kinds are `<number> <text>`-shaped, so indentation is not what
/// tells them apart — apt's own leading `***` marker for the installed version
/// would break a rule built on it. What separates them is which field carries
/// the priority: an origin row leads with it (`500 https://… candidate/main …`,
/// `100 /var/lib/dpkg/status`), a version row follows its version with it
/// (`0.11.0-beta.0.210 500`).
fn parse_table_row(trimmed: &str) -> Option<TableRow> {
    let row = trimmed.strip_prefix("***").unwrap_or(trimmed).trim_start();
    let mut fields = row.split_whitespace();
    let first = fields.next()?;
    if first.parse::<u32>().is_ok() {
        let origin = fields.next()?;
        if origin == DPKG_STATUS_ORIGIN {
            return Some(TableRow::Origin(None));
        }
        // `<uri> <suite>/<component> <architecture> Packages`. The suite is the
        // field after the URI, and apt prints it as `suite/component`.
        let suite = fields
            .next()
            .and_then(|release| release.split('/').next())
            .filter(|suite| !suite.is_empty())
            .map(str::to_owned);
        return Some(TableRow::Origin(suite));
    }
    // A version row is exactly the version and its priority; anything else is a
    // heading ("Version table:") or prose.
    let priority = fields.next()?;
    if priority.parse::<u32>().is_err() || fields.next().is_some() {
        return None;
    }
    Some(TableRow::Version(first.to_owned()))
}

/// A version field apt filled in, or nothing for its `(none)` placeholder.
fn present_version(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value != "(none)").then(|| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `apt-cache policy ok-player` on the machine that reported #725: the
    /// package was installed from a downloaded `.deb` and no OK Player source
    /// exists, so dpkg's status file is the only thing apt knows it from.
    /// Captured verbatim in a clean `debian:13-slim` container that installed
    /// `ok-player_0.11.0-beta.0.208_amd64.deb` by hand.
    const NO_SOURCE: &str = "\
ok-player:
  Installed: 0.11.0-beta.0.208
  Candidate: 0.11.0-beta.0.208
  Version table:
 *** 0.11.0-beta.0.208 100
        100 /var/lib/dpkg/status
";

    /// The same container after following the README's `stable` instructions.
    /// The suite is configured and carries only the published releases, so the
    /// installed candidate build stays the candidate: apt has nothing to
    /// deliver, and the rolling build the feed knows about is not something
    /// this machine subscribes to.
    const STABLE_SUBSCRIBED: &str = "\
ok-player:
  Installed: 0.11.0-beta.0.208
  Candidate: 0.11.0-beta.0.208
  Version table:
 *** 0.11.0-beta.0.208 100
        100 /var/lib/dpkg/status
     0.1.0-linux-alpha.112 500
        500 https://befeast.github.io/ok-player/apt stable/main amd64 Packages
     0.1.0-linux-alpha.111 500
        500 https://befeast.github.io/ok-player/apt stable/main amd64 Packages
";

    /// The same container subscribed to `candidate` instead: the newer build is
    /// in the table, served by a source, and apt has chosen it.
    const CANDIDATE_SUBSCRIBED: &str = "\
ok-player:
  Installed: 0.11.0-beta.0.208
  Candidate: 0.11.0-beta.0.210
  Version table:
     0.11.0-beta.0.210 500
        500 https://befeast.github.io/ok-player/apt candidate/main amd64 Packages
 *** 0.11.0-beta.0.208 500
        500 https://befeast.github.io/ok-player/apt candidate/main amd64 Packages
        100 /var/lib/dpkg/status
     0.11.0-beta.0.197 500
        500 https://befeast.github.io/ok-player/apt candidate/main amd64 Packages
";

    #[test]
    fn a_package_only_dpkg_knows_about_has_no_source() {
        assert_eq!(
            package_source_from_policy(NO_SOURCE),
            PackageSourceEvidence::NoSource
        );
    }

    #[test]
    fn a_subscribed_suite_that_carries_nothing_newer_delivers_nothing() {
        assert_eq!(
            package_source_from_policy(STABLE_SUBSCRIBED),
            PackageSourceEvidence::Source {
                suite: "stable".to_owned(),
                deliverable: None,
            }
        );
    }

    #[test]
    fn a_subscribed_suite_that_carries_a_newer_build_delivers_it() {
        assert_eq!(
            package_source_from_policy(CANDIDATE_SUBSCRIBED),
            PackageSourceEvidence::Source {
                suite: "candidate".to_owned(),
                deliverable: Some("0.11.0-beta.0.210".to_owned()),
            }
        );
    }

    /// Since #709 the archive carries the Debian encoding of the build version.
    /// What the surface names is the build, because that is the version the
    /// rest of the app — About, the feeds, the skip list — speaks.
    #[test]
    fn an_encoded_candidate_is_named_as_the_build_it_holds() {
        let encoded = "\
ok-player:
  Installed: 1:0.11.0~beta.0.208
  Candidate: 1:0.11.0~beta.0.210
  Version table:
     1:0.11.0~beta.0.210 500
        500 https://befeast.github.io/ok-player/apt candidate/main amd64 Packages
 *** 1:0.11.0~beta.0.208 100
        100 /var/lib/dpkg/status
";
        assert_eq!(
            package_source_from_policy(encoded),
            PackageSourceEvidence::Source {
                suite: "candidate".to_owned(),
                deliverable: Some("0.11.0-beta.0.210".to_owned()),
            }
        );
    }

    /// A machine carrying both stanzas is described by the suite that would
    /// actually deliver, not by whichever apt happened to print first.
    #[test]
    fn the_suite_named_is_the_one_the_candidate_comes_from() {
        let both = "\
ok-player:
  Installed: 0.11.0-beta.0.208
  Candidate: 0.11.0-beta.0.210
  Version table:
     0.11.0-beta.0.210 500
        500 https://befeast.github.io/ok-player/apt candidate/main amd64 Packages
     0.1.0-linux-alpha.112 500
        500 https://befeast.github.io/ok-player/apt stable/main amd64 Packages
 *** 0.11.0-beta.0.208 100
        100 /var/lib/dpkg/status
";
        assert_eq!(
            package_source_from_policy(both),
            PackageSourceEvidence::Source {
                suite: "candidate".to_owned(),
                deliverable: Some("0.11.0-beta.0.210".to_owned()),
            }
        );
    }

    /// A package apt has never heard of establishes no source — and no panic.
    #[test]
    fn output_that_names_no_versions_has_no_source() {
        for output in [
            "",
            "N: Unable to locate package ok-player\n",
            "ok-player:\n",
        ] {
            assert_eq!(
                package_source_from_policy(output),
                PackageSourceEvidence::NoSource,
                "{output:?}"
            );
        }
    }

    /// The package is not installed at all (the surface cannot reach this — the
    /// player is running — but the parser must not read `(none)` as a version).
    #[test]
    fn an_uninstalled_package_still_reports_what_a_source_would_deliver() {
        let uninstalled = "\
ok-player:
  Installed: (none)
  Candidate: 0.11.0-beta.0.210
  Version table:
     0.11.0-beta.0.210 500
        500 https://befeast.github.io/ok-player/apt candidate/main amd64 Packages
";
        assert_eq!(
            package_source_from_policy(uninstalled),
            PackageSourceEvidence::Source {
                suite: "candidate".to_owned(),
                deliverable: Some("0.11.0-beta.0.210".to_owned()),
            }
        );
    }
}
