# Verify bundled Python + draft SDKs without a system Python on PATH.
# Usage (from repo root):
#   powershell -ExecutionPolicy Bypass -File scripts/verify-packaged-python.ps1
# After `npm run python:fetch` this checks src-tauri/resources/python.
# After `npm run tauri:build -- -b nsis` it prefers the packaged copy under target/release.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path (Join-Path $root 'src-tauri'))) {
  $root = Get-Location
}

function Get-PathWithoutPython {
  $parts = @()
  foreach ($entry in ($env:Path -split ';')) {
    if (-not $entry) { continue }
    $hasPython = Test-Path (Join-Path $entry 'python.exe')
    $hasPy = Test-Path (Join-Path $entry 'py.exe')
    if (-not $hasPython -and -not $hasPy) {
      $parts += $entry
    }
  }
  return ($parts -join ';')
}

function Find-BundledPython {
  $searchRoot = Join-Path $root 'src-tauri\target\release'
  if (Test-Path $searchRoot) {
    $items = Get-ChildItem -Path $searchRoot -Recurse -Filter 'python.exe' -ErrorAction SilentlyContinue
    foreach ($item in $items) {
      $sdk = Join-Path $item.DirectoryName 'Lib\site-packages\pyJianYingDraft'
      $dll = Join-Path $item.DirectoryName 'MediaInfo.dll'
      if ((Test-Path $sdk) -and (Test-Path $dll)) {
        return $item.FullName
      }
    }
  }
  $dev = Join-Path $root 'src-tauri\resources\python\python.exe'
  $devSdk = Join-Path $root 'src-tauri\resources\python\Lib\site-packages\pyJianYingDraft'
  $devDll = Join-Path $root 'src-tauri\resources\python\MediaInfo.dll'
  if ((Test-Path $dev) -and (Test-Path $devSdk) -and (Test-Path $devDll)) {
    return $dev
  }
  return $null
}

$python = Find-BundledPython
if (-not $python) {
  throw 'Bundled python.exe with draft SDKs was not found. Run npm run python:fetch first.'
}

$packagedRoot = Join-Path $root 'src-tauri\target\release'
$requirePackaged = Test-Path (Join-Path $root 'src-tauri\target\release\bundle\nsis')
if ($requirePackaged -and ($python.IndexOf($packagedRoot, [System.StringComparison]::OrdinalIgnoreCase) -lt 0)) {
  throw 'NSIS bundle exists but python.exe was not found under target/release. Rebuild with npm run tauri:build -- -b nsis.'
}

$ffmpeg = Join-Path $root 'src-tauri\resources\ffmpeg\ffmpeg.exe'
$releaseFfmpeg = Join-Path $root 'src-tauri\target\release\resources\ffmpeg\ffmpeg.exe'
if (Test-Path $releaseFfmpeg) {
  $ffmpeg = $releaseFfmpeg
}

Write-Host ('Bundled python: {0}' -f $python)

$cleanPath = Get-PathWithoutPython
$env:PATH = $cleanPath
$wherePython = Get-Command python -ErrorAction SilentlyContinue
$wherePy = Get-Command py -ErrorAction SilentlyContinue
if ($wherePython -or $wherePy) {
  throw 'PATH still has python or py after stripping interpreter directories.'
}
Write-Host 'PATH no longer contains system python/py.'

$version = & $python --version
if ($LASTEXITCODE -ne 0) { throw 'bundled python --version failed' }
$versionText = ($version | Out-String).Trim()
if ($versionText -notmatch 'Python 3\.12\.') {
  throw ('unexpected python version: {0}' -f $versionText)
}
Write-Host $versionText

& $python -c "import pyJianYingDraft, pycapcut, pymediainfo; print('sdk-ok')"
if ($LASTEXITCODE -ne 0) {
  throw 'bundled python cannot import pyJianYingDraft, pycapcut, and pymediainfo'
}

if (Test-Path $ffmpeg) {
  $ffmpegDir = Split-Path $ffmpeg
  $env:PATH = ($ffmpegDir + ';' + $cleanPath)
  $which = & $python -c "import shutil; print(shutil.which('ffmpeg') or '')"
  if ($LASTEXITCODE -ne 0 -or -not $which) {
    throw 'bundled python cannot see ffmpeg after PATH prepend'
  }
  Write-Host ('python shutil.which(ffmpeg)= {0}' -f $which.Trim())
}

Write-Host 'Bundled Python runtime verified without system Python on PATH.'
