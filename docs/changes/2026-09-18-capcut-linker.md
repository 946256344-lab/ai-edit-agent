# 2026-09-18：CapCut 投放链接器

## 结果

CapCut 成为与剪映平行的投放端口。草稿库从该设备 `%LOCALAPPDATA%\CapCut\User Data\Projects\com.lveditor.draft\root_meta_info.json` 读取 `draft_root_path`，不写死盘符。换一台电脑后，只要那台机器打开过 CapCut 并有本地草稿库，就能自己识别；没装或没打开过则拒绝，不猜路径。

## 范围

- `src-tauri/src/capcut.rs`：发现、创建、延迟注册
- `src-tauri/scripts/create_jianying_draft.py`：`editor=capcut` 绑定 pycapcut
- `handoff/deliver.rs`：可选交付
- `docs/api.md`、`docs/architecture.md`、`docs/decisions.md`

## 禁止变化

- 不覆盖已有 CapCut / 剪映草稿
- 不从 CapCut 回读
- 不把剪映草稿改名丢进 CapCut
- 不接 CapCut 云 API

## 验证

- `py -3 -m unittest test_create_jianying_draft.py`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- handoff capcut`
- `npm run harness:check`
