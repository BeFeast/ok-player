#requires -Version 5.1
# Policy tests for scripts/assert-windows-installed-tree.ps1.
#
# The Windows installer lane can only prove a complete shipping tree while this
# assertion actually names every file that can silently go missing. These tests
# build synthetic install roots on disk, so they run anywhere pwsh runs - no
# installer, no Windows, no publish.

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$scriptsRoot = Split-Path -Parent (Split-Path -Parent $PSCommandPath)
$assert = Join-Path -Path $scriptsRoot -ChildPath 'assert-windows-installed-tree.ps1'

$work = Join-Path -Path ([System.IO.Path]::GetTempPath()) -ChildPath ("okp-tree-tests-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $work -Force | Out-Null

$allFiles = @('OkPlayer.exe', 'libmpv-2.dll', 'ffmpeg.exe')
$failures = 0

function New-InstallRoot {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [string[]]$Omit = @()
    )
    $root = Join-Path -Path $work -ChildPath $Name
    New-Item -ItemType Directory -Path $root -Force | Out-Null
    foreach ($file in $script:allFiles) {
        if ($Omit -contains $file) { continue }
        Set-Content -LiteralPath (Join-Path -Path $root -ChildPath $file) -Value 'x' -NoNewline
    }
    # Files the installer legitimately places next to the app must not be
    # mistaken for the required ones.
    Set-Content -LiteralPath (Join-Path -Path $root -ChildPath 'OkPlayer.dll') -Value 'x' -NoNewline
    return $root
}

function Invoke-Assert {
    param([Parameter(Mandatory = $true)][string]$Root)
    try {
        & $script:assert -InstallRoot $Root | Out-Null
        return $null
    }
    catch {
        return $_.Exception.Message
    }
}

function Assert-Rejects {
    param(
        [Parameter(Mandatory = $true)][string]$Label,
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$Expected
    )
    $message = Invoke-Assert -Root $Root
    if (-not $message) {
        Write-Host "FAIL: $Label - the assertion accepted the tree"
        $script:failures++
        return
    }
    if ($message -notlike "*$Expected*") {
        Write-Host "FAIL: $Label - message did not mention '$Expected': $message"
        $script:failures++
        return
    }
    Write-Host "ok: $Label"
}

try {
    # A complete tree is accepted.
    $complete = New-InstallRoot -Name 'complete'
    $message = Invoke-Assert -Root $complete
    if ($message) {
        Write-Host "FAIL: a complete installed tree was rejected: $message"
        $failures++
    }
    else {
        Write-Host 'ok: a complete installed tree is accepted'
    }

    # Each individually droppable file must be named by the assertion. Without
    # its entry in the required list, these cases pass and the lane ships a
    # green installer with no engine or no ffmpeg.
    Assert-Rejects -Label 'a tree without the playback engine is rejected' `
        -Root (New-InstallRoot -Name 'no-libmpv' -Omit @('libmpv-2.dll')) `
        -Expected 'libmpv-2.dll'
    Assert-Rejects -Label 'a tree without ffmpeg is rejected' `
        -Root (New-InstallRoot -Name 'no-ffmpeg' -Omit @('ffmpeg.exe')) `
        -Expected 'ffmpeg.exe'
    Assert-Rejects -Label 'a tree without the application is rejected' `
        -Root (New-InstallRoot -Name 'no-exe' -Omit @('OkPlayer.exe')) `
        -Expected 'OkPlayer.exe'

    # Every missing file is reported at once, not just the first.
    $none = New-InstallRoot -Name 'nothing' -Omit $allFiles
    $message = Invoke-Assert -Root $none
    if (-not $message) {
        Write-Host 'FAIL: an empty installed tree was accepted'
        $failures++
    }
    else {
        foreach ($file in $allFiles) {
            if ($message -notlike "*$file*") {
                Write-Host "FAIL: an empty installed tree did not report $file"
                $failures++
            }
        }
        Write-Host 'ok: an empty installed tree reports every missing file'
    }

    # A missing install root is a failure, not a vacuous pass.
    $absent = Join-Path -Path $work -ChildPath 'does-not-exist'
    $message = Invoke-Assert -Root $absent
    if (-not $message) {
        Write-Host 'FAIL: a nonexistent install root was accepted'
        $failures++
    }
    else {
        Write-Host 'ok: a nonexistent install root is rejected'
    }
}
finally {
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}

if ($failures -gt 0) {
    throw "$failures installed-tree assertion policy test(s) failed."
}
Write-Host 'Windows installed-tree assertion policy tests passed.'
