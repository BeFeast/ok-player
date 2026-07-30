#requires -Version 5.1
# Policy tests for scripts/assert-windows-installed-tree.ps1.
#
# The Windows installer lane can only prove a complete shipping tree while this
# assertion names every file that can silently go missing AND refuses a file
# that is present but empty or truncated. These tests build synthetic install
# roots on disk, so they run anywhere pwsh runs - no installer, no Windows, no
# publish.

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$scriptsRoot = Split-Path -Parent (Split-Path -Parent $PSCommandPath)
$assert = Join-Path -Path $scriptsRoot -ChildPath 'assert-windows-installed-tree.ps1'

$work = Join-Path -Path ([System.IO.Path]::GetTempPath()) -ChildPath ("okp-tree-tests-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $work -Force | Out-Null

# Sizes a real installed tree comfortably exceeds, used for the "healthy" case.
# Written with SetLength so the tests stay cheap on disk and fast.
$plausibleSizes = @{
    'OkPlayer.exe'             = 256KB
    'libmpv-2.dll'             = 4MB
    'ffmpeg.exe'               = 4MB
    'LICENSE.txt'              = 32KB
    'LICENSE.LGPL-3.0.txt'     = 7KB
    'THIRD-PARTY-NOTICES.md'   = 4KB
}
$allFiles = @(
    'OkPlayer.exe',
    'libmpv-2.dll',
    'ffmpeg.exe',
    'LICENSE.txt',
    'LICENSE.LGPL-3.0.txt',
    'THIRD-PARTY-NOTICES.md'
)
$failures = 0

function New-SizedFile {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][long]$Bytes
    )
    $stream = [System.IO.File]::Create($Path)
    try { $stream.SetLength($Bytes) }
    finally { $stream.Dispose() }
}

function New-InstallRoot {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [string[]]$Omit = @(),
        [hashtable]$Truncate = @{}
    )
    $root = Join-Path -Path $work -ChildPath $Name
    New-Item -ItemType Directory -Path $root -Force | Out-Null
    foreach ($file in $script:allFiles) {
        if ($Omit -contains $file) { continue }
        $bytes = $script:plausibleSizes[$file]
        if ($Truncate.ContainsKey($file)) { $bytes = $Truncate[$file] }
        New-SizedFile -Path (Join-Path -Path $root -ChildPath $file) -Bytes $bytes
    }
    # Files the installer legitimately places next to the app must not be
    # mistaken for the required ones.
    New-SizedFile -Path (Join-Path -Path $root -ChildPath 'OkPlayer.dll') -Bytes 64KB
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
        [Parameter(Mandatory = $true)][string[]]$Expected
    )
    $message = Invoke-Assert -Root $Root
    if (-not $message) {
        Write-Host "FAIL: $Label - the assertion accepted the tree"
        $script:failures++
        return
    }
    foreach ($needle in $Expected) {
        if ($message -notlike "*$needle*") {
            Write-Host "FAIL: $Label - message did not mention '$needle': $message"
            $script:failures++
            return
        }
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
        -Expected @('libmpv-2.dll', 'missing')
    Assert-Rejects -Label 'a tree without ffmpeg is rejected' `
        -Root (New-InstallRoot -Name 'no-ffmpeg' -Omit @('ffmpeg.exe')) `
        -Expected @('ffmpeg.exe', 'missing')
    Assert-Rejects -Label 'a tree without the application is rejected' `
        -Root (New-InstallRoot -Name 'no-exe' -Omit @('OkPlayer.exe')) `
        -Expected @('OkPlayer.exe', 'missing')

    # Issue #743. The installer can be built, signed and run with no licence
    # document in it, and nothing about the running app says so. GPLv3 section 4
    # and LGPLv3 section 4(b) both require these to accompany the package, so
    # the assertion has to name them the same way it names the natives.
    Assert-Rejects -Label 'a tree without the GPL text is rejected' `
        -Root (New-InstallRoot -Name 'no-gpl' -Omit @('LICENSE.txt')) `
        -Expected @('LICENSE.txt', 'missing')
    Assert-Rejects -Label 'a tree without the LGPL text is rejected' `
        -Root (New-InstallRoot -Name 'no-lgpl' -Omit @('LICENSE.LGPL-3.0.txt')) `
        -Expected @('LICENSE.LGPL-3.0.txt', 'missing')
    Assert-Rejects -Label 'a tree without the third-party notices is rejected' `
        -Root (New-InstallRoot -Name 'no-notices' -Omit @('THIRD-PARTY-NOTICES.md')) `
        -Expected @('THIRD-PARTY-NOTICES.md', 'missing')
    Assert-Rejects -Label 'a zero-byte LGPL text is rejected' `
        -Root (New-InstallRoot -Name 'empty-lgpl' -Truncate @{ 'LICENSE.LGPL-3.0.txt' = 0 }) `
        -Expected @('LICENSE.LGPL-3.0.txt', 'truncated')

    # Present but empty or truncated. A failed or interrupted best-effort fetch
    # leaves exactly this behind, and an existence-only assertion accepts it.
    Assert-Rejects -Label 'a zero-byte playback engine is rejected' `
        -Root (New-InstallRoot -Name 'empty-libmpv' -Truncate @{ 'libmpv-2.dll' = 0 }) `
        -Expected @('libmpv-2.dll', 'truncated', '0 bytes')
    Assert-Rejects -Label 'a zero-byte ffmpeg is rejected' `
        -Root (New-InstallRoot -Name 'empty-ffmpeg' -Truncate @{ 'ffmpeg.exe' = 0 }) `
        -Expected @('ffmpeg.exe', 'truncated')
    Assert-Rejects -Label 'a zero-byte application is rejected' `
        -Root (New-InstallRoot -Name 'empty-exe' -Truncate @{ 'OkPlayer.exe' = 0 }) `
        -Expected @('OkPlayer.exe', 'truncated')
    Assert-Rejects -Label 'a partially downloaded native is rejected' `
        -Root (New-InstallRoot -Name 'partial-libmpv' -Truncate @{ 'libmpv-2.dll' = 128KB }) `
        -Expected @('libmpv-2.dll', 'truncated')

    # Every problem is reported at once, not just the first, and missing and
    # truncated files are reported together.
    $mixed = New-InstallRoot -Name 'mixed' -Omit @('ffmpeg.exe') -Truncate @{ 'libmpv-2.dll' = 0 }
    $message = Invoke-Assert -Root $mixed
    if (-not $message) {
        Write-Host 'FAIL: a tree with a missing and a truncated file was accepted'
        $failures++
    }
    else {
        foreach ($needle in @('ffmpeg.exe', 'libmpv-2.dll', '2 required file(s)')) {
            if ($message -notlike "*$needle*") {
                Write-Host "FAIL: the combined report did not mention $needle"
                $failures++
            }
        }
        Write-Host 'ok: missing and truncated files are reported together'
    }

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
