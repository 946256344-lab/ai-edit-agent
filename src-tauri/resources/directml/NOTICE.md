# DirectML runtime notice

- Binary: Microsoft `DirectML.dll` 1.15.4 (x64), from NuGet package `Microsoft.AI.DirectML` 1.15.4
- Upstream: https://www.nuget.org/packages/Microsoft.AI.DirectML/1.15.4
- License: Microsoft DirectML license (`LICENSE.txt`), third-party notices in `ThirdPartyNotices.txt`
- Package SHA-256: `4e7cb7ddce8cf837a7a75dc029209b520ca0101470fcdf275c1f49736a3615b9`
- DLL SHA-256: `9c9e6d822561c6c41b90e6994b3e8857cf1d66dbfb1e0c4c799c7c89b4e92da1` (Authenticode: Microsoft Corporation)

Redistributed only so the bundled ONNX Runtime can run the local BGE/CLIP models on the GPU. The Windows inbox DirectML (1.8) is too old for ONNX Runtime 1.20. The app loads this DLL by full path at runtime; without it, local models run on the CPU.

The binary is gitignored. Fetch with `npm run directml:fetch` before running or building.
