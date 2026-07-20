#requires -Version 7
<#
.SYNOPSIS
  Build one local Velopack package set for the rolling Windows candidate lane.
.DESCRIPTION
  This entry point never uploads or creates a release. It publishes the WinUI
  app from the caller's clean checkout, stamps the candidate update lane, and
  packs the distinct win-candidate channel for later identity verification and
  feed-last promotion by release-windows-candidate.yml.
#>
param(
  [Parameter(Mandatory = $true)]
  [string]$Version,
  [string]$OutputDirectory = 'artifacts\windows-candidate'
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$publishDir = Join-Path $repo 'artifacts\windows-candidate-publish'
$releases = [System.IO.Path]::GetFullPath((Join-Path $repo $OutputDirectory))
$appProj = Join-Path $repo 'src\OkPlayer.App\OkPlayer.App.csproj'
$icon = Join-Path $repo 'src\OkPlayer.App\Assets\OkPlayer.ico'

$vpk = (Get-Command vpk -ErrorAction SilentlyContinue)?.Source
if (-not $vpk) { $vpk = Join-Path $env:USERPROFILE '.dotnet\tools\vpk.exe' }
if (-not (Test-Path $vpk)) { throw 'vpk 1.2.0 is required.' }

if (Test-Path $publishDir) { Remove-Item $publishDir -Recurse -Force }
if (Test-Path $releases) { Remove-Item $releases -Recurse -Force }
New-Item -ItemType Directory -Force -Path $publishDir, $releases | Out-Null

$publishArgs = @(
  'publish', $appProj,
  '-c', 'Release',
  '-r', 'win-x64',
  '-o', $publishDir,
  "-p:Version=$Version",
  '-p:OkPlayerWindowsCandidate=true'
)
dotnet @publishArgs
if ($LASTEXITCODE -ne 0) { throw "dotnet publish failed ($LASTEXITCODE)" }

Copy-Item (Join-Path $repo 'LICENSE') (Join-Path $publishDir 'LICENSE.txt') -Force
Copy-Item (Join-Path $repo 'THIRD-PARTY-NOTICES.md') $publishDir -Force
Copy-Item (Join-Path $repo 'README.md') $publishDir -Force

$packArgs = @(
  'pack',
  '--packId', 'com.befeast.okplayer',
  '--packTitle', 'OK Player Candidate',
  '--packAuthors', 'BeFeast',
  '--packVersion', $Version,
  '--packDir', $publishDir,
  '--mainExe', 'OkPlayer.exe',
  '--icon', $icon,
  '--channel', 'win-candidate',
  '--outputDir', $releases
)
& $vpk @packArgs
if ($LASTEXITCODE -ne 0) { throw "vpk pack failed ($LASTEXITCODE)" }

$feed = Join-Path $releases 'releases.win-candidate.json'
if (-not (Test-Path $feed)) { throw 'Velopack did not produce releases.win-candidate.json.' }
Write-Host "Windows candidate package set written to $releases"
