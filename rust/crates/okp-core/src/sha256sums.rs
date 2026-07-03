//! SHA256SUMS manifest parsing and payload verification.
//!
//! Linux releases publish a `SHA256SUMS` asset (GNU coreutils `sha256sum`
//! output) covering the downloadable packages. The updater verifies a
//! downloaded payload against that manifest before handing it to a
//! privileged installer. Parsing is strict and verification fails closed:
//! any malformed manifest line, conflicting duplicate entry, missing entry,
//! or digest mismatch is an error, never a skip.

use std::fmt;
use std::fmt::Write as _;

use sha2::{Digest, Sha256};

/// Why a payload failed SHA256SUMS verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sha256SumsError {
    /// The manifest contained no entries at all.
    ManifestEmpty,
    /// A manifest line was not `<64 hex chars><space><space or *><name>`.
    ManifestLineInvalid { line_number: usize },
    /// The manifest listed the same file twice with different digests.
    ConflictingEntries { file_name: String },
    /// The manifest has no entry for the payload's file name.
    FileNotListed { file_name: String },
    /// The payload's digest differs from the published one.
    DigestMismatch {
        file_name: String,
        expected: String,
        actual: String,
    },
}

impl fmt::Display for Sha256SumsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ManifestEmpty => write!(formatter, "SHA256SUMS manifest is empty"),
            Self::ManifestLineInvalid { line_number } => {
                write!(formatter, "SHA256SUMS line {line_number} is malformed")
            }
            Self::ConflictingEntries { file_name } => write!(
                formatter,
                "SHA256SUMS lists conflicting digests for {file_name}"
            ),
            Self::FileNotListed { file_name } => {
                write!(formatter, "SHA256SUMS has no entry for {file_name}")
            }
            Self::DigestMismatch {
                file_name,
                expected,
                actual,
            } => write!(
                formatter,
                "sha256 mismatch for {file_name}: expected {expected}, got {actual}"
            ),
        }
    }
}

impl std::error::Error for Sha256SumsError {}

/// Parsed `SHA256SUMS` manifest: file names mapped to lowercase hex digests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sha256Sums {
    entries: Vec<(String, String)>,
}

impl Sha256Sums {
    pub fn parse(text: &str) -> Result<Self, Sha256SumsError> {
        let mut entries: Vec<(String, String)> = Vec::new();
        for (index, raw_line) in text.lines().enumerate() {
            let line = raw_line.trim_end_matches('\r');
            if line.trim().is_empty() {
                continue;
            }
            let line_number = index + 1;
            let (file_name, digest) =
                parse_entry(line).ok_or(Sha256SumsError::ManifestLineInvalid { line_number })?;
            match entries.iter().find(|(name, _)| *name == file_name) {
                Some((_, existing)) if *existing != digest => {
                    return Err(Sha256SumsError::ConflictingEntries { file_name });
                }
                Some(_) => {}
                None => entries.push((file_name, digest)),
            }
        }
        if entries.is_empty() {
            return Err(Sha256SumsError::ManifestEmpty);
        }
        Ok(Self { entries })
    }

    /// Published lowercase hex digest for `file_name`, if listed.
    pub fn expected_hex(&self, file_name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(name, _)| name == file_name)
            .map(|(_, digest)| digest.as_str())
    }
}

/// One manifest line: `<64 hex chars><space><space or *><file name>`.
/// The second separator byte is ` ` for text mode and `*` for binary mode.
fn parse_entry(line: &str) -> Option<(String, String)> {
    let (digest, rest) = line.split_at_checked(64)?;
    if !digest
        .chars()
        .all(|character| character.is_ascii_hexdigit())
    {
        return None;
    }
    let mut rest = rest.chars();
    if rest.next()? != ' ' {
        return None;
    }
    if !matches!(rest.next()?, ' ' | '*') {
        return None;
    }
    let file_name = rest.as_str();
    if file_name.is_empty() {
        return None;
    }
    Some((file_name.to_owned(), digest.to_ascii_lowercase()))
}

/// Lowercase hex SHA-256 digest of `payload`.
pub fn sha256_hex(payload: &[u8]) -> String {
    let digest = Sha256::digest(payload);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Verify `payload` named `file_name` against a raw `SHA256SUMS` manifest.
pub fn verify_payload(
    manifest: &str,
    file_name: &str,
    payload: &[u8],
) -> Result<(), Sha256SumsError> {
    let sums = Sha256Sums::parse(manifest)?;
    let expected = sums
        .expected_hex(file_name)
        .ok_or_else(|| Sha256SumsError::FileNotListed {
            file_name: file_name.to_owned(),
        })?;
    let actual = sha256_hex(payload);
    if actual != expected {
        return Err(Sha256SumsError::DigestMismatch {
            file_name: file_name.to_owned(),
            expected: expected.to_owned(),
            actual,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn sha256_hex_matches_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(sha256_hex(b"abc"), ABC_SHA256);
    }

    #[test]
    fn parse_accepts_text_and_binary_modes_and_blank_lines() {
        let manifest = format!(
            "{ABC_SHA256}  ok-player_0.1.0_amd64.deb\r\n\n{ABC_SHA256} *OK-Player-0.1.0-x86_64.AppImage\n"
        );
        let sums = Sha256Sums::parse(&manifest).expect("manifest should parse");

        assert_eq!(
            sums.expected_hex("ok-player_0.1.0_amd64.deb"),
            Some(ABC_SHA256)
        );
        assert_eq!(
            sums.expected_hex("OK-Player-0.1.0-x86_64.AppImage"),
            Some(ABC_SHA256)
        );
        assert_eq!(sums.expected_hex("other.deb"), None);
    }

    #[test]
    fn parse_normalizes_uppercase_digests() {
        let manifest = format!("{}  pkg.deb\n", ABC_SHA256.to_ascii_uppercase());
        let sums = Sha256Sums::parse(&manifest).expect("manifest should parse");

        assert_eq!(sums.expected_hex("pkg.deb"), Some(ABC_SHA256));
    }

    #[test]
    fn parse_rejects_malformed_lines_with_line_number() {
        let truncated_digest = format!("{}  pkg.deb", &ABC_SHA256[..63]);
        let not_hex = format!("{}zz  pkg.deb", &ABC_SHA256[..62]);
        let bad_separator = format!("{ABC_SHA256} -pkg.deb");
        let missing_name = format!("{ABC_SHA256}  ");
        for malformed in [
            truncated_digest,
            not_hex,
            bad_separator,
            missing_name,
            "not a manifest".to_owned(),
        ] {
            let manifest = format!("{ABC_SHA256}  first.deb\n{malformed}\n");
            assert_eq!(
                Sha256Sums::parse(&manifest),
                Err(Sha256SumsError::ManifestLineInvalid { line_number: 2 }),
                "line should be rejected: {malformed:?}"
            );
        }
    }

    #[test]
    fn parse_rejects_empty_manifest() {
        for empty in ["", "\n \n\r\n"] {
            assert_eq!(
                Sha256Sums::parse(empty),
                Err(Sha256SumsError::ManifestEmpty)
            );
        }
    }

    #[test]
    fn parse_rejects_conflicting_duplicates_but_allows_identical_ones() {
        let identical = format!("{ABC_SHA256}  pkg.deb\n{ABC_SHA256}  pkg.deb\n");
        assert!(Sha256Sums::parse(&identical).is_ok());

        let conflicting = format!("{ABC_SHA256}  pkg.deb\n{}  pkg.deb\n", sha256_hex(b""));
        assert_eq!(
            Sha256Sums::parse(&conflicting),
            Err(Sha256SumsError::ConflictingEntries {
                file_name: "pkg.deb".to_owned()
            })
        );
    }

    #[test]
    fn verify_payload_accepts_matching_payload() {
        let manifest = format!("{ABC_SHA256}  pkg.deb\n");

        assert_eq!(verify_payload(&manifest, "pkg.deb", b"abc"), Ok(()));
    }

    #[test]
    fn verify_payload_accepts_uppercase_manifest_digest() {
        let manifest = format!("{}  pkg.deb\n", ABC_SHA256.to_ascii_uppercase());

        assert_eq!(verify_payload(&manifest, "pkg.deb", b"abc"), Ok(()));
    }

    #[test]
    fn verify_payload_rejects_single_flipped_byte() {
        let payload = b"pretend this is a .deb archive".to_vec();
        let manifest = format!("{}  pkg.deb\n", sha256_hex(&payload));
        let mut tampered = payload.clone();
        tampered[payload.len() / 2] ^= 0x01;

        assert_eq!(verify_payload(&manifest, "pkg.deb", &payload), Ok(()));
        assert_eq!(
            verify_payload(&manifest, "pkg.deb", &tampered),
            Err(Sha256SumsError::DigestMismatch {
                file_name: "pkg.deb".to_owned(),
                expected: sha256_hex(&payload),
                actual: sha256_hex(&tampered),
            })
        );
    }

    #[test]
    fn verify_payload_rejects_unlisted_file() {
        let manifest = format!("{ABC_SHA256}  pkg.deb\n");

        assert_eq!(
            verify_payload(&manifest, "other.deb", b"abc"),
            Err(Sha256SumsError::FileNotListed {
                file_name: "other.deb".to_owned()
            })
        );
    }
}
