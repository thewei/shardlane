#
# [INPUT]: cargo build 产物（target/<triple>/release/shardlane.exe）、仓库 LICENSE、Compress-Archive
# [OUTPUT]: dist/Shardlane-windows-x86_64.zip + .sha256（exe + README + LICENSE）
# [POS]: scripts 的 Windows 发布打包入口，release.yml windows-x86_64 作业调用
# [PROTOCOL]: Update scripts/CLAUDE.md on change, then check /CLAUDE.md.
param(
    [string]$Target = "x86_64-pc-windows-msvc"
)
$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
$Bin = Join-Path $RepoRoot "target\$Target\release\shardlane.exe"
if (-not (Test-Path $Bin)) { throw "missing release binary: $Bin" }

$Stage = Join-Path ([System.IO.Path]::GetTempPath()) ("shardlane-win-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $Stage | Out-Null
try {
    $Dir = Join-Path $Stage "Shardlane-windows-x86_64"
    New-Item -ItemType Directory -Path $Dir | Out-Null
    Copy-Item $Bin (Join-Path $Dir "shardlane.exe")
    Copy-Item (Join-Path $RepoRoot "LICENSE") (Join-Path $Dir "LICENSE")
    @"
Shardlane (Windows x86_64)
==========================

Run:
  shardlane.exe

Runtime requirements:
  - Herdr runtime (https://herdr.dev), installed natively with PowerShell:
      irm https://herdr.dev/install.ps1 | iex
    Shardlane discovers the herdr CLI on PATH and drives the local runtime
    through Herdr's named-pipe socket.

Note:
  - The zip is not signed; SmartScreen may ask for confirmation on first run.
"@ | Out-File -FilePath (Join-Path $Dir "README-windows.txt") -Encoding utf8

    $Dist = Join-Path $RepoRoot "dist"
    New-Item -ItemType Directory -Force -Path $Dist | Out-Null
    $Zip = Join-Path $Dist "Shardlane-windows-x86_64.zip"
    if (Test-Path $Zip) { Remove-Item $Zip }
    Compress-Archive -Path $Dir -DestinationPath $Zip
    $hash = (Get-FileHash -Algorithm SHA256 $Zip).Hash.ToLower()
    $hash + "  " + (Split-Path -Leaf $Zip) | Out-File -FilePath ($Zip + ".sha256") -Encoding ascii
    Write-Host "packaged: $Zip"
} finally {
    Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
}
