/**
 * IDE-facing mirror of the bounded Agent skill space. Execution authority is
 * the Rust OBSERVATION_TOOLS/EDIT_TOOLS whitelist; the versioned fixture under
 * src-tauri/tests/fixtures must change with that whitelist. These names are
 * internal model skills, not commands that React may invoke directly.
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

/** Canonical control actions defined by the versioned contract fixture. */
export type AgentControlToolName = 'ask_user' | 'finish'

/** Accepted aliases; the current production prompt still advertises `no_action`. */
export type AgentControlToolAlias = 'no_action' | 'done'

export type AgentToolName = AgentObservationToolName | AgentSideEffectToolName | AgentControlToolName

export type AgentAcceptedToolName = AgentToolName | AgentControlToolAlias
