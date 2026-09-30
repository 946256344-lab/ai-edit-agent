# Voycut — Third-party notices / 第三方许可声明

This distribution includes third-party software and model assets. Their rights remain with their respective owners. This inventory is not a legal compliance determination. Unknown facts are marked **待确认 / To be confirmed**.

## Bundled components / 随包组件

Versions below come from `scripts/fetch-*.ps1`, resource `NOTICE.md` files and the locked Cargo dependency, not the latest upstream release. All paths below are relative to the installation resource directory. Complete collected texts and the production dependency inventories are in `resources/third-party/ALL.txt`, also available from Settings → Third-party notices.

| Component | Pinned version | License | Copyright owner / attribution | Full text in this distribution |
| --- | --- | --- | --- | --- |
| FFmpeg / FFprobe, Gyan full_build | 8.1.2 | GPLv3 | Copyright (c) 2000-2026 the FFmpeg developers (binary version output); Windows build: Gyan Doshi | `resources/third-party/licenses/FFmpeg-GPL-3.0.txt`; original `resources/ffmpeg/LICENSE` |
| CPython Windows embeddable x64 | 3.12.10 | PSF License Agreement and included historical / third-party terms | Python Software Foundation and owners named in text | `resources/third-party/licenses/Python-3.12.10.txt`; original `resources/python/LICENSE.txt` |
| Tesseract, UB Mannheim Windows build | 5.4.0.20240606 | Apache-2.0 (engine) | Tesseract contributors; exact copyright notice 待确认 (payload contains standard terms); distribution: UB Mannheim | `resources/third-party/licenses/Tesseract.txt`; original `resources/tesseract/LICENSE` |
| Tesseract English traineddata | 待确认 (fetch script copies the pinned installer payload) | Apache-2.0 as declared in resource NOTICE | Tesseract contributors; exact attribution 待确认 | `resources/third-party/licenses/Apache-2.0.txt` |
| pyJianYingDraft | 0.3.0 | Apache-2.0 (supplied LICENSE; metadata not declared) | Copyright 2024 Gary Guan | `resources/third-party/components.txt`, Python distribution LICENSE text |
| pycapcut | 0.0.3 | **待确认 / To be confirmed** — package metadata declares no license | gary318 (package author; copyright owner 待确认) | No full license supplied by installed distribution; **待确认** |
| MediaInfo DLL x64 | 24.12 | BSD-2-Clause | MediaArea.net SARL | `resources/third-party/licenses/MediaInfo-24.12.txt` |
| Microsoft.AI.DirectML | 1.15.4 | Microsoft DirectML software license terms | Microsoft Corporation | `resources/third-party/licenses/DirectML-LICENSE.txt` and `DirectML-ThirdPartyNotices.txt`; originals under `resources/directml/` |
| BGE-small-zh-v1.5, Xenova ONNX conversion | v1.5; SHA-256 `69a0b846f4f116b5e6aabf9546ea6754d02264f3211a13a1bd69b31b8040749a` | MIT | Copyright (c) 2022 staoxiao (supplied LICENSE); model publisher: BAAI; conversion attribution / revision 待确认 | `resources/models/bge-small-zh-v1.5/LICENSE`, also collected in `components.txt` |
| CLIP ViT-B/32, Qdrant vision / text ONNX packaging | ViT-B/32; upstream revision 待确认; hashes below | MIT as declared in resource NOTICE / upstream model cards | OpenAI; Qdrant packaging copyright 待确认 | `resources/third-party/licenses/CLIP-MIT.txt` |
| ONNX Runtime native library (ort-sys) | 1.20.0, selected by ort-sys 2.0.0-rc.9 build.rs | MIT and included third-party terms | Microsoft Corporation and owners named in third-party text | `resources/third-party/licenses/ONNXRuntime-1.20.0.txt`, `ONNXRuntime-ThirdPartyNotices.txt` |
| npm production packages | Exact versions in `npm-production.json` | Per-package declarations | Per-package text / 待确认 | `resources/third-party/npm-production.txt` |
| Cargo Windows runtime crates | Exact versions in `cargo-production-windows.json` | Per-crate declarations | Per-crate text / 待确认 | `resources/third-party/cargo-production-windows.txt` |

The Python runtime also carries pip and SDK dependencies. Each installed distribution, its actual version (unfixed indirect dependencies are labelled as snapshots), metadata and license texts are listed in `components.txt`. Tesseract companion DLLs are individually inventoried there; their versions and license attribution are **待确认** where the fetch script and available payload do not establish them. CPython, NumPy, Pillow and ONNX Runtime notices contain additional embedded-library terms; retain these entire texts.

BGE/CLIP configuration and tokenizers are included in the normal bundle; ONNX weights download after installation unless `--full-models` / `ASSEMBLY_BUNDLE_FULL_MODELS=1` is selected. These notices cover both modes. CLIP pinned hashes: vision `c68d3d9a200ddd2a8c8a5510b576d4c94d1ae383bf8b36dd8c084f94e1fb4d63`; text `4dbe762b11e36488304471e439cde89da053ad7acaddbf9e096745d142ec8d8b`.

## FFmpeg source / 源码获取

The pinned binary archive is `ffmpeg-8.1.2-full_build.zip`, SHA-256 `b8cdefab5f50590a076c27c2b56b0294a0e6154faded28ba1ba05ebc4f801f57`. Its recorded upstream FFmpeg source is https://github.com/FFmpeg/FFmpeg/commit/38b88335f9 and the source archive can be obtained at https://github.com/FFmpeg/FFmpeg/archive/38b88335f9.tar.gz. Binary distributor and build information: https://github.com/GyanD/codexffmpeg/releases/tag/8.1.2 and https://www.gyan.dev/ffmpeg/builds/ .

This is an upstream source route, not a written source offer. Exact corresponding source for the full static build, including linked external libraries, patches and build scripts, must be confirmed with the distributor before release. The collected GPLv3 text is supplied regardless of the fetch script's original LICENSE fallback. Legal review remains required for redistribution obligations, linked libraries and codec patent licensing (including H.264 / HEVC). No compliance conclusion is made here.

## Regeneration / 重新生成

The `npm run tauri:build` / `tauri:build:full` wrapper collects the actual fetched runtime metadata and regenerates this inventory before invoking Tauri. Generation failure stops packaging. Existing resource merging retains this notice directory in both bundle modes.

Run `npm run notices:generate` (equivalent: `node scripts/generate-third-party-notices.mjs`) after the repository dependencies are present (including TypeScript as the generator's parser). The tool works offline: it parses non-type imports / exports and literal dynamic imports in `src/`, starts from referenced production packages, then walks their `package-lock.json` dependency closure (`dev`, `devOptional`, and `@types/*` compiler declarations excluded), checks installed versions and copies actual license texts. Unreferenced declarations `@moviemasher/moviemasher.js` and `jassub` are excluded. It uses `cargo metadata --offline --locked --filter-platform x86_64-pc-windows-msvc`; it walks normal dependencies from the application, excluding development / build-only dependencies and proc-macro compilation tools. The list is the production runtime dependency closure, not a claim that every crate survives linker elimination or every transitive npm package survives bundler tree shaking. Transitive native libraries embedded by those packages are covered by their supplied notices; unresolved attribution stays marked pending.

To refresh the Python and companion-runtime snapshot after fetching runtimes, run `node scripts/collect-runtime-notices.mjs` first. Default input is this worktree's `src-tauri/resources`; `--runtime-root <path>` can read an existing fetched resource tree without modifying it. Downloads in that collector fetch only the versioned upstream license texts recorded in `sources.json`; refresh requires network unless those text files are already present. These are public license files, not runtime binaries. Keep `THIRD_PARTY_NOTICES.md` aligned with fetch scripts when versions change, then regenerate and commit `resources/third-party/`.

For dependency packages that omit license files, run `node scripts/collect-dependency-license-supplements.mjs` after the first generation, then regenerate. This network collector uses Cargo `.cargo_vcs_info.json` commits for upstream license files and the Mozilla-published MPL-2.0 text for MPL packages. Checked-in supplemental texts and `supplemental-sources.json` let normal regeneration stay offline. Text files normalize BOM, CRLF and trailing whitespace without changing license wording.

Generated records preserve declared SPDX expressions without choosing among OR licenses. Copyright is extracted from supplied license / notice text; metadata authors are labelled as such, and are not substituted for unverified copyright holders. Missing license text, missing declarations and unresolved companion DLL terms require follow-up before release. Displaying notices and passing compilation do not close those release questions.

## Sources / 依据

- Repository: `scripts/fetch-ffmpeg.ps1`, `fetch-python.ps1`, `fetch-tesseract.ps1`, `fetch-directml.ps1`, `fetch-clip-models.ps1`, `src-tauri/resources/*/NOTICE.md`, `package-lock.json`, `src-tauri/Cargo.lock`.
- Python: https://www.python.org/downloads/release/python-31210/ ; draft SDK metadata copied from the fetched distributions.
- Models: https://huggingface.co/BAAI/bge-small-zh-v1.5 , https://huggingface.co/Qdrant/clip-ViT-B-32-vision , https://huggingface.co/Qdrant/clip-ViT-B-32-text .
- License text source URLs and SHA-256 values: `resources/third-party/sources.json`.
