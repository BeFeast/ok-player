//! The encoding between a build version and the version a package manager
//! compares (#709).
//!
//! OK Player names its builds the way semver does — `0.11.0-beta.0.209` for a
//! candidate, `0.11.0` for the release it leads to. dpkg does not read that tail
//! as a prerelease: everything after the last `-` is the *Debian revision*, and a
//! version that has one sorts **above** the same version without one. Measured
//! with dpkg 1.23.7, `0.11.0-beta.0.208 gt 0.11.0` is true — so every `.deb`
//! published under the build version outranks the release that is supposed to
//! follow it, and the APT lane could never ship a stable version at all.
//!
//! Debian's construct for "comes before the release" is `~`, which sorts below
//! the empty string; but adopting it alone makes the corrected version sort
//! *below* what testers already have installed, and apt refuses it as a
//! downgrade. The one construct that outranks a version already on a machine is
//! an epoch. So the Debian versions this project emits are
//!
//! ```text
//! 1: + the build version with every `-` replaced by `~`
//! ```
//!
//! `scripts/linux-package-version.sh` owns the same rule for the packaging side,
//! and this module owns it for the shells that have to read a version back —
//! chiefly the installed-build watch (#707/#708), which asks `dpkg-query` what is
//! installed and orders the answer against the version this session is running.
//! Because a build version may not contain `~` or `:`, the substitution is a
//! bijection and that comparison needs no second comparator.
//!
//! What is deliberately *not* encoded is the artifact file name: `.deb` files
//! stay `ok-player_0.11.0-beta.0.209_amd64.deb`, named by the build, like the
//! AppImage and the candidate pointer beside them.

/// The epoch every Debian version this project emits carries.
///
/// Permanent: dropping it later would strand every machine that had accepted a
/// version carrying it, which is exactly the failure it exists to end.
pub const DEBIAN_VERSION_EPOCH: u32 = 1;

/// Whether `version` is a build version this encoding can carry reversibly.
///
/// `~` and `:` are the two characters the encoding uses to mean something, so a
/// build version holding either would make the mapping ambiguous — a version
/// could be read back as one it was never made from.
pub fn is_encodable_build_version(version: &str) -> bool {
    let mut characters = version.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    if !first.is_ascii_digit() {
        return false;
    }
    if version.ends_with('-') {
        return false;
    }
    version
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '+' | '-'))
}

/// The `Version:` field a `.deb` built from `build_version` carries, or nothing
/// when the build version cannot be encoded reversibly.
pub fn debian_version_for_build(build_version: &str) -> Option<String> {
    is_encodable_build_version(build_version)
        .then(|| format!("{DEBIAN_VERSION_EPOCH}:{}", build_version.replace('-', "~")))
}

/// The build version a `.deb` reporting `debian_version` was made from, or
/// nothing when that cannot be established.
///
/// Refused rather than guessed when the string is not one this packaging emits:
///
/// * **no epoch** — a package from before the encoding, or from somebody else's
///   archive. Nothing here can say which build it holds.
/// * **a Debian revision** — this packaging never emits one, so the `-` is a
///   rebuilder's. It is also precisely the shape whose orderings disagree:
///   `1.0.0` → `1.0.0-1` is an upgrade to dpkg and a downgrade to the
///   semver-shaped ordering, and answering it at all would let the two
///   comparators contradict each other. Refusing keeps one of them in charge.
///
/// A refusal is silent to the user: the watch that asks this question re-asks a
/// bounded number of times and then settles, so an unrecognised version means
/// "say nothing", never "announce something that might be wrong".
pub fn build_version_from_debian_version(debian_version: &str) -> Option<String> {
    let epoch_prefix = format!("{DEBIAN_VERSION_EPOCH}:");
    let upstream = debian_version.strip_prefix(&epoch_prefix)?;
    if upstream.contains('-') || upstream.contains(':') {
        return None;
    }
    let build_version = upstream.replace('~', "-");
    is_encodable_build_version(&build_version).then_some(build_version)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shapes the Linux lanes actually publish, both ways. These are the
    /// pairs `scripts/tests/linux-package-version.Tests.sh` drives the shell
    /// implementation over, so the two cannot drift.
    const ENCODINGS: &[(&str, &str)] = &[
        ("0.11.0-beta.0.209", "1:0.11.0~beta.0.209"),
        ("0.11.0-beta.0.9", "1:0.11.0~beta.0.9"),
        ("0.11.0-beta.0.10", "1:0.11.0~beta.0.10"),
        ("0.11.0-beta.1", "1:0.11.0~beta.1"),
        ("0.11.0-alpha.109", "1:0.11.0~alpha.109"),
        ("0.11.0", "1:0.11.0"),
        ("1.0.0", "1:1.0.0"),
        // More than one `-`, so the encoded string still carries no Debian
        // revision anywhere: the legacy Linux release naming.
        ("0.1.0-linux-alpha.112", "1:0.1.0~linux~alpha.112"),
    ];

    #[test]
    fn the_encoding_round_trips_every_shape_this_project_publishes() {
        for (build, debian) in ENCODINGS {
            assert_eq!(
                debian_version_for_build(build).as_deref(),
                Some(*debian),
                "{build} must be packaged as {debian}"
            );
            assert_eq!(
                build_version_from_debian_version(debian).as_deref(),
                Some(*build),
                "{debian} must be read back as the build it was made from"
            );
        }
    }

    #[test]
    fn an_encoded_version_never_carries_a_debian_revision() {
        for (build, _) in ENCODINGS {
            let debian = debian_version_for_build(build).expect("this shape is encodable");
            let upstream = debian.split_once(':').expect("the epoch is stamped").1;
            assert!(
                !upstream.contains('-'),
                "{debian} carries a Debian revision, which would sort above its own release"
            );
        }
    }

    /// A build version that could be read back as a different one is refused at
    /// package time, which is the only place it can still be fixed.
    #[test]
    fn a_build_version_that_would_not_round_trip_is_refused() {
        for rejected in [
            "",
            "0.11.0~beta.1",
            "1:0.11.0",
            "v0.11.0",
            "0.11.0-",
            "0.11.0 beta",
            "0.11.0_beta.1",
        ] {
            assert_eq!(
                debian_version_for_build(rejected),
                None,
                "{rejected:?} must not be packaged"
            );
        }
    }

    /// The #708 mismatch, closed. A version carrying a real Debian revision is
    /// the one shape where dpkg's ordering and the semver-shaped ordering
    /// disagree, so it is never turned into a claim about which build is
    /// installed.
    #[test]
    fn a_version_this_packaging_never_emits_is_not_read_back() {
        for rejected in [
            "0.11.0-beta.0.208", // published before the encoding existed
            "0.11.0~beta.0.209", // the encoding without its epoch
            "1.0.0-1",           // somebody's rebuild
            "1:1.0.0-1",         // ours, rebuilt by somebody
            "2:0.11.0~beta.1",   // a second epoch this project never stamps
            "1:",
            "0.11.0",
        ] {
            assert_eq!(
                build_version_from_debian_version(rejected),
                None,
                "{rejected:?} does not establish which build is installed"
            );
        }
    }
}
