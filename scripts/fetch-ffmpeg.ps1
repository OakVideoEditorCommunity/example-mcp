# SPDX-License-Identifier: GPL-3.0-or-later
#
# Download the pinned FFmpeg 8.1 Windows build (BtbN/FFmpeg-Builds) into
# <repo>\.cache\ffmpeg and export the environment cargo needs.
#
# Usage (from the repository root, PowerShell):
#   . .\scripts\fetch-ffmpeg.ps1          # dot-source so the env vars stick
#   cargo build --release
#
# Or just inspect the result:
#   .\scripts\fetch-ffmpeg.ps1
#
# The asset, checksum verification, extraction and prefix layout mirror
# quercus-engine's Windows CI.

[CmdletBinding()]
param(
    [string]$Prefix = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
# GitHub requires TLS 1.2; Windows PowerShell 5.1 may default to older TLS.
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

if ($env:OS -ne "Windows_NT") {
    throw "fetch-ffmpeg.ps1 is Windows-only. On Linux/macOS run ./scripts/ffmpeg/build-ffmpeg.sh."
}

$ScriptDir = $PSScriptRoot
if ([string]::IsNullOrEmpty($ScriptDir)) {
    $ScriptDir = Split-Path -Parent $PSCommandPath
}
if ([string]::IsNullOrEmpty($ScriptDir)) {
    throw "Cannot locate the scripts directory (run the script from a file, not a string)."
}

$Root = Split-Path -Parent $ScriptDir
$Cache = Join-Path $Root ".cache"
if ([string]::IsNullOrEmpty($Prefix)) {
    $Prefix = Join-Path $Cache "ffmpeg"
} elseif (-not [IO.Path]::IsPathRooted($Prefix)) {
    $Prefix = Join-Path $Root $Prefix
}

$CacheFull = [IO.Path]::GetFullPath($Cache).TrimEnd([char[]]@('\', '/'))
$Prefix = [IO.Path]::GetFullPath($Prefix)
$Separator = [IO.Path]::DirectorySeparatorChar
if (-not $Prefix.StartsWith($CacheFull + $Separator, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing to write outside '$Cache': $Prefix"
}

$Base = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest"
$Asset = "ffmpeg-n8.1-latest-win64-gpl-shared-8.1.zip"
$Download = Join-Path $env:TEMP "oak-mcp-ffmpeg"
$Checksums = Join-Path $Download "checksums.sha256"
$Zip = Join-Path $Download $Asset
$Extract = Join-Path $Cache "ffmpeg-extract"

New-Item -ItemType Directory -Force -Path $Cache, $Download | Out-Null

Write-Host ">> downloading $Asset"
Invoke-WebRequest -Uri "$Base/checksums.sha256" -OutFile $Checksums
$Line = Select-String -Path $Checksums -Pattern ([regex]::Escape($Asset) + "\s*$") |
    Select-Object -First 1
if (-not $Line) {
    throw "No checksum entry for $Asset in $Checksums"
}
$Expected = $Line.Line.Split()[0].ToLowerInvariant()

Invoke-WebRequest -Uri "$Base/$Asset" -OutFile $Zip
$Actual = (Get-FileHash -LiteralPath $Zip -Algorithm SHA256).Hash.ToLowerInvariant()
if ($Actual -ne $Expected) {
    throw "FFmpeg checksum mismatch: expected $Expected, got $Actual"
}

if (Test-Path -LiteralPath $Prefix) {
    Remove-Item -LiteralPath $Prefix -Recurse -Force
}
if (Test-Path -LiteralPath $Extract) {
    Remove-Item -LiteralPath $Extract -Recurse -Force
}

Write-Host ">> extracting to $Prefix"
Expand-Archive -Path $Zip -DestinationPath $Extract -Force
$Inner = Get-ChildItem -LiteralPath $Extract -Directory | Select-Object -First 1
if ($null -eq $Inner) {
    throw "Archive did not contain the expected top-level FFmpeg directory"
}
Move-Item -LiteralPath $Inner.FullName -Destination $Prefix

$env:FFMPEG_DIR = $Prefix
$env:PKG_CONFIG_PATH = Join-Path $Prefix "lib\pkgconfig"
$env:PATH = (Join-Path $Prefix "bin") + ";" + $env:PATH
$env:OCIO_RS_ENABLE_REAL = "1"
$env:OCIO_RS_LINK = "static"
$env:CC = "cl"
$env:CXX = "cl"

Write-Host ""
Write-Host "FFmpeg ready: $Prefix"
Write-Host "FFMPEG_DIR=$env:FFMPEG_DIR"
Write-Host "PKG_CONFIG_PATH=$env:PKG_CONFIG_PATH"
Write-Host ""
Write-Host "Next: cargo build --release"
if ($MyInvocation.InvocationName -ne ".") {
    Write-Host ""
    Write-Host "Note: this process only. To keep the variables in your shell, run:"
    Write-Host "  . .\scripts\fetch-ffmpeg.ps1"
}
