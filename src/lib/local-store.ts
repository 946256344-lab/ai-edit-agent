// 前端唯一的应用 Tauri command bridge：集中公开类型和静态命令名，不承载 UI 状态。
import { invoke } from '@tauri-apps/api/core'

export type StoreStatus = { databaseReady: boolean; schemaVersion: number }
export type ExperimentalOAuthStatus = {
  state: 'disconnected' | 'pending' | 'connected' | 'failed'
  message: string | null
  experimental: boolean
}

export type ExperimentalOAuthStart = {
  authorizationUrl: string
  experimental: boolean
}

export type CustomApiStatus = {
  state: 'connected' | 'disconnected' | 'failed'
  message: string | null
  baseUrl: string | null
  model: string | null
  coarseVisualModel: string | null
}

export type StoredProject = { id: string; name: string; createdAt: number; updatedAt: number }

export type StoredConversation = {
  id: string
  projectId: string
  editingTaskId: string
  title: string
  summary: string
  status: string
  createdAt: number
  updatedAt: number
}

export type StoredEditingTask = {
  id: string
  projectId: string
  title: string
  brief: string
  createdAt: number
  updatedAt: number
}

export type StoredEditingSession = {
  id: string
  projectId: string
  conversationId: string | null
  title: string
  brief: string
  summary: string
  status: string
  createdAt: number
  updatedAt: number
}

export type StoredMessage = {
  id: string
  conversationId: string
  role: 'user' | 'assistant' | 'agent' | 'tool' | 'system'
  content: string
  createdAt: number
}

export type StoredAsset = {
  id: string
  projectId: string
  kind: 'video' | 'image' | 'audio' | 'other'
  displayName: string
  folderName: string | null
  relativePath: string | null
  directoryKey: string | null
  analysisStatus: 'queued' | 'analyzing' | 'ready' | 'failed'
  visualAnalysisStatus: 'queued' | 'running' | 'ready' | 'failed' | 'skipped'
  durationMs: number | null
  width: number | null
  height: number | null
  fps: number | null
  hasAudio: boolean
  thumbnailPath: string | null
  keyframeCount: number
  sceneCount: number
  ocrTextCount: number
  visualTagCount: number
  favorite: boolean
  rating: number
  note: string
  excluded: boolean
  userTags: string[]
  collectionIds: string[]
  sourceHealthStatus: 'unchecked' | 'online' | 'missing' | 'changed' | 'unreadable'
  sourceHealthCheckedAt: number | null
  createdAt: number
  updatedAt: number
}

export type AssetDirectory = {
  key: string
  name: string
  parentKey: string | null
  directAssetCount: number
}

export type AssetPage = {
  items: StoredAsset[]
  total: number
  offset: number
  limit: number
  directories: AssetDirectory[]
  unfiledCount: number
  counts: { total: number; ready: number; analyzing: number; queued: number; failed: number }
}

export type AssetTaskCenter = {
  technical: { queued: number; running: number; failed: number; skipped: number }
  visual: { queued: number; running: number; failed: number; skipped: number }
  recentFailures: Array<{ assetId: string; displayName: string; stage: 'technical' | 'visual'; reasonCode: string; updatedAt: number }>
}
export type AssetHealthScanSummary = { total: number; unchecked: number; online: number; missing: number; changed: number; unreadable: number; checked: number; activeTaskId: string | null; activeTaskStatus: string | null }

export type BatchAssetActionResult = { requestedCount: number; updatedCount: number; skippedCount: number }
export type AssetCollection = { id: string; projectId: string; name: string; assetCount: number; createdAt: number; updatedAt: number }

export type AssetRelinkPreview = {
  matches: Array<{ assetId: string; displayName: string }>
  unmatchedCount: number
}

export type AssetRelinkResult = {
  relinkedCount: number
}
export type CollectProjectMediaPreview = { collectableCount: number; unavailableCount: number; totalBytes: number }
export type CollectProjectMediaResult = { copiedCount: number; unavailableCount: number; outputDirectory: string }

export type AssetEvidence = {
  id: string
  displayName: string
  analysisStatus: string
  durationMs: number | null
  visualAnalysisStatus: 'queued' | 'running' | 'ready' | 'failed' | 'skipped'
  keyframes: Array<{ timeMs: number; imagePath: string }>
  ocrEvidence: Array<{ timeMs: number | null; text: string }>
  visualEvidence: Array<{ timeMs: number | null; subjects: string[]; scene: string | null; actions: string[]; products: string[]; qualityNotes: string[] }>
  visualAnalysisNote: string | null
}

export type StoryboardVersion = {
  id: string
  projectId: string
  editingTaskId: string
  versionNumber: number
  brief: string
  title: string
  summary: string
  targetDurationMs: number
  scriptMode: 'full_script' | 'key_message'
  beats: Array<{ id: string; purpose: string; requiredVisual: string }>
  uncoveredBeatIds: string[]
  shots: Array<{ orderIndex: number; durationMs: number; purpose: string; onScreenText: string; assetId: string; sourceStartMs: number; sourceEndMs: number; reason: string; beatId: string; matchLevel: 'direct' | 'contextual' }>
  createdAt: number
}

export type TextAnimation = { templateId: string; durationMs: number; intensity: number }
export type TextCue = {
  id: string
  templateId: string | null
  startMs: number
  endMs: number
  text: string
  style: { fontKey: string; fontSize: number; bold: boolean; color: string; strokeColor: string | null; strokeWidth: number; shadow: boolean; backgroundColor: string | null; alignment: string; letterSpacing: number; lineSpacing: number }
  layout: { anchor: string; x: number; y: number; maxWidth: number; safeArea: string }
  entrance: TextAnimation | null
  exit: TextAnimation | null
  loopAnimation: TextAnimation | null
  jianyingCompatibility: 'verified' | 'local_preview_only'
}
export type TextTrack = { id: string; role: 'subtitle' | 'headline' | 'callout' | 'cta' | 'label'; layer: number; enabled: boolean; cues: TextCue[] }

export type TimelineVersion = {
  id: string
  projectId: string
  storyboardVersionId: string
  versionNumber: number
  clips: Array<{ shotIndex: number; assetId: string; sourceStartMs: number; sourceEndMs: number; timelineStartMs: number; timelineEndMs: number; onScreenText: string }>
  textTracks: TextTrack[]
  qualityReport: PreviewQualityReport | null
  createdAt: number
}

export type PreviewQualityReport = { checks: Array<{ category: string; severity: string; message: string; shotIndices: number[] }> }

export type PreviewResult = { timelineVersionId: string; previewPath: string; qualityReport: PreviewQualityReport }
export type JianyingDraftResult = {
  draftDirectory: string
  draftContentPath: string
  registrationStatus: 'pending' | 'registered'
}

export type JianyingRegistrationStatus = {
  timelineVersionId: string
  draftName: string
  status: 'pending' | 'registered' | 'failed'
}

export type LatestTimeline = { timeline: TimelineVersion; preview: PreviewResult | null }

export type AgentEditResult = {
  agentTaskId: string
  message: string
  storyboard: StoryboardVersion | null
  timeline: TimelineVersion | null
  preview: PreviewResult | null
  jianyingDraft: JianyingDraftResult | null
}

export type AgentEditEvent = {
  agentTaskId: string
  status: 'completed' | 'partially_completed' | 'failed' | 'needs_clarification'
  result: AgentEditResult
}

export type ConversationTurnResult =
  | { kind: 'immediate'; status: 'response' | 'clarification'; message: string }
  | { kind: 'run'; agentTaskId: string }

export type TaskRouteResult = {
  action: 'continue_current' | 'switch_existing' | 'create_new' | 'clarify'
  taskId: string | null
  conversationId: string | null
  confidence: number
  question: string | null
  suggestedTitle: string | null
  reasonCode: string
  deferredRequest: string | null
  routeReceipt: string | null
}

export type StoredAgentTask = {
  id: string
  projectId: string
  editingTaskId: string | null
  conversationId: string | null
  toolName: string
  status: 'queued' | 'running' | 'completed' | 'partially_completed' | 'failed' | 'cancelled' | 'needs_clarification' | 'needs_review'
  input: Record<string, unknown>
  result: Record<string, unknown> | null
  error: string | null
  createdAt: number
  updatedAt: number
}

export type StoredAgentRunStep = {
  id: string
  projectId: string
  editingTaskId: string
  agentTaskId: string
  stepNumber: number
  toolName: string
  status: 'queued' | 'running' | 'completed' | 'failed'
  artifactType: string | null
  artifactId: string | null
  errorCode: string | null
  createdAt: number
  startedAt: number | null
  completedAt: number | null
  updatedAt: number
}

export type StoredAgentDiagnostic = {
  id: string
  projectId: string
  editingTaskId: string
  conversationId: string
  agentTaskId: string
  stepNumber: number | null
  kind: 'model_response' | 'tool_error' | 'pipeline_error'
  content: string
  createdAt: number
}

export type StoredOperationLog = {
  id: string
  projectId: string
  editingTaskId: string | null
  conversationId: string | null
  agentTaskId: string | null
  actor: 'user' | 'agent' | 'system'
  operationType: string
  entityType: string
  entityId: string
  before: Record<string, unknown> | null
  after: Record<string, unknown> | null
  createdAt: number
}

export function isDesktopRuntime() {
  return '__TAURI_INTERNALS__' in window
}

function requireDesktopRuntime() {
  if (!isDesktopRuntime()) {
    throw new Error('The local project store is available only in the desktop application.')
  }
}

export async function initializeLocalStore() {
  requireDesktopRuntime()
  return invoke<StoreStatus>('initialize_local_store')
}

export async function getExperimentalOpenAIOAuthStatus() {
  requireDesktopRuntime()
  return invoke<ExperimentalOAuthStatus>('get_experimental_openai_oauth_status')
}

export async function startExperimentalOpenAIOAuth() {
  requireDesktopRuntime()
  return invoke<ExperimentalOAuthStart>('start_experimental_openai_oauth')
}

export async function clearExperimentalOpenAIOAuth() {
  requireDesktopRuntime()
  return invoke<ExperimentalOAuthStatus>('clear_experimental_openai_oauth')
}

export async function getCustomApiStatus() {
  requireDesktopRuntime()
  return invoke<CustomApiStatus>('get_custom_api_status')
}

export async function saveCustomApi(baseUrl: string, model: string, coarseVisualModel: string, apiKey: string) {
  requireDesktopRuntime()
  return invoke<CustomApiStatus>('save_custom_api', { baseUrl, model, coarseVisualModel, apiKey })
}

export async function clearCustomApi() {
  requireDesktopRuntime()
  return invoke<CustomApiStatus>('clear_custom_api')
}

export async function listProjects() {
  requireDesktopRuntime()
  return invoke<StoredProject[]>('list_projects')
}

export async function createProject(name: string) {
  requireDesktopRuntime()
  return invoke<StoredProject>('create_project', { name })
}

export async function listConversations(projectId: string, editingTaskId?: string) {
  requireDesktopRuntime()
  return invoke<StoredConversation[]>('list_conversations', { projectId, editingTaskId })
}

export async function createConversation(projectId: string, editingTaskId: string, title: string) {
  requireDesktopRuntime()
  return invoke<StoredConversation>('create_conversation', { projectId, editingTaskId, title })
}

export async function createEditingTask(projectId: string, title: string) {
  requireDesktopRuntime()
  return invoke<StoredEditingTask>('create_editing_task', { projectId, title })
}

export async function createEditingSession(projectId: string, title: string) {
  requireDesktopRuntime()
  return invoke<StoredEditingSession>('create_editing_session', { projectId, title })
}

export async function listEditingSessions(projectId: string) {
  requireDesktopRuntime()
  return invoke<StoredEditingSession[]>('list_editing_sessions', { projectId })
}

export async function listEditingTasks(projectId: string) {
  requireDesktopRuntime()
  return invoke<StoredEditingTask[]>('list_editing_tasks', { projectId })
}

export async function updateEditingTaskBrief(editingTaskId: string, brief: string) {
  requireDesktopRuntime()
  return invoke<void>('update_editing_task_brief', { editingTaskId, brief })
}

export async function listMessages(conversationId: string) {
  requireDesktopRuntime()
  return invoke<StoredMessage[]>('list_messages', { conversationId })
}

export async function createMessage(conversationId: string, role: StoredMessage['role'], content: string, routeReceipt?: string) {
  requireDesktopRuntime()
  return invoke<StoredMessage>('create_message', { conversationId, role, content, routeReceipt })
}

export async function setConversationStatus(conversationId: string, status: StoredConversation['status']) {
  requireDesktopRuntime()
  return invoke<void>('set_conversation_status', { conversationId, status })
}

export async function importAssets(projectId: string, sourceReferences: string[]) {
  requireDesktopRuntime()
  return invoke<StoredAsset[]>('import_assets', { projectId, sourceReferences })
}

export async function importAssetFolder(projectId: string, sourceDirectory: string) {
  requireDesktopRuntime()
  return invoke<StoredAsset[]>('import_asset_folder', { projectId, sourceDirectory })
}

export async function previewAssetRelink(projectId: string, sourceDirectory: string) {
  requireDesktopRuntime()
  return invoke<AssetRelinkPreview>('preview_asset_relink', { projectId, sourceDirectory })
}

export async function confirmAssetRelink(projectId: string, sourceDirectory: string, assetIds: string[], preserveAnalysis: boolean) {
  requireDesktopRuntime()
  return invoke<AssetRelinkResult>('confirm_asset_relink', { projectId, sourceDirectory, assetIds, preserveAnalysis })
}

export async function previewCollectProjectMedia(projectId: string) { requireDesktopRuntime(); return invoke<CollectProjectMediaPreview>('preview_collect_project_media', { projectId }) }
export async function collectProjectMedia(projectId: string, destinationDirectory: string) { requireDesktopRuntime(); return invoke<CollectProjectMediaResult>('collect_project_media', { projectId, destinationDirectory }) }

export async function listAssets(projectId: string) {
  requireDesktopRuntime()
  return invoke<StoredAsset[]>('list_assets', { projectId })
}

export async function listAssetPage(projectId: string, options: { search?: string; kind?: StoredAsset['kind']; analysisStatus?: StoredAsset['analysisStatus']; visualStatus?: StoredAsset['visualAnalysisStatus'] | 'storyboard-ready'; directoryKey?: string; userFilter?: 'favorite' | 'excluded' | 'available'; collectionId?: string; offset: number; limit: number }) {
  requireDesktopRuntime()
  return invoke<AssetPage>('list_asset_page', { projectId, ...options })
}

export async function getAssetTaskCenter(projectId: string) {
  requireDesktopRuntime()
  return invoke<AssetTaskCenter>('get_asset_task_center', { projectId })
}

export async function getAssetHealthScanSummary(projectId: string) { requireDesktopRuntime(); return invoke<AssetHealthScanSummary>('get_asset_health_scan_summary', { projectId }) }
export async function startAssetHealthScan(projectId: string) { requireDesktopRuntime(); return invoke<{ taskId: string }>('start_asset_health_scan', { projectId }) }
export async function cancelAssetHealthScan(projectId: string, taskId: string) { requireDesktopRuntime(); return invoke<void>('cancel_asset_health_scan', { projectId, taskId }) }

export async function retryAssetAnalysisBatch(projectId: string, assetIds: string[]) {
  requireDesktopRuntime()
  return invoke<BatchAssetActionResult>('retry_asset_analysis_batch', { projectId, assetIds })
}

export async function skipAssetVisualAnalysisBatch(projectId: string, assetIds: string[]) {
  requireDesktopRuntime()
  return invoke<BatchAssetActionResult>('skip_asset_visual_analysis_batch', { projectId, assetIds })
}

export async function updateAssetUserMetadataBatch(projectId: string, assetIds: string[], fields: { favorite?: boolean; rating?: number; note?: string; excluded?: boolean }) {
  requireDesktopRuntime()
  return invoke<BatchAssetActionResult>('update_asset_user_metadata_batch', { projectId, assetIds, ...fields })
}

export async function addAssetTagBatch(projectId: string, assetIds: string[], tag: string) {
  requireDesktopRuntime()
  return invoke<BatchAssetActionResult>('add_asset_tag_batch', { projectId, assetIds, tag })
}

export async function removeAssetTagBatch(projectId: string, assetIds: string[], tag: string) {
  requireDesktopRuntime()
  return invoke<BatchAssetActionResult>('remove_asset_tag_batch', { projectId, assetIds, tag })
}

export async function createAssetCollection(projectId: string, name: string) {
  requireDesktopRuntime()
  return invoke<AssetCollection>('create_asset_collection', { projectId, name })
}

export async function listAssetCollections(projectId: string) {
  requireDesktopRuntime()
  return invoke<AssetCollection[]>('list_asset_collections', { projectId })
}

export async function addAssetsToCollection(projectId: string, collectionId: string, assetIds: string[]) {
  requireDesktopRuntime()
  return invoke<BatchAssetActionResult>('add_assets_to_collection', { projectId, collectionId, assetIds })
}

export async function getAssetEvidence(assetId: string) {
  requireDesktopRuntime()
  return invoke<AssetEvidence>('get_asset_evidence', { assetId })
}

export async function generateStoryboard(projectId: string, editingTaskId: string, brief: string) {
  requireDesktopRuntime()
  return invoke<StoryboardVersion>('generate_storyboard', { projectId, editingTaskId, brief })
}

export async function getLatestStoryboard(projectId: string, editingTaskId: string) {
  requireDesktopRuntime()
  return invoke<StoryboardVersion | null>('get_latest_storyboard', { projectId, editingTaskId })
}

export async function createTimelineDraft(projectId: string, storyboardVersionId: string) {
  requireDesktopRuntime()
  return invoke<TimelineVersion>('create_timeline_draft', { projectId, storyboardVersionId })
}

export async function getLatestTimeline(projectId: string, storyboardVersionId: string) {
  requireDesktopRuntime()
  return invoke<LatestTimeline | null>('get_latest_timeline', { projectId, storyboardVersionId })
}

export async function listTimelineVersions(projectId: string, editingTaskId: string, storyboardVersionId: string) {
  requireDesktopRuntime()
  return invoke<TimelineVersion[]>('list_timeline_versions', { projectId, editingTaskId, storyboardVersionId })
}

export async function listAgentTasks(projectId: string, editingTaskId: string, conversationId?: string) {
  requireDesktopRuntime()
  return invoke<StoredAgentTask[]>('list_agent_tasks', { projectId, editingTaskId, conversationId })
}

export async function listAgentRunSteps(projectId: string, editingTaskId: string, agentTaskId: string) {
  requireDesktopRuntime()
  return invoke<StoredAgentRunStep[]>('list_agent_run_steps', { projectId, editingTaskId, agentTaskId })
}

export async function listAgentDiagnostics(projectId: string, editingTaskId: string, agentTaskId: string) {
  requireDesktopRuntime()
  return invoke<StoredAgentDiagnostic[]>('list_agent_diagnostics', { projectId, editingTaskId, agentTaskId })
}

export async function listOperationLogs(projectId: string, editingTaskId: string, agentTaskId?: string) {
  requireDesktopRuntime()
  return invoke<StoredOperationLog[]>('list_operation_logs', { projectId, editingTaskId, agentTaskId })
}

export async function renderPreview(timelineVersionId: string) {
  requireDesktopRuntime()
  return invoke<PreviewResult>('render_preview', { timelineVersionId })
}

export async function createJianyingDraft(timelineVersionId: string) {
  requireDesktopRuntime()
  return invoke<JianyingDraftResult>('create_jianying_draft', { timelineVersionId })
}

export async function getJianyingRegistrationStatus(timelineVersionId: string) {
  requireDesktopRuntime()
  return invoke<JianyingRegistrationStatus | null>('get_jianying_registration_status', { timelineVersionId })
}

export async function executeAgentEdit(projectId: string, editingTaskId: string, conversationId: string, storyboardVersionId: string | null, timelineVersionId: string | null, request: string, routeReceipt: string) {
  requireDesktopRuntime()
  return invoke<string>('execute_agent_edit', { projectId, editingTaskId, conversationId, storyboardVersionId, timelineVersionId, request, routeReceipt })
}

export async function submitConversationTurn(projectId: string, editingTaskId: string, conversationId: string, storyboardVersionId: string | null, timelineVersionId: string | null, request: string, routeReceipt: string) {
  requireDesktopRuntime()
  return invoke<ConversationTurnResult>('submit_conversation_turn', { projectId, editingTaskId, conversationId, storyboardVersionId, timelineVersionId, request, routeReceipt })
}

export async function confirmStoryboardAndPreview(projectId: string, editingTaskId: string, conversationId: string, storyboardVersionId: string) {
  requireDesktopRuntime()
  return invoke<string>('confirm_storyboard_and_preview', { projectId, editingTaskId, conversationId, storyboardVersionId })
}

export async function resolveConversationTask(projectId: string, activeEditingTaskId: string | null, request: string) {
  requireDesktopRuntime()
  return invoke<TaskRouteResult>('resolve_conversation_task', { projectId, activeEditingTaskId, request })
}
