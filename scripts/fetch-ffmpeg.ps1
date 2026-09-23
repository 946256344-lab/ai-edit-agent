# Fetch pinned Gyan full_build FFmpeg/FFprobe for the Windows installer.
# Essentials 缺少 preview 字幕所需的 libass/`ass` 滤镜，因此使用 full_build 静态版。
# 二进制超过 GitHub 100MB，不进 git；构建前由本脚本或 `npm run tauri:build` 拉取。
#
# Usage (from repo root):
#   powershell -ExecutionPolicy Bypass -File scripts/fetch-ffmpeg.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path (Join-Path $root "src-tauri"))) {
  $root = Get-Location
}

$outDir = Join-Path $root "src-tauri\resources\ffmpeg"
$ffmpegOut = Join-Path $outDir "ffmpeg.exe"
$ffprobeOut = Join-Path $outDir "ffprobe.exe"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

$version = "8.1.2"
$zipName = "ffmpeg-$version-full_build.zip"
$zipSha = "b8cdefab5f50590a076c27c2b56b0294a0e6154faded28ba1ba05ebc4f801f57"
$urls = @(
  "https://github.com/GyanD/codexffmpeg/releases/download/$version/$zipName"
  "https://www.gyan.dev/ffmpeg/builds/packages/$zipName"
  "https://ghfast.top/https://github.com/GyanD/codexffmpeg/releases/download/$version/$zipName"
)

function Get-Sha256([string]$Path) {
  (Get-FileHash -Algorithm SHA256 -Path $Path).Hash.ToLowerInvariant()
}

function Test-AssFilter([string]$FfmpegPath) {
  $filters = & $FfmpegPath -hide_banner -filters 2>&1 | Out-String
  return [bool]($filters -match '(?m)\bass\s+V->V\b')
}

if ((Test-Path $ffmpegOut) -and (Test-Path $ffprobeOut) -and (Test-AssFilter $ffmpegOut)) {
  Write-Host "OK $ffmpegOut"
  Write-Host "OK $ffprobeOut"
  exit 0
}

$cacheDir = Join-Path $outDir ".cache"
New-Item -ItemType Directory -Force -Path $cacheDir | Out-Null
$zipPath = Join-Path $cacheDir $zipName

$lastError = $null
$zipReady = $false
if ((Test-Path $zipPath) -and ((Get-Sha256 $zipPath) -eq $zipSha)) {
  $zipReady = $true
  Write-Host "OK cached $zipName"
}
if (-not $zipReady) {
  foreach ($Url in $urls) {
    Write-Host "GET $Url"
    & curl.exe -L --retry 8 --retry-delay 3 --connect-timeout 30 --max-time 0 -C - -o $zipPath $Url
    if ($LASTEXITCODE -ne 0) {
      $lastError = "download failed: $Url (exit $LASTEXITCODE)"
      continue
    }
    $actual = Get-Sha256 $zipPath
    if ($actual -ne $zipSha) {
      $lastError = "SHA-256 mismatch for $zipName`nexpected=$zipSha`nactual=$actual"
      Remove-Item -Force $zipPath -ErrorAction SilentlyContinue
      continue
    }
    $zipReady = $true
    break
  }
}
if (-not $zipReady) {
  throw $lastError
}

$extractDir = Join-Path $cacheDir "extract"
if (Test-Path $extractDir) {
  Remove-Item -Recurse -Force $extractDir
}
New-Item -ItemType Directory -Force -Path $extractDir | Out-Null
Write-Host "Extract $zipName"
& tar.exe -xf $zipPath -C $extractDir
if ($LASTEXITCODE -ne 0) {
  throw "failed to extract $zipName"
}

$binDir = Get-ChildItem -Path $extractDir -Directory | Select-Object -First 1
$srcFfmpeg = Join-Path $binDir.FullName "bin\ffmpeg.exe"
$srcFfprobe = Join-Path $binDir.FullName "bin\ffprobe.exe"
if (-not (Test-Path $srcFfmpeg) -or -not (Test-Path $srcFfprobe)) {
  throw "zip did not contain bin/ffmpeg.exe and bin/ffprobe.exe"
}

Copy-Item -Force $srcFfmpeg $ffmpegOut
Copy-Item -Force $srcFfprobe $ffprobeOut
$licenseOut = Join-Path $outDir "LICENSE"
$license = Join-Path $binDir.FullName "LICENSE"
if (Test-Path $license) {
  Copy-Item -Force $license $licenseOut
} elseif (-not (Test-Path $licenseOut)) {
  @"
FFmpeg Windows binaries in this directory come from Gyan full_build $version
and are licensed under GPLv3. Source: https://github.com/FFmpeg/FFmpeg
See NOTICE.md in this folder.
"@ | Set-Content -Encoding utf8 $licenseOut
}

if (-not (Test-AssFilter $ffmpegOut)) {
  Remove-Item -Force $ffmpegOut, $ffprobeOut -ErrorAction SilentlyContinue
  throw "bundled ffmpeg is missing the ass filter; preview subtitles require Gyan full_build"
}

Write-Host "OK $ffmpegOut"
Write-Host "OK $ffprobeOut"
Write-Host "FFmpeg $version full_build ready under src-tauri/resources/ffmpeg/"
