#requires -Version 7
<#
.SYNOPSIS
  Build the OK Player Windows installer (.exe) from a self-contained Release publish.
.DESCRIPTION
  Publishes the app self-contained for win-x64, stages LICENSE.txt + THIRD-PARTY-NOTICES.md + README
  next to it, then invokes the Inno Setup compiler (ISCC) against installer\OkPlayer.iss. The result
  is artifacts\OkPlayer-Setup-v<Version>-win-x64.exe.
.PARAMETER Version
  Version string baked into the installer (default 0.2.0).
.EXAMPLE
  pwsh installer\build-installer.ps1 -Version 0.2.0
#>
param(
  [string]$Version = '0.2.0'
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$publish = Join-Path $repo 'artifacts\publish'
$artifacts = Join-Path $repo 'artifacts'

Write-Host "Publishing self-contained Release -> $publish"
if (Test-Path $publish) { Remove-Item $publish -Recurse -Force }
dotnet publish (Join-Path $repo 'src\OkPlayer.App\OkPlayer.App.csproj') -c Release -r win-x64 -o $publish
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
