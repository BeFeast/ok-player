#requires -Version 7
<#
.SYNOPSIS
  Build the OK Player Velopack release (auto-updating Setup.exe + portable zip) from a self-contained
  Release publish.
.DESCRIPTION
  Publishes the app self-contained for win-x64, stages LICENSE.txt + THIRD-PARTY-NOTICES.md + README next
  to it, then runs `vpk pack` to produce the Velopack artifacts in artifacts\releases:
    - OkPlayer-win-Setup.exe      the auto-updating installer users download once
    - OkPlayer-win-Portable.zip   the no-install portable build (our fallback "as before")
    - OkPlayer-<ver>-full.nupkg   the full release package (the update payload)
    - OkPlayer-<ver>-delta.nupkg  binary delta vs the previous release (from v2 onward)
    - releases.win.json           the channel manifest describing the packages above
  With -Publish, `vpk upload github` attaches all of the above to a (pre-)release on tag v<Version>.
  Since issue #131 the GitHub Release is the asset store, not the runtime feed: publishing fires the
  `release published` event, which runs .github/workflows/publish-win-feed.yml — that workflow re-derives
  the static feed on GitHub Pages (the URL installed builds actually poll) from this release's
  releases.win.json, so the feed update rides the release automatically. Without -Publish this is a
  local pack only (no network, no upload) — for verifying the pipeline.
.PARAMETER Version
  Version baked into the published assembly AND the Velopack package. Optional — defaults to the <Version>
  in src\OkPlayer.App\OkPlayer.App.csproj (the single source of truth, also shown in Settings -> About).
.PARAMETER Publish
  Also upload the artifacts to a GitHub pre-release on tag v<Version> (the live update feed). Requires a
  token in $env:GH_TOKEN or a logged-in `gh`. Omit for a local-only pack.
.EXAMPLE
  .\installer\build-velopack.ps1                 # local pack only (artifacts\releases)
  .\installer\build-velopack.ps1 -Publish        # pack + publish the GitHub pre-release (the feed)
#>
param(
  [string]$Version,
  [switch]$Publish
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$publishDir = Join-Path $repo 'artifacts\publish'
$releases = Join-Path $repo 'artifacts\releases'
$appProj = Join-Path $repo 'src\OkPlayer.App\OkPlayer.App.csproj'
$icon = Join-Path $repo 'src\OkPlayer.App\Assets\OkPlayer.ico'
$repoUrl = 'https://github.com/BeFeast/ok-player'

# Single source of truth: the app version lives in the csproj <Version>. Read it unless -Version overrides,
# so the package, the release tag, and the in-app "About" can never drift apart.
if (-not $Version) {
  $m = Select-String -Path $appProj -Pattern '<Version>\s*([^<\s]+)\s*</Version>' | Select-Object -First 1
  if (-not $m) { throw "No <Version> in $appProj and no -Version passed." }
  $Version = $m.Matches[0].Groups[1].Value
}
Write-Host "Version: $Version"

# Resolve vpk: prefer PATH, else the global-tool install location (a freshly-installed tool isn't on PATH
# until the shell is reopened). Keep it version-matched to the Velopack NuGet package.
$vpk = (Get-Command vpk -ErrorAction SilentlyContinue)?.Source
if (-not $vpk) { $vpk = Join-Path $env:USERPROFILE '.dotnet\tools\vpk.exe' }
if (-not (Test-Path $vpk)) { throw "vpk not found. Install it: dotnet tool install -g vpk --version 1.2.0" }

Write-Host "Publishing self-contained Release -> $publishDir"
if (Test-Path $publishDir) { Remove-Item $publishDir -Recurse -Force }
dotnet publish $appProj -c Release -r win-x64 -o $publishDir -p:Version=$Version
if ($LASTEXITCODE -ne 0) { throw "dotnet publish failed ($LASTEXITCODE)" }

Copy-Item (Join-Path $repo 'LICENSE') (Join-Path $publishDir 'LICENSE.txt') -Force
Copy-Item (Join-Path $repo 'THIRD-PARTY-NOTICES.md') $publishDir -Force
if (Test-Path (Join-Path $repo 'README.md')) { Copy-Item (Join-Path $repo 'README.md') $publishDir -Force }

# Start each run from a clean releases dir: vpk computes deltas against whatever prior .nupkg files sit in the
# output dir, so a stale local pack left here from a previous run could otherwise produce a delta against the
# wrong predecessor. On -Publish we repopulate it from the real GitHub feed just below.
if (Test-Path $releases) { Remove-Item $releases -Recurse -Force }
New-Item -ItemType Directory -Force -Path $releases | Out-Null

# When publishing, pull the prior releases first so vpk can compute a binary delta against them. The first
# release has no predecessor, so this is best-effort: on failure we fall back to a full-only release (always
# applies for clients; just no delta optimization), rather than deltaing against stale local files.
if ($Publish) {
  Write-Host "Fetching prior releases for delta computation"
  & $vpk download github --repoUrl $repoUrl --channel win --pre --outputDir $releases 2>&1 | Write-Host
  if ($LASTEXITCODE -ne 0) { Write-Host "  (no prior releases to delta against — first build / full-only)" }
}

# Pack the entire publish folder (self-contained runtime + libmpv + .pri + icon) into Velopack artifacts.
# --packId must stay 'OkPlayer' forever (changing it breaks the update chain); the human name is --packTitle.
# --channel is pinned to 'win', matching the client's ExplicitChannel (UpdateFeed.WinChannel) so the manifest
# name (releases.win.json) can never fork between pack and client. "beta" is deliberately NOT a Velopack
# channel here — it's the GitHub pre-release flag (--pre, on upload) plus the in-app display label, so there's
# a single feed and no channel split to keep in sync.
Write-Host "Packing Velopack release -> $releases"
& $vpk pack `
  --packId OkPlayer `
  --packTitle 'OK Player' `
  --packAuthors 'BeFeast' `
  --packVersion $Version `
  --packDir $publishDir `
  --mainExe OkPlayer.exe `
  --icon $icon `
  --channel win `
  --outputDir $releases
if ($LASTEXITCODE -ne 0) { throw "vpk pack failed ($LASTEXITCODE)" }

$setup = Join-Path $releases 'OkPlayer-win-Setup.exe'
if (Test-Path $setup) { Write-Host "Setup built: $setup ($([int]((Get-Item $setup).Length/1MB)) MB)" }

if ($Publish) {
  $token = $env:GH_TOKEN
  if (-not $token) { $token = (gh auth token 2>$null) }
  if (-not $token) { throw "No GitHub token: set `$env:GH_TOKEN or run `gh auth login`." }
  Write-Host "Uploading GitHub pre-release v$Version (the update feed)"
  & $vpk upload github `
    --repoUrl $repoUrl `
    --outputDir $releases `
    --channel win `
    --publish `
    --pre `
    --tag "v$Version" `
    --releaseName "OK Player $Version" `
    --token $token
  if ($LASTEXITCODE -ne 0) { throw "vpk upload github failed ($LASTEXITCODE)" }
  Write-Host "Published v$Version. Testers install OkPlayer-win-Setup.exe once; updates then apply in-app."
  # Publishing with a user token fires `release published`, which runs the publish-win-feed workflow —
  # the static feed on GitHub Pages updates without further action here (issue #131).
  Write-Host "The publish-win-feed workflow now refreshes the static update feed:"
  Write-Host "  https://befeast.github.io/ok-player/updates/win/releases.win.json"
  Write-Host "Check it with: gh run list --workflow publish-win-feed.yml --limit 1"
  Write-Host "  (fallback if that run went missing: gh workflow run publish-win-feed.yml)"
} else {
  Write-Host "Local pack complete (no upload). Re-run with -Publish to push the GitHub pre-release feed."
}
