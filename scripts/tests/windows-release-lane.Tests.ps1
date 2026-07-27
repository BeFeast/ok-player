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

# Publication must be pinned to main: a dispatch from a feature branch with
# publish:true would ship unreviewed code to the stable win channel.
$refGuard = Get-Step -Yaml $releaseLane -Name 'Refuse to publish from a non-main ref'
if ($refGuard -notmatch [regex]::Escape("github.ref != 'refs/heads/main'")) { throw 'Ref guard does not compare against refs/heads/main' }
if ($refGuard -notmatch 'inputs\.publish\s*==\s*true') { throw 'Ref guard must only fire on publishing runs' }
if ($releaseLane.IndexOf('Refuse to publish from a non-main ref') -gt $releaseLane.IndexOf('actions/checkout')) { throw 'Ref guard must run before checkout' }
$publishIdx = $releaseLane.IndexOf('Publish the versioned release')
$tail = $releaseLane.Substring($publishIdx)
foreach ($forbidden in @('build-velopack.ps1', 'vpk pack', 'dotnet publish')) {
  if ($tail -match [regex]::Escape($forbidden)) { throw "Found '$forbidden' after the publish step - publish must upload the gated artifacts, never rebuild" }
}
$launchIdx = $releaseLane.IndexOf('Launch the installed application')
if ($launchIdx -lt 0 -or $launchIdx -gt $publishIdx) { throw 'Publish step must come after the launch gate' }
Write-Host 'ok: publish is opt-in, after the gates, with no rebuild after gating'

# Execute the Build step's actual run script against a stub build-velopack.ps1
# and verify the version and the publish-only DownloadPrior switch bind by
# name - the first live dispatch failed exactly here (array splatting bound
# the literal '-Version' as the version value).
$buildStep = Get-Step -Yaml $releaseLane -Name 'Build the Velopack installer'
$runIdx = $buildStep.IndexOf('run: |')
if ($runIdx -lt 0) { throw 'Build step has no multiline run script' }
$runScript = (($buildStep.Substring($runIdx + 6) -split "`n" | ForEach-Object { $_.Trim() }) | Where-Object { $_ }) -join "`n"

$sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("okp-release-lane-test-" + [guid]::NewGuid())
New-Item -ItemType Directory -Force -Path (Join-Path $sandbox 'installer') | Out-Null
@'
param([string]$Version, [switch]$Publish, [switch]$DownloadPrior)
"$Version|$($DownloadPrior.IsPresent)" | Set-Content -Path (Join-Path $PSScriptRoot '..' 'bound.txt')
'@ | Set-Content -Path (Join-Path $sandbox 'installer' 'build-velopack.ps1')

function Invoke-BuildStep([string]$Publish) {
  $script = $runScript.Replace('${{ inputs.version }}', '9.9.9-test.1').Replace('${{ inputs.publish }}', $Publish)
  Push-Location $sandbox
  try { Invoke-Expression $script } finally { Pop-Location }
  return (Get-Content (Join-Path $sandbox 'bound.txt')).Trim()
}

try {
  $bound = Invoke-BuildStep 'true'
  if ($bound -ne '9.9.9-test.1|True') { throw "publishing run bound '$bound', want '9.9.9-test.1|True'" }
  $bound = Invoke-BuildStep 'false'
  if ($bound -ne '9.9.9-test.1|False') { throw "gate-only run bound '$bound', want '9.9.9-test.1|False'" }
} finally {
  Remove-Item -Recurse -Force $sandbox -ErrorAction SilentlyContinue
}
Write-Host 'ok: build step binds Version by name and DownloadPrior only on publishing runs'

# Execute the publish step's feed-run discovery filter construction against
# real jq: inline quote-escaping once garbled this argument silently and the
# first publishing run failed AFTER a successful upload (run 30272655758).
$publishBody = Get-Step -Yaml $releaseLane -Name 'Publish the versioned release (tag v<version>, channel win)'
$filterLine = [regex]::Match($publishBody, '\$feedRunFilter = (.+)')
if (-not $filterLine.Success) { throw 'Publish step no longer builds $feedRunFilter' }
$dispatchedAfter = '2026-07-27T14:00:00.0000000Z'
$feedRunFilter = Invoke-Expression $filterLine.Groups[1].Value
$fixture = '[{"databaseId":11,"createdAt":"2026-07-27T13:59:59Z"},{"databaseId":22,"createdAt":"2026-07-27T14:01:00Z"}]'
$selected = ($fixture | jq $feedRunFilter) 2>&1
if ($LASTEXITCODE -ne 0) { throw "jq rejected the constructed discovery filter: $selected" }
if ("$selected" -ne '22') { throw "discovery filter selected '$selected' from the fixture, want the post-dispatch run 22" }
$none = ('[{"databaseId":11,"createdAt":"2026-07-27T13:59:59Z"}]' | jq $feedRunFilter)
if ("$none" -ne 'null') { throw "discovery filter matched a pre-dispatch run: $none" }
Write-Host 'ok: feed-run discovery filter parses and selects only runs created after the dispatch'
