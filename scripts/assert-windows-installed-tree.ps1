#requires -Version 5.1
<#
.SYNOPSIS
    Assert that an installed OK Player tree carries everything the app needs.

.DESCRIPTION
    The Windows installer can be built, installed, and started successfully
    while shipping without its playback engine or without ffmpeg, because both
    natives are bundled through Content items guarded by Condition="Exists(...)"
    in src/OkPlayer.App/OkPlayer.App.csproj, and scripts/fetch-natives.ps1
    treats the ffmpeg download as best-effort (a failed fetch only warns).
    Neither omission is visible from a successful publish, a successful pack, or
    a window appearing on screen, so the installed tree is asserted directly.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$InstallRoot
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# Relative paths that must exist under the installed root, with the reason each
# one can silently go missing.
$required = @(
    @{
        Path   = 'OkPlayer.exe'
        Reason = 'the application itself'
    },
    @{
        Path   = 'libmpv-2.dll'
        Reason = 'the playback engine; bundled by a Condition="Exists(...)" Content item, so a publish that ran without native/libmpv/libmpv-2.dll ships an installer with no engine'
    },
    @{
        Path   = 'ffmpeg.exe'
        Reason = 'media processing (subtitle auto-sync); fetch-natives.ps1 treats the ffmpeg download as best-effort, so a failed fetch ships an installer without it'
    }
)

if (-not (Test-Path -LiteralPath $InstallRoot -PathType Container)) {
    throw "Installed tree not found: $InstallRoot"
}

$missing = New-Object System.Collections.Generic.List[string]
foreach ($entry in $required) {
    $path = Join-Path -Path $InstallRoot -ChildPath $entry.Path
    if (Test-Path -LiteralPath $path -PathType Leaf) {
        $size = (Get-Item -LiteralPath $path).Length
        Write-Host "present: $($entry.Path) ($size bytes)"
    }
    else {
        $missing.Add("$($entry.Path) - $($entry.Reason)")
    }
}

if ($missing.Count -gt 0) {
    $detail = ($missing | ForEach-Object { "  - $_" }) -join [Environment]::NewLine
    throw ("The installed tree at $InstallRoot is incomplete; $($missing.Count) required file(s) missing:" +
        [Environment]::NewLine + $detail)
}

Write-Host "Installed tree is complete: $($required.Count) required file(s) present under $InstallRoot"
