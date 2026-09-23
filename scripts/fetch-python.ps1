# Fetch pinned Windows embeddable CPython and vendor Jianying/CapCut draft SDKs.
# 适配器会再调 ffmpeg，因此随包 python.exe 启动时由 Rust 把 FFmpeg 目录加进 PATH。
# 整个 runtime 不进 git；构建前由本脚本或 `npm run tauri:build` 拉取。
#
# Usage (from repo root):
#   powershell -ExecutionPolicy Bypass -File scripts/fetch-python.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path (Join-Path $root "src-tauri"))) {
  $root = Get-Location
}

$outDir = Join-Path $root "src-tauri\resources\python"
$pythonOut = Join-Path $outDir "python.exe"
$mediaInfoOut = Join-Path $outDir "MediaInfo.dll"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

$pyVersion = "3.12.10"
$zipName = "python-$pyVersion-embed-amd64.zip"
$zipSha = "4acbed6dd1c744b0376e3b1cf57ce906f9dc9e95e68824584c8099a63025a3c3"
$pyUrls = @(
  "https://www.python.org/ftp/python/$pyVersion/$zipName"
)

$mediaInfoZipName = "MediaInfo_DLL_24.12_Windows_x64_WithoutInstaller.zip"
$mediaInfoSha = "6189f3110c96ea0c53e6b56ae6356669a65c0c5809da896aa85b040508eed11b"
$mediaInfoUrls = @(
  "https://mediaarea.net/download/binary/libmediainfo0/24.12/$mediaInfoZipName"
)

$sdkPackages = @(
  "pyJianYingDraft==0.3.0"
  "pycapcut==0.0.3"
)

function Get-Sha256([string]$Path) {
  (Get-FileHash -Algorithm SHA256 -Path $Path).Hash.ToLowerInvariant()
}

function Test-DraftSdks([string]$PythonPath) {
  if (-not (Test-Path $PythonPath)) { return $false }
  if (-not (Test-Path (Join-Path (Split-Path $PythonPath) "MediaInfo.dll"))) { return $false }
  & $PythonPath -c "import pyJianYingDraft, pycapcut, pymediainfo"
  return ($LASTEXITCODE -eq 0)
}

if (Test-DraftSdks $pythonOut) {
  Write-Host "OK $pythonOut"
  Write-Host "OK draft SDKs (pyJianYingDraft, pycapcut)"
  exit 0
}

$cacheDir = Join-Path $root "src-tauri\resources\.cache\python"
New-Item -ItemType Directory -Force -Path $cacheDir | Out-Null
$zipPath = Join-Path $cacheDir $zipName
$legacyZip = Join-Path $outDir ".cache\$zipName"
if ((Test-Path $legacyZip) -and -not (Test-Path $zipPath)) {
  Move-Item -Force $legacyZip $zipPath
}

function Get-CachedFile([string]$Path, [string]$Sha, [string[]]$Urls, [string]$Label) {
  if ((Test-Path $Path) -and ((Get-Sha256 $Path) -eq $Sha)) {
    Write-Host "OK cached $Label"
    return
  }
  $lastError = $null
  foreach ($Url in $Urls) {
    Write-Host "GET $Url"
    & curl.exe -L --retry 8 --retry-delay 3 --connect-timeout 30 --max-time 0 -C - -o $Path $Url
    if ($LASTEXITCODE -ne 0) {
      $lastError = "download failed: $Url (exit $LASTEXITCODE)"
      continue
    }
    $actual = Get-Sha256 $Path
    if ($actual -ne $Sha) {
      $lastError = "SHA-256 mismatch for $Label`nexpected=$Sha`nactual=$actual"
      Remove-Item -Force $Path -ErrorAction SilentlyContinue
      continue
    }
    return
  }
  throw $lastError
}

Get-CachedFile $zipPath $zipSha $pyUrls $zipName

$extractDir = Join-Path $cacheDir "extract-python"
if (Test-Path $extractDir) {
  Remove-Item -Recurse -Force $extractDir
}
New-Item -ItemType Directory -Force -Path $extractDir | Out-Null
Write-Host "Extract $zipName"
& tar.exe -xf $zipPath -C $extractDir
if ($LASTEXITCODE -ne 0) {
  throw "failed to extract $zipName"
}

$embedRoot = $extractDir
if (-not (Test-Path (Join-Path $embedRoot "python.exe"))) {
  $embedRoot = Get-ChildItem -Path $extractDir -Directory | Select-Object -First 1 | ForEach-Object { $_.FullName }
}
if (-not (Test-Path (Join-Path $embedRoot "python.exe"))) {
  throw "zip did not contain python.exe"
}

Get-ChildItem -Path $embedRoot -Force | ForEach-Object {
  Copy-Item -Force -Recurse $_.FullName (Join-Path $outDir $_.Name)
}

$pth = Get-ChildItem -Path $outDir -Filter "python*._pth" | Select-Object -First 1
if (-not $pth) {
  throw "embeddable python is missing python*._pth"
}
$stdlibZip = Get-ChildItem -Path $outDir -Filter "python*.zip" | Where-Object { $_.Name -match '^python\d+\.zip$' } | Select-Object -First 1
if (-not $stdlibZip) {
  throw "embeddable python is missing pythonNNN.zip"
}
@(
  $stdlibZip.Name
  "."
  "Lib\site-packages"
  "import site"
) | Set-Content -Encoding ascii -Path $pth.FullName
Write-Host "Enabled import site in $($pth.Name)"

$getPip = Join-Path $cacheDir "get-pip.py"
$getPipUrls = @(
  "https://bootstrap.pypa.io/get-pip.py"
  "https://github.com/pypa/get-pip/raw/main/public/get-pip.py"
)
$gotPip = $false
foreach ($Url in $getPipUrls) {
  Write-Host "GET $Url"
  & curl.exe -L --retry 8 --retry-delay 3 --connect-timeout 30 --max-time 120 -o $getPip $Url
  if ($LASTEXITCODE -eq 0 -and (Test-Path $getPip) -and ((Get-Item $getPip).Length -gt 10000)) {
    $gotPip = $true
    break
  }
}
if (-not $gotPip) {
  throw "failed to download get-pip.py"
}

Write-Host "Install pip"
& $pythonOut $getPip --no-warn-script-location --disable-pip-version-check
if ($LASTEXITCODE -ne 0) {
  throw "get-pip.py failed"
}

$pipIndexes = @(
  "https://pypi.org/simple"
  "https://pypi.tuna.tsinghua.edu.cn/simple"
)
$pipOk = $false
$lastPip = $null
foreach ($Index in $pipIndexes) {
  Write-Host "pip install ($Index) $($sdkPackages -join ' ')"
  $env:PIP_DISABLE_PIP_VERSION_CHECK = "1"
  $env:PIP_NO_CACHE_DIR = "1"
  & $pythonOut -m pip install --no-warn-script-location --disable-pip-version-check -i $Index @sdkPackages
  if ($LASTEXITCODE -eq 0) {
    $pipOk = $true
    break
  }
  $lastPip = "pip install failed via $Index (exit $LASTEXITCODE)"
}
if (-not $pipOk) {
  throw $lastPip
}

$mediaInfoZip = Join-Path $cacheDir $mediaInfoZipName
Get-CachedFile $mediaInfoZip $mediaInfoSha $mediaInfoUrls $mediaInfoZipName
$mediaExtract = Join-Path $cacheDir "extract-mediainfo"
if (Test-Path $mediaExtract) {
  Remove-Item -Recurse -Force $mediaExtract
}
New-Item -ItemType Directory -Force -Path $mediaExtract | Out-Null
& tar.exe -xf $mediaInfoZip -C $mediaExtract
if ($LASTEXITCODE -ne 0) {
  throw "failed to extract $mediaInfoZipName"
}
$dll = Get-ChildItem -Path $mediaExtract -Recurse -Filter "MediaInfo.dll" | Select-Object -First 1
if (-not $dll) {
  throw "MediaInfo zip did not contain MediaInfo.dll"
}
Copy-Item -Force $dll.FullName $mediaInfoOut

if (-not (Test-DraftSdks $pythonOut)) {
  throw "bundled python cannot import pyJianYingDraft, pycapcut, and pymediainfo"
}

Write-Host "OK $pythonOut"
Write-Host "OK $mediaInfoOut"
Write-Host "Embeddable Python $pyVersion + draft SDKs ready under src-tauri/resources/python/"
