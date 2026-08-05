export type ModelProvider = 'openai-oauth' | 'custom-api' | 'local'

export type ToolStatus = 'queued' | 'running' | 'completed' | 'failed' | 'cancelled'

export type AgentToolName =
  | 'analyze_assets'
  | 'search_media_segments'
  | 'create_timeline_draft'
  | 'render_preview'
  | 'create_jianying_draft'

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
