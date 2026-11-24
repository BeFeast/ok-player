//! The authenticated release protocol shared by the macOS ZIP and Linux AppImage.
use crate::sha256sums::{hash_reader, sha256_hex as digest};
use sequoia_openpgp as pgp;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::Duration;

pub const AUTHORITY: &str = "77D0FCDEB0D594E13E50F43A9337815EB0F78C63";
pub const ORIGIN: &str = "https://updates.befeast.com";
pub const CHECK_INTERVAL: Duration = Duration::from_secs(28 * 60);
pub const FOCUS_THROTTLE: Duration = Duration::from_secs(5 * 60);
const CERT: &[u8] = include_bytes!("../../../packaging/linux/ok-player-archive-keyring.asc");
const MAX_METADATA: u64 = 64 * 1024;
const MAX_PACKAGE: u64 = 512 * 1024 * 1024;

type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Beta,
    Stable,
}

impl Channel {
    pub fn name(self) -> &'static str {
        match self {
            Self::Beta => "beta",
            Self::Stable => "stable",
        }
    }
}

impl std::str::FromStr for Channel {
    type Err = String;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "beta" => Ok(Self::Beta),
            "stable" => Ok(Self::Stable),
            _ => Err("Unknown release channel.".into()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Linux,
    Mac,
}

impl Platform {
    fn prefix(self) -> &'static str {
        match self {
            Self::Linux => "ok-player/linux",
            Self::Mac => "ok-player/macos",
        }
    }

    fn platform(self) -> &'static str {
        match self {
            Self::Linux => "linux-appimage",
            Self::Mac => "macos-arm64",
        }
    }

    pub fn build(self, value: &str) -> Result<(u64, u64)> {
        fn decimal(value: &str) -> Option<u64> {
            let number: u64 = value.parse().ok()?;
            (number.to_string() == value).then_some(number)
        }
        let build = match self {
            Self::Linux => decimal(value).map(|n| (n, 0)),
            Self::Mac => value
                .split_once('.')
                .and_then(|(a, b)| Some((decimal(a)?, decimal(b)?))),
        };
        build
            .filter(|(first, _)| *first != 0)
            .ok_or_else(|| "Invalid build number.".into())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum Build {
    Linux(u64),
    Mac(String),
}

impl fmt::Display for Build {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Linux(number) => write!(formatter, "{number}"),
            Self::Mac(number) => formatter.write_str(number),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub source: String,
    pub build: Build,
    pub version: String,
    pub filename: String,
    pub sha256: String,
    pub size: u64,
}

// A separate wire struct keeps unknown-field rejection on the signed object.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Release {
    pub source: String,
    pub build: Build,
    pub version: String,
    pub filename: String,
    pub sha256: String,
    pub size: u64,
    pub schema: u32,
    pub channel: Channel,
    pub platform: String,
    pub fingerprint: String,
    pub key: String,
    pub provenance_sha256: String,
}

impl Release {
    fn validate(&self, platform: Platform, channel: Channel, fingerprint: &str) -> Result<()> {
        let build = self.build.to_string();
        platform.build(&build)?;
        let (filename, valid_version) = match (&self.build, platform) {
            (Build::Linux(_), Platform::Linux) => (
                format!("OK-Player-{}-x86_64.AppImage", self.version),
                self.version == format!("0.11.0-beta.0.{build}"),
            ),
            (Build::Mac(_), Platform::Mac) => (
                "ok-player-macos-arm64.zip".into(),
                self.version.split('.').count() == 3
                    && self
                        .version
                        .split('.')
                        .all(|n| n.parse::<u64>().is_ok_and(|digit| digit.to_string() == n)),
            ),
            _ => return Err("Update has an incompatible build number.".into()),
        };
        if self.schema != 1
            || self.channel != channel
            || self.platform != platform.platform()
            || self.fingerprint != fingerprint
            || !valid_version
            || self.filename != filename
            || self.key != format!("{}/builds/{build}/{filename}", platform.prefix())
            || !is_hex(&self.source, 40)
            || !is_hex(&self.sha256, 64)
            || !is_hex(&self.provenance_sha256, 64)
            || !(1024..=MAX_PACKAGE).contains(&self.size)
        {
            return Err("Update manifest contains an invalid release identity.".into());
        }
        Ok(())
    }

    fn provenance(&self) -> Provenance {
        Provenance {
            source: self.source.clone(),
            build: self.build.clone(),
            version: self.version.clone(),
            filename: self.filename.clone(),
            sha256: self.sha256.clone(),
            size: self.size,
        }
    }
}

fn is_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

struct Verify(pgp::Cert);

impl pgp::parse::stream::VerificationHelper for Verify {
    fn get_certs(&mut self, _: &[pgp::KeyHandle]) -> pgp::Result<Vec<pgp::Cert>> {
        Ok(vec![self.0.clone()])
    }

    fn check(&mut self, structure: pgp::parse::stream::MessageStructure) -> pgp::Result<()> {
        use pgp::parse::stream::{GoodChecksum, MessageLayer};
        let mut layers = structure.into_iter();
        if let Some(MessageLayer::SignatureGroup { results }) = layers.next()
            && let [Ok(GoodChecksum { sig, .. })] = results.as_slice()
            && sig.typ() == pgp::types::SignatureType::Binary
            && layers.next().is_none()
        {
            return Ok(());
        }
        Err(
            pgp::Error::BadSignature("Expected one binary signature from the release key".into())
                .into(),
        )
    }
}

pub struct Feed {
    agent: ureq::Agent,
    origin: String,
    cert: pgp::Cert,
}

impl Feed {
    pub fn production() -> Result<Self> {
        let feed = Self::new(ORIGIN.into(), CERT)?;
        if feed.fingerprint() != AUTHORITY {
            return Err("The bundled release key has an unexpected fingerprint.".into());
        }
        Ok(feed)
    }

    fn new(origin: String, cert: &[u8]) -> Result<Self> {
        use pgp::parse::Parse;
        let cert = pgp::Cert::from_bytes(cert).map_err(|e| format!("Invalid release key: {e}"))?;
        let agent = ureq::Agent::config_builder()
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(180)))
            .build()
            .into();
        Ok(Self {
            agent,
            origin,
            cert,
        })
    }

    /// Acceptance runs use a disposable loopback feed and trust anchor.
    pub fn fixture(port: u16, key: &[u8]) -> Result<Self> {
        Self::new(format!("http://127.0.0.1:{port}"), key)
    }

    fn fingerprint(&self) -> String {
        self.cert.fingerprint().to_hex()
    }

    fn verify(&self, data: impl Read + Send + Sync, signature: &[u8]) -> Result<()> {
        use pgp::parse::{Parse, stream::DetachedVerifierBuilder};
        let policy = pgp::policy::StandardPolicy::new();
        DetachedVerifierBuilder::from_bytes(signature)
            .and_then(|builder| builder.with_policy(&policy, None, Verify(self.cert.clone())))
            .and_then(|mut verifier| verifier.verify_reader(data))
            .map_err(|e| format!("Update signature could not be verified: {e}"))
    }

    fn fetch(&self, key: &str, limit: u64, mut output: impl Write) -> Result<u64> {
        let mut response = self
            .agent
            .get(format!("{}/{key}", self.origin))
            .header("Cache-Control", "no-cache")
            .header("User-Agent", "OK Player Updater/1")
            .call()
            .map_err(|e| format!("Could not fetch update data: {e}"))?;
        if response.status() != 200 {
            return Err(format!(
                "Update server returned HTTP {}.",
                response.status()
            ));
        }
        let size = io::copy(
            &mut response.body_mut().as_reader().take(limit + 1),
            &mut output,
        )
        .map_err(|e| format!("Could not read update data: {e}"))?;
        if size > limit {
            return Err("Update data exceeds its expected size.".into());
        }
        Ok(size)
    }

    fn metadata(&self, key: &str) -> Result<Vec<u8>> {
        let mut data = Vec::new();
        self.fetch(key, MAX_METADATA, &mut data)?;
        Ok(data)
    }

    pub fn check(
        &self,
        platform: Platform,
        channel: Channel,
        running: &str,
        source: &str,
    ) -> Result<Option<Release>> {
        let running = platform.build(running)?;
        let prefix = format!("{}/{}", platform.prefix(), channel.name());
        let data = self.metadata(&format!("{prefix}/latest.json"))?;
        let signature = self.metadata(&format!("{prefix}/manifests/{}.sig", digest(&data)))?;
        self.verify(data.as_slice(), &signature)?;
        let release: Release =
            serde_json::from_slice(&data).map_err(|e| format!("Invalid update manifest: {e}"))?;
        release.validate(platform, channel, &self.fingerprint())?;
        let available = platform.build(&release.build.to_string())?;
        if available == running && release.source != source {
            return Err(
                "The release has this build number but a different source revision.".into(),
            );
        }
        Ok((available > running).then_some(release))
    }

    /// Download into a caller-owned private staging directory. The returned file
    /// has passed provenance, length, SHA-256, and detached-signature checks.
    pub fn stage(&self, release: &Release, directory: &Path) -> Result<std::path::PathBuf> {
        let (prefix, _) = release.key.rsplit_once('/').ok_or("Invalid update path.")?;
        let provenance = self.metadata(&format!("{prefix}/provenance.json"))?;
        if digest(&provenance) != release.provenance_sha256
            || serde_json::from_slice::<Provenance>(&provenance).ok() != Some(release.provenance())
        {
            return Err("Update provenance does not match the signed manifest.".into());
        }
        let signature = self.metadata(&format!("{}.sig", release.key))?;
        let path = directory.join(&release.filename);
        let mut file = File::create_new(&path).map_err(|e| e.to_string())?;
        if self.fetch(&release.key, release.size, &mut file)? != release.size {
            return Err("Update download has the wrong size.".into());
        }
        file.sync_all().map_err(|e| e.to_string())?;
        if !matches_payload(release, &path)? {
            return Err("Update download has the wrong SHA-256.".into());
        }
        self.verify(File::open(&path).map_err(|e| e.to_string())?, &signature)?;
        Ok(path)
    }
}

pub fn matches_payload(release: &Release, path: &Path) -> Result<bool> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    let (hash, bytes) =
        hash_reader(&mut file.take(release.size + 1)).map_err(|error| error.to_string())?;
    Ok(bytes == release.size && hash == release.sha256)
}
