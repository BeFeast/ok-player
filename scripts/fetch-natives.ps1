#!/usr/bin/env pwsh
# Fetch the libmpv engine (GPL build, x64) into native/libmpv/ — consumed by OkPlayer.Render / OkPlayer.App
# (CopyToOutputDirectory) and by the real-libmpv integration tests. Idempotent: skips when already present.
#
#   pwsh ./scripts/fetch-natives.ps1
#
# Source: GPL `mpv-dev-x86_64` build from https://github.com/zhongfly/mpv-winbuild (matches OK Player's
# GPL-3.0-or-later licence). See native/README.md and THIRD-PARTY-NOTICES.md.
param(
    [string]$Dest = (Join-Path $PSScriptRoot '..' 'native' 'libmpv')
)
$ErrorActionPreference = 'Stop'

$dll = Join-Path $Dest 'libmpv-2.dll'
if (Test-Path $dll) {
    Write-Host "libmpv already present: $dll"
    return
}
New-Item -ItemType Directory -Force $Dest | Out-Null

$headers = @{ 'User-Agent' = 'okplayer-fetch-natives' }
# Authenticate ONLY the api.github.com lookup when a token is available (CI sets GITHUB_TOKEN). The
# unauthenticated API limit (60/hr per IP) is easily exhausted on shared Actions-runner IPs and fails this
# step with HTTP 403 "rate limit exceeded". The asset download below is a redirect to a CDN that rejects a
# second auth mechanism, so it must keep the token-free $headers.
$apiHeaders = $headers.Clone()
if ($env:GITHUB_TOKEN) { $apiHeaders['Authorization'] = "Bearer $env:GITHUB_TOKEN" }
Write-Host 'Resolving latest mpv-dev (GPL, x86_64) from zhongfly/mpv-winbuild...'
$rel = Invoke-RestMethod 'https://api.github.com/repos/zhongfly/mpv-winbuild/releases/latest' -Headers $apiHeaders
# GPL (non-lgpl) x86_64 dev build. Prefer the baseline (non-v3) for max CPU compatibility, but fall back to
# the x86-64-v3 build when upstream only ships that — as of 2026-06 zhongfly dropped the non-v3 GPL dev asset
# and offers only mpv-dev-x86_64-v3. Win11's hardware floor (8th-gen Intel / Zen+) supports x86-64-v3 (AVX2),
# so the v3 build is safe for this Win11-only app.
$candidates = $rel.assets |
    Where-Object { $_.name -like 'mpv-dev-x86_64-*' -and $_.name -notlike '*lgpl*' -and $_.name -like '*.7z' }
$asset = ($candidates | Where-Object { $_.name -notlike '*-v3-*' } | Select-Object -First 1)
if (-not $asset) { $asset = $candidates | Select-Object -First 1 } # fall back to the x86-64-v3 build
if (-not $asset) { throw 'No mpv-dev-x86_64 (GPL) .7z asset in the latest release' }

$archive = Join-Path $env:TEMP $asset.name
Write-Host "Downloading $($asset.name) ($([int]($asset.size / 1MB)) MB)..."
Invoke-WebRequest $asset.browser_download_url -OutFile $archive -Headers $headers

# Extract just libmpv-2.dll. Prefer 7z on PATH (GitHub's windows runner has it); fall back to the install dir.
$sevenZip = (Get-Command 7z -ErrorAction SilentlyContinue)?.Source
if (-not $sevenZip) { $sevenZip = (Get-Command 7za -ErrorAction SilentlyContinue)?.Source }
if (-not $sevenZip) { $sevenZip = 'C:\Program Files\7-Zip\7z.exe' }
& $sevenZip e $archive "-o$Dest" 'libmpv-2.dll' -y | Out-Null
if (-not (Test-Path $dll)) { throw "Extraction did not produce $dll" }

Write-Host "Done: $dll"
