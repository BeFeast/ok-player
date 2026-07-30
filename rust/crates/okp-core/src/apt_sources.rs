//! What the machine's apt *configuration* says, as opposed to what apt has fetched.
//!
//! [`crate::apt_policy`] reads `apt-cache policy`, which answers "exclusively on the data
//! acquired by an update" — apt's own words. That is the right observable for "what can this
//! machine install", and the wrong one for "does this machine have a source at all", because
//! the two answers differ for a window that every `.deb` install passes through.
//!
//! Since #726 the package writes `/etc/apt/sources.list.d/ok-player.sources` in its `postinst`
//! and stops there; running `apt update` is not a maintainer script's business. So on first
//! launch the stanza is on disk and `apt-cache policy` still shows nothing but
//! `/var/lib/dpkg/status`. Reading that as "no repository is configured" made the app
//! contradict its own packaging out loud — and, worse, offer a user setup commands for a suite
//! that might not be theirs, which would silently move a candidate tester onto `stable`.
//!
//! This module is the second observable that separates the two: the configuration itself. The
//! shell reads the files, this decides what they mean, and nothing here touches the filesystem.
//!
//! **The rules here are deliberately the same three exclusions `postinst` applies**, and are
//! deliberately a duplicate of them for now: a commented-out entry, one turned off with
//! `Enabled: no`, and a source-only entry all build no Packages index, so none of them is a
//! subscription. Unifying the two — including the cases this still gets wrong, such as an entry
//! naming a foreign architecture or an unpublished component — is issue #754. What this needs
//! to be right about is the case #726 creates, which is the stanza the package itself writes.

/// The archive the packaging provisions and the README documents.
pub const ARCHIVE_URL: &str = "https://befeast.github.io/ok-player/apt";

/// A source this machine has configured for the OK Player archive.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfiguredSource {
    /// The suite it subscribes to, when the entry names one this reader understands.
    ///
    /// Optional because a malformed or unusual entry must not be turned into a guess: a suite
    /// invented here would be shown to the user as theirs.
    pub suite: Option<String>,
}

/// The OK Player source this machine has configured, if any, from the contents of its apt
/// source files.
///
/// Every file is offered; the first entry that could actually fetch packages from the archive
/// decides. Order is the caller's, and the canonical `ok-player.sources` should come first so
/// that the package's own stanza is what describes a machine carrying several.
pub fn configured_source<'a>(files: impl IntoIterator<Item = &'a str>) -> Option<ConfiguredSource> {
    files
        .into_iter()
        .find_map(|contents| deb822_source(contents).or_else(|| one_line_source(contents)))
}

/// The suite a stanza subscribes to — used for the stanza the package carries at
/// `/usr/share/ok-player/apt/ok-player.sources`, which is how a build says which channel it
/// came from without anything having to guess.
pub fn stanza_suite(stanza: &str) -> Option<String> {
    field(stanza, "suites").and_then(|value| first_word(&value))
}

/// A deb822 file: stanzas separated by blank lines, fields possibly continued on indented
/// lines. An entry counts only if it names the archive, is not disabled, and carries binary
/// packages.
fn deb822_source(contents: &str) -> Option<ConfiguredSource> {
    contents.split("\n\n").find_map(|stanza| {
        if !stanza.contains(ARCHIVE_URL) {
            return None;
        }
        let uris = field(stanza, "uris")?;
        if !uris.split_whitespace().any(|uri| uri == ARCHIVE_URL) {
            return None;
        }
        let enabled = field(stanza, "enabled")
            .map(|value| {
                !matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "no" | "false" | "0"
                )
            })
            .unwrap_or(true);
        if !enabled {
            return None;
        }
        let types = field(stanza, "types")?;
        if !types.split_whitespace().any(|kind| kind == "deb") {
            return None;
        }
        Some(ConfiguredSource {
            suite: field(stanza, "suites").and_then(|value| first_word(&value)),
        })
    })
}

/// A one-line file: `deb [options] <uri> <suite> <components…>`. `deb-src` fetches a Sources
/// index and never a Packages one, so it is not a subscription that can deliver.
fn one_line_source(contents: &str) -> Option<ConfiguredSource> {
    contents.lines().find_map(|line| {
        let line = line.trim();
        if line.starts_with('#') {
            return None;
        }
        let mut fields = line.split_whitespace();
        if fields.next()? != "deb" {
            return None;
        }
        // Options are a bracketed group; they may contain whitespace, so they are skipped as a
        // run rather than as one token.
        let mut fields = fields.skip_while(|token| token.starts_with('['));
        let mut token = fields.next()?;
        while token.ends_with(']') && !token.starts_with('[') {
            token = fields.next()?;
        }
        if token != ARCHIVE_URL {
            return None;
        }
        Some(ConfiguredSource {
            suite: fields.next().map(str::to_owned),
        })
    })
}

/// One deb822 field's value, folding the continuation lines that apt accepts. Field names are
/// case-insensitive; `name` must be given lowercase.
fn field(stanza: &str, name: &str) -> Option<String> {
    let mut lines = stanza.lines();
    let mut value = loop {
        let line = lines.next()?;
        if line.starts_with('#') {
            continue;
        }
        if let Some((key, rest)) = line.split_once(':')
            && key.trim().eq_ignore_ascii_case(name)
        {
            break rest.trim().to_owned();
        }
    };
    // A continuation is an indented line; it belongs to the field above it.
    for line in lines {
        if !line.starts_with(' ') && !line.starts_with('\t') {
            break;
        }
        value.push(' ');
        value.push_str(line.trim());
    }
    Some(value.trim().to_owned())
}

fn first_word(value: &str) -> Option<String> {
    value.split_whitespace().next().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stanza `scripts/package-linux-deb.sh` carries and its `postinst` installs. This is
    /// the case the whole module exists for: on first launch it is on disk and apt has not read
    /// it, and the app must not call that "no repository configured".
    const PACKAGE_STANZA: &str = "\
Types: deb
URIs: https://befeast.github.io/ok-player/apt
Suites: candidate
Components: main
Architectures: amd64
Signed-By: /usr/share/keyrings/ok-player-archive-keyring.gpg
";

    #[test]
    fn the_stanza_the_package_installs_is_a_configured_source() {
        assert_eq!(
            configured_source([PACKAGE_STANZA]),
            Some(ConfiguredSource {
                suite: Some("candidate".to_owned()),
            })
        );
    }

    /// And the same stanza read as the package's own channel, which is where setup instructions
    /// take their suite from rather than from a constant.
    #[test]
    fn the_carried_stanza_names_the_channel_the_build_came_from() {
        assert_eq!(stanza_suite(PACKAGE_STANZA), Some("candidate".to_owned()));
        assert_eq!(stanza_suite("Types: deb\nURIs: x\n"), None);
    }

    #[test]
    fn a_stable_stanza_is_read_as_stable() {
        let stable = PACKAGE_STANZA.replace("candidate", "stable");
        assert_eq!(
            configured_source([stable.as_str()]),
            Some(ConfiguredSource {
                suite: Some("stable".to_owned()),
            })
        );
    }

    /// The three ways an entry can name the archive and still deliver nothing. Each is the same
    /// exclusion `postinst` applies, and each has to agree with it: a machine the packaging
    /// considers unconfigured must not be described here as configured, or the app would report
    /// a subscription that the next reinstall silently replaces.
    #[test]
    fn an_entry_that_cannot_deliver_packages_is_not_a_configured_source() {
        let disabled = format!("{PACKAGE_STANZA}Enabled: no\n");
        let source_only = PACKAGE_STANZA.replace("Types: deb", "Types: deb-src");
        let commented = format!("# deb {ARCHIVE_URL} stable main\n");
        let one_line_source_only = format!("deb-src {ARCHIVE_URL} stable main\n");
        for contents in [
            disabled.as_str(),
            source_only.as_str(),
            commented.as_str(),
            one_line_source_only.as_str(),
        ] {
            assert_eq!(configured_source([contents]), None, "{contents:?}");
        }
    }

    /// deb822 lets a value continue on an indented line, and apt honours it. A reader that
    /// evaluated `Enabled` per line would read this stanza as enabled.
    #[test]
    fn a_field_continued_on_the_next_line_is_still_that_field() {
        let folded = format!("{PACKAGE_STANZA}Enabled:\n no\n");
        assert_eq!(configured_source([folded.as_str()]), None);

        let folded_suite = "\
Types: deb
URIs:
 https://befeast.github.io/ok-player/apt
Suites:
 stable
Components: main
";
        assert_eq!(
            configured_source([folded_suite]),
            Some(ConfiguredSource {
                suite: Some("stable".to_owned()),
            })
        );
    }

    #[test]
    fn a_one_line_entry_is_a_configured_source_and_names_its_suite() {
        let plain = format!("deb {ARCHIVE_URL} stable main\n");
        assert_eq!(
            configured_source([plain.as_str()]),
            Some(ConfiguredSource {
                suite: Some("stable".to_owned()),
            })
        );

        let with_options = format!(
            "deb [signed-by=/usr/share/keyrings/ok-player-archive-keyring.gpg] {ARCHIVE_URL} candidate main\n"
        );
        assert_eq!(
            configured_source([with_options.as_str()]),
            Some(ConfiguredSource {
                suite: Some("candidate".to_owned()),
            })
        );
    }

    /// A different archive whose URL merely starts with this one is not this one. The URI is
    /// compared as a whole field for that reason.
    #[test]
    fn an_archive_under_a_longer_url_is_a_different_archive() {
        let deb822 = PACKAGE_STANZA.replace(ARCHIVE_URL, &format!("{ARCHIVE_URL}-old"));
        let one_line = format!("deb {ARCHIVE_URL}-old stable main\n");
        for contents in [deb822.as_str(), one_line.as_str()] {
            assert_eq!(configured_source([contents]), None, "{contents:?}");
        }
    }

    #[test]
    fn a_machine_with_no_ok_player_entry_has_no_configured_source() {
        let other = "deb https://deb.debian.org/debian trixie main\n";
        assert_eq!(configured_source([other, "", "# nothing here\n"]), None);
    }

    /// A stanza this reader cannot make sense of still counts as configured — it names the
    /// archive and could fetch from it — but its suite is not guessed. Showing a user a suite
    /// invented here would be showing them somebody else's channel as their own.
    #[test]
    fn an_entry_with_no_readable_suite_is_configured_without_one() {
        let no_suite =
            "Types: deb\nURIs: https://befeast.github.io/ok-player/apt\nComponents: main\n";
        assert_eq!(
            configured_source([no_suite]),
            Some(ConfiguredSource { suite: None })
        );
    }
}
