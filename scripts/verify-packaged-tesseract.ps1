# Verify packaged Tesseract without a system installation on PATH.
# Usage (from repo root, after npm run tauri:build -- -b nsis):
#   powershell -ExecutionPolicy Bypass -File scripts/verify-packaged-tesseract.ps1

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path (Join-Path $root 'src-tauri'))) {
  $root = Get-Location
}

$searchRoot = Join-Path $root 'src-tauri\target\release'
$program = Get-ChildItem -Path $searchRoot -Recurse -Filter 'tesseract.exe' -ErrorAction SilentlyContinue |
  Where-Object { Test-Path (Join-Path $_.DirectoryName 'tessdata\eng.traineddata') } |
  Select-Object -First 1
if (-not $program) {
  throw 'Packaged tesseract.exe with eng.traineddata was not found under target/release.'
}

$cleanPath = ($env:Path -split ';' | Where-Object {
  $_ -and -not (Test-Path (Join-Path $_ 'tesseract.exe'))
}) -join ';'
$env:PATH = $cleanPath
if (Get-Command tesseract -ErrorAction SilentlyContinue) {
  throw 'PATH still has a system Tesseract after stripping interpreter directories.'
}

$version = & $program.FullName --version 2>&1 | Out-String
if ($LASTEXITCODE -ne 0 -or $version -notmatch 'tesseract v?5\.4\.0') {
  throw "Unexpected packaged Tesseract version: $version"
}
$languages = & $program.FullName --list-langs 2>&1 | Out-String
if ($LASTEXITCODE -ne 0 -or $languages -notmatch '(?m)^eng\s*$') {
  throw 'Packaged Tesseract cannot load English language data.'
}

Write-Host ('Packaged Tesseract: {0}' -f $program.FullName)
Write-Host 'OK packaged Tesseract works with eng language data and no system Tesseract on PATH.'
