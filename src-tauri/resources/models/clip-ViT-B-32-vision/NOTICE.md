# CLIP ViT-B/32 (Qdrant ONNX)

Used for offline Phase 2 image–text ranking.

- Vision: `Qdrant/clip-ViT-B-32-vision`
- Text: `Qdrant/clip-ViT-B-32-text`

ONNX weights are **not** committed (each file exceeds GitHub’s 100MB limit) and are **not** bundled in the installer.
Release builds download them into the app data `runtime-models` directory on first need.
Developers can also prefetch with:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/fetch-clip-models.ps1
```

SHA-256 (must match loader checks):

- vision `model.onnx`: `c68d3d9a200ddd2a8c8a5510b576d4c94d1ae383bf8b36dd8c084f94e1fb4d63`
- text `model.onnx`: `4dbe762b11e36488304471e439cde89da053ad7acaddbf9e096745d142ec8d8b`

License: MIT (OpenAI CLIP / Qdrant ONNX packaging). See upstream model cards.
