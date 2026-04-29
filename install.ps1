# csshd installer (Windows PowerShell).
#
# Detects arch, downloads the matching release zip from github.com/css-md/csshd,
# verifies the SHA256 checksum, and drops csshd.exe into
# %USERPROFILE%\.csshd\bin (creating it if necessary).
#
# Usage (from a release URL):
#   powershell -ExecutionPolicy ByPass -c `
#     "irm https://github.com/css-md/csshd/releases/latest/download/csshd-installer.ps1 | iex"
#
# This file uses the placeholder __TAG__ which the release workflow
# replaces with the actual git tag (e.g. v0.1.0) before publishing.

$ErrorActionPreference = 'Stop'

$Repo    = 'css-md/csshd'
$Tag     = '__TAG__'
$InstallDir = if ($env:CSSHD_INSTALL_DIR) { $env:CSSHD_INSTALL_DIR } else { Join-Path $env:USERPROFILE '.csshd\bin' }

# Arch detection
$arch = switch ($env:PROCESSOR_ARCHITECTURE) {
  'AMD64' { 'x86_64' }
  'ARM64' { 'aarch64' }
  default { throw "csshd: unsupported arch: $($env:PROCESSOR_ARCHITECTURE)" }
}
# Currently we only ship x86_64-pc-windows-msvc; aarch64-windows isn't built yet.
if ($arch -ne 'x86_64') {
  throw "csshd: Windows on $arch isn't a published target yet. Open an issue if you need it."
}

$target  = "$arch-pc-windows-msvc"
$version = $Tag -replace '^v', ''
$stem    = "csshd-$version-$target"
$archive = "$stem.zip"
$url     = "https://github.com/$Repo/releases/download/$Tag/$archive"
$shaUrl  = "$url.sha256"

$tmp = Join-Path $env:TEMP "csshd-install-$(Get-Random)"
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
try {
  Write-Host "csshd: downloading $url"
  Invoke-WebRequest -Uri $url -OutFile (Join-Path $tmp $archive) -UseBasicParsing

  try {
    Invoke-WebRequest -Uri $shaUrl -OutFile (Join-Path $tmp "$archive.sha256") -UseBasicParsing
    Write-Host 'csshd: verifying checksum'
    $expected = (Get-Content (Join-Path $tmp "$archive.sha256")).Split(' ')[0]
    $actual   = (Get-FileHash -Algorithm SHA256 (Join-Path $tmp $archive)).Hash.ToLower()
    if ($expected -ne $actual) {
      throw "csshd: checksum mismatch (expected $expected, got $actual)"
    }
  } catch [System.Net.WebException] {
    Write-Host 'csshd: checksum file unavailable, skipping verification' -ForegroundColor Yellow
  }

  Expand-Archive -Path (Join-Path $tmp $archive) -DestinationPath $tmp -Force
  $bin = Join-Path $tmp "$stem\csshd.exe"
  if (-not (Test-Path $bin)) {
    throw "csshd: binary not found in archive (expected $bin)"
  }

  New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
  Copy-Item $bin (Join-Path $InstallDir 'csshd.exe') -Force

  Write-Host ""
  Write-Host "csshd: installed -> $InstallDir\csshd.exe" -ForegroundColor Green

  $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
  if ($userPath -notlike "*$InstallDir*") {
    Write-Host ""
    Write-Host "Adding $InstallDir to your User PATH..."
    [Environment]::SetEnvironmentVariable('Path', "$userPath;$InstallDir", 'User')
    Write-Host "Open a new terminal for the change to take effect." -ForegroundColor Yellow
  }

  Write-Host ""
  Write-Host "Next: csshd login --helpdesk https://your-helpdesk-url"
} finally {
  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
