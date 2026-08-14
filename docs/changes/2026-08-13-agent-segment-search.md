# Agent 片段级检索

- 新增 `search_asset_segments` 只读观察工具，要求查询词并可限定素材 ID。
- 结果绑定已有场景段的 `sourceStartMs/sourceEndMs`，单页最多 20 条并支持游标。
- OCR 只参与本地匹配，不返回正文；本地路径和媒体内容也不进入结果。
- 禁止使用、缺失、已变化和不可读素材不会成为候选。
