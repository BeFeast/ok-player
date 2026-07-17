#requires -Version 5.1
<#
.SYNOPSIS
  Produce a machine-readable environment report for an OK Player Windows development VM and verify it
  against the toolchain baseline in scripts/windows-dev-versions.json.
.DESCRIPTION
  Captures the versions that define a reproducible build/test surface -- OS, Windows SDK, MSVC compiler /
  Visual Studio, Rust (MSVC toolchain + target), .NET SDK, WinUI / Windows App SDK, Git, CMake, Ninja,
  PowerShell, and the resolved libmpv native -- and emits them as a single JSON document. The WinUI /
  Windows App SDK and Windows SDK BuildTools versions are read from Directory.Packages.props and the app
  csproj (their real source of truth in this repo), never duplicated, so the report cannot drift from what
  the build actually restores.

  Each captured value is checked against its baseline. The overall result is `ok` only when every REQUIRED
  tool is present and at/above its minimum. Optional tools (CMake/Ninja -- needed only for source native
  builds) are reported but never fail the run.

  Contains no hostnames, usernames, network addresses, licenses, or secrets: only tool identities and
  versions of the machine it runs on.
.PARAMETER OutFile
  Write the JSON report to this path (parent directory is created). Omit to print JSON to stdout only.
.PARAMETER NoFile
  Never write a file, even if OutFile is set.
.PARAMETER FailOnMissing
  Exit non-zero when a required tool is missing or below its baseline. Without it the report is emitted and
  the exit code is always 0 (pure reporting).
.EXAMPLE
  pwsh ./scripts/report-windows-env.ps1 -OutFile artifacts\windows-env-report.json
.EXAMPLE
  pwsh ./scripts/report-windows-env.ps1 -FailOnMissing | Out-Null
  # CI/snapshot gate: succeeds only when the VM meets the baseline.
#>
[CmdletBinding()]
param(
    [string]$OutFile,
    [switch]$NoFile,
    [switch]$FailOnMissing
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repoRoot = Split-Path -Parent $PSScriptRoot
$manifestPath = Join-Path $PSScriptRoot 'windows-dev-versions.json'
$manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json

# --- helpers --------------------------------------------------------------------------------------
function Get-CmdOutput {
    param([string]$Exe, [string[]]$Args)
    $cmd = Get-Command $Exe -ErrorAction SilentlyContinue
    if (-not $cmd) { return $null }
    try { return (& $cmd.Source @Args 2>$null | Out-String).Trim() }
    catch { return $null }
}

# Extract the first dotted numeric version (e.g. "2.43.0.windows.1" -> [version]2.43.0) for comparison.
function Get-VersionTriple {
    param([string]$Text)
    if (-not $Text) { return $null }
    $m = [regex]::Match($Text, '(\d+)\.(\d+)(?:\.(\d+))?')
    if (-not $m.Success) { return $null }
    $patch = if ($m.Groups[3].Success) { $m.Groups[3].Value } else { '0' }
    return [version]("{0}.{1}.{2}" -f $m.Groups[1].Value, $m.Groups[2].Value, $patch)
}

function Get-PropsVersion {
    param([string]$File, [string]$PackageId)
    if (-not (Test-Path $File)) { return $null }
    $text = Get-Content -Raw -LiteralPath $File
    $m = [regex]::Match($text, "Include=""$([regex]::Escape($PackageId))""\s+Version=""([^""]+)""")
    if ($m.Success) { return $m.Groups[1].Value }
    return $null
}

function Get-CsprojValue {
    param([string]$File, [string]$Element)
    if (-not (Test-Path $File)) { return $null }
    $text = Get-Content -Raw -LiteralPath $File
    $m = [regex]::Match($text, "<$Element>\s*([^<\s]+)\s*</$Element>")
    if ($m.Success) { return $m.Groups[1].Value }
    return $null
}

$checks = New-Object System.Collections.ArrayList
function Add-Check {
    param([string]$Name, [string]$Found, [string]$Min, [bool]$Required, [bool]$Ok)
    $status = if ($Ok) { 'ok' } elseif (-not $Required) { 'optional-missing' } else { 'FAIL' }
    [void]$checks.Add([ordered]@{ name = $Name; found = $Found; minimum = $Min; required = $Required; status = $status })
    return $Ok
}

# Compares a captured version string against a manifest minimum; missing tool = fail (or optional).
function Test-ToolVersion {
    param([string]$Name, [string]$Found, [string]$Min, [bool]$Required = $true)
    if (-not $Found) { return (Add-Check -Name $Name -Found '(missing)' -Min $Min -Required $Required -Ok:$false) }
    $fv = Get-VersionTriple $Found
    $mv = Get-VersionTriple $Min
    $ok = if ($fv -and $mv) { $fv -ge $mv } else { $true }  # unparseable but present => reported, not failed
    return (Add-Check -Name $Name -Found $Found -Min $Min -Required $Required -Ok:$ok)
}

# --- capture --------------------------------------------------------------------------------------
$os = Get-CimInstance Win32_OperatingSystem -ErrorAction SilentlyContinue
$osCaption = if ($os) { $os.Caption } else { [System.Environment]::OSVersion.VersionString }
$osBuild = if ($os) { $os.BuildNumber } else { ([System.Environment]::OSVersion.Version.Build).ToString() }

$dotnetVersion = Get-CmdOutput 'dotnet' @('--version')
$dotnetSdks = Get-CmdOutput 'dotnet' @('--list-sdks')
$gitVersion = Get-CmdOutput 'git' @('--version')
$cmakeVersion = Get-CmdOutput 'cmake' @('--version')
$ninjaVersion = Get-CmdOutput 'ninja' @('--version')
$rustcVersion = Get-CmdOutput 'rustc' @('--version')
$cargoVersion = Get-CmdOutput 'cargo' @('--version')
$rustupToolchain = Get-CmdOutput 'rustup' @('show', 'active-toolchain')
$rustTargets = Get-CmdOutput 'rustup' @('target', 'list', '--installed')

# Visual Studio / MSVC via vswhere (products * so Build Tools counts, not only the IDE editions).
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$vsVersion = $null; $vsProduct = $null; $vsHasWorkloads = $false
if (Test-Path $vswhere) {
    $vsVersion = Get-CmdOutput $vswhere @('-products', '*', '-latest', '-prerelease', '-property', 'installationVersion')
    $vsProduct = Get-CmdOutput $vswhere @('-products', '*', '-latest', '-prerelease', '-property', 'displayName')
    $req = @($manifest.tools.visualStudio.components)
    $wl = & $vswhere -products '*' -latest -prerelease -requires @req -property installationPath 2>$null
    $vsHasWorkloads = [bool]$wl
}

# In-repo pinned versions (source of truth -- not duplicated in the manifest).
$packagesProps = Join-Path $repoRoot 'Directory.Packages.props'
$appCsproj = Join-Path $repoRoot 'src\OkPlayer.App\OkPlayer.App.csproj'
$winAppSdk = Get-PropsVersion -File $packagesProps -PackageId 'Microsoft.WindowsAppSDK'
$winSdkBuildTools = Get-PropsVersion -File $packagesProps -PackageId 'Microsoft.Windows.SDK.BuildTools'
$appTfm = Get-CsprojValue -File $appCsproj -Element 'TargetFramework'
$appMinPlatform = Get-CsprojValue -File $appCsproj -Element 'TargetPlatformMinVersion'

# libmpv native (resolved product version of the fetched DLL).
$libmpvDll = Join-Path $repoRoot 'native\libmpv\libmpv-2.dll'
$libmpvVersion = $null
if (Test-Path $libmpvDll) {
    $vi = (Get-Item $libmpvDll).VersionInfo
    $libmpvVersion = if ($vi.ProductVersion) { $vi.ProductVersion.Trim() } else { $vi.FileVersion }
}

# --- checks (required unless noted) ---------------------------------------------------------------
$okDotnet = Test-ToolVersion -Name '.NET SDK' -Found $dotnetVersion -Min $manifest.tools.dotnetSdk.minVersion
$okGit = Test-ToolVersion -Name 'Git' -Found $gitVersion -Min $manifest.tools.git.minVersion
$okRustc = Test-ToolVersion -Name 'Rust (rustc)' -Found $rustcVersion -Min $manifest.tools.rustup.minVersion
$targetName = $manifest.tools.rustup.target
$okTarget = Add-Check -Name "Rust target $targetName" -Found ($(if ($rustTargets -match [regex]::Escape($targetName)) { $targetName } else { '(missing)' })) -Min $targetName -Required $true -Ok:([bool]($rustTargets -match [regex]::Escape($targetName)))
$okVs = Add-Check -Name 'Visual Studio workloads' -Found ($(if ($vsHasWorkloads) { "$vsProduct $vsVersion" } else { '(workloads missing)' })) -Min $manifest.tools.visualStudio.minVersion -Required $true -Ok:$vsHasWorkloads
$okCmake = Test-ToolVersion -Name 'CMake' -Found $cmakeVersion -Min $manifest.tools.cmake.minVersion -Required:$false
$okNinja = Test-ToolVersion -Name 'Ninja' -Found $ninjaVersion -Min $manifest.tools.ninja.minVersion -Required:$false

# OS build floor: the toolchain can be fully installed on an older Windows build, but the VM must still
# meet the documented baseline (manifest os.minBuild, == the app's TargetPlatformMinVersion). Compare as
# integers -- the OS build is a single number, not a dotted version.
$minBuild = [int]$manifest.os.minBuild
$osBuildNum = 0
$osBuildOk = [int]::TryParse([string]$osBuild, [ref]$osBuildNum) -and ($osBuildNum -ge $minBuild)
$okOs = Add-Check -Name 'Windows OS build' -Found "$osCaption (build $osBuild)" -Min ([string]$minBuild) -Required $true -Ok:$osBuildOk

$requiredOk = $okOs -and $okDotnet -and $okGit -and $okRustc -and $okTarget -and $okVs

# --- assemble report ------------------------------------------------------------------------------
$overall = if ($requiredOk) { 'ok' } else { 'incomplete' }
$report = [ordered]@{
    schema           = $manifest.schema
    tool             = 'report-windows-env'
    overall          = $overall
    vmEnvelope       = $manifest.vmEnvelope
    os               = [ordered]@{ caption = $osCaption; build = $osBuild; minBuild = $manifest.os.minBuild }
    dotnet           = [ordered]@{ version = $dotnetVersion; sdks = ($dotnetSdks -split "`r?`n" | Where-Object { $_ }) }
    visualStudio     = [ordered]@{ product = $vsProduct; version = $vsVersion; workloadsPresent = $vsHasWorkloads; requiredComponents = @($manifest.tools.visualStudio.components) }
    windowsSdk       = [ordered]@{ buildToolsPackage = $winSdkBuildTools; appTargetFramework = $appTfm; appMinPlatform = $appMinPlatform }
    windowsAppSdk    = [ordered]@{ version = $winAppSdk; source = 'Directory.Packages.props' }
    rust             = [ordered]@{ rustc = $rustcVersion; cargo = $cargoVersion; activeToolchain = $rustupToolchain; installedTargets = ($rustTargets -split "`r?`n" | Where-Object { $_ }) }
    git              = [ordered]@{ version = $gitVersion }
    cmake            = [ordered]@{ version = $cmakeVersion; note = 'required only for source native builds' }
    ninja            = [ordered]@{ version = $ninjaVersion; note = 'required only for source native builds' }
    powershell       = [ordered]@{ version = $PSVersionTable.PSVersion.ToString(); edition = $PSVersionTable.PSEdition }
    libmpv           = [ordered]@{ version = $libmpvVersion; present = [bool]$libmpvVersion; source = 'scripts/fetch-natives.ps1' }
    checks           = @($checks)
}

$json = $report | ConvertTo-Json -Depth 6

# --- output ---------------------------------------------------------------------------------------
if ($OutFile -and -not $NoFile) {
    $dir = Split-Path -Parent $OutFile
    if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
    Set-Content -LiteralPath $OutFile -Value $json -Encoding UTF8
    Write-Host "Environment report written to $OutFile" -ForegroundColor Cyan
}

# Human summary to the host stream; JSON stays clean on the success/stdout path when no file is written.
Write-Host ''
Write-Host "OS            : $osCaption (build $osBuild)"
Write-Host ".NET SDK      : $dotnetVersion"
Write-Host "Visual Studio : $vsProduct $vsVersion (workloads: $vsHasWorkloads)"
Write-Host "Windows SDK   : app TFM $appTfm, BuildTools $winSdkBuildTools"
Write-Host "Windows AppSDK: $winAppSdk"
Write-Host "Rust          : $rustcVersion  [$rustupToolchain]"
Write-Host "Git           : $gitVersion"
Write-Host "libmpv        : $(if ($libmpvVersion) { $libmpvVersion } else { '(not fetched)' })"
Write-Host ''
foreach ($c in $checks) {
    $color = if ($c.status -eq 'ok') { 'Green' } elseif ($c.status -eq 'FAIL') { 'Red' } else { 'DarkGray' }
    Write-Host ("  [{0,-16}] {1}  (min {2})" -f $c.status, $c.name, $c.minimum) -ForegroundColor $color
}

if (-not ($OutFile -and -not $NoFile)) { Write-Output $json }

if ($FailOnMissing -and -not $requiredOk) { exit 1 }
exit 0
