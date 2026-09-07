# Media download and replay-cache API

Issue #785 introduces one native, cancellable full-media download seam. The
cache is its first caller; explicit user-owned Save in #794 is the second. The
downloader does not know whether the replay-cache setting is enabled and never
places user saves in cache ownership.

The portable request and event contract is in
`rust/crates/okp-core/src/media_download.rs`:

```rust
pub struct DownloadJobId(pub u64);

pub enum DownloadPurpose {
    ReplayCache,
    UserSave,
}

pub enum DownloadContainer {
    Preserve,
    Matroska,
}

pub struct DownloadTarget {
    pub staging_directory: PathBuf,
    pub file_stem: String,
}

pub struct MediaDownloadRequest {
    pub source_url: String,
    pub target: DownloadTarget,
    pub purpose: DownloadPurpose,
    pub container: DownloadContainer,
    pub max_bytes: u64,
    pub format_selector: Option<String>,
}

impl MediaDownloadRequest {
    pub fn public_vod(
        source_url: impl Into<String>,
        target: DownloadTarget,
        purpose: DownloadPurpose,
        container: DownloadContainer,
        max_bytes: u64,
    ) -> Result<Self, DownloadRejection>;
}

pub enum MediaDownloadEvent {
    Started { job_id: DownloadJobId },
    Progress(MediaDownloadProgress),
    Terminal {
        job_id: DownloadJobId,
        outcome: MediaDownloadOutcome,
    },
}
```

`MediaDownloadProgress` carries downloaded bytes and an optional total.
`MediaDownloadOutcome::Completed(DownloadedMedia)` carries the actual completed
path, byte length, and lowercase extension. Other terminal outcomes are
`Cancelled`, `Rejected(DownloadRejection)`, and `Failed { message }`. Every
accepted job produces exactly one terminal event.

The Linux implementation is
`rust/crates/okp-linux-gtk/src/media_download.rs`:

```rust
pub struct MediaDownloader { /* one active native job */ }

impl MediaDownloader {
    pub fn request(
        &mut self,
        request: MediaDownloadRequest,
    ) -> Result<DownloadJobId, DownloadBusy>;
    pub fn cancel(&mut self, job_id: DownloadJobId) -> bool;
    pub fn drain_events(&mut self) -> Vec<MediaDownloadEvent>;
    pub fn shutdown(&mut self);
}
```

Requests are asynchronous and never block playback or the GTK thread. The
adapter invokes external `yt-dlp` without loading configuration or browser
cookies, rejects live and playlist downloads, limits staged bytes, and owns one
job at a time with no queue. Cancellation and shutdown terminate the whole
yt-dlp/ffmpeg process group, reap it, and remove only that job's staging
directory. A successful process/mux exit is required before a completed outcome
is emitted.

The portable cache API is in `rust/crates/okp-core/src/replay_cache.rs`:

```rust
pub const DEFAULT_REPLAY_CACHE_CAPACITY_BYTES: u64 = 5 * 1024 * 1024 * 1024;

pub struct ReplayCache;
pub struct ReplayCacheStaging;
pub struct AcquiredReplay {
    pub source_url: String,
    pub path: PathBuf,
    pub extension: String,
    pub byte_len: u64,
    pub pin: ReplayCachePin,
}

impl ReplayCache {
    pub fn open(root: impl Into<PathBuf>, capacity_bytes: u64) -> io::Result<Self>;
    pub fn begin_download(
        &self,
        source_url: &str,
        job_id: DownloadJobId,
    ) -> io::Result<ReplayCacheStaging>;
    pub fn complete_download(
        &self,
        staging: ReplayCacheStaging,
        media: DownloadedMedia,
        now_unix: i64,
    ) -> io::Result<ReplayCacheCommit>;
    pub fn abandon_download(&self, staging: ReplayCacheStaging) -> io::Result<()>;
    pub fn acquire(
        &self,
        source_url: &str,
        now_unix: i64,
    ) -> io::Result<Option<AcquiredReplay>>;
    pub fn invalidate(&self, source_url: &str) -> io::Result<bool>;
    pub fn clear_unpinned(&self) -> io::Result<ReplayCacheClear>;
}
```

`begin_download` returns a `DownloadTarget` plus the 5 GiB admission limit for
the adapter. `complete_download` is the only cache promotion path: after the
native adapter reports success, it validates the staged result, evicts the
oldest unused entries as needed, atomically renames the real container suffix,
and then persists the manifest. The retained limit is 5 GiB, with one separately
bounded staging job up to 5 GiB (at most 10 GiB transient storage, less when files
are pinned). The adapter monitors all partial and mux bytes while the process
runs and aborts a job that exceeds its admission budget. Missing, changed, or unreadable files are cache
misses and the caller falls back to the original URL. Holding
`AcquiredReplay::pin` protects the active file from eviction or clearing.

For Save #794, first ask the native destination picker for the final target.
Then either acquire and copy a completed cache hit while holding its pin, or
submit a `DownloadPurpose::UserSave` request. Use `Preserve` when the chosen
destination is based on a metadata probe, or `Matroska` to guarantee `.mkv`.
The Save caller moves the successful staged file to the user destination; it
must not call `ReplayCache::complete_download`, so the result is never
evictable. An existing cached MP4 retains `.mp4`; changing only its filename to
`.mkv` is not a valid save.

## GTK caller integration

`PlayerState.replay_cache` is the shared `ReplayCacheRuntime`. Call
`state.replay_cache.acquire(original_url, format_selector.as_deref())` for an
`Option<AcquiredReplay>` with shared pin accounting. Never open a second store
for Save. Obtain the selector from
`playlist_ops::configured_url_load_options(&state.settings, original_url)`.
Assign its `ytdl_format().map(str::to_owned)` to the public request field
`format_selector` after the existing `public_vod` constructor. The default is
`None`. A changed selector misses the retained URL entry; only the latest
completed selection for that original URL is retained.

Save obtains/pins an available cache file before the destination chooser so it
can suggest the actual container extension. Only a confirmed chooser may start
a new UserSave download. In a private session Save skips cache lookup and
metadata persistence, using its independent downloader and selected file only.
