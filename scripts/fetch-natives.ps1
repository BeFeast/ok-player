#!/usr/bin/env pwsh
# Fetch the native binaries OK Player ships but does not commit (the public repo stays lean):
#   * libmpv-2.dll  -> native/libmpv/   (the playback engine; OkPlayer.Render / OkPlayer.App + integration tests)
#   * ffmpeg.exe     -> native/ffmpeg/    (media processing: subtitle-sync audio clips, future cut/convert/remux)
# Both are GPL builds, matching OK Player's GPL-3.0-or-later licence. Idempotent: each is skipped when present.
#
#   pwsh ./scripts/fetch-natives.ps1
#
# Sources: GPL `mpv-dev-x86_64` from https://github.com/zhongfly/mpv-winbuild and the GPL `win64-gpl` build from
# https://github.com/BtbN/FFmpeg-Builds. See native/README.md and THIRD-PARTY-NOTICES.md.
param(
    # -Dest is the libmpv output folder (kept for backward compatibility with existing callers). ffmpeg is
    # fetched into a sibling `ffmpeg/` folder under the same `native/` root.
    [string]$Dest = (Join-Path $PSScriptRoot '..' 'native' 'libmpv')
)
$ErrorActionPreference = 'Stop'

$headers = @{ 'User-Agent' = 'okplayer-fetch-natives' }
# Authenticate ONLY the api.github.com lookups when a token is available (CI sets GITHUB_TOKEN). The
# unauthenticated API limit (60/hr per IP) is easily exhausted on shared Actions-runner IPs and fails with HTTP
# 403 "rate limit exceeded". Asset downloads are CDN redirects that reject a second auth mechanism, so they keep
# the token-free $headers.
$apiHeaders = $headers.Clone()
if ($env:GITHUB_TOKEN) { $apiHeaders['Authorization'] = "Bearer $env:GITHUB_TOKEN" }

function Get-SevenZip {
    $sz = (Get-Command 7z -ErrorAction SilentlyContinue)?.Source
    if (-not $sz) { $sz = (Get-Command 7za -ErrorAction SilentlyContinue)?.Source }
    if (-not $sz) { $sz = 'C:\Program Files\7-Zip\7z.exe' }
    return $sz
}

# --- libmpv (GPL, x86_64) -> native/libmpv/libmpv-2.dll  (REQUIRED — failure is fatal) ---
$dll = Join-Path $Dest 'libmpv-2.dll'
if (Test-Path $dll) {
    Write-Host "libmpv already present: $dll"
}
else {
    New-Item -ItemType Directory -Force $Dest | Out-Null
    Write-Host 'Resolving a recent mpv-dev (GPL, x86_64) build from zhongfly/mpv-winbuild...'
    # Scan RECENT releases, not just `latest`: zhongfly occasionally ships a release with only LGPL dev builds
    # (e.g. 2026-06-30 had no GPL mpv-dev asset), which would break a `releases/latest`-only lookup. Take the
    # first release that carries a GPL (non-lgpl) x86_64 dev .7z. Prefer the baseline build for max CPU
    # compatibility, falling back to x86-64-v3 (AVX2 — within Win11's 8th-gen Intel / Zen+ hardware floor).
    $releases = Invoke-RestMethod 'https://api.github.com/repos/zhongfly/mpv-winbuild/releases?per_page=15' -Headers $apiHeaders
    $asset = $null
    foreach ($r in $releases) {
        $candidates = $r.assets |
            Where-Object { $_.name -like 'mpv-dev-x86_64-*' -and $_.name -notlike '*lgpl*' -and $_.name -like '*.7z' }
        $pick = ($candidates | Where-Object { $_.name -notlike '*-v3-*' } | Select-Object -First 1)
        if (-not $pick) { $pick = $candidates | Select-Object -First 1 }
        if ($pick) { $asset = $pick; break }
    }
    if (-not $asset) { throw 'No mpv-dev-x86_64 (GPL) .7z asset in the latest 15 releases' }

    $archive = Join-Path $env:TEMP $asset.name
    Write-Host "Downloading $($asset.name) ($([int]($asset.size / 1MB)) MB)..."
    Invoke-WebRequest $asset.browser_download_url -OutFile $archive -Headers $headers
    & (Get-SevenZip) e $archive "-o$Dest" 'libmpv-2.dll' -y | Out-Null
    if (-not (Test-Path $dll)) { throw "Extraction did not produce $dll" }
    Write-Host "Done: $dll"
}

# --- ffmpeg (GPL, win64) -> native/ffmpeg/ffmpeg.exe  (OPTIONAL — failure warns, never blocks the build/tests
#     that only need libmpv; the ffmpeg-backed features just degrade until it's present) ---
$ffDest = Join-Path (Split-Path -Parent $Dest) 'ffmpeg'
$ffexe = Join-Path $ffDest 'ffmpeg.exe'
if (Test-Path $ffexe) {
    Write-Host "ffmpeg already present: $ffexe"
}
else {
    try {
        New-Item -ItemType Directory -Force $ffDest | Out-Null
        # BtbN publishes a rolling `latest` tag with stable asset names — use the direct download URL rather than
        # the releases API (the auto-build release is a prerelease, so `releases/latest` can miss it). Static
        # (non-shared) GPL win64 build = a single self-contained ffmpeg.exe, no extra DLLs to ship.
        $ffUrl = 'https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip'
        $ffarchive = Join-Path $env:TEMP 'ffmpeg-master-latest-win64-gpl.zip'
        Write-Host "Downloading ffmpeg (GPL, win64 static) from BtbN/FFmpeg-Builds..."
        Invoke-WebRequest $ffUrl -OutFile $ffarchive -Headers $headers
        # The zip nests bin/ffmpeg.exe under a versioned top folder; -r finds it, and we extract only ffmpeg.exe
        # (not ffprobe/ffplay) — media inspection already goes through libmpv.
        & (Get-SevenZip) e $ffarchive "-o$ffDest" 'ffmpeg.exe' -r -y | Out-Null
        if (-not (Test-Path $ffexe)) { throw "Extraction did not produce $ffexe" }
        Write-Host "Done: $ffexe"
    }
    catch {
        Write-Warning "ffmpeg fetch failed ($($_.Exception.Message)). The media-processing features (subtitle auto-sync) will be unavailable until you re-run this script. The libmpv-only build/tests are unaffected."
    }
}
