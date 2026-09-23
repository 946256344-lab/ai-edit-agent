# FFmpeg runtime notice

- Binaries: Gyan `ffmpeg-8.1.2-full_build` (`ffmpeg.exe`, `ffprobe.exe`)
- Upstream: https://github.com/GyanD/codexffmpeg/releases/tag/8.1.2
- FFmpeg source: https://github.com/FFmpeg/FFmpeg/commit/38b88335f9
- License: GPLv3 (Gyan Windows full_build)
- Zip SHA-256: `b8cdefab5f50590a076c27c2b56b0294a0e6154faded28ba1ba05ebc4f801f57`

These executables are redistributed only as the local media runtime for Assembly Video Agent (analysis, preview, quality checks). They are not linked into the application binary.

The `.exe` files exceed GitHub’s 100MB limit and are gitignored. Fetch with `npm run ffmpeg:fetch` before `npm run tauri:build`. Tesseract and Python are bundled from their own resource directories.
