#requires -Version 5.1
<#
.SYNOPSIS
  Provision a persistent Windows 11 development VM for OK Player (WinUI 3 shell + Rust workspace) to a
  known, reproducible toolchain state. Idempotent: re-running reaches the same verified state.
.DESCRIPTION
  Installs and verifies every system-provisioned tool the build/test/package loop needs, reading the
  version baselines from scripts/windows-dev-versions.json (the single source of truth for tools that
  live OUTSIDE the repository):

    * Visual Studio 2026 Build Tools (or Community) with the Managed Desktop + Native Desktop workloads,
      the Windows 11 SDK (26100) component, and the Windows App SDK C# templates
    * .NET 9 SDK
    * Rust MSVC toolchain (rustup + stable + the x86_64-pc-windows-msvc target)
    * Git, 7-Zip (for native archive extraction), and (for source native builds only) CMake + Ninja
    * PowerShell 7 (so the pwsh-only repo scripts run natively)
    * libmpv / ffmpeg native binaries via scripts/fetch-natives.ps1

  Each step checks for the tool first and only installs what is missing, so the script is safe to run on
  a fresh VM, on a half-provisioned VM, and on an already-complete VM -- every run converges to the same
  state. It never touches any parked physical verification checkout: it provisions the machine it runs on
  and clones/mutates nothing outside this repository.

  Installs run through winget (App Installer). On a clean Windows 11 VM winget is present; if it is not,
  the script stops with guidance rather than guessing.

  After provisioning it runs scripts/report-windows-env.ps1 and, unless -NoReport, writes the machine-
  readable environment report so the VM's verified state is captured alongside the run.
.PARAMETER CheckOnly
  Verify the toolchain and emit the environment report WITHOUT installing anything. Exit code is non-zero
  when a required tool is missing or below its baseline -- use this in CI or a snapshot gate.
.PARAMETER SkipVisualStudio
  Do not install/modify Visual Studio. Useful when the IDE is provisioned by a separate image step.
.PARAMETER SkipNatives
  Do not fetch libmpv/ffmpeg. The managed build and headless Core tests do not need them.
.PARAMETER ReportPath
  Where to write the JSON environment report. Defaults to artifacts\windows-env-report.json.
.PARAMETER NoReport
  Do not write the report file (the summary is still printed).
.EXAMPLE
  pwsh ./scripts/bootstrap-windows-dev-vm.ps1
  # Full provision from a clean VM, then write artifacts\windows-env-report.json.
.EXAMPLE
  powershell -File .\scripts\bootstrap-windows-dev-vm.ps1 -CheckOnly
  # Verify a snapshot's toolchain without changing it; non-zero exit means the baseline is not met.
#>
[CmdletBinding()]
param(
    [switch]$CheckOnly,
    [switch]$SkipVisualStudio,
    [switch]$SkipNatives,
    [string]$ReportPath,
    [switch]$NoReport
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repoRoot = Split-Path -Parent $PSScriptRoot
$manifestPath = Join-Path $PSScriptRoot 'windows-dev-versions.json'
if (-not (Test-Path $manifestPath)) { throw "Version manifest not found: $manifestPath" }
$manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json

function Write-Step { param([string]$Message) Write-Host "==> $Message" -ForegroundColor Cyan }
function Write-Ok { param([string]$Message) Write-Host "  ok  $Message" -ForegroundColor Green }
function Write-Skip { param([string]$Message) Write-Host "  --  $Message" -ForegroundColor DarkGray }

function Test-Winget {
    $wg = Get-Command winget -ErrorAction SilentlyContinue
    if (-not $wg) {
        throw @"
winget (App Installer) is not available. On Windows 11 it ships in-box; install/repair it from the
Microsoft Store ('App Installer') or https://aka.ms/getwinget, then re-run this script. This bootstrap
deliberately does not shell out to an unverified installer to add winget itself.
"@
    }
    return $wg.Source
}

# Idempotent winget install: skip when one of the package ids is already present, otherwise install using
# the first id that succeeds (exact match). When a primary id is not yet published to the winget repository,
# AlternativeIds are tried in order. An override means the installer must run even for an existing package
# (for example, to add missing Visual Studio workloads), so that path uses the installed id with --force.
function Install-WingetPackage {
    param(
        [Parameter(Mandatory)][string]$Id,
        [string]$Name = $Id,
        [string[]]$OverrideArgs,
        [string[]]$AlternativeIds
    )
    $ids = @(@($Id) + @($AlternativeIds) | Where-Object { $_ })
    Write-Step "Ensuring $Name ($($ids -join ' / '))"
    $installedId = $null
    foreach ($candidate in $ids) {
        $listed = winget list --id $candidate -e --accept-source-agreements 2>$null
        if ($LASTEXITCODE -eq 0 -and ($listed -match [regex]::Escape($candidate))) {
            $installedId = $candidate
            break
        }
    }
    if ($installedId -and -not $OverrideArgs) {
        Write-Skip "$Name already installed ($installedId)"
        return
    }
    if ($CheckOnly) {
        $action = if ($installedId) { 'modify' } else { 'install' }
        Write-Host "  would $action $Name" -ForegroundColor Yellow
        return
    }

    $lastExit = $null
    $candidateIds = if ($installedId) { @($installedId) } else { $ids }
    foreach ($candidate in $candidateIds) {
        $wingetArgs = @('install', '--id', $candidate, '-e', '--accept-source-agreements',
            '--accept-package-agreements', '--disable-interactivity')
        if ($OverrideArgs) { $wingetArgs += $OverrideArgs }
        if ($installedId) { $wingetArgs += '--force' }
        winget @wingetArgs
        $lastExit = $LASTEXITCODE
        if ($lastExit -eq 0) {
            $action = if ($installedId) { 'modified' } else { 'installed' }
            Write-Ok "$Name $action ($candidate)"
            return
        }
    }
    throw "winget failed to install $Name; last exit code $lastExit"
}

function Get-VsWherePath {
    $p = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (Test-Path $p) { return $p }
    return $null
}

function Test-VisualStudioComponents {
    param([string[]]$Required)
    $vswhere = Get-VsWherePath
    if (-not $vswhere) { return $false }
    # -products * so Build Tools (not only the IDE editions) count; -requires all listed components.
    $found = & $vswhere -products '*' -latest -prerelease -requires @Required -property installationPath 2>$null
    return [bool]$found
}

function Install-VisualStudio {
    $vs = $manifest.tools.visualStudio
    $components = @($vs.components)
    if (Test-VisualStudioComponents -Required $components) {
        Write-Skip 'Visual Studio workloads already present (Managed Desktop, Native Desktop, Win11 SDK 26100, WinAppSDK C#)'
        return
    }
    if ($CheckOnly) { Write-Host '  would install/modify Visual Studio workloads' -ForegroundColor Yellow; return }

    # Pass the workload/component set to the VS installer via --override. --wait blocks until the (long)
    # install finishes; --norestart lets the bootstrap own reboot timing.
    $override = @('--quiet', '--wait', '--norestart')
    foreach ($c in $components) { $override += @('--add', $c) }
    $overrideStr = ($override -join ' ')
    Install-WingetPackage -Id $vs.wingetId -Name 'Visual Studio 2026 Build Tools' `
        -OverrideArgs @('--override', $overrideStr) `
        -AlternativeIds @($vs.alternativeWingetIds)
}

function Install-RustMsvc {
    $rust = $manifest.tools.rustup
    Install-WingetPackage -Id $rust.wingetId -Name 'Rust (rustup)'
    if ($CheckOnly) { return }
    $rustup = Get-Command rustup -ErrorAction SilentlyContinue
    if (-not $rustup) {
        # rustup lands under %USERPROFILE%\.cargo\bin, which the winget install adds to PATH for new
        # sessions; make it usable in THIS session too so the toolchain steps below succeed on first run.
        $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
        if (Test-Path (Join-Path $cargoBin 'rustup.exe')) { $env:Path = "$cargoBin;$env:Path" }
        $rustup = Get-Command rustup -ErrorAction SilentlyContinue
    }
    if (-not $rustup) { Write-Skip 'rustup not on PATH yet; open a new shell and re-run to finish the Rust toolchain'; return }
    Write-Step "Ensuring Rust $($rust.toolchain) + target $($rust.target)"
    & rustup toolchain install $rust.toolchain --profile minimal --no-self-update
    & rustup default $rust.toolchain
    & rustup target add $rust.target
    Write-Ok "Rust $($rust.toolchain) ($($rust.target)) ready"
}

# ---- Provision -----------------------------------------------------------------------------------
Write-Host ''
Write-Host "OK Player -- Windows development VM bootstrap" -ForegroundColor White
Write-Host "Baseline VM envelope: $($manifest.vmEnvelope.vcpu) vCPU, $($manifest.vmEnvelope.memoryGiB) GiB RAM, $($manifest.vmEnvelope.diskGiB) GiB SSD (development baseline, not an app requirement)" -ForegroundColor DarkGray
Write-Host ''

Test-Winget | Out-Null

Install-WingetPackage -Id 'Microsoft.PowerShell' -Name 'PowerShell 7'
Install-WingetPackage -Id $manifest.tools.git.wingetId -Name 'Git'
Install-WingetPackage -Id $manifest.tools.dotnetSdk.wingetId -Name '.NET 9 SDK'
Install-WingetPackage -Id $manifest.tools.cmake.wingetId -Name 'CMake'
Install-WingetPackage -Id $manifest.tools.ninja.wingetId -Name 'Ninja'
Install-WingetPackage -Id $manifest.tools.sevenZip.wingetId -Name '7-Zip'
Install-RustMsvc

if ($SkipVisualStudio) { Write-Skip 'Visual Studio skipped (-SkipVisualStudio)' } else { Install-VisualStudio }

if ($SkipNatives) {
    Write-Skip 'Native binaries skipped (-SkipNatives)'
} elseif ($CheckOnly) {
    Write-Host '  would fetch libmpv/ffmpeg via scripts/fetch-natives.ps1' -ForegroundColor Yellow
} else {
    Write-Step 'Fetching libmpv/ffmpeg native binaries'
    $fetchScript = Join-Path $PSScriptRoot 'fetch-natives.ps1'
    if ($PSVersionTable.PSEdition -eq 'Core') {
        # Already running under PowerShell 7 -- invoke in-process.
        & $fetchScript
        if ($LASTEXITCODE) { throw "fetch-natives.ps1 failed; exit code $LASTEXITCODE" }
    } else {
        # fetch-natives.ps1 uses PowerShell 7 syntax (null-conditional ?.), so it must NOT be parsed by the
        # in-box Windows PowerShell 5.1 the documented first run uses. Re-invoke it through the pwsh this
        # bootstrap just installed. On a first run pwsh may not be on PATH yet (a new shell picks it up);
        # skip with guidance rather than failing, so a re-run converges once PATH refreshes.
        $pwsh = Get-Command pwsh -ErrorAction SilentlyContinue
        if (-not $pwsh) {
            $pwshDefault = Join-Path $env:ProgramFiles 'PowerShell\7\pwsh.exe'
            if (Test-Path $pwshDefault) { $pwsh = Get-Command $pwshDefault -ErrorAction SilentlyContinue }
        }
        if (-not $pwsh) {
            Write-Skip 'PowerShell 7 (pwsh) not on PATH yet; open a new shell and re-run to fetch libmpv/ffmpeg'
        } else {
            & $pwsh.Source -NoProfile -File $fetchScript
            if ($LASTEXITCODE) { throw "fetch-natives.ps1 failed; exit code $LASTEXITCODE" }
        }
    }
}

# ---- Verify + report -----------------------------------------------------------------------------
Write-Host ''
Write-Step 'Verifying toolchain and building the environment report'
$reportScript = Join-Path $PSScriptRoot 'report-windows-env.ps1'
if (-not $ReportPath) { $ReportPath = Join-Path $repoRoot 'artifacts\windows-env-report.json' }

$reportArgs = @{ FailOnMissing = $true }
if ($NoReport) { $reportArgs['NoFile'] = $true } else { $reportArgs['OutFile'] = $ReportPath }

& $reportScript @reportArgs
$reportExit = $LASTEXITCODE

Write-Host ''
if ($reportExit -eq 0) {
    Write-Host 'Bootstrap complete -- toolchain meets the OK Player baseline.' -ForegroundColor Green
} else {
    Write-Host 'Bootstrap finished but the toolchain does NOT meet the baseline (see report above).' -ForegroundColor Yellow
    if (-not $CheckOnly) {
        Write-Host 'A reboot or a new shell is often required after a first-time Visual Studio / Rust install; re-run to converge.' -ForegroundColor Yellow
    }
}
exit $reportExit
