//! Velopack output identity resolution shared by Linux package lanes.
//!
//! Velopack qualifies artifact names with the selected channel. Packaging must
//! therefore trust the generated feed and package bytes instead of guessing
//! public-channel file names in a shell script.

use std::fmt::Write as FmtWrite;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::ZipArchive;

pub const LINUX_VELOPACK_PACKAGE_ID: &str = "com.befeast.okplayer";

#[derive(Clone, Debug, Deserialize)]
struct VelopackFeed {
    #[serde(rename = "Assets")]
    assets: Vec<VelopackFeedAsset>,
}

#[derive(Clone, Debug, Deserialize)]
struct VelopackFeedAsset {
    #[serde(rename = "PackageId")]
    package_id: String,
    #[serde(rename = "Version")]
    version: String,
    #[serde(rename = "Type")]
    kind: String,
    #[serde(rename = "FileName")]
    file_name: String,
    #[serde(rename = "SHA256")]
    sha256: String,
    #[serde(rename = "Size")]
    size: u64,
}

/// The exact generated identities accepted from one Velopack pack operation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct VelopackArtifactIdentity {
    pub channel: String,
    pub feed_file_name: String,
    pub package_id: String,
    pub version: String,
    pub full_package_file_name: String,
    pub full_package_size: u64,
    pub full_package_sha256: String,
    pub appimage_file_name: String,
    pub appimage_size: u64,
    pub appimage_sha256: String,
    pub versioned_appimage_file_name: String,
}

/// Resolve a generated Velopack feed/package pair, prove that the standalone
/// AppImage is byte-identical to the AppImage embedded in the Full nupkg, then
/// atomically stage the user-facing versioned AppImage.
///
/// The destination is removed before inspection so a failed retry cannot leave
/// a stale or zero-byte artifact carrying the requested release name.
pub fn stage_versioned_appimage(
    output_dir: &Path,
    channel: &str,
    package_id: &str,
    version: &str,
    versioned_appimage_file_name: &str,
) -> Result<VelopackArtifactIdentity, String> {
    let versioned_name = safe_file_name(versioned_appimage_file_name)?;
    let destination = output_dir.join(versioned_name);
    match fs::remove_file(&destination) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("{}: {error}", destination.display())),
    }

    let resolved = resolve_pack(output_dir, channel, package_id, version, versioned_name)?;
    atomic_copy_verified(
        &resolved.appimage_path,
        &destination,
        resolved.identity.appimage_size,
        &resolved.identity.appimage_sha256,
    )?;
    Ok(resolved.identity)
}

struct ResolvedPack {
    identity: VelopackArtifactIdentity,
    appimage_path: PathBuf,
}

fn resolve_pack(
    output_dir: &Path,
    channel: &str,
    package_id: &str,
    version: &str,
    versioned_name: &str,
) -> Result<ResolvedPack, String> {
    let channel = safe_channel(channel)?;
    if package_id.trim().is_empty() {
        return Err("Velopack package id is empty".to_owned());
    }
    if version.trim().is_empty() {
        return Err("Velopack package version is empty".to_owned());
    }

    let feed_file_name = format!("releases.{channel}.json");
    let feed_path = output_dir.join(&feed_file_name);
    let feed_text = fs::read_to_string(&feed_path)
        .map_err(|error| format!("{}: {error}", feed_path.display()))?;
    let feed: VelopackFeed = serde_json::from_str(&feed_text)
        .map_err(|error| format!("{}: {error}", feed_path.display()))?;
    let matches = feed
        .assets
        .into_iter()
        .filter(|asset| {
            asset.package_id == package_id
                && asset.version == version
                && asset.kind.eq_ignore_ascii_case("full")
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(format!(
            "{} must contain exactly one Full asset for package {package_id} version {version}; found {}",
            feed_path.display(),
            matches.len()
        ));
    }
    let full = matches.into_iter().next().expect("one matching asset");
    let full_name = safe_file_name(&full.file_name)?;
    let full_path = output_dir.join(full_name);
    let (full_sha256, full_size) = hash_file(&full_path)?;
    if full_size == 0 {
        return Err(format!("{} is empty", full_path.display()));
    }
    if full_size != full.size {
        return Err(format!(
            "{} size is {full_size}, feed declares {}",
            full_path.display(),
            full.size
        ));
    }
    if !full_sha256.eq_ignore_ascii_case(&full.sha256) {
        return Err(format!(
            "{} SHA256 is {full_sha256}, feed declares {}",
            full_path.display(),
            full.sha256
        ));
    }

    let (embedded_sha256, embedded_size) = embedded_appimage_identity(&full_path)?;
    if embedded_size == 0 {
        return Err(format!(
            "{} contains an empty AppImage",
            full_path.display()
        ));
    }
    let mut matching_appimages = Vec::new();
    for entry in
        fs::read_dir(output_dir).map_err(|error| format!("{}: {error}", output_dir.display()))?
    {
        let entry = entry.map_err(|error| format!("{}: {error}", output_dir.display()))?;
        let path = entry.path();
        if path.file_name().and_then(|name| name.to_str()) == Some(versioned_name)
            || path.extension().and_then(|extension| extension.to_str()) != Some("AppImage")
            || !path.is_file()
        {
            continue;
        }
        let (sha256, size) = hash_file(&path)?;
        if size == embedded_size && sha256.eq_ignore_ascii_case(&embedded_sha256) {
            matching_appimages.push(path);
        }
    }
    if matching_appimages.len() != 1 {
        return Err(format!(
            "{} must contain exactly one standalone AppImage matching the Full package; found {}",
            output_dir.display(),
            matching_appimages.len()
        ));
    }
    let appimage_path = matching_appimages.pop().expect("one matching AppImage");
    let appimage_file_name = appimage_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("{} has no UTF-8 file name", appimage_path.display()))?
        .to_owned();

    Ok(ResolvedPack {
        identity: VelopackArtifactIdentity {
            channel: channel.to_owned(),
            feed_file_name,
            package_id: package_id.to_owned(),
            version: version.to_owned(),
            full_package_file_name: full.file_name,
            full_package_size: full_size,
            full_package_sha256: full_sha256,
            appimage_file_name,
            appimage_size: embedded_size,
            appimage_sha256: embedded_sha256,
            versioned_appimage_file_name: versioned_name.to_owned(),
        },
        appimage_path,
    })
}

fn embedded_appimage_identity(full_package: &Path) -> Result<(String, u64), String> {
    let file =
        File::open(full_package).map_err(|error| format!("{}: {error}", full_package.display()))?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| format!("{}: {error}", full_package.display()))?;
    let mut matches = Vec::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| format!("{}: {error}", full_package.display()))?;
        let name = entry.name().to_owned();
        if name.starts_with("lib/app/") && name.ends_with(".AppImage") && !entry.is_dir() {
            matches.push(index);
        }
    }
    if matches.len() != 1 {
        return Err(format!(
            "{} must contain exactly one lib/app/*.AppImage entry; found {}",
            full_package.display(),
            matches.len()
        ));
    }
    let mut appimage = archive
        .by_index(matches[0])
        .map_err(|error| format!("{}: {error}", full_package.display()))?;
    hash_reader(&mut appimage).map_err(|error| format!("{}: {error}", full_package.display()))
}

fn atomic_copy_verified(
    source: &Path,
    destination: &Path,
    expected_size: u64,
    expected_sha256: &str,
) -> Result<(), String> {
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("{} has no UTF-8 file name", destination.display()))?;
    let mut temporary = None;
    for attempt in 0..100_u32 {
        let candidate = destination
            .with_file_name(format!(".{file_name}.tmp-{}-{attempt}", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                temporary = Some((candidate, file));
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("{}: {error}", candidate.display())),
        }
    }
    let (temporary_path, mut temporary_file) = temporary.ok_or_else(|| {
        format!(
            "could not allocate a temporary file beside {}",
            destination.display()
        )
    })?;
    let result = (|| {
        let mut source_file =
            File::open(source).map_err(|error| format!("{}: {error}", source.display()))?;
        io::copy(&mut source_file, &mut temporary_file)
            .map_err(|error| format!("{}: {error}", temporary_path.display()))?;
        temporary_file
            .flush()
            .map_err(|error| format!("{}: {error}", temporary_path.display()))?;
        temporary_file
            .sync_all()
            .map_err(|error| format!("{}: {error}", temporary_path.display()))?;
        fs::set_permissions(
            &temporary_path,
            fs::metadata(source)
                .map_err(|error| format!("{}: {error}", source.display()))?
                .permissions(),
        )
        .map_err(|error| format!("{}: {error}", temporary_path.display()))?;
        let (sha256, size) = hash_file(&temporary_path)?;
        if size != expected_size || !sha256.eq_ignore_ascii_case(expected_sha256) {
            return Err(format!(
                "{} does not match source AppImage identity",
                temporary_path.display()
            ));
        }
        fs::rename(&temporary_path, destination)
            .map_err(|error| format!("{}: {error}", destination.display()))?;
        let (sha256, size) = hash_file(destination)?;
        if size != expected_size || !sha256.eq_ignore_ascii_case(expected_sha256) {
            return Err(format!(
                "{} does not match source AppImage identity after staging",
                destination.display()
            ));
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
        let _ = fs::remove_file(destination);
    }
    result
}

fn hash_file(path: &Path) -> Result<(String, u64), String> {
    let mut file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    hash_reader(&mut file).map_err(|error| format!("{}: {error}", path.display()))
}

fn hash_reader(reader: &mut impl Read) -> io::Result<(String, u64)> {
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size += read as u64;
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut hex, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok((hex, size))
}

fn safe_channel(channel: &str) -> Result<&str, String> {
    if channel.is_empty()
        || !channel
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(format!("invalid Velopack channel {channel:?}"));
    }
    Ok(channel)
}

fn safe_file_name(file_name: &str) -> Result<&str, String> {
    let path = Path::new(file_name);
    if file_name.is_empty()
        || path.file_name().and_then(|name| name.to_str()) != Some(file_name)
        || path.components().count() != 1
    {
        return Err(format!("invalid Velopack file name {file_name:?}"));
    }
    Ok(file_name)
}
