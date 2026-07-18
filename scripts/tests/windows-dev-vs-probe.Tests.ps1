#requires -Version 5.1

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$scriptsRoot = Split-Path -Parent (Split-Path -Parent $PSCommandPath)
. (Join-Path $scriptsRoot 'windows-dev-vs.ps1')

function Invoke-MixedVisualStudioProbe {
    $query = @($args)
    $hasRequirements = $query -contains '-requires'
    $versionIndex = [array]::IndexOf($query, '-version')
    $hasVersionFloor = $versionIndex -ge 0
    if ($hasVersionFloor -and $query[$versionIndex + 1] -ne '[17.12.0,)') {
        throw "Unexpected version range: $($query[$versionIndex + 1])"
    }
    if ($hasRequirements -and $hasVersionFloor) {
        # Mixed installation state: the workload-bearing instance is below the requested version floor.
        '[]'
    } elseif ($hasRequirements) {
        '[{"displayName":"Visual Studio Build Tools 2022","installationVersion":"17.11.0"}]'
    } else {
        '[{"displayName":"Visual Studio Community 2022","installationVersion":"17.12.0"}]'
    }
}

$required = @('ManagedDesktopBuildTools', 'VCTools')
$qualified = Get-VisualStudioInstance -VsWherePath 'Invoke-MixedVisualStudioProbe' -RequiredComponents $required -MinimumVersion '17.12.0'
if ($null -ne $qualified) {
    throw 'A newer incomplete instance and an older workload-bearing instance must not jointly satisfy the Visual Studio gate.'
}

$workloadInstance = Get-VisualStudioInstance -VsWherePath 'Invoke-MixedVisualStudioProbe' -RequiredComponents $required
if (-not $workloadInstance -or $workloadInstance.installationVersion -ne '17.11.0') {
    throw 'The workload-qualified probe did not return the expected older instance.'
}

$latestInstance = Get-VisualStudioInstance -VsWherePath 'Invoke-MixedVisualStudioProbe'
if (-not $latestInstance -or $latestInstance.installationVersion -ne '17.12.0') {
    throw 'The unconstrained diagnostic probe did not return the expected newer instance.'
}

Write-Host 'Visual Studio probe correlation regression passed.'

$bootstrapPath = Join-Path $scriptsRoot 'bootstrap-windows-dev-vm.ps1'
$tokens = $null
$parseErrors = $null
$bootstrapAst = [System.Management.Automation.Language.Parser]::ParseFile(
    $bootstrapPath,
    [ref]$tokens,
    [ref]$parseErrors
)
if ($parseErrors) { throw 'Bootstrap function extraction failed because the script did not parse.' }
$velopackFunction = $bootstrapAst.Find({
        param($node)
        $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -eq 'Install-VelopackCli'
    }, $true)
if (-not $velopackFunction) { throw 'Install-VelopackCli was not found in the bootstrap script.' }
. ([scriptblock]::Create($velopackFunction.Extent.Text))
$wingetFunction = $bootstrapAst.Find({
        param($node)
        $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -eq 'Install-WingetPackage'
    }, $true)
if (-not $wingetFunction) { throw 'Install-WingetPackage was not found in the bootstrap script.' }
. ([scriptblock]::Create($wingetFunction.Extent.Text))

function Write-Step { param([string]$Message) }
function Write-Ok { param([string]$Message) }
function Write-Skip { param([string]$Message) }

$script:wingetCalls = @()
function winget {
    $query = @($args)
    $script:wingetCalls += ,$query
    if ($query[0] -eq 'list') {
        $global:LASTEXITCODE = 0
        return '7zip.7zip 23.01'
    }
    $global:LASTEXITCODE = 0
}

$CheckOnly = $false
Install-WingetPackage -Id '7zip.7zip' -Name '7-Zip' -InstalledVersion '7-Zip 23.01' -MinimumVersion '24.0.0'
$actions = @($script:wingetCalls | Where-Object { $_[0] -ne 'list' })
if ($actions.Count -ne 1 -or $actions[0][0] -ne 'upgrade' -or $actions[0][2] -ne '7zip.7zip') {
    throw 'An installed 7-Zip below the manifest floor must trigger one exact winget upgrade.'
}

$script:wingetCalls = @()
Install-WingetPackage -Id '7zip.7zip' -Name '7-Zip' -InstalledVersion '7-Zip 24.09' -MinimumVersion '24.0.0'
$actions = @($script:wingetCalls | Where-Object { $_[0] -ne 'list' })
if ($actions.Count -ne 0) {
    throw 'A satisfying 7-Zip version must remain idempotent and perform no winget action.'
}

$script:wingetCalls = @()
$CheckOnly = $true
$checkOnlyOutput = (& {
        Install-WingetPackage -Id '7zip.7zip' -Name '7-Zip' -InstalledVersion '7-Zip 23.01' -MinimumVersion '24.0.0'
    } 6>&1 | Out-String)
$actions = @($script:wingetCalls | Where-Object { $_[0] -ne 'list' })
if ($actions.Count -ne 0 -or $checkOnlyOutput -notmatch 'would update 7-Zip') {
    throw 'Check-only must report the below-floor 7-Zip update without invoking winget.'
}

Remove-Item -Path Function:\winget
Write-Host '7-Zip version convergence regressions passed.'

$repoRoot = Split-Path -Parent $scriptsRoot
$CheckOnly = $true
$originalPath = $env:Path
$originalProgramFiles = $env:ProgramFiles
$emptyTools = Join-Path ([System.IO.Path]::GetTempPath()) ("ok-player-empty-tools-{0}" -f [guid]::NewGuid())
New-Item -ItemType Directory -Path $emptyTools | Out-Null
try {
    $env:Path = $emptyTools
    $env:ProgramFiles = $emptyTools
    Install-VelopackCli
} finally {
    $env:Path = $originalPath
    $env:ProgramFiles = $originalProgramFiles
    Remove-Item -LiteralPath $emptyTools -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host 'Velopack check-only missing-dotnet regression passed.'
