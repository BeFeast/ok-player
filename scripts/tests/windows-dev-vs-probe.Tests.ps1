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
