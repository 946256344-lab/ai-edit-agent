# Bundled Python runtime notice

- CPython: Windows embeddable 3.12.10 x64 from python.org
- License: PSF License Agreement
- Source: https://www.python.org/ftp/python/3.12.10/python-3.12.10-embed-amd64.zip
- Zip SHA-256: `4acbed6dd1c744b0376e3b1cf57ce906f9dc9e95e68824584c8099a63025a3c3`
- Draft SDKs: `pyJianYingDraft==0.3.0`, `pycapcut==0.0.3` (and their pip dependencies)
- MediaInfo DLL 24.12 x64: redistributed only so `pymediainfo` can read local media duration
- MediaInfo zip SHA-256: `6189f3110c96ea0c53e6b56ae6356669a65c0c5809da896aa85b040508eed11b`

This tree is the Jianying/CapCut draft adapter runtime for Assembly Video Agent. It is not a general-purpose Python install. Fetch with `npm run python:fetch` before `npm run tauri:build`. Tesseract is not bundled here.
