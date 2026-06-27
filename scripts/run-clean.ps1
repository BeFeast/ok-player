#requires -Version 7
<#
.SYNOPSIS
  Build and launch a guaranteed-clean Release of OK Player (intended: the main branch).
.DESCRIPTION
  Produces a clean, correct-branch Release build, then launches it. Three guards make a
  wrong build hard to get:

    1. Branch guard. Refuses to build unless the checkout is on 'main', so you never
       accidentally ship/run unmerged feature-branch code. Override with -AllowBranch.
    2. Process kill. Stops any running OkPlayer.exe first. A running instance locks its
       own output DLLs, so an incremental build can't overwrite them and silently yields
       a half-updated "Frankenbuild". We kill it before publishing.
    3. Clean publish. Wipes artifacts\clean-main and does a from-scratch self-contained
       win-x64 publish, so nothing stale survives.

  It then prints the exact git short SHA and the OkPlayer.exe path that was built, and
  (unless -NoLaunch) starts it. This mirrors the publish conventions in
  installer\build-installer.ps1.
.PARAMETER NoLaunch
  Build but do not launch the resulting OkPlayer.exe.
.PARAMETER AllowBranch
  Skip the branch guard and build whatever branch is currently checked out.
.EXAMPLE
  .\scripts\run-clean.ps1                 # clean Release build of main, then launch
.EXAMPLE
  .\scripts\run-clean.ps1 -NoLaunch       # clean build only, do not launch
.EXAMPLE
  .\scripts\run-clean.ps1 -AllowBranch    # build the current (non-main) branch on purpose
#>
param(
  [switch]$NoLaunch,
  [switch]$AllowBranch
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$appProj = Join-Path $repo 'src\OkPlayer.App\OkPlayer.App.csproj'
$outDir = Join-Path $repo 'artifacts\clean-main'
$exePath = Join-Path $outDir 'OkPlayer.exe'

# 1. Branch guard: never build unmerged code by accident.
$branch = (git -C $repo rev-parse --abbrev-ref HEAD).Trim()
if ($LASTEXITCODE -ne 0) { throw "git rev-parse failed ($LASTEXITCODE). Is $repo a git checkout?" }
Write-Host "Branch: $branch"
if ($branch -ne 'main' -and -not $AllowBranch) {
  Write-Warning "Checkout is on '$branch', not 'main' -- you are likely about to build UNMERGED code."
  throw "Refusing to build off '$branch'. Run 'git checkout main' first, or re-run with -AllowBranch to build this branch on purpose."
}

# 2. Stop any instance launched from THIS output dir so it can't lock its own DLLs. Split running OkPlayers:
#    ones we can CONFIRM by full path (force-close those -- never the installed app or a namesake), vs ones
#    whose path we can't read. A non-elevated script can neither inspect nor terminate an elevated process, so
#    an OkPlayer we can't read is almost certainly running as admin; record its PID so a later locked-file
#    failure can name exactly what to close instead of failing generically.
# PowerShell's added .Path member returns $null (it does NOT throw) when the path can't be read -- which is
# exactly what happens for a higher-integrity (elevated) process seen from this normal one. So classify by the
# value: matches our exe -> ours; readable-but-different -> ignore; empty/unreadable -> opaque. (The try/catch
# is belt-and-suspenders for any host where .Path throws instead of returning $null.)
$ours   = [System.Collections.Generic.List[System.Diagnostics.Process]]::new()
$opaque = [System.Collections.Generic.List[int]]::new()
foreach ($p in (Get-Process OkPlayer -ErrorAction SilentlyContinue)) {
  $path = try { $p.Path } catch { $null }
  if ($path -eq $exePath) { $ours.Add($p) }
  elseif ([string]::IsNullOrEmpty($path)) { $opaque.Add($p.Id) }
}
if ($ours.Count) {
  Write-Host "Stopping the dev instance from $outDir : PID $(($ours | ForEach-Object Id) -join ', ')"
  $stuck = [System.Collections.Generic.List[int]]::new()
  foreach ($proc in $ours) {
    try { $proc.Kill(); if (-not $proc.WaitForExit(5000)) { $stuck.Add($proc.Id) } }
    catch { $stuck.Add($proc.Id) }   # Kill can throw "Access is denied" (e.g. an elevated instance) -- still stuck
  }
  # A process we couldn't stop still holds the output DLLs; fail clearly instead of a cryptic publish error.
  if ($stuck.Count) { throw "Couldn't stop the running instance (PID $($stuck -join ', ')) -- close OK Player and try again." }
} else {
  Write-Host "No confirmed OkPlayer instance from this output dir."
}

# 3. Clean publish: wipe the output folder, then publish from scratch. Retry the delete with backoff so a
#    lingering AV/OS handle on a just-killed exe/DLL doesn't abort the build.
if (Test-Path $outDir) {
  Write-Host "Removing previous output: $outDir"
  $cleared = $false
  for ($attempt = 1; $attempt -le 40 -and -not $cleared; $attempt++) {
    try { Remove-Item $outDir -Recurse -Force -ErrorAction Stop; $cleared = $true }
    catch { Start-Sleep -Milliseconds 250 }
  }
  # If a file is still locked after ~10s, stop with an actionable message: dotnet publish can't overwrite an
  # OS-locked file either, so proceeding would only surface a cryptic build error. If an opaque (likely
  # elevated) OkPlayer is running, name its PID -- this script can't stop it, so the user must close it.
  if (-not $cleared) {
    $elev = if ($opaque.Count) { " An OK Player may be running as administrator (PID $($opaque -join ', ')); a normally-launched script can't stop it -- close it manually." } else { '' }
    throw "A file in $outDir is locked (antivirus, or an OK Player still running from it).$elev Close OK Player, pause real-time scanning if needed, and try again."
  }
}

Write-Host "Publishing clean self-contained Release (win-x64) -> $outDir"
dotnet publish $appProj -c Release -r win-x64 --self-contained true -o $outDir
if ($LASTEXITCODE -ne 0) { throw "dotnet publish failed ($LASTEXITCODE)" }

# 4. Report exactly what was built so there is no ambiguity. --short=7 matches the SHA the app stamps into
#    Settings -> About (App.GitSha), so you can cross-check the running build against this line.
$sha = (git -C $repo rev-parse --short=7 HEAD).Trim()
Write-Host ""
Write-Host "Clean build complete."
Write-Host "  Branch: $branch"
Write-Host "  Commit: $sha"
Write-Host "  Exe:    $exePath"

# 5. Launch unless suppressed.
if ($NoLaunch) {
  Write-Host "-NoLaunch set: not starting the app."
} else {
  if (-not (Test-Path $exePath)) { throw "Expected $exePath but it does not exist." }
  Write-Host "Launching $exePath"
  Start-Process -FilePath $exePath
}
