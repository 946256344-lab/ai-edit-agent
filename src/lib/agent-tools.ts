/**
 * 供 IDE 导航的 Agent 技能名称镜像。真正执行授权属于 Rust policy.rs 的白名单；
 * 白名单变化时必须同步版本化 fixture。这些是模型内部技能，不是 React 可直接调用的命令。
 */
export type AgentObservationToolName =
  | 'get_edit_status'
  | 'get_asset_health_summary'
  | 'list_assets'
  | 'search_assets'
  | 'search_asset_segments'
  | 'search_music'
  | 'get_storyboard'
  | 'get_timeline'
  | 'get_text_capabilities'

export type AgentSideEffectToolName =
  | 'download_music'
  | 'use_online_music'
  | 'request_asset_analysis'
  | 'generate_storyboard'
  | 'create_timeline_draft'
  | 'replace_clips'
  | 'change_clip_duration'
  | 'reorder_clips'
  | 'replace_text_tracks'
  | 'replace_music_tracks'
  | 'render_preview'
  | 'create_jianying_draft'

export type AgentToolName = AgentObservationToolName | AgentSideEffectToolName
