#requires -Version 7
<#
.SYNOPSIS
  Deploy the current OK Player build over the locally-installed app, so the existing taskbar / desktop /
  Start-menu shortcut (which points at the installed app, not the dev artifacts) runs the latest build.
.DESCRIPTION
  The installed app lives at %LOCALAPPDATA%\Programs\OK Player and is what the pinned shortcuts launch.
  During active development it goes stale fast: merging a fix updates the repo, not that folder, so a click
  keeps running yesterday's build (Settings -> About shows an old `build <sha>`). This script closes that gap.

  It clean-publishes whatever is checked out (via dev-preview.ps1, which rebuilds only when stale), kills any
  running instance so its DLLs aren't locked, then mirrors the publish into the installed folder. Afterwards
  the installed exe reports the same `build <sha>` as the repo HEAD.

  Use this to put a freshly-merged build behind the existing shortcut, without hunting artifact folders.
.PARAMETER NoKill
  Don't terminate a running OK Player first (the copy will fail if it has files open).
.EXAMPLE
  .\scripts\deploy-installed.ps1     # publish the current build and update the installed app in place
#>
param([switch]$NoKill)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$src  = Join-Path $repo 'artifacts\clean-main'
$dst  = Join-Path $env:LOCALAPPDATA 'Programs\OK Player'

if (-not (Test-Path $dst)) {
  throw "OK Player isn't installed at $dst -- run the installer once so the shortcuts exist, then re-run this."
}

# Build the clean self-contained publish (the launcher rebuilds only if the artifact is out of date).
& (Join-Path $PSScriptRoot 'dev-preview.ps1') -NoLaunch
$srcExe = Join-Path $src 'OkPlayer.exe'
if (-not (Test-Path $srcExe)) { throw "Expected a publish at $srcExe but it does not exist." }

if (-not $NoKill) {
  Get-Process OkPlayer -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
}

# Mirror the publish into the installed folder. /E copies subtrees and overwrites; no /MIR, so the installer's
# own files (uninstaller, registry stubs) are left intact. robocopy exit codes 0-7 are success.
robocopy $src $dst /E /NFL /NDL /NJH /NJS /NP /R:3 /W:1 | Out-Null
if ($LASTEXITCODE -ge 8) { throw "robocopy failed (exit $LASTEXITCODE) -- is a file in $dst locked?" }

$pv = ([System.Diagnostics.FileVersionInfo]::GetVersionInfo((Join-Path $dst 'OkPlayer.exe'))).ProductVersion
Write-Host "Deployed to $dst -- installed build is now $pv (matches HEAD)." -ForegroundColor Green
