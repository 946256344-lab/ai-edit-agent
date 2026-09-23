# Verify packaged FFmpeg/FFprobe without a system PATH install.
# Usage (from repo root, after npm run tauri:build -- -b nsis):
#   powershell -ExecutionPolicy Bypass -File scripts/verify-packaged-ffmpeg.ps1

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path (Join-Path $root 'src-tauri'))) {
  $root = Get-Location
}

function Get-PathWithoutFfmpeg {
  $parts = @()
  foreach ($entry in ($env:Path -split ';')) {
    if (-not $entry) { continue }
    $hasFfmpeg = Test-Path (Join-Path $entry 'ffmpeg.exe')
    $hasFfprobe = Test-Path (Join-Path $entry 'ffprobe.exe')
    if (-not $hasFfmpeg -and -not $hasFfprobe) {
      $parts += $entry
    }
  }
  return ($parts -join ';')
}

function Find-PackagedMedia {
  $searchRoot = Join-Path $root 'src-tauri\target\release'
  if (-not (Test-Path $searchRoot)) { return $null }
  $items = Get-ChildItem -Path $searchRoot -Recurse -Filter 'ffmpeg.exe' -ErrorAction SilentlyContinue
  foreach ($item in $items) {
    $probe = Join-Path $item.DirectoryName 'ffprobe.exe'
    if (Test-Path $probe) {
      return [pscustomobject]@{
        Ffmpeg = $item.FullName
        Ffprobe = $probe
        Directory = $item.DirectoryName
      }
    }
  }
  return $null
}

$packaged = Find-PackagedMedia
if (-not $packaged) {
  throw 'Packaged ffmpeg.exe/ffprobe.exe were not found under target/release. Run npm run tauri:build -- -b nsis first.'
}

$bundleDir = Join-Path $root 'src-tauri\target\release\bundle\nsis'
$installer = Get-ChildItem -Path $bundleDir -Filter '*.exe' -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $installer) {
  throw 'NSIS installer was not found. Confirm tauri build finished packaging.'
}

Write-Host ('Installer: {0} ({1:N1} MB)' -f $installer.FullName, ($installer.Length / 1MB))
Write-Host ('Packaged ffmpeg: {0}' -f $packaged.Ffmpeg)
Write-Host ('Packaged ffprobe: {0}' -f $packaged.Ffprobe)

$cleanPath = Get-PathWithoutFfmpeg
$env:PATH = $cleanPath
$whereFfmpeg = Get-Command ffmpeg -ErrorAction SilentlyContinue
if ($whereFfmpeg) {
  throw ('PATH still has ffmpeg: {0}' -f $whereFfmpeg.Source)
}
Write-Host 'PATH no longer contains system FFmpeg/FFprobe.'

$work = Join-Path $env:TEMP ('ava-ffmpeg-verify-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $work | Out-Null
$clip = Join-Path $work 'preview-smoke.mp4'

$version = & $packaged.Ffmpeg -hide_banner -version
if ($LASTEXITCODE -ne 0) { throw 'packaged ffmpeg -version failed' }
$versionLine = @($version)[0]
if ($versionLine -notmatch 'ffmpeg version 8\.1\.2-full_build') {
  throw ('unexpected ffmpeg version: {0}' -f $versionLine)
}

$filters = & $packaged.Ffmpeg -hide_banner -filters 2>&1 | Out-String
if ($filters -notmatch '(?m)\bass\s+V->V\b') {
  throw 'packaged ffmpeg is missing the ass filter'
}

& $packaged.Ffmpeg -y -hide_banner -loglevel error -f lavfi -i 'color=c=black:s=540x960:r=30' -t 1 -pix_fmt yuv420p -c:v libx264 -preset veryfast -crf 28 $clip
if ($LASTEXITCODE -ne 0 -or -not (Test-Path $clip)) {
  throw 'packaged ffmpeg could not render a 540x960 preview-like clip'
}

$probeJson = & $packaged.Ffprobe -hide_banner -loglevel error -print_format json -show_streams $clip | Out-String
if ($LASTEXITCODE -ne 0) { throw 'packaged ffprobe failed' }
if ($probeJson -notmatch '"codec_name"\s*:\s*"h264"') {
  throw 'preview-like clip is not h264'
}
if ($probeJson -notmatch '"width"\s*:\s*540' -or $probeJson -notmatch '"height"\s*:\s*960') {
  throw 'preview-like clip is not 540x960'
}

Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
Write-Host 'OK packaged FFmpeg/FFprobe work with no system ffmpeg on PATH (540x960 h264 smoke).'
