# Fetch pinned Microsoft.AI.DirectML redistributable for local ONNX models on the GPU.
# Windows 自带的 DirectML 过旧（1.8），ONNX Runtime 1.20 的 DirectML 推理会报错；随包带新版 DirectML.dll。
# 二进制不进 git；开发前或 `npm run tauri:build` 前由本脚本拉取。
#
# Usage (from repo root):
#   powershell -ExecutionPolicy Bypass -File scripts/fetch-directml.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path (Join-Path $root "src-tauri"))) {
  $root = Get-Location
}

$outDir = Join-Path $root "src-tauri\resources\directml"
$dllOut = Join-Path $outDir "DirectML.dll"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

$version = "1.15.4"
$packageName = "microsoft.ai.directml.$version.nupkg"
$packageSha = "4e7cb7ddce8cf837a7a75dc029209b520ca0101470fcdf275c1f49736a3615b9"
$dllSha = "9c9e6d822561c6c41b90e6994b3e8857cf1d66dbfb1e0c4c799c7c89b4e92da1"
$url = "https://api.nuget.org/v3-flatcontainer/microsoft.ai.directml/$version/$packageName"

function Get-Sha256([string]$Path) {
  (Get-FileHash -Algorithm SHA256 -Path $Path).Hash.ToLowerInvariant()
}

if ((Test-Path $dllOut) -and ((Get-Sha256 $dllOut) -eq $dllSha)) {
  Write-Host "OK $dllOut"
  exit 0
}

$cacheDir = Join-Path $outDir ".cache"
New-Item -ItemType Directory -Force -Path $cacheDir | Out-Null
$packagePath = Join-Path $cacheDir $packageName

if (-not ((Test-Path $packagePath) -and ((Get-Sha256 $packagePath) -eq $packageSha))) {
  Write-Host "GET $url"
  & curl.exe -L --retry 8 --retry-delay 3 --connect-timeout 30 --max-time 0 -o $packagePath $url
  if ($LASTEXITCODE -ne 0) {
    throw "download failed: $url (exit $LASTEXITCODE)"
  }
  $actual = Get-Sha256 $packagePath
  if ($actual -ne $packageSha) {
    Remove-Item -Force $packagePath -ErrorAction SilentlyContinue
    throw "SHA-256 mismatch for $packageName`nexpected=$packageSha`nactual=$actual"
  }
}

Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [System.IO.Compression.ZipFile]::OpenRead($packagePath)
try {
  foreach ($pair in @(
      @("bin/x64-win/DirectML.dll", "DirectML.dll"),
      @("LICENSE.txt", "LICENSE.txt"),
      @("ThirdPartyNotices.txt", "ThirdPartyNotices.txt")
    )) {
    $entry = $zip.GetEntry($pair[0])
    if ($null -eq $entry) {
      throw "missing $($pair[0]) in $packageName"
    }
    [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, (Join-Path $outDir $pair[1]), $true)
  }
} finally {
  $zip.Dispose()
}

$actualDll = Get-Sha256 $dllOut
if ($actualDll -ne $dllSha) {
  throw "SHA-256 mismatch for DirectML.dll`nexpected=$dllSha`nactual=$actualDll"
}
$signature = Get-AuthenticodeSignature $dllOut
if ($signature.Status -ne "Valid") {
  throw "DirectML.dll signature is not valid: $($signature.Status)"
}
Write-Host "OK $dllOut"
