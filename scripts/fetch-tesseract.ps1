# Fetch a pinned Tesseract OCR Windows runtime with English language data.
# Usage (from repo root):
#   powershell -ExecutionPolicy Bypass -File scripts/fetch-tesseract.ps1

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path (Join-Path $root 'src-tauri'))) {
  $root = Get-Location
}

$outDir = Join-Path $root 'src-tauri\resources\tesseract'
$exeOut = Join-Path $outDir 'tesseract.exe'
$engOut = Join-Path $outDir 'tessdata\eng.traineddata'
$notice = Join-Path $outDir 'NOTICE.md'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

$version = '5.4.0.20240606'
$installerName = "tesseract-ocr-w64-setup-$version.exe"
$installerSha = 'c885fff6998e0608ba4bb8ab51436e1c6775c2bafc2559a19b423e18678b60c9'
$url = "https://github.com/UB-Mannheim/tesseract/releases/download/v$version/$installerName"
$sevenZipVersion = '26.03'
$sevenZipBootstrapName = '7zr.exe'
$sevenZipBootstrapSha = 'ad4c82fadcbdf93c03b4fc440f300509c7d60c5c2f4d183e35d9d70d6957037d'
$sevenZipInstallerName = '7z2603-x64.exe'
$sevenZipInstallerSha = '0859c524b8a63551848f0c246abddcb1d0b7b656b0fbfe879f8d85e61a9e6edd'

function Get-Sha256([string]$Path) {
  $stream = [System.IO.File]::OpenRead($Path)
  try {
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
      return ([System.BitConverter]::ToString($sha.ComputeHash($stream))).Replace('-', '').ToLowerInvariant()
    } finally {
      $sha.Dispose()
    }
  } finally {
    $stream.Dispose()
  }
}

function Test-Tesseract([string]$Program) {
  if (-not (Test-Path $Program) -or -not (Test-Path $engOut)) { return $false }
  $languages = & $Program --list-langs 2>&1 | Out-String
  return ($LASTEXITCODE -eq 0) -and ($languages -match '(?m)^eng\s*$')
}

if (Test-Tesseract $exeOut) {
  Write-Host "OK $exeOut"
  exit 0
}

$cacheDir = Join-Path $outDir '.cache'
New-Item -ItemType Directory -Force -Path $cacheDir | Out-Null
$installer = Join-Path $cacheDir $installerName

function Get-VerifiedFile([string]$Url, [string]$Path, [string]$Sha256) {
  if ((Test-Path $Path) -and (Get-Sha256 $Path) -eq $Sha256) { return }
  Remove-Item -Force $Path -ErrorAction SilentlyContinue
  Write-Host "GET $Url"
  & curl.exe -L --retry 8 --retry-delay 3 --connect-timeout 30 --max-time 0 -o $Path $Url
  if ($LASTEXITCODE -ne 0) { throw "download failed: $Url (exit $LASTEXITCODE)" }
  $actual = Get-Sha256 $Path
  if ($actual -ne $Sha256) {
    throw "SHA-256 mismatch for $Path`nexpected=$Sha256`nactual=$actual"
  }
}

Get-VerifiedFile $url $installer $installerSha

$sevenZipBootstrap = Join-Path $cacheDir $sevenZipBootstrapName
$sevenZipBootstrapUrl = "https://github.com/ip7z/7zip/releases/download/$sevenZipVersion/$sevenZipBootstrapName"
Get-VerifiedFile $sevenZipBootstrapUrl $sevenZipBootstrap $sevenZipBootstrapSha

$sevenZipInstaller = Join-Path $cacheDir $sevenZipInstallerName
$sevenZipInstallerUrl = "https://github.com/ip7z/7zip/releases/download/$sevenZipVersion/$sevenZipInstallerName"
Get-VerifiedFile $sevenZipInstallerUrl $sevenZipInstaller $sevenZipInstallerSha

$sevenZipDir = Join-Path $cacheDir '7zip'
New-Item -ItemType Directory -Force -Path $sevenZipDir | Out-Null
& $sevenZipBootstrap x -y "-o$sevenZipDir" $sevenZipInstaller | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Failed to extract the pinned 7-Zip runtime.' }
$sevenZip = Join-Path $sevenZipDir '7z.exe'
if (-not (Test-Path $sevenZip)) { throw 'Pinned 7-Zip runtime did not contain 7z.exe.' }

$extractDir = Join-Path $cacheDir 'extract'
New-Item -ItemType Directory -Force -Path $extractDir | Out-Null
& $sevenZip x -y "-o$extractDir" $installer | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Failed to extract the Tesseract NSIS installer.' }

Copy-Item -Force (Join-Path $extractDir 'tesseract.exe') $exeOut
Get-ChildItem -LiteralPath $extractDir -File -Filter '*.dll' |
  Copy-Item -Destination $outDir -Force
New-Item -ItemType Directory -Force -Path (Split-Path $engOut) | Out-Null
Copy-Item -Force (Join-Path $extractDir 'tessdata\eng.traineddata') $engOut
Copy-Item -Force (Join-Path $extractDir 'doc\LICENSE') (Join-Path $outDir 'LICENSE')

if (-not (Test-Path $notice)) {
  throw 'Tesseract NOTICE.md was unexpectedly removed.'
}
if (-not (Test-Tesseract $exeOut)) {
  throw 'Bundled Tesseract is missing tesseract.exe or English language data.'
}

Write-Host "OK $exeOut"
Write-Host "Tesseract $version with eng.traineddata ready under src-tauri/resources/tesseract/"
