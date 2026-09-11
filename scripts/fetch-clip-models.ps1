# Fetch bundled CLIP ViT-B/32 ONNX models (Qdrant).
# Large files are gitignored (>100MB GitHub limit). Run before tauri:dev / tauri:build
# when CLIP ranking is desired.
#
# Usage (from repo root):
#   powershell -ExecutionPolicy Bypass -File scripts/fetch-clip-models.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path (Join-Path $root "src-tauri"))) {
  $root = Get-Location
}
$visionDir = Join-Path $root "src-tauri\resources\models\clip-ViT-B-32-vision"
$textDir = Join-Path $root "src-tauri\resources\models\clip-ViT-B-32-text"
New-Item -ItemType Directory -Force -Path $visionDir, $textDir | Out-Null

$visionSha = "c68d3d9a200ddd2a8c8a5510b576d4c94d1ae383bf8b36dd8c084f94e1fb4d63"
$textSha = "4dbe762b11e36488304471e439cde89da053ad7acaddbf9e096745d142ec8d8b"

function Get-Sha256([string]$Path) {
  (Get-FileHash -Algorithm SHA256 -Path $Path).Hash.ToLowerInvariant()
}

function Ensure-File([string]$Url, [string]$OutPath, [string]$ExpectedSha) {
  if ((Test-Path $OutPath) -and $ExpectedSha -and ((Get-Sha256 $OutPath) -eq $ExpectedSha)) {
    Write-Host "OK $OutPath"
    return
  }
  Write-Host "GET $Url"
  curl.exe -L --retry 8 --retry-delay 3 -C - -o $OutPath $Url
  if ($LASTEXITCODE -ne 0) { throw "download failed: $Url" }
  if ($ExpectedSha) {
    $actual = Get-Sha256 $OutPath
    if ($actual -ne $ExpectedSha) {
      throw "SHA-256 mismatch for $OutPath`nexpected=$ExpectedSha`nactual=$actual"
    }
  }
}

Ensure-File "https://huggingface.co/Qdrant/clip-ViT-B-32-vision/resolve/main/preprocessor_config.json" (Join-Path $visionDir "preprocessor_config.json") $null
Ensure-File "https://huggingface.co/Qdrant/clip-ViT-B-32-vision/resolve/main/config.json" (Join-Path $visionDir "config.json") $null
Ensure-File "https://huggingface.co/Qdrant/clip-ViT-B-32-vision/resolve/main/model.onnx" (Join-Path $visionDir "model.onnx") $visionSha

Ensure-File "https://huggingface.co/Qdrant/clip-ViT-B-32-text/resolve/main/config.json" (Join-Path $textDir "config.json") $null
Ensure-File "https://huggingface.co/Qdrant/clip-ViT-B-32-text/resolve/main/tokenizer.json" (Join-Path $textDir "tokenizer.json") $null
Ensure-File "https://huggingface.co/Qdrant/clip-ViT-B-32-text/resolve/main/tokenizer_config.json" (Join-Path $textDir "tokenizer_config.json") $null
Ensure-File "https://huggingface.co/Qdrant/clip-ViT-B-32-text/resolve/main/special_tokens_map.json" (Join-Path $textDir "special_tokens_map.json") $null
Ensure-File "https://huggingface.co/Qdrant/clip-ViT-B-32-text/resolve/main/model.onnx" (Join-Path $textDir "model.onnx") $textSha

Write-Host "CLIP models ready under src-tauri/resources/models/clip-ViT-B-32-*"
