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

    Existence alone is not enough. The same best-effort fetch path that can skip
    a download can also leave a truncated or empty file behind, and a zero-byte
    libmpv-2.dll satisfies any Test-Path check. Each required file therefore
    also carries a minimum size. These floors are a check against a broken
    fetch, not a version check: they sit orders of magnitude below the real
    artifacts, so they cannot start failing because a native was legitimately
    rebuilt smaller.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$InstallRoot
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# Relative paths that must exist under the installed root, with the reason each
# one can silently go missing and the floor its size must clear. Observed sizes
# on a real installed tree: OkPlayer.exe ~310 KB, libmpv-2.dll ~113 MB,
# ffmpeg.exe ~138 MB.
$required = @(
    @{
        Path     = 'OkPlayer.exe'
        MinBytes = 64KB
        Reason   = 'the application itself'
    },
    @{
        Path     = 'libmpv-2.dll'
        MinBytes = 1MB
        Reason   = 'the playback engine; bundled by a Condition="Exists(...)" Content item, so a publish that ran without native/libmpv/libmpv-2.dll ships an installer with no engine'
    },
    @{
        Path     = 'ffmpeg.exe'
        MinBytes = 1MB
        Reason   = 'media processing (subtitle auto-sync); fetch-natives.ps1 treats the ffmpeg download as best-effort, so a failed fetch ships an installer without it'
    }
)

if (-not (Test-Path -LiteralPath $InstallRoot -PathType Container)) {
    throw "Installed tree not found: $InstallRoot"
}

$problems = New-Object System.Collections.Generic.List[string]
foreach ($entry in $required) {
    $path = Join-Path -Path $InstallRoot -ChildPath $entry.Path
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        $problems.Add("$($entry.Path) - missing - $($entry.Reason)")
        continue
    }

    $size = (Get-Item -LiteralPath $path).Length
    if ($size -lt $entry.MinBytes) {
        $problems.Add(
            "$($entry.Path) - truncated: $size bytes, below the $($entry.MinBytes) byte floor - $($entry.Reason)")
        continue
    }

    Write-Host "present: $($entry.Path) ($size bytes, floor $($entry.MinBytes))"
}

if ($problems.Count -gt 0) {
    $detail = ($problems | ForEach-Object { "  - $_" }) -join [Environment]::NewLine
    throw ("The installed tree at $InstallRoot is incomplete; $($problems.Count) required file(s) missing or truncated:" +
        [Environment]::NewLine + $detail)
}

Write-Host "Installed tree is complete: $($required.Count) required file(s) present and above their size floors under $InstallRoot"
