# Fetch BGE + CLIP ONNX models for local/dev or full release bundles.
# Release installs also download into app_data via runtime_models (official + hf-mirror).
# Large CLIP files are gitignored (>100MB GitHub limit); BGE may already be in resources.
#
# Usage (from repo root):
#   powershell -ExecutionPolicy Bypass -File scripts/fetch-clip-models.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path (Join-Path $root "src-tauri"))) {
  $root = Get-Location
}

$bgeDir = Join-Path $root "src-tauri\resources\models\bge-small-zh-v1.5\onnx"
$visionDir = Join-Path $root "src-tauri\resources\models\clip-ViT-B-32-vision"
$textDir = Join-Path $root "src-tauri\resources\models\clip-ViT-B-32-text"
New-Item -ItemType Directory -Force -Path $bgeDir, $visionDir, $textDir | Out-Null

$bgeSha = "69a0b846f4f116b5e6aabf9546ea6754d02264f3211a13a1bd69b31b8040749a"
$visionSha = "c68d3d9a200ddd2a8c8a5510b576d4c94d1ae383bf8b36dd8c084f94e1fb4d63"
$textSha = "4dbe762b11e36488304471e439cde89da053ad7acaddbf9e096745d142ec8d8b"

# Prefer SHAs declared in Rust sources (avoid drift).
$semanticRs = Join-Path $root "src-tauri\src\storyboard\semantic.rs"
if (Test-Path $semanticRs) {
  $m = Select-String -Path $semanticRs -Pattern 'MODEL_SHA256[\s\S]*?"([0-9a-f]{64})"' | Select-Object -First 1
  if (-not $m) {
    $lines = Get-Content $semanticRs
    for ($i = 0; $i -lt $lines.Count; $i++) {
      if ($lines[$i] -match 'MODEL_SHA256') {
        if ($lines[$i] -match '"([0-9a-f]{64})"') { $bgeSha = $Matches[1] }
        elseif ($i + 1 -lt $lines.Count -and $lines[$i + 1] -match '"([0-9a-f]{64})"') { $bgeSha = $Matches[1] }
        break
      }
    }
  } else {
    $bgeSha = $m.Matches[0].Groups[1].Value
  }
}
$clipRs = Join-Path $root "src-tauri\src\storyboard\clip.rs"
if (Test-Path $clipRs) {
  $lines = Get-Content $clipRs
  for ($i = 0; $i -lt $lines.Count; $i++) {
    if ($lines[$i] -match 'VISION_MODEL_SHA256' -and $i + 1 -lt $lines.Count -and $lines[$i + 1] -match '"([0-9a-f]{64})"') {
      $visionSha = $Matches[1]
    }
    if ($lines[$i] -match 'TEXT_MODEL_SHA256' -and $i + 1 -lt $lines.Count -and $lines[$i + 1] -match '"([0-9a-f]{64})"') {
      $textSha = $Matches[1]
    }
  }
}

function Get-Sha256([string]$Path) {
  (Get-FileHash -Algorithm SHA256 -Path $Path).Hash.ToLowerInvariant()
}

function Ensure-File([string[]]$Urls, [string]$OutPath, [string]$ExpectedSha) {
  if ((Test-Path $OutPath) -and $ExpectedSha -and ((Get-Sha256 $OutPath) -eq $ExpectedSha)) {
    Write-Host "OK $OutPath"
    return
  }
  $lastError = $null
  foreach ($Url in $Urls) {
    Write-Host "GET $Url"
    & curl.exe -L --retry 8 --retry-delay 3 --connect-timeout 30 --max-time 0 -C - -o $OutPath $Url
    if ($LASTEXITCODE -ne 0) {
      $lastError = "download failed: $Url (exit $LASTEXITCODE)"
      continue
    }
    if ($ExpectedSha) {
      $actual = Get-Sha256 $OutPath
      if ($actual -ne $ExpectedSha) {
        $lastError = "SHA-256 mismatch for $OutPath`nexpected=$ExpectedSha`nactual=$actual"
        Remove-Item -Force $OutPath -ErrorAction SilentlyContinue
        continue
      }
    }
    Write-Host "OK $OutPath"
    return
  }
  throw $lastError
}

function MirrorPair([string]$HfPath) {
  @(
    "https://huggingface.co/$HfPath"
    "https://hf-mirror.com/$HfPath"
  )
}

Ensure-File (MirrorPair "Xenova/bge-small-zh-v1.5/resolve/main/onnx/model.onnx") (Join-Path $bgeDir "model.onnx") $bgeSha

Ensure-File (MirrorPair "Qdrant/clip-ViT-B-32-vision/resolve/main/preprocessor_config.json") (Join-Path $visionDir "preprocessor_config.json") $null
Ensure-File (MirrorPair "Qdrant/clip-ViT-B-32-vision/resolve/main/config.json") (Join-Path $visionDir "config.json") $null
Ensure-File (MirrorPair "Qdrant/clip-ViT-B-32-vision/resolve/main/model.onnx") (Join-Path $visionDir "model.onnx") $visionSha

Ensure-File (MirrorPair "Qdrant/clip-ViT-B-32-text/resolve/main/config.json") (Join-Path $textDir "config.json") $null
Ensure-File (MirrorPair "Qdrant/clip-ViT-B-32-text/resolve/main/tokenizer.json") (Join-Path $textDir "tokenizer.json") $null
Ensure-File (MirrorPair "Qdrant/clip-ViT-B-32-text/resolve/main/tokenizer_config.json") (Join-Path $textDir "tokenizer_config.json") $null
Ensure-File (MirrorPair "Qdrant/clip-ViT-B-32-text/resolve/main/special_tokens_map.json") (Join-Path $textDir "special_tokens_map.json") $null
Ensure-File (MirrorPair "Qdrant/clip-ViT-B-32-text/resolve/main/model.onnx") (Join-Path $textDir "model.onnx") $textSha

Write-Host "Models ready under src-tauri/resources/models/*"
Write-Host "Full installer: npm run tauri:build:full  (after this script succeeds)"
