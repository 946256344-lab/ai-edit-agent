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
  role: 'user' | 'agent' | 'tool' | 'system'
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
  analysisStatus: 'queued' | 'analyzing' | 'ready' | 'failed'
  sourceAvailable: boolean
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
  createdAt: number
  updatedAt: number
}

export type AssetEvidence = {
  id: string
  displayName: string
  analysisStatus: string
  keyframes: Array<{ timeMs: number; imagePath: string }>
  ocrEvidence: Array<{ timeMs: number | null; text: string }>
  visualEvidence: Array<{ timeMs: number | null; subjects: string[]; scene: string | null; actions: string[]; products: string[]; qualityNotes: string[] }>
}

export type StoryboardVersion = {
  id: string
  projectId: string
  editingTaskId: string
  versionNumber: number
  brief: string
  title: string
  summary: string
  shots: Array<{ orderIndex: number; durationMs: number; purpose: string; onScreenText: string; assetId: string; sourceStartMs: number; sourceEndMs: number; reason: string }>
  createdAt: number
}

export type TimelineVersion = {
  id: string
  projectId: string
  storyboardVersionId: string
  versionNumber: number
  clips: Array<{ shotIndex: number; assetId: string; sourceStartMs: number; sourceEndMs: number; timelineStartMs: number; timelineEndMs: number; onScreenText: string }>
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
  message: string
  storyboard: StoryboardVersion | null
  timeline: TimelineVersion | null
  preview: PreviewResult | null
  jianyingDraft: JianyingDraftResult | null
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

export async function createMessage(conversationId: string, role: StoredMessage['role'], content: string) {
  requireDesktopRuntime()
  return invoke<StoredMessage>('create_message', { conversationId, role, content })
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

export async function listAssets(projectId: string) {
  requireDesktopRuntime()
  return invoke<StoredAsset[]>('list_assets', { projectId })
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

export async function executeAgentEdit(projectId: string, editingTaskId: string, storyboardVersionId: string | null, timelineVersionId: string | null, request: string) {
  requireDesktopRuntime()
  return invoke<AgentEditResult>('execute_agent_edit', { projectId, editingTaskId, storyboardVersionId, timelineVersionId, request })
}
