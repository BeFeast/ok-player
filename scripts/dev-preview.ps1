#requires -Version 7
<#
.SYNOPSIS
  Launch the current OK Player Development Preview — the one click that always opens the latest build.
.DESCRIPTION
  Point a desktop/taskbar shortcut at this script and you never hunt through artifact folders again.

  It launches the self-contained dev build at artifacts\clean-main\OkPlayer.exe, and rebuilds it FIRST only
  when it is out of date — so a click is instant when nothing changed, and never stale when it did:

    - Compares the built exe's embedded commit (Settings -> About shows the same `build <sha>`) against the
      repo's current HEAD. A moved branch or an uncommitted change ("-dirty") triggers a fresh rebuild.
    - A rebuild kills any running instance first (so it can't lock its own DLLs), then clean-publishes.

  Builds whatever is checked out (normally `main`); the branch + sha are printed so it's never a mystery.
.PARAMETER ForceRebuild
  Rebuild even if the existing build is already current.
.PARAMETER NoLaunch
  Build/refresh if needed but do not start the app (used to warm the build).
.EXAMPLE
  .\scripts\dev-preview.ps1            # launch the current dev preview (rebuild only if stale)
#>
param(
  [switch]$ForceRebuild,
  [switch]$NoLaunch
)

$ErrorActionPreference = 'Stop'
$repo    = Split-Path -Parent $PSScriptRoot
$appProj = Join-Path $repo 'src\OkPlayer.App\OkPlayer.App.csproj'
$outDir  = Join-Path $repo 'artifacts\clean-main'
$exe     = Join-Path $outDir 'OkPlayer.exe'

$head   = (git -C $repo rev-parse --short=7 HEAD).Trim()
if ($LASTEXITCODE -ne 0) { throw "git rev-parse failed -- is $repo a git checkout?" }
$branch = (git -C $repo rev-parse --abbrev-ref HEAD).Trim()
$dirty  = [bool]((git -C $repo status --porcelain) | Select-Object -First 1)
$want   = if ($dirty) { "$head-dirty" } else { $head }

# The build stamps "<version>+<sha>" into the assembly (StampGitShaRevision in the csproj); read it back.
$built = $null
if (Test-Path $exe) {
  $pv = ([System.Diagnostics.FileVersionInfo]::GetVersionInfo($exe)).ProductVersion
  if ($pv -match '\+(.+)$') { $built = $Matches[1] }
}

$stale = $ForceRebuild -or (-not (Test-Path $exe)) -or ($built -ne $want)

if ($stale) {
  $reason = if (-not (Test-Path $exe)) { 'no build yet' } elseif ($ForceRebuild) { 'forced' } else { "built '$built', want '$want'" }
  Write-Host "Dev preview out of date on '$branch' ($reason) -- rebuilding..." -ForegroundColor Yellow
  Get-Process OkPlayer -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep -Milliseconds 500   # let the OS release the file handles before we overwrite the binaries
  if (Test-Path $outDir) { Remove-Item $outDir -Recurse -Force }
  dotnet publish $appProj -c Release -r win-x64 --self-contained true -o $outDir
  if ($LASTEXITCODE -ne 0) { throw "dotnet publish failed ($LASTEXITCODE)" }
  $built = $want
  Write-Host "Built $branch @ $built" -ForegroundColor Green
} else {
  Write-Host "Dev preview is current ($branch @ $built) -- launching." -ForegroundColor Green
}

if ($NoLaunch) { Write-Host "-NoLaunch set: not starting the app." ; return }
if (-not (Test-Path $exe)) { throw "Expected $exe but it does not exist." }
Start-Process -FilePath $exe
