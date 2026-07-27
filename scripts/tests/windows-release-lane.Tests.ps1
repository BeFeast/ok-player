#requires -Version 7
<#
Behavioural guard for the versioned Windows release lane (release-windows.yml).

The release lane's install/launch gates were copied from the Windows Installer
lane (windows-package.yml). Copies drift: someone hardens the installer lane's
launch gate and the release lane silently keeps shipping through the weaker
copy. This test extracts each shared gate step from both workflows and fails
on any difference, and asserts the release lane's structural safety rules:
dispatch-only triggering, publish gated on the input, and no build or pack
step after the gates (publish must upload the exact gated artifacts).
#>
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$installerLane = Get-Content -Raw (Join-Path $root '.github/workflows/windows-package.yml')
$releaseLane = Get-Content -Raw (Join-Path $root '.github/workflows/release-windows.yml')

function Get-Step {
  param([string]$Yaml, [string]$Name)
  # A step body runs from its "- name:" line to the next "- name:" at the same
  # list level (6 spaces in both these files) or end of file.
  $pattern = "(?ms)^      - name: $([regex]::Escape($Name))\r?\n(.*?)(?=^      - name: |\z)"
  $m = [regex]::Match($Yaml, $pattern)
  if (-not $m.Success) { throw "Step '$Name' not found" }
  # Normalize line endings and trailing whitespace so only content differences
  # count, then drop trailing comment/blank lines - they document the NEXT
  # step and legitimately differ between the two lanes.
  $lines = [System.Collections.Generic.List[string]](($m.Groups[1].Value -split "\r?\n" | ForEach-Object { $_.TrimEnd() }))
  while ($lines.Count -gt 0 -and ($lines[-1] -eq '' -or $lines[-1].TrimStart().StartsWith('#'))) {
    $lines.RemoveAt($lines.Count - 1)
  }
  return ($lines -join "`n").Trim()
}

$sharedGates = @(
  'Assert the shipping installer artifacts exist',
  'Install the packaged application',
  'Assert the installed tree is complete',
  'Assert the bundled ffmpeg runs',
  'Arm crash-dump collection for the installed app',
  'Launch the installed application',
  "Collect the installed app's own diagnostics"
)

foreach ($gate in $sharedGates) {
  $a = Get-Step -Yaml $installerLane -Name $gate
  $b = Get-Step -Yaml $releaseLane -Name $gate
  if ($a -ne $b) {
    Write-Host "--- installer lane ---`n$a`n--- release lane ---`n$b"
    throw "Gate step '$gate' has drifted between windows-package.yml and release-windows.yml"
  }
}
Write-Host "ok: all $($sharedGates.Count) shared gate steps are identical across the two lanes"

# The release lane must be dispatch-only: a push- or PR-triggered release lane
# could publish from unreviewed refs.
$onBlock = [regex]::Match($releaseLane, '(?ms)^on:\r?\n(.*?)(?=^\S)').Groups[1].Value
foreach ($trigger in @('push:', 'pull_request:', 'schedule:')) {
  if ($onBlock -match [regex]::Escape($trigger)) { throw "release-windows.yml must not trigger on $trigger" }
}
if ($onBlock -notmatch 'workflow_dispatch:') { throw 'release-windows.yml lost its workflow_dispatch trigger' }
Write-Host 'ok: release lane is dispatch-only'

# Publish must be opt-in and must come after every gate with no rebuild or
# repack in between - the uploaded bytes must be the gated bytes.
$publishStep = Get-Step -Yaml $releaseLane -Name 'Publish the versioned release (tag v<version>, channel win)'
if ($publishStep -notmatch 'if:\s*inputs\.publish\s*==\s*true') { throw 'Publish step is not gated on inputs.publish' }
$publishIdx = $releaseLane.IndexOf('Publish the versioned release')
$tail = $releaseLane.Substring($publishIdx)
foreach ($forbidden in @('build-velopack.ps1', 'vpk pack', 'dotnet publish')) {
  if ($tail -match [regex]::Escape($forbidden)) { throw "Found '$forbidden' after the publish step - publish must upload the gated artifacts, never rebuild" }
}
$launchIdx = $releaseLane.IndexOf('Launch the installed application')
if ($launchIdx -lt 0 -or $launchIdx -gt $publishIdx) { throw 'Publish step must come after the launch gate' }
Write-Host 'ok: publish is opt-in, after the gates, with no rebuild after gating'
