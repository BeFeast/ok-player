//! Complete, bounded replay files keyed by the original page URL.
//!
//! Only a successful native download can be promoted. Pins protect files used by
//! playback or export; filenames and partial directories remain cache-owned.
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::media_download::{DownloadJobId, DownloadTarget, DownloadedMedia};

pub const DEFAULT_REPLAY_CACHE_CAPACITY_BYTES: u64 = 5 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Entry {
    file: String,
    extension: String,
    byte_len: u64,
    modified_ns: u64,
    last_used: i64,
    #[serde(default)]
    format_selector: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Index {
    version: u32,
    entries: BTreeMap<String, Entry>,
}

impl Default for Index {
    fn default() -> Self {
        Self {
            version: 1,
            entries: BTreeMap::new(),
        }
    }
}

struct Inner {
    root: PathBuf,
    capacity: u64,
    index: Index,
    pins: BTreeMap<String, usize>,
}

#[derive(Clone)]
pub struct ReplayCache {
    inner: Arc<Mutex<Inner>>,
}

pub struct ReplayCachePin {
    inner: Arc<Mutex<Inner>>,
    file: String,
}

impl Drop for ReplayCachePin {
    fn drop(&mut self) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(count) = inner.pins.get_mut(&self.file) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                inner.pins.remove(&self.file);
            }
        }
    }
}

pub struct AcquiredReplay {
    pub source_url: String,
    pub path: PathBuf,
    pub extension: String,
    pub byte_len: u64,
    pub pin: ReplayCachePin,
}

pub struct ReplayCacheStaging {
    root: PathBuf,
    directory: PathBuf,
    source_url: String,
    format_selector: Option<String>,
    max_bytes: u64,
}

impl ReplayCacheStaging {
    pub fn download_target(&self) -> DownloadTarget {
        DownloadTarget {
            staging_directory: self.directory.clone(),
            file_stem: "media".into(),
        }
    }
    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }
}

impl Drop for ReplayCacheStaging {
    fn drop(&mut self) {
        // This path is created exclusively by begin_download; it is never a
        // user-supplied media/save directory.
        if self.directory.parent() == Some(self.root.join("staging").as_path()) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }
}

pub struct ReplayCacheCommit {
    pub path: PathBuf,
    pub evicted: usize,
}

pub struct ReplayCacheClear {
    pub removed: usize,
    pub retained_pinned: usize,
}

impl ReplayCache {
    pub fn open(root: impl Into<PathBuf>, capacity_bytes: u64) -> io::Result<Self> {
        if capacity_bytes == 0 {
            return Err(invalid("cache capacity is zero"));
        }
        let root = root.into();
        fs::create_dir_all(&root)?;
        let root = fs::canonicalize(root)?;
        fs::create_dir_all(root.join("files"))?;
        fs::create_dir_all(root.join("staging"))?;
        let index = match fs::read(root.join("index.json")) {
            Ok(bytes) => serde_json::from_slice::<Index>(&bytes)
                .ok()
                .filter(|i| i.version == 1)
                .unwrap_or_default(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Index::default(),
            Err(e) => return Err(e),
        };
        let mut inner = Inner {
            root,
            capacity: capacity_bytes,
            index,
            pins: BTreeMap::new(),
        };
        inner
            .index
            .entries
            .retain(|_, entry| valid_name(&entry.file));
        Ok(Self {
            inner: Arc::new(Mutex::new(inner)),
        })
    }

    pub fn begin_download(
        &self,
        source_url: &str,
        job_id: DownloadJobId,
    ) -> io::Result<ReplayCacheStaging> {
        self.begin_download_for_format(source_url, job_id, None)
    }

    pub fn begin_download_for_format(
        &self,
        source_url: &str,
        job_id: DownloadJobId,
        format_selector: Option<&str>,
    ) -> io::Result<ReplayCacheStaging> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        // Retain completed replays while downloading. One staging job is bounded
        // separately; promotion reclaims oldest unused files before committing.
        let pinned = inner
            .index
            .entries
            .values()
            .filter(|e| inner.pins.contains_key(&e.file))
            .map(|e| e.byte_len)
            .sum::<u64>();
        let staged = owned_bytes(&inner.root.join("staging"))?;
        let available = inner.capacity.saturating_sub(pinned).saturating_sub(staged);
        if available == 0 {
            return Err(io::Error::other(
                "cache is full with active or staged files",
            ));
        }
        let key = Sha256::digest(source_url.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let directory = inner.root.join("staging").join(format!(
            "job-{}-{}-{}",
            std::process::id(),
            job_id.0,
            &key[..16]
        ));
        fs::create_dir(&directory)?;
        Ok(ReplayCacheStaging {
            root: inner.root.clone(),
            directory,
            source_url: source_url.into(),
            format_selector: format_selector.map(str::to_owned),
            max_bytes: available,
        })
    }

    pub fn complete_download(
        &self,
        staging: ReplayCacheStaging,
        media: DownloadedMedia,
        now_unix: i64,
    ) -> io::Result<ReplayCacheCommit> {
        media.validate().map_err(|e| invalid(&e.to_string()))?;
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if staging.root != inner.root {
            return Err(invalid("staging belongs to another cache"));
        }
        let metadata = fs::symlink_metadata(&media.path)?;
        if !metadata.file_type().is_file()
            || metadata.len() != media.byte_len
            || metadata.len() > staging.max_bytes
        {
            return Err(invalid("completed media size is invalid"));
        }
        let source = fs::canonicalize(&media.path)?;
        if source.parent() != Some(staging.directory.as_path()) {
            return Err(invalid("completed media is outside staging"));
        }
        let mut evicted = 0;
        let total = owned_bytes(&inner.root.join("files"))?
            .saturating_add(owned_bytes(&inner.root.join("staging"))?);
        let mut required = total.saturating_sub(inner.capacity);
        for url in eviction_order(&inner) {
            if required == 0 {
                break;
            }
            let bytes = inner.index.entries.get(&url).map_or(0, |e| e.byte_len);
            remove_entry(&mut inner, &url)?;
            required = required.saturating_sub(bytes);
            evicted += 1;
        }
        if required != 0 {
            return Err(io::Error::other("cache capacity is exhausted"));
        }
        if let Some(old) = inner.index.entries.get(&staging.source_url) {
            if inner.pins.contains_key(&old.file) {
                return Err(io::Error::other("existing replay is in use"));
            }
        }
        let name = format!(
            "{}.{}",
            staging.directory.file_name().unwrap().to_string_lossy(),
            media.extension
        );
        let destination = inner.root.join("files").join(&name);
        if destination.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "cache output exists",
            ));
        }
        fs::rename(&source, &destination)?;
        let entry = Entry {
            file: name,
            extension: media.extension,
            byte_len: media.byte_len,
            modified_ns: modified_ns(&fs::metadata(&destination)?),
            last_used: now_unix,
            format_selector: staging.format_selector.clone(),
        };
        let old = inner
            .index
            .entries
            .insert(staging.source_url.clone(), entry);
        if let Err(error) = persist(&inner) {
            inner.index.entries.remove(&staging.source_url);
            if let Some(old) = old {
                inner.index.entries.insert(staging.source_url.clone(), old);
            }
            let _ = fs::remove_file(&destination);
            return Err(error);
        }
        if let Some(old) = old {
            let _ = fs::remove_file(inner.root.join("files").join(old.file));
        }
        Ok(ReplayCacheCommit {
            path: destination,
            evicted,
        })
    }

    pub fn abandon_download(&self, staging: ReplayCacheStaging) -> io::Result<()> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if staging.root != inner.root {
            return Err(invalid("staging belongs to another cache"));
        }
        drop(staging);
        Ok(())
    }

    pub fn acquire(&self, source_url: &str, now_unix: i64) -> io::Result<Option<AcquiredReplay>> {
        self.acquire_for_format(source_url, None, now_unix)
    }

    pub fn acquire_for_format(
        &self,
        source_url: &str,
        format_selector: Option<&str>,
        now_unix: i64,
    ) -> io::Result<Option<AcquiredReplay>> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(entry) = inner.index.entries.get(source_url).cloned() else {
            return Ok(None);
        };
        if entry.format_selector.as_deref() != format_selector {
            return Ok(None);
        }
        let path = inner.root.join("files").join(&entry.file);
        let valid = fs::symlink_metadata(&path).is_ok_and(|m| {
            m.file_type().is_file()
                && m.len() == entry.byte_len
                && modified_ns(&m) == entry.modified_ns
        });
        if !valid {
            if !inner.pins.contains_key(&entry.file) {
                remove_entry(&mut inner, source_url)?;
                persist(&inner)?;
            }
            return Ok(None);
        }
        inner.index.entries.get_mut(source_url).unwrap().last_used = now_unix;
        persist(&inner)?;
        *inner.pins.entry(entry.file.clone()).or_default() += 1;
        Ok(Some(AcquiredReplay {
            source_url: source_url.into(),
            path,
            extension: entry.extension,
            byte_len: entry.byte_len,
            pin: ReplayCachePin {
                inner: Arc::clone(&self.inner),
                file: entry.file,
            },
        }))
    }

    pub fn invalidate(&self, source_url: &str) -> io::Result<bool> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner
            .index
            .entries
            .get(source_url)
            .is_some_and(|e| inner.pins.contains_key(&e.file))
        {
            return Ok(false);
        }
        let changed = remove_entry(&mut inner, source_url)?;
        if changed {
            persist(&inner)?;
        }
        Ok(changed)
    }

    pub fn clear_unpinned(&self) -> io::Result<ReplayCacheClear> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let candidates = eviction_order(&inner);
        let removed = candidates.len();
        for url in candidates {
            remove_entry(&mut inner, &url)?;
        }
        persist(&inner)?;
        Ok(ReplayCacheClear {
            removed,
            retained_pinned: inner.index.entries.len(),
        })
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && Path::new(name).file_name().and_then(|s| s.to_str()) == Some(name)
        && name != "."
        && name != ".."
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn modified_ns(metadata: &fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos().min(u64::MAX as u128) as u64)
}
fn owned_bytes(root: &Path) -> io::Result<u64> {
    let mut sum: u64 = 0;
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_dir() {
            sum = sum.saturating_add(owned_bytes(&entry.path())?);
        } else if metadata.file_type().is_file() {
            sum = sum.saturating_add(metadata.len());
        }
    }
    Ok(sum)
}
fn eviction_order(inner: &Inner) -> Vec<String> {
    let mut entries = inner
        .index
        .entries
        .iter()
        .filter(|(_, e)| !inner.pins.contains_key(&e.file))
        .map(|(url, e)| (e.last_used, url.clone()))
        .collect::<Vec<_>>();
    entries.sort();
    entries.into_iter().map(|(_, url)| url).collect()
}
fn remove_entry(inner: &mut Inner, url: &str) -> io::Result<bool> {
    let Some(entry) = inner.index.entries.get(url) else {
        return Ok(false);
    };
    match fs::remove_file(inner.root.join("files").join(&entry.file)) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    inner.index.entries.remove(url);
    Ok(true)
}
fn persist(inner: &Inner) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(&inner.index)?;
    let temporary = inner.root.join(format!("index-{}.tmp", std::process::id()));
    fs::write(&temporary, bytes)?;
    if let Err(error) = fs::rename(&temporary, inner.root.join("index.json")) {
        let _ = fs::remove_file(temporary);
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn complete(cache: &ReplayCache, url: &str, id: u64, format: Option<&str>) -> PathBuf {
        let staging = cache
            .begin_download_for_format(url, DownloadJobId(id), format)
            .unwrap();
        let path = staging
            .download_target()
            .staging_directory
            .join("media.mp4");
        fs::write(&path, [1, 2, 3, 4]).unwrap();
        cache
            .complete_download(staging, DownloadedMedia::new(path, 4).unwrap(), id as i64)
            .unwrap()
            .path
    }
    #[test]
    fn complete_replay_preserves_original_url_and_format_without_extractor() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ReplayCache::open(dir.path(), 20).unwrap();
        let url = "https://example.com/video";
        let path = complete(&cache, url, 1, Some("best[height<=1080]"));
        let replay = cache
            .acquire_for_format(url, Some("best[height<=1080]"), 2)
            .unwrap()
            .unwrap();
        assert_eq!(replay.source_url, url);
        assert_eq!(replay.path, path);
        assert!(
            cache
                .acquire_for_format(url, Some("worst"), 3)
                .unwrap()
                .is_none()
        );
        drop(replay);
        let reopened = ReplayCache::open(dir.path(), 20).unwrap();
        assert_eq!(
            reopened
                .acquire_for_format(url, Some("best[height<=1080]"), 4)
                .unwrap()
                .unwrap()
                .byte_len,
            4
        );
    }
    #[test]
    fn pinned_media_survives_eviction_and_clear_until_export_finishes() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ReplayCache::open(dir.path(), 8).unwrap();
        let a = complete(&cache, "https://example.com/a", 1, None);
        let pin = cache.acquire("https://example.com/a", 2).unwrap().unwrap();
        let b = complete(&cache, "https://example.com/b", 3, None);
        let result = cache.clear_unpinned().unwrap();
        assert_eq!(result.removed, 1);
        assert_eq!(result.retained_pinned, 1);
        assert!(a.exists());
        assert!(!b.exists());
        drop(pin);
        assert_eq!(cache.clear_unpinned().unwrap().removed, 1);
        assert!(!a.exists());
    }
    #[test]
    fn retains_multiple_replays_and_evicts_oldest_only_when_capacity_requires_it() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ReplayCache::open(dir.path(), 8).unwrap();
        let a = complete(&cache, "https://example.com/a", 1, None);
        let b = complete(&cache, "https://example.com/b", 2, None);
        assert!(a.exists() && b.exists());
        let c = complete(&cache, "https://example.com/c", 3, None);
        assert!(!a.exists());
        assert!(b.exists() && c.exists());
        assert!(cache.acquire("https://example.com/a", 4).unwrap().is_none());
    }

    #[test]
    fn partial_failure_missing_and_changed_file_are_cache_misses() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ReplayCache::open(dir.path(), 20).unwrap();
        let staging = cache
            .begin_download("https://example.com/a", DownloadJobId(1))
            .unwrap();
        let partial = staging
            .download_target()
            .staging_directory
            .join("media.mp4.part");
        fs::write(&partial, b"partial").unwrap();
        assert!(cache.acquire("https://example.com/a", 1).unwrap().is_none());
        cache.abandon_download(staging).unwrap();
        assert!(!partial.exists());
        let path = complete(&cache, "https://example.com/a", 2, None);
        fs::write(&path, b"changed").unwrap();
        assert!(cache.acquire("https://example.com/a", 3).unwrap().is_none());
        let path = complete(&cache, "https://example.com/a", 4, None);
        fs::remove_file(path).unwrap();
        assert!(cache.acquire("https://example.com/a", 5).unwrap().is_none());
    }
    #[test]
    fn invalid_promotion_cleans_only_own_staging_and_preserves_user_file() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ReplayCache::open(dir.path().join("cache"), 20).unwrap();
        let user = dir.path().join("saved.mp4");
        fs::write(&user, b"user").unwrap();
        let staging = cache
            .begin_download("https://example.com/a", DownloadJobId(1))
            .unwrap();
        let owned = staging.download_target().staging_directory;
        assert!(
            cache
                .complete_download(staging, DownloadedMedia::new(&user, 4).unwrap(), 1)
                .is_err()
        );
        assert!(user.exists());
        assert!(!owned.exists());
    }
}
