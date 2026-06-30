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
    [string]$NativeRoot = (Join-Path $PSScriptRoot '..' 'native')
)
$ErrorActionPreference = 'Stop'

$headers = @{ 'User-Agent' = 'okplayer-fetch-natives' }
# Authenticate ONLY the api.github.com lookups when a token is available (CI sets GITHUB_TOKEN). The
# unauthenticated API limit (60/hr per IP) is easily exhausted on shared Actions-runner IPs and fails with HTTP
# 403 "rate limit exceeded". The asset downloads below are redirects to a CDN that rejects a second auth
# mechanism, so they must keep the token-free $headers.
$apiHeaders = $headers.Clone()
if ($env:GITHUB_TOKEN) { $apiHeaders['Authorization'] = "Bearer $env:GITHUB_TOKEN" }

# Resolve a 7-Zip once for both extractions: prefer 7z/7za on PATH (GitHub's windows runner has it), then the
# default install dir.
function Get-SevenZip {
    $sz = (Get-Command 7z -ErrorAction SilentlyContinue)?.Source
    if (-not $sz) { $sz = (Get-Command 7za -ErrorAction SilentlyContinue)?.Source }
    if (-not $sz) { $sz = 'C:\Program Files\7-Zip\7z.exe' }
    return $sz
}

# --- libmpv (GPL, x86_64) -> native/libmpv/libmpv-2.dll ---
$mpvDest = Join-Path $NativeRoot 'libmpv'
$dll = Join-Path $mpvDest 'libmpv-2.dll'
if (Test-Path $dll) {
    Write-Host "libmpv already present: $dll"
}
else {
    New-Item -ItemType Directory -Force $mpvDest | Out-Null
    Write-Host 'Resolving latest mpv-dev (GPL, x86_64) from zhongfly/mpv-winbuild...'
    $rel = Invoke-RestMethod 'https://api.github.com/repos/zhongfly/mpv-winbuild/releases/latest' -Headers $apiHeaders
    # GPL (non-lgpl) x86_64 dev build. Prefer the baseline (non-v3) for max CPU compatibility, but fall back to
    # the x86-64-v3 build when upstream only ships that — as of 2026-06 zhongfly dropped the non-v3 GPL dev asset
    # and offers only mpv-dev-x86_64-v3. Win11's hardware floor (8th-gen Intel / Zen+) supports x86-64-v3 (AVX2).
    $candidates = $rel.assets |
        Where-Object { $_.name -like 'mpv-dev-x86_64-*' -and $_.name -notlike '*lgpl*' -and $_.name -like '*.7z' }
    $asset = ($candidates | Where-Object { $_.name -notlike '*-v3-*' } | Select-Object -First 1)
    if (-not $asset) { $asset = $candidates | Select-Object -First 1 } # fall back to the x86-64-v3 build
    if (-not $asset) { throw 'No mpv-dev-x86_64 (GPL) .7z asset in the latest release' }

    $archive = Join-Path $env:TEMP $asset.name
    Write-Host "Downloading $($asset.name) ($([int]($asset.size / 1MB)) MB)..."
    Invoke-WebRequest $asset.browser_download_url -OutFile $archive -Headers $headers
    & (Get-SevenZip) e $archive "-o$mpvDest" 'libmpv-2.dll' -y | Out-Null
    if (-not (Test-Path $dll)) { throw "Extraction did not produce $dll" }
    Write-Host "Done: $dll"
}

# --- ffmpeg (GPL, win64) -> native/ffmpeg/ffmpeg.exe ---
$ffDest = Join-Path $NativeRoot 'ffmpeg'
$ffexe = Join-Path $ffDest 'ffmpeg.exe'
if (Test-Path $ffexe) {
    Write-Host "ffmpeg already present: $ffexe"
}
else {
    New-Item -ItemType Directory -Force $ffDest | Out-Null
    Write-Host 'Resolving ffmpeg (GPL, win64) from BtbN/FFmpeg-Builds...'
    $ffrel = Invoke-RestMethod 'https://api.github.com/repos/BtbN/FFmpeg-Builds/releases/latest' -Headers $apiHeaders
    # The static (non-shared) GPL win64 build — a single self-contained ffmpeg.exe, no extra DLLs to ship.
    $ffasset = $ffrel.assets |
        Where-Object { $_.name -like '*win64-gpl*.zip' -and $_.name -notlike '*shared*' -and $_.name -notlike '*lgpl*' } |
        Select-Object -First 1
    if (-not $ffasset) { throw 'No static win64-gpl ffmpeg .zip in the latest BtbN release' }

    $ffarchive = Join-Path $env:TEMP $ffasset.name
    Write-Host "Downloading $($ffasset.name) ($([int]($ffasset.size / 1MB)) MB)..."
    Invoke-WebRequest $ffasset.browser_download_url -OutFile $ffarchive -Headers $headers
    # The zip nests bin/ffmpeg.exe under a versioned top folder; -r finds it, and we extract only ffmpeg.exe
    # (not ffprobe/ffplay) — media inspection already goes through libmpv.
    & (Get-SevenZip) e $ffarchive "-o$ffDest" 'ffmpeg.exe' -r -y | Out-Null
    if (-not (Test-Path $ffexe)) { throw "Extraction did not produce $ffexe" }
    Write-Host "Done: $ffexe"
}
