#requires -Version 7
<#
.SYNOPSIS
  Build the OK Player Windows installer (.exe) from a self-contained Release publish.
.DESCRIPTION
  Publishes the app self-contained for win-x64, stages LICENSE.txt + THIRD-PARTY-NOTICES.md + README
  next to it, then invokes the Inno Setup compiler (ISCC) against installer\OkPlayer.iss. The result
  is artifacts\OkPlayer-Setup-v<Version>-win-x64.exe.
.PARAMETER Version
  Version baked into the installer AND the published assembly. Optional — defaults to the <Version>
  in src\OkPlayer.App\OkPlayer.App.csproj (the single source of truth, also shown in the app's
  Settings -> About). Pass -Version only to override for a one-off build.
.EXAMPLE
  .\installer\build-installer.ps1                  # uses the csproj <Version>
  .\installer\build-installer.ps1 -Version 0.8.0   # override
#>
param(
  [string]$Version
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$publish = Join-Path $repo 'artifacts\publish'
$artifacts = Join-Path $repo 'artifacts'
$appProj = Join-Path $repo 'src\OkPlayer.App\OkPlayer.App.csproj'

# Single source of truth: the app version lives in the csproj <Version>. Read it unless -Version overrides,
# so the installer, the release tag, and the in-app "About" can never drift apart.
if (-not $Version) {
  $m = Select-String -Path $appProj -Pattern '<Version>\s*([^<\s]+)\s*</Version>' | Select-Object -First 1
  if (-not $m) { throw "No <Version> in $appProj and no -Version passed." }
  $Version = $m.Matches[0].Groups[1].Value
}
Write-Host "Version: $Version"

Write-Host "Publishing self-contained Release -> $publish"
if (Test-Path $publish) { Remove-Item $publish -Recurse -Force }
dotnet publish $appProj -c Release -r win-x64 -o $publish -p:Version=$Version
if ($LASTEXITCODE -ne 0) { throw "dotnet publish failed ($LASTEXITCODE)" }

Copy-Item (Join-Path $repo 'LICENSE') (Join-Path $publish 'LICENSE.txt') -Force
Copy-Item (Join-Path $repo 'THIRD-PARTY-NOTICES.md') $publish -Force
if (Test-Path (Join-Path $repo 'README.md')) { Copy-Item (Join-Path $repo 'README.md') $publish -Force }

$iscc = Get-Command ISCC.exe -ErrorAction SilentlyContinue
if (-not $iscc) {
  $iscc = Get-ChildItem 'C:\Program Files (x86)\Inno Setup 6\ISCC.exe',
                        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe" -ErrorAction SilentlyContinue |
          Select-Object -First 1
}
if (-not $iscc) { throw "ISCC.exe (Inno Setup 6) not found. Install it: winget install JRSoftware.InnoSetup" }
$isccPath = if ($iscc -is [System.Management.Automation.CommandInfo]) { $iscc.Source } else { $iscc.FullName }

Write-Host "Compiling installer with $isccPath"
& $isccPath "/DSourceDir=$publish" "/DAppVersion=$Version" "/DRepoRoot=$repo" "/O$artifacts" (Join-Path $PSScriptRoot 'OkPlayer.iss')
if ($LASTEXITCODE -ne 0) { throw "ISCC failed ($LASTEXITCODE)" }

$setup = Join-Path $artifacts "OkPlayer-Setup-v$Version-win-x64.exe"
Write-Host "Installer built: $setup ($([int]((Get-Item $setup).Length/1MB)) MB)"
