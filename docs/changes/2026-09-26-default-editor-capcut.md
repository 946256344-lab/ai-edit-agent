# 2026-09-26：默认输出编辑器改为 CapCut

## 结果

项目没有保存 `outputEditor` 时，默认输出编辑器不再固定为剪映：本机检测到 CapCut 草稿库，或两种编辑器都没检测到时，默认 CapCut；只检测到剪映时仍默认剪映。海外用户首次交付不会落到没装的剪映上。

## 范围

- `src-tauri/src/handoff/deliver.rs`：`read_output_editor` 按草稿库检测结果回落；目录列表和未指定编辑器的交付（含 Agent 自动交付）共用这一规则。
- 前端：`useArtifactWorkspaceController` 的初始值与读取失败回落、`EditorOutputPort` 无目录时的占位选项改为 CapCut；删除不再使用的 `output.jianying` 文案键。
- `docs/decisions.md`「输出端口可选编辑器」同步默认规则。

## 禁止变化

- 用户已选择的编辑器照常生效，默认值不写回 `settings_json`。
- 交付失败仍如实显示，不静默换到别的编辑器。

## 验证

- `cargo check`、`npx tsc -b`、`npm run lint`：通过。
- 真实桌面上 CapCut 草稿交付随 `docs/release-checklist.md` §1.3 实测。
