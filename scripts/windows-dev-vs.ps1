#requires -Version 5.1

function Get-VisualStudioWherePath {
    $path = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (Test-Path $path) { return $path }
    return $null
}

# Return the newest Visual Studio instance satisfying every supplied constraint. Keeping the workload
# and version filters in one vswhere query prevents separate installations from jointly passing a gate.
function Get-VisualStudioInstance {
    param(
        [Parameter(Mandatory)][string]$VsWherePath,
        [string[]]$RequiredComponents,
        [string]$MinimumVersion
    )

    $queryArgs = @('-products', '*', '-latest', '-prerelease')
    if ($MinimumVersion) { $queryArgs += @('-version', "[$MinimumVersion,)") }
    if ($RequiredComponents) {
        $queryArgs += '-requires'
        $queryArgs += @($RequiredComponents)
    }
    $queryArgs += @('-format', 'json')

    try { $json = (& $VsWherePath @queryArgs 2>$null | Out-String).Trim() }
    catch { return $null }
    if (-not $json) { return $null }

    try { $instances = @(ConvertFrom-Json $json) }
    catch { return $null }
    if ($instances.Count -eq 0) { return $null }
    return $instances[0]
}
