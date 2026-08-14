export type ModelProvider = 'openai-oauth' | 'custom-api' | 'local'

export type ToolStatus = 'queued' | 'running' | 'completed' | 'partially_completed' | 'failed' | 'cancelled' | 'needs_clarification' | 'needs_review'

export type AgentToolName =
  | 'analyze_assets'
  | 'search_media_segments'
  | 'search_assets'
  | 'get_text_capabilities'
  | 'generate_storyboard'
  | 'create_timeline_draft'
  | 'replace_clips'
  | 'change_clip_duration'
  | 'reorder_clips'
  | 'replace_text_tracks'
  | 'render_preview'
  | 'create_jianying_draft'
  | 'request_clarification'
  | 'no_action'
  | 'replace_timeline_clip'

export type ToolInvocation<TInput, TResult> = {
  id: string
  name: AgentToolName
  status: ToolStatus
  input: TInput
  result?: TResult
  error?: string
  createdAt: string
}

export type TimelineDraftInput = {
  projectId: string
  storyboardVersionId: string
}

export type JianyingDraftInput = {
  timelineVersionId: string
}

export type JianyingDraftResult = {
  draftDirectory: string
  draftContentPath: string
}

/**
 * The desktop backend implements this interface through named, locally
 * validated Tauri commands.
 */
export interface VideoAgentTools {
  analyzeAssets(projectId: string, assetIds: string[]): Promise<ToolInvocation<{ projectId: string; assetIds: string[] }, unknown>>
  createTimelineDraft(input: TimelineDraftInput): Promise<ToolInvocation<TimelineDraftInput, { timelineVersionId: string }>>
  renderPreview(timelineVersionId: string): Promise<ToolInvocation<{ timelineVersionId: string }, { previewPath: string }>>
  createJianyingDraft(input: JianyingDraftInput): Promise<ToolInvocation<JianyingDraftInput, JianyingDraftResult>>
}
