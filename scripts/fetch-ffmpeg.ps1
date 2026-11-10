# SPDX-License-Identifier: GPL-3.0-or-later
#
# Download the pinned FFmpeg 9.0 Windows build into <repo>\.cache\ffmpeg and
# export the environment cargo needs.
#
# Usage (from the repository root, PowerShell):
#   . .\scripts\fetch-ffmpeg.ps1          # dot-source so the env vars stick
#   . .\scripts\fetch-ffmpeg.ps1 -Source AtomGit -AllowUnverified
#   . .\scripts\fetch-ffmpeg.ps1 -Source Cloudflare -AllowUnverified
#
# The GitHub source mirrors quercus-engine's Windows CI (BtbN/FFmpeg-Builds)
# and verifies checksums.sha256. AtomGit and Cloudflare are fallbacks for slow
# or blocked downloads; ask the user before switching to them.

[CmdletBinding()]
param(
    [string]$Prefix = "",

    [ValidateSet("GitHub", "AtomGit", "Cloudflare")]
    [string]$Source = "GitHub",

    # AtomGit/Cloudflare may not publish an upstream checksum. Proceed only
    # after the user approves; the script prints the SHA-256 for the handoff.
    [switch]$AllowUnverified
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
    $ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
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

$GitHubBase = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest"
$AtomGitBase = "https://atomgit.com/oak-and-quercus/FFmpeg/releases/download/v9.0"
$CloudflareBase = "https://ffmpeg.oakvideoeditor.org/ffmpeg"

switch ($Source) {
    "GitHub" {
        $Base = $GitHubBase
        $Asset = "ffmpeg-n9.0-latest-win64-gpl-shared-9.0.zip"
        $RequirePublishedChecksum = $true
    }
    "AtomGit" {
        $Base = $AtomGitBase
        $Asset = "ffmpeg-n9.0-latest-win64-gpl-shared-9.0.zip"
        $RequirePublishedChecksum = $false
    }
    "Cloudflare" {
        $Base = $CloudflareBase
        $Asset = "ffmpeg-n9.0-win64-gpl-shared-9.0.zip"
        $RequirePublishedChecksum = $false
    }
}

$Download = Join-Path $env:TEMP "oak-mcp-ffmpeg"
$Checksums = Join-Path $Download "checksums.sha256"
$Zip = Join-Path $Download $Asset
$Extract = Join-Path $Cache "ffmpeg-extract"

New-Item -ItemType Directory -Force -Path $Cache, $Download | Out-Null
Remove-Item -LiteralPath $Checksums, $Zip -Force -ErrorAction SilentlyContinue

Write-Host ">> source: $Source"
Write-Host ">> downloading $Asset"

$Expected = $null
try {
    Invoke-WebRequest -Uri "$Base/checksums.sha256" -OutFile $Checksums -TimeoutSec 45
    $Line = Select-String -Path $Checksums -Pattern ([regex]::Escape($Asset) + "\s*$") |
        Select-Object -First 1
    if ($Line) {
        $Expected = $Line.Line.Split()[0].ToLowerInvariant()
    } elseif ($RequirePublishedChecksum) {
        throw "No checksum entry for $Asset in $Checksums"
    } else {
        Write-Warning "No checksum entry for $Asset at $Source; the archive is not upstream-verified."
    }
} catch {
    if ($RequirePublishedChecksum) {
        throw "Cannot fetch checksums from ${Source}: $($_.Exception.Message). Ask the user before retrying with -Source AtomGit or -Source Cloudflare."
    }
    Write-Warning "No checksums available at $Source; the archive is not upstream-verified."
}

try {
    Invoke-WebRequest -Uri "$Base/$Asset" -OutFile $Zip -TimeoutSec 300
} catch {
    throw "Download failed or timed out from ${Source}: $($_.Exception.Message). Ask the user before retrying with -Source AtomGit or -Source Cloudflare."
}

$Actual = (Get-FileHash -LiteralPath $Zip -Algorithm SHA256).Hash.ToLowerInvariant()
if ($null -ne $Expected) {
    if ($Actual -ne $Expected) {
        throw "FFmpeg checksum mismatch: expected $Expected, got $Actual"
    }
    Write-Host ">> checksum verified"
} else {
    if (-not $AllowUnverified) {
        throw "No published checksum for $Source. Review this SHA-256 and rerun with -AllowUnverified: $Actual"
    }
    Write-Warning "Proceeding with an unverified archive (SHA-256 $Actual)."
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
Write-Host "Source: $Source"
Write-Host "SHA256: $Actual"
Write-Host "FFMPEG_DIR=$env:FFMPEG_DIR"
Write-Host "PKG_CONFIG_PATH=$env:PKG_CONFIG_PATH"
Write-Host ""
Write-Host "Next: cargo build --release"
if ($MyInvocation.InvocationName -ne ".") {
    Write-Host ""
    Write-Host "Note: this process only. To keep the variables in your shell, run:"
    Write-Host "  . .\scripts\fetch-ffmpeg.ps1"
}
