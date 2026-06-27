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

# 2. Kill any running instance so it can't lock its own output DLLs.
$running = Get-Process OkPlayer -ErrorAction SilentlyContinue
if ($running) {
  Write-Host "Stopping running OkPlayer instance(s): PID $($running.Id -join ', ')"
  $running | Stop-Process -Force
  # Give the OS a moment to release the file handles before we overwrite the binaries.
  Start-Sleep -Seconds 2
} else {
  Write-Host "No running OkPlayer instance found."
}

# 3. Clean publish: wipe the output folder, then publish from scratch.
if (Test-Path $outDir) {
  Write-Host "Removing previous output: $outDir"
  Remove-Item $outDir -Recurse -Force
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
