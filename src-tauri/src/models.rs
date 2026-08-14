use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreStatus {
    pub database_ready: bool,
    pub schema_version: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: String,
    pub project_id: String,
    pub editing_task_id: String,
    pub title: String,
    pub summary: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditingTask {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub brief: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditingSession {
    pub id: String,
    pub project_id: String,
    pub conversation_id: Option<String>,
    pub title: String,
    pub brief: String,
    pub summary: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskRouteResult {
    pub action: String,
    pub task_id: Option<String>,
    pub conversation_id: Option<String>,
    pub confidence: f64,
    pub question: Option<String>,
    pub suggested_title: Option<String>,
    pub reason_code: String,
    pub deferred_request: Option<String>,
    pub route_receipt: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub role: String,
    pub content: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PendingClarificationSnapshot {
    pub id: String,
    pub source_kind: String,
    pub source_agent_task_id: Option<String>,
    pub goal: Option<String>,
    pub question: String,
    pub created_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: String,
    pub project_id: String,
    pub kind: String,
    pub display_name: String,
    pub folder_name: Option<String>,
    pub relative_path: Option<String>,
    pub analysis_status: String,
    pub visual_analysis_status: String,
    pub duration_ms: Option<i64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub fps: Option<f64>,
    pub has_audio: bool,
    pub thumbnail_path: Option<String>,
    pub keyframe_count: usize,
    pub scene_count: usize,
    pub ocr_text_count: usize,
    pub visual_tag_count: usize,
    pub favorite: bool,
    pub rating: i64,
    pub note: String,
    pub excluded: bool,
    pub user_tags: Vec<String>,
    pub collection_ids: Vec<String>,
    pub source_health_status: String,
    pub source_health_checked_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetPage {
    pub items: Vec<Asset>,
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub folders: Vec<String>,
    pub counts: AssetStatusCounts,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetStatusCounts {
    pub total: usize,
    pub ready: usize,
    pub analyzing: usize,
    pub queued: usize,
    pub failed: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetTaskCenter {
    pub technical: AssetTaskStageCounts,
    pub visual: AssetTaskStageCounts,
    pub recent_failures: Vec<AssetTaskFailure>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetTaskStageCounts {
    pub queued: usize,
    pub running: usize,
    pub failed: usize,
    pub skipped: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetTaskFailure {
    pub asset_id: String,
    pub display_name: String,
    pub stage: String,
    pub reason_code: String,
    pub updated_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchAssetActionResult {
    pub requested_count: usize,
    pub updated_count: usize,
    pub skipped_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetCollection {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub asset_count: usize,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetRelinkMatch {
    pub asset_id: String,
    pub display_name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetRelinkPreview {
    pub matches: Vec<AssetRelinkMatch>,
    pub unmatched_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetRelinkResult {
    pub relinked_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetHealthScanSummary {
    pub total: usize,
    pub unchecked: usize,
    pub online: usize,
    pub missing: usize,
    pub changed: usize,
    pub unreadable: usize,
    pub checked: usize,
    pub active_task_id: Option<String>,
    pub active_task_status: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetHealthScanStart {
    pub task_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectProjectMediaPreview {
    pub collectable_count: usize,
    pub unavailable_count: usize,
    pub total_bytes: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectProjectMediaResult {
    pub copied_count: usize,
    pub unavailable_count: usize,
    pub output_directory: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetEvidence {
    pub id: String,
    pub display_name: String,
    pub analysis_status: String,
    pub duration_ms: Option<i64>,
    pub visual_analysis_status: String,
    pub keyframes: Vec<KeyframeMetadata>,
    pub ocr_evidence: Vec<OcrEvidence>,
    pub visual_evidence: Vec<VisualEvidence>,
    pub visual_analysis_note: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryboardVersion {
    pub id: String,
    pub project_id: String,
    pub editing_task_id: String,
    pub version_number: i64,
    pub brief: String,
    pub title: String,
    pub summary: String,
    pub target_duration_ms: i64,
    pub script_mode: String,
    pub beats: Vec<StoryboardBeat>,
    pub uncovered_beat_ids: Vec<String>,
    pub shots: Vec<StoryboardShot>,
    pub created_at: i64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineVersion {
    pub id: String,
    pub project_id: String,
    pub storyboard_version_id: String,
    pub version_number: i64,
    pub clips: Vec<TimelineClip>,
    pub text_tracks: Vec<TextTrack>,
    pub music_tracks: Vec<MusicTrack>,
    pub quality_report: Option<PreviewQualityReport>,
    pub created_at: i64,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicTrack {
    pub id: String,
    pub enabled: bool,
    pub cues: Vec<MusicCue>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicCue {
    pub id: String,
    pub asset_id: String,
    pub source_start_ms: i64,
    pub source_end_ms: i64,
    pub timeline_start_ms: i64,
    pub timeline_end_ms: i64,
    #[serde(default)]
    pub loop_enabled: bool,
    pub volume: f64,
    #[serde(default)]
    pub fade_in_ms: i64,
    #[serde(default)]
    pub fade_out_ms: i64,
    #[serde(default = "default_music_jianying_compatibility")]
    pub jianying_compatibility: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub license_url: Option<String>,
    #[serde(default)]
    pub attribution: Option<String>,
}

fn default_music_jianying_compatibility() -> String {
    "not_deliverable".to_owned()
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextTrack {
    pub id: String,
    pub role: String,
    pub layer: i64,
    pub enabled: bool,
    pub cues: Vec<TextCue>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextCue {
    pub id: String,
    #[serde(default)]
    pub template_id: Option<String>,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    #[serde(default)]
    pub style: TextStyle,
    #[serde(default)]
    pub layout: TextLayout,
    pub entrance: Option<TextAnimation>,
    pub exit: Option<TextAnimation>,
    pub loop_animation: Option<TextAnimation>,
    #[serde(default = "default_jianying_compatibility")]
    pub jianying_compatibility: String,
}

fn default_jianying_compatibility() -> String {
    "local_preview_only".to_owned()
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextStyle {
    pub font_key: String,
    pub font_size: f64,
    pub bold: bool,
    pub color: String,
    pub stroke_color: Option<String>,
    pub stroke_width: f64,
    pub shadow: bool,
    pub background_color: Option<String>,
    pub alignment: String,
    pub letter_spacing: i64,
    pub line_spacing: i64,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_key: "jianying_default".to_owned(),
            font_size: 0.055,
            bold: true,
            color: "#FFFFFF".to_owned(),
            stroke_color: None,
            stroke_width: 0.0,
            shadow: false,
            background_color: None,
            alignment: "center".to_owned(),
            letter_spacing: 0,
            line_spacing: 0,
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextLayout {
    pub anchor: String,
    pub x: f64,
    pub y: f64,
    pub max_width: f64,
    pub safe_area: String,
}

impl Default for TextLayout {
    fn default() -> Self {
        Self {
            anchor: "bottom".to_owned(),
            x: 0.5,
            y: 0.82,
            max_width: 0.86,
            safe_area: "title_safe".to_owned(),
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextAnimation {
    pub template_id: String,
    pub duration_ms: i64,
    pub intensity: f64,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewQualityReport {
    pub checks: Vec<PreviewQualityCheck>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewQualityCheck {
    pub category: String,
    pub severity: String,
    pub message: String,
    pub shot_indices: Vec<i64>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineClip {
    pub shot_index: i64,
    pub asset_id: String,
    pub source_start_ms: i64,
    pub source_end_ms: i64,
    pub timeline_start_ms: i64,
    pub timeline_end_ms: i64,
    pub on_screen_text: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineContent {
    pub(crate) clips: Vec<TimelineClip>,
    #[serde(default)]
    pub(crate) text_tracks: Vec<TextTrack>,
    #[serde(default)]
    pub(crate) music_tracks: Vec<MusicTrack>,
    #[serde(default)]
    pub(crate) quality_report: Option<PreviewQualityReport>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewResult {
    pub timeline_version_id: String,
    pub preview_path: String,
    pub quality_report: PreviewQualityReport,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JianyingDraftResult {
    pub draft_directory: String,
    pub draft_content_path: String,
    pub registration_status: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JianyingRegistrationStatus {
    pub timeline_version_id: String,
    pub draft_name: String,
    pub status: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestTimeline {
    pub timeline: TimelineVersion,
    pub preview: Option<PreviewResult>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClipReplacementParams {
    pub(crate) shot_index: i64,
    pub(crate) asset_id: String,
    pub(crate) source_start_ms: i64,
    pub(crate) source_end_ms: i64,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClipAdjustmentParams {
    pub(crate) shot_index: i64,
    #[serde(default)]
    pub(crate) new_duration_ms: Option<i64>,
    #[serde(default)]
    pub(crate) new_source_start_ms: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEditResult {
    pub agent_task_id: String,
    pub message: String,
    pub storyboard: Option<StoryboardVersion>,
    pub timeline: Option<TimelineVersion>,
    pub preview: Option<PreviewResult>,
    pub jianying_draft: Option<JianyingDraftResult>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEditEvent {
    pub agent_task_id: String,
    pub status: String,
    pub result: AgentEditResult,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTask {
    pub id: String,
    pub project_id: String,
    pub editing_task_id: Option<String>,
    pub conversation_id: Option<String>,
    pub tool_name: String,
    pub status: String,
    pub input: serde_json::Value,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ConversationTurnResult {
    Immediate {
        status: String,
        message: String,
    },
    Run {
        #[serde(rename = "agentTaskId")]
        agent_task_id: String,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunStep {
    pub id: String,
    pub project_id: String,
    pub editing_task_id: String,
    pub agent_task_id: String,
    pub step_number: i64,
    pub tool_name: String,
    pub status: String,
    pub artifact_type: Option<String>,
    pub artifact_id: Option<String>,
    pub error_code: Option<String>,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub updated_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDiagnostic {
    pub id: String,
    pub project_id: String,
    pub editing_task_id: String,
    pub conversation_id: String,
    pub agent_task_id: String,
    pub step_number: Option<i64>,
    pub kind: String,
    pub content: String,
    pub created_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationLog {
    pub id: String,
    pub project_id: String,
    pub editing_task_id: Option<String>,
    pub conversation_id: Option<String>,
    pub agent_task_id: Option<String>,
    pub actor: String,
    pub operation_type: String,
    pub entity_type: String,
    pub entity_id: String,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
    pub created_at: i64,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryboardShot {
    pub order_index: i64,
    pub duration_ms: i64,
    pub purpose: String,
    pub on_screen_text: String,
    pub asset_id: String,
    pub source_start_ms: i64,
    pub source_end_ms: i64,
    pub reason: String,
    #[serde(default)]
    pub beat_id: String,
    #[serde(default = "default_storyboard_match_level")]
    pub match_level: String,
}

fn default_storyboard_match_level() -> String {
    "contextual".to_owned()
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryboardBeat {
    pub id: String,
    pub purpose: String,
    pub required_visual: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryboardContent {
    #[serde(default)]
    pub(crate) brief: String,
    pub(crate) title: String,
    pub(crate) summary: String,
    #[serde(default)]
    pub(crate) target_duration_ms: i64,
    #[serde(default = "default_storyboard_script_mode")]
    pub(crate) script_mode: String,
    #[serde(default)]
    pub(crate) beats: Vec<StoryboardBeat>,
    #[serde(default)]
    pub(crate) uncovered_beat_ids: Vec<String>,
    pub(crate) shots: Vec<StoryboardShot>,
}

fn default_storyboard_script_mode() -> String {
    "full_script".to_owned()
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryboardSource {
    pub(crate) asset_id: String,
    pub(crate) kind: String,
    pub(crate) duration_ms: Option<i64>,
    pub(crate) scene_segments: Vec<SceneSegment>,
    pub(crate) ocr_evidence: Vec<OcrEvidence>,
    pub(crate) visual_evidence: Vec<VisualEvidence>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TechnicalMetadata {
    pub(crate) duration_ms: Option<i64>,
    pub(crate) width: Option<i64>,
    pub(crate) height: Option<i64>,
    pub(crate) fps: Option<f64>,
    pub(crate) has_audio: bool,
    pub(crate) thumbnail_path: Option<String>,
    #[serde(default)]
    pub(crate) keyframes: Vec<KeyframeMetadata>,
    #[serde(default)]
    pub(crate) scene_segments: Vec<SceneSegment>,
    #[serde(default)]
    pub(crate) ocr_evidence: Vec<OcrEvidence>,
    #[serde(default)]
    pub(crate) visual_evidence: Vec<VisualEvidence>,
    #[serde(default)]
    pub(crate) visual_analysis_note: Option<String>,
    #[serde(default = "default_visual_analysis_status")]
    pub(crate) visual_analysis_status: String,
}

fn default_visual_analysis_status() -> String {
    "queued".to_owned()
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyframeMetadata {
    pub(crate) time_ms: i64,
    pub(crate) image_path: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneSegment {
    pub(crate) start_ms: i64,
    pub(crate) end_ms: i64,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrEvidence {
    pub(crate) time_ms: Option<i64>,
    pub(crate) text: String,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualEvidence {
    pub(crate) time_ms: Option<i64>,
    #[serde(default)]
    pub(crate) subjects: Vec<String>,
    pub(crate) scene: Option<String>,
    #[serde(default)]
    pub(crate) actions: Vec<String>,
    #[serde(default)]
    pub(crate) products: Vec<String>,
    #[serde(default)]
    pub(crate) quality_notes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::ConversationTurnResult;
    use serde_json::json;

    #[test]
    fn conversation_run_result_uses_the_frontend_task_id_contract() {
        let serialized = serde_json::to_value(ConversationTurnResult::Run {
            agent_task_id: "agent-task-1".to_owned(),
        })
        .expect("serialize conversation run result");

        assert_eq!(
            serialized,
            json!({
                "kind": "run",
                "agentTaskId": "agent-task-1",
            })
        );
    }
}
