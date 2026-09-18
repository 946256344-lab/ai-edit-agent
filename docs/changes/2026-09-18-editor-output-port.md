# 2026-09-18：输出端口可选编辑器

## 结果

工作台顶部可选输出编辑器，选择记在项目 `settings_json.outputEditor`。剪映仍写本机草稿；FCPXML / OTIO 写出导入文件；CapCut 列出但拒绝交付。自动预览后的交付走当前选择，不覆盖已有工程。

## 范围

- `src-tauri/src/handoff/`：目录、项目选择、`deliver_to_editor`、FCPXML/OTIO 写出
- `src/components/EditorOutputPort.tsx`、成果/顶栏交付按钮
- Agent `generate_storyboard` 收尾改走所选端口
- `docs/api.md`、`docs/architecture.md`、`docs/decisions.md`

## 禁止变化

- 不覆盖已有剪映草稿
- 不从目标编辑器回读
- 不把 OTIO 当成内部时间线
- 不实现 CapCut 草稿写出

## 验证

- `cargo check --manifest-path src-tauri/Cargo.toml`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- handoff`
- `npm run lint`
- `npm run harness:check`
