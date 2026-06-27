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

# The build stamps "<version>+<sha>" into the assembly (StampGitShaRevision in the csproj); read it back.
$built = $null
if (Test-Path $exe) {
  $pv = ([System.Diagnostics.FileVersionInfo]::GetVersionInfo($exe)).ProductVersion
  if ($pv -match '\+(.+)$') { $built = $Matches[1] }
}

# A dirty tree ALWAYS rebuilds: the "-dirty" stamp can't tell one set of uncommitted edits from the next, so
# a stamp match would otherwise launch a stale build for the exact case this launcher exists to cover. When
# the tree is clean, the embedded sha must equal HEAD.
$stale = $ForceRebuild -or (-not (Test-Path $exe)) -or $dirty -or ($built -ne $head)

if ($stale) {
  $reason = if (-not (Test-Path $exe)) { 'no build yet' } elseif ($ForceRebuild) { 'forced' } elseif ($dirty) { 'uncommitted changes' } else { "built '$built', want '$head'" }
  Write-Host "Dev preview out of date on '$branch' ($reason) -- rebuilding..." -ForegroundColor Yellow

  # Find every running OkPlayer, then split it: instances we can CONFIRM were launched from this output dir
  # (force-close those -- they lock the DLLs we're about to overwrite), vs instances whose path we can't read.
  # Matching by full path means we never force-close the installed app or another OkPlayer that just shares the
  # name. A non-elevated launcher can neither read the path of nor terminate an elevated process, so an
  # OkPlayer we can't inspect is almost certainly running as admin: we can't stop it, but we record its PID so
  # that if it turns out to be holding these files, the cleanup failure below can name exactly what to close.
  # PowerShell's added .Path member returns $null (it does NOT throw) when the path can't be read -- which is
  # exactly what happens for a higher-integrity (elevated) process seen from this normal one. So classify by the
  # value: matches our exe -> ours; readable-but-different -> ignore; empty/unreadable -> opaque. (The try/catch
  # is belt-and-suspenders for any host where .Path throws instead of returning $null.)
  $ours   = [System.Collections.Generic.List[System.Diagnostics.Process]]::new()
  $opaque = [System.Collections.Generic.List[int]]::new()
  foreach ($p in (Get-Process OkPlayer -ErrorAction SilentlyContinue)) {
    $path = try { $p.Path } catch { $null }
    if ($path -eq $exe) { $ours.Add($p) }
    elseif ([string]::IsNullOrEmpty($path)) { $opaque.Add($p.Id) }
  }

  $stuck = [System.Collections.Generic.List[int]]::new()
  foreach ($proc in $ours) {
    try { $proc.Kill(); if (-not $proc.WaitForExit(5000)) { $stuck.Add($proc.Id) } }
    catch { $stuck.Add($proc.Id) }   # Kill can throw "Access is denied" (e.g. an elevated instance) -- still stuck
  }
  if ($stuck.Count) { throw "Couldn't stop the running dev preview (PID $($stuck -join ', ')) -- close OK Player and try again." }

  # Clear the previous publish, retrying ~10s to wait out a transient AV/OS handle on a just-killed exe/DLL
  # (the common case). If a file is still locked after that, stop with an actionable message: dotnet publish
  # cannot overwrite an OS-locked file either, so proceeding would only surface a cryptic build error.
  if (Test-Path $outDir) {
    $cleared = $false
    for ($attempt = 1; $attempt -le 40 -and -not $cleared; $attempt++) {
      try { Remove-Item $outDir -Recurse -Force -ErrorAction Stop; $cleared = $true }
      catch { Start-Sleep -Milliseconds 250 }
    }
    if (-not $cleared) {
      $elev = if ($opaque.Count) { " An OK Player may be running as administrator (PID $($opaque -join ', ')); a normally-launched script can't stop it -- close it manually." } else { '' }
      throw "A file in $outDir is locked (antivirus, or an OK Player still running from it).$elev Close OK Player, pause real-time scanning if needed, and run the Dev Preview again."
    }
  }

  dotnet publish $appProj -c Release -r win-x64 --self-contained true -o $outDir
  if ($LASTEXITCODE -ne 0) { throw "dotnet publish failed ($LASTEXITCODE)" }
  $pv = ([System.Diagnostics.FileVersionInfo]::GetVersionInfo($exe)).ProductVersion
  $built = if ($pv -match '\+(.+)$') { $Matches[1] } else { $head }
  Write-Host "Built $branch @ $built" -ForegroundColor Green
} else {
  Write-Host "Dev preview is current ($branch @ $built) -- launching." -ForegroundColor Green
}

if ($NoLaunch) { Write-Host "-NoLaunch set: not starting the app." ; return }
if (-not (Test-Path $exe)) { throw "Expected $exe but it does not exist." }
Start-Process -FilePath $exe
