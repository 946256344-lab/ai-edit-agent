//! 提示构建、历史渲染与状态快照加载。
//!
//! 所有向模型发送的 prompt 文本在这里组装；状态快照从 SQLite 和磁盘读取后
//! 注入 prompt。不执行任何技能副作用，也不修改 LoopState 之外的任何持久化记录。

use crate::models::PendingClarificationSnapshot;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::path::Path;
use tauri::Manager;

use super::schema::{
    AgentScopeSnapshot, AgentStateSnapshot, ArtifactPresenceSnapshot, AssetAvailabilitySnapshot,
    JianyingArtifactSnapshot, LoopState, TimelineArtifactSnapshot, VersionArtifactSnapshot,
};

/// Maximum number of recent messages loaded for conversation memory.
const MAX_HISTORY_MESSAGES: usize = 12;
/// Character budget for the conversation history fed to the model.
const MAX_HISTORY_CHARS: usize = 8000;

/// Loads the most recent conversation messages for the given conversation,
/// excluding the message that is exactly the current request. Returns
/// chronological (role, content) pairs, capped by message count and total
/// character budget.
pub(super) fn load_message_history(
    connection: &Connection,
    conversation_id: &str,
    exclude_request: &str,
) -> Vec<(String, String)> {
    let mut statement = match connection.prepare(
        "SELECT role, content FROM messages WHERE conversation_id = ?1 ORDER BY created_at DESC LIMIT ?2",
    ) {
        Ok(statement) => statement,
        Err(_) => return Vec::new(),
    };
    let rows = match statement.query_map(
        params![conversation_id, MAX_HISTORY_MESSAGES as i64 + 1],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    ) {
        Ok(rows) => rows,
        Err(_) => return Vec::new(),
    };
    let mut newest_first: Vec<(String, String)> = rows.filter_map(Result::ok).collect();
    if let Some((role, content)) = newest_first.first() {
        if role == "user" && content.trim() == exclude_request.trim() {
            newest_first.remove(0);
        }
    }
    let mut kept: Vec<(String, String)> = Vec::new();
    let mut total_chars = 0;
    for (role, content) in newest_first {
        let chars = content.chars().count();
        if total_chars + chars > MAX_HISTORY_CHARS {
            continue;
        }
        total_chars += chars;
        kept.push((role, content));
    }
    kept.reverse();
    kept
}

/// Loads native conversation messages as protocol items, preserving real roles.
pub(super) fn load_native_message_history(
    connection: &Connection,
    conversation_id: &str,
    exclude_request: &str,
) -> Vec<Value> {
    let mut statement = match connection.prepare(
        "SELECT role, content FROM messages
         WHERE conversation_id = ?1 AND role IN ('user', 'assistant', 'agent')
         ORDER BY created_at DESC, id DESC LIMIT ?2",
    ) {
        Ok(statement) => statement,
        Err(_) => return Vec::new(),
    };
    let rows = match statement.query_map(
        params![conversation_id, MAX_HISTORY_MESSAGES as i64 + 1],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    ) {
        Ok(rows) => rows,
        Err(_) => return Vec::new(),
    };
    let mut newest_first = Vec::new();
    let mut skipped_current = false;
    let mut total_chars = 0;
    for row in rows.filter_map(Result::ok) {
        let (role, content) = row;
        if !skipped_current && role == "user" && content.trim() == exclude_request.trim() {
            skipped_current = true;
            continue;
        }
        let role = match role.as_str() {
            "user" => "user",
            "assistant" | "agent" => "assistant",
            _ => continue,
        };
        let chars = content.chars().count();
        if total_chars + chars > MAX_HISTORY_CHARS {
            continue;
        }
        total_chars += chars;
        let content_type = if role == "assistant" {
            "output_text"
        } else {
            "input_text"
        };
        newest_first.push(json!({
            "role": role,
            "content": [{"type": content_type, "text": content}],
        }));
    }
    newest_first.reverse();
    newest_first
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_history_uses_real_roles_without_speaker_labels() {
        let connection = Connection::open_in_memory().expect("open history database");
        crate::db::migrate(&connection).expect("migrate history database");
        connection
            .execute_batch(
                "INSERT INTO projects (id, name, created_at, updated_at) VALUES ('p', 'Project', 1, 1);
                 INSERT INTO editing_tasks (id, project_id, title, brief, created_at, updated_at) VALUES ('t', 'p', 'Task', '', 1, 1);
                 INSERT INTO conversations (id, project_id, editing_task_id, title, status, created_at, updated_at) VALUES ('c', 'p', 't', 'Conversation', 'ready', 1, 1);
                 INSERT INTO messages (id, conversation_id, role, content, created_at) VALUES
                   ('u1', 'c', 'user', '现在有多少素材？', 2),
                   ('a1', 'c', 'assistant', '项目中有 10 个素材。', 3),
                   ('u2', 'c', 'user', '现在有多少素材？', 4);",
            )
            .expect("seed history messages");

        let history = load_native_message_history(&connection, "c", "现在有多少素材？");

        assert_eq!(history.len(), 2);
        assert_eq!(history[0]["role"], "user");
        assert_eq!(history[1]["role"], "assistant");
        assert_eq!(history[0]["content"][0]["text"], "现在有多少素材？");
        assert_eq!(history[1]["content"][0]["text"], "项目中有 10 个素材。");
        assert!(!history.iter().any(|item| {
            item.to_string().contains("用户：") || item.to_string().contains("助手：")
        }));
    }
}

/// Renders conversation history as a compact labelled text block for the model.
pub(super) fn render_history(history: &[(String, String)]) -> String {
    if history.is_empty() {
        return "（无）".to_owned();
    }
    let lines: Vec<String> = history
        .iter()
        .map(|(role, content)| {
            let speaker = match role.as_str() {
                "user" => "用户",
                "agent" => "Agent",
                _ => "系统",
            };
            format!("{speaker}: {content}")
        })
        .collect();
    lines.join("\n")
}

/// 加载当前作用域的待澄清问题（若存在）。
pub(crate) fn load_pending_clarification(
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
    conversation_id: &str,
) -> Result<Option<PendingClarificationSnapshot>, String> {
    connection
        .query_row(
            "SELECT id, source_kind, source_agent_task_id, goal, question, created_at FROM pending_clarifications WHERE project_id = ?1 AND editing_task_id = ?2 AND conversation_id = ?3 AND status = 'pending' ORDER BY updated_at DESC LIMIT 1",
            params![project_id, editing_task_id, conversation_id],
            |row| Ok(PendingClarificationSnapshot {
                id: row.get(0)?,
                source_kind: row.get(1)?,
                source_agent_task_id: row.get(2)?,
                goal: row.get(3)?,
                question: row.get(4)?,
                created_at: row.get(5)?,
            }),
        )
        .optional()
        .map_err(|_| "Pending clarification could not be read.".to_owned())
}

/// 从 SQLite 加载素材可用性快照。
pub(super) fn load_asset_availability(
    connection: &Connection,
    project_id: &str,
) -> Result<AssetAvailabilitySnapshot, String> {
    let mut statement = connection
        .prepare("SELECT analysis_status, source_reference FROM assets WHERE project_id = ?1")
        .map_err(|_| "Asset availability unreadable.".to_owned())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|_| "Asset availability unreadable.".to_owned())?;
    let mut snapshot = AssetAvailabilitySnapshot {
        total_count: 0,
        usable_count: 0,
        pending_analysis_count: 0,
        failed_analysis_count: 0,
        unavailable_source_count: 0,
    };
    for row in rows {
        let (analysis_status, source_reference) =
            row.map_err(|_| "Asset availability unreadable.".to_owned())?;
        let source_available = Path::new(&source_reference).is_file();
        snapshot.total_count += 1;
        if !source_available {
            snapshot.unavailable_source_count += 1;
        }
        match analysis_status.as_str() {
            "ready" if source_available => snapshot.usable_count += 1,
            "queued" | "analyzing" => snapshot.pending_analysis_count += 1,
            "ready" => {}
            _ => snapshot.failed_analysis_count += 1,
        }
    }
    Ok(snapshot)
}

/// 检查 preview 文件是否实际存在于磁盘。
pub(super) fn preview_presence(
    state: &LoopState,
    timeline_version_id: &str,
) -> Result<TimelineArtifactSnapshot, String> {
    let marked_ready = state
        .connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM timeline_versions WHERE id = ?1 AND project_id = ?2 AND status = 'preview_ready')",
            params![timeline_version_id, state.project_id],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|_| "Agent preview state could not be read.".to_owned())?;
    let preview_exists = marked_ready
        && state
            .app
            .path()
            .app_data_dir()
            .ok()
            .map(|directory| {
                directory
                    .join("previews")
                    .join(timeline_version_id)
                    .join("preview.mp4")
                    .is_file()
            })
            .unwrap_or(false);
    Ok(TimelineArtifactSnapshot {
        exists: preview_exists,
        timeline_version_id: preview_exists.then(|| timeline_version_id.to_owned()),
    })
}

/// 检查 Jianying 草稿是否实际存在于磁盘。
pub(super) fn jianying_presence(
    connection: &Connection,
    project_id: &str,
    timeline_version_id: &str,
) -> Result<JianyingArtifactSnapshot, String> {
    let mut statement = connection
        .prepare(
            "SELECT status, input_json, result_json FROM agent_tasks
             WHERE project_id = ?1 AND tool_name = 'register_jianying_draft'
             ORDER BY created_at DESC",
        )
        .map_err(|_| "Jianying draft state unreadable.".to_owned())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|_| "Jianying draft state unreadable.".to_owned())?;
    for row in rows {
        let (task_status, input_json, result_json) =
            row.map_err(|_| "Jianying draft state unreadable.".to_owned())?;
        let input: Value = match serde_json::from_str(&input_json) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if input.get("timelineVersionId").and_then(Value::as_str) != Some(timeline_version_id) {
            continue;
        }
        let result = result_json.as_deref().and_then(|value| {
            serde_json::from_str::<crate::models::JianyingDraftResult>(value).ok()
        });
        let exists = result.as_ref().is_some_and(|draft| {
            Path::new(&draft.draft_directory).is_dir()
                && Path::new(&draft.draft_content_path).is_file()
        });
        let registration_status = result.map(|draft| draft.registration_status).or_else(|| {
            Some(
                match task_status.as_str() {
                    "completed" => "registered",
                    "failed" | "cancelled" => "failed",
                    _ => "pending",
                }
                .to_owned(),
            )
        });
        return Ok(JianyingArtifactSnapshot {
            exists,
            timeline_version_id: exists.then(|| timeline_version_id.to_owned()),
            registration_status,
        });
    }
    Ok(JianyingArtifactSnapshot {
        exists: false,
        timeline_version_id: None,
        registration_status: None,
    })
}

/// 从 SQLite 和磁盘读取当前产物存在性快照。
pub(super) fn current_artifact_presence(
    state: &LoopState,
) -> Result<ArtifactPresenceSnapshot, String> {
    let storyboard = if let Some(storyboard) = &state.storyboard {
        let exists = state
            .connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM storyboard_versions WHERE id = ?1 AND project_id = ?2 AND editing_task_id = ?3)",
                params![storyboard.id, state.project_id, state.editing_task_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(|_| "Storyboard state unreadable.".to_owned())?;
        VersionArtifactSnapshot {
            exists,
            version_id: exists.then(|| storyboard.id.clone()),
            version_number: exists.then_some(storyboard.version_number),
        }
    } else {
        VersionArtifactSnapshot {
            exists: false,
            version_id: None,
            version_number: None,
        }
    };

    let candidate = state
        .timelines
        .iter()
        .max_by_key(|timeline| timeline.version_number);
    let current_timeline = if let Some(timeline) = candidate {
        let exists = state
            .connection
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM timeline_versions timeline
                    JOIN storyboard_versions storyboard ON storyboard.id = timeline.storyboard_version_id
                    WHERE timeline.id = ?1 AND timeline.project_id = ?2 AND storyboard.editing_task_id = ?3
                )",
                params![timeline.id, state.project_id, state.editing_task_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(|_| "Timeline state unreadable.".to_owned())?;
        exists.then_some(timeline)
    } else {
        None
    };
    let timeline = VersionArtifactSnapshot {
        exists: current_timeline.is_some(),
        version_id: current_timeline.map(|value| value.id.clone()),
        version_number: current_timeline.map(|value| value.version_number),
    };
    let preview = current_timeline
        .map(|value| preview_presence(state, &value.id))
        .transpose()?
        .unwrap_or(TimelineArtifactSnapshot {
            exists: false,
            timeline_version_id: None,
        });
    let jianying_draft = current_timeline
        .map(|value| jianying_presence(state.connection, state.project_id, &value.id))
        .transpose()?
        .unwrap_or(JianyingArtifactSnapshot {
            exists: false,
            timeline_version_id: None,
            registration_status: None,
        });
    Ok(ArtifactPresenceSnapshot {
        storyboard,
        timeline,
        preview,
        jianying_draft,
    })
}

/// 计算当前目标的未满足条件列表。
pub(super) fn unmet_conditions(
    goal: super::policy::LoopGoal,
    assets: &AssetAvailabilitySnapshot,
    artifacts: &ArtifactPresenceSnapshot,
    task_brief_is_empty: bool,
    goal_satisfied: bool,
) -> Vec<String> {
    use super::policy::LoopGoal;
    if goal == LoopGoal::Question {
        return Vec::new();
    }
    let mut unmet = Vec::new();
    if !goal_satisfied {
        unmet.push(format!("requested_{}_not_produced", goal.code()));
    }
    let needs_storyboard_creation = matches!(goal, LoopGoal::Storyboard)
        || (!artifacts.timeline.exists
            && matches!(
                goal,
                LoopGoal::Timeline | LoopGoal::Preview | LoopGoal::JianyingDraft
            )
            && !artifacts.storyboard.exists);
    if needs_storyboard_creation {
        if task_brief_is_empty {
            unmet.push("task_brief_missing".to_owned());
        }
        if assets.total_count == 0 {
            unmet.push("no_imported_media".to_owned());
        } else if assets.usable_count == 0 {
            if assets.pending_analysis_count > 0 {
                unmet.push("asset_analysis_incomplete".to_owned());
            } else {
                unmet.push("no_usable_analyzed_assets".to_owned());
            }
        }
    }
    if matches!(
        goal,
        LoopGoal::Timeline | LoopGoal::Preview | LoopGoal::JianyingDraft
    ) && !artifacts.timeline.exists
        && !artifacts.storyboard.exists
    {
        unmet.push("storyboard_missing_for_timeline_creation".to_owned());
    }
    if goal == LoopGoal::Preview && !artifacts.timeline.exists {
        unmet.push("timeline_missing_for_preview".to_owned());
    }
    if goal == LoopGoal::JianyingDraft && !artifacts.timeline.exists {
        unmet.push("timeline_missing_for_jianying_draft".to_owned());
    }
    unmet
}

/// 构建完整的 Agent 状态快照，供每步 prompt 使用。
pub(super) fn build_agent_state_snapshot(
    state: &LoopState,
    remaining_steps: usize,
) -> Result<AgentStateSnapshot, String> {
    let assets = load_asset_availability(state.connection, state.project_id)?;
    let artifacts = current_artifact_presence(state)?;
    let unmet = if state.goal_locked {
        unmet_conditions(
            state.goal,
            &assets,
            &artifacts,
            state.task_brief.trim().is_empty(),
            state.goal.satisfied_by(&state.last_outcome),
        )
    } else {
        Vec::new()
    };
    Ok(AgentStateSnapshot {
        scope: AgentScopeSnapshot {
            project_id: state.project_id.to_owned(),
            editing_task_id: state.editing_task_id.to_owned(),
            conversation_id: state.conversation_id.to_owned(),
        },
        assets,
        artifacts,
        executed_steps: state.executed_steps.clone(),
        remaining_steps,
        goal: if state.goal_locked {
            state.goal.code().to_owned()
        } else {
            "pending".to_owned()
        },
        pending_clarification: state.pending_clarification.clone(),
        unmet_conditions: unmet,
    })
}

/// 确定性前置条件提示：识别最短有效依赖链，不执行任何技能。
pub(super) fn deterministic_prerequisite_hints(snapshot: &AgentStateSnapshot) -> Vec<String> {
    let mut hints = Vec::new();
    if snapshot
        .unmet_conditions
        .iter()
        .any(|value| value == "no_imported_media")
    {
        hints.push("当前没有素材；需要创作产物时应使用 ask_user 请用户先导入素材。".to_owned());
        return hints;
    }
    if snapshot
        .unmet_conditions
        .iter()
        .any(|value| value == "asset_analysis_incomplete")
    {
        hints.push(
            "素材分析尚未完成；不要从文件名猜测内容。可观察素材状态、请求可用素材的本地分析，或在确实缺少输入时澄清。"
                .to_owned(),
        );
        return hints;
    }
    if snapshot
        .unmet_conditions
        .iter()
        .any(|value| value == "no_usable_analyzed_assets")
    {
        hints.push(
            "没有可用且已分析的素材；可观察当前素材状态，并根据用户意图选择请求分析或澄清。"
                .to_owned(),
        );
        return hints;
    }
    match snapshot.goal.as_str() {
        "pending" => hints.push(
            "本轮目标尚未锁定；首次响应必须同时声明 goal/isQuestion 并选择一个实际技能或 finish。"
                .to_owned(),
        ),
        "storyboard" => hints.push("素材已可用；可用技能包括 generate_storyboard 与观察技能。根据用户意图选择下一步。".to_owned()),
        "timeline" if snapshot.artifacts.timeline.exists => hints.push(
            "当前内部时间线已存在；可用时间线编辑技能和观察技能。根据用户意图选择，不要无理由重建 storyboard。"
                .to_owned(),
        ),
        "timeline" if snapshot.artifacts.storyboard.exists => {
            hints.push("当前已有 storyboard，但没有内部时间线；可用技能包括 create_timeline_draft 与观察技能。".to_owned())
        }
        "timeline" => hints.push(
            "当前没有可编辑时间线；现有素材、storyboard 与时间线状态如快照所示。选择满足用户意图的合法下一步。"
                .to_owned(),
        ),
        "preview" if snapshot.artifacts.timeline.exists => hints.push(
            "当前内部时间线已存在；可用技能包括 render_preview、时间线编辑和观察技能。"
                .to_owned(),
        ),
        "preview" if snapshot.artifacts.storyboard.exists => hints.push(
            "当前缺少时间线但已有 storyboard；render_preview 需要时间线，可用技能包括 create_timeline_draft 与观察技能。".to_owned(),
        ),
        "preview" => hints.push(
            "当前缺少 storyboard 和时间线；快照中的素材状态决定哪些创作工具当前可用。"
                .to_owned(),
        ),
        "jianying_draft" if snapshot.artifacts.timeline.exists => hints.push(
            "当前内部时间线已存在；可用技能包括 create_jianying_draft、时间线编辑和观察技能。"
                .to_owned(),
        ),
        "jianying_draft" if snapshot.artifacts.storyboard.exists => hints.push(
            "当前缺少时间线但已有 storyboard；create_jianying_draft 需要时间线，可用技能包括 create_timeline_draft 与观察技能。"
                .to_owned(),
        ),
        "jianying_draft" => hints.push(
            "当前缺少 storyboard 和时间线；快照中的素材状态决定哪些创作和交付工具当前可用。"
                .to_owned(),
        ),
        _ => hints.push("问答目标可先使用观察技能获取当前事实，再如实回答。".to_owned()),
    }
    hints
}

pub(super) fn project_fact_completion_instruction(
    project_fact_question: bool,
    successful_observation: bool,
) -> &'static str {
    if project_fact_question && successful_observation {
        "This is a project-fact question and at least one read-only observation has already succeeded. If the latest tool result contains the count, status, or fact the user asked for, choose finish now and answer from that result. Do not call a semantically overlapping observation tool merely to confirm the same fact. Call another observation only when a specifically requested fact is absent from the latest result."
    } else {
        "For a project-fact question, obtain one successful read-only observation before answering."
    }
}

/// 构建单步 prompt 文本。
pub(super) fn build_step_prompt(
    state: &LoopState,
    transcript: &[Value],
    snapshot: &AgentStateSnapshot,
    prerequisite_hints: &[String],
) -> Result<String, String> {
    let snapshot_json = serde_json::to_string(snapshot).map_err(|e| e.to_string())?;
    let prerequisite_json = serde_json::to_string(prerequisite_hints).map_err(|e| e.to_string())?;
    let transcript_json = serde_json::to_string(transcript).map_err(|e| e.to_string())?;
    let history_text = render_history(&state.history);
    let goal_label = if state.goal_locked {
        state.goal.label()
    } else {
        "待模型结合本轮请求与历史声明"
    };
    let clarification_hint = state.pending_clarification.as_ref().map_or_else(
        || "There is no pending clarification marker.".to_owned(),
        |pending| format!(
            "There is an unanswered scoped clarification: {} Treat the current user message as a possible answer, use the full history, and do not repeat it blindly.",
            pending.question
        ),
    );
    let project_fact_instruction = project_fact_completion_instruction(
        state.project_fact_question,
        state.successful_observation,
    );
    let denied_tools = state.tool_policy.prompt_label();
    Ok(format!(
        "You are Assembly Agent, the local video-editing loop for a project. The requested deliverable \
         for THIS request is: {goal}. You must only call finish after you have REALLY produced that \
         deliverable; finishing without producing it will be rejected and the loop will continue. \
         never claim in an answer that you performed edits that you did not actually execute. If the \
         state snapshot says remainingSteps is 0, choose finish now: summarize only real artifacts and \
         any incomplete work. A truthful partial completion is preferable to another tool call.\n\n\
         Recent conversation history (before this request):\n{history_text}\n\n\
         {clarification_hint}\n\n\
         {project_fact_instruction}\n\n\
         User-denied side-effect tools for this request: {denied_tools}. These tools are unavailable even if they would normally be a useful follow-up. Do not declare a goal whose deliverable requires a denied tool.\n\n\
         If the requested deliverable above is pending, this first response must BOTH declare goal \
         (question|storyboard|timeline|preview|jianying_draft) and isQuestion, and choose the first \
         skill or finish in the same JSON object. A long narration/script supplied after the Agent asked \
         for a creative goal is normally an answer to that clarification, even when its title is phrased \
         as a rhetorical question. Once declared, the goal is fixed. If the backend already pins a goal, \
         that goal is authoritative and model output cannot replace it. For a question that does not need \
         project facts, answer with finish in this same step instead of calling an observation skill.\n\n\
         Pick exactly ONE skill for this step from the list below. Put every argument field at the TOP \
         LEVEL of the JSON object (no nested parameter wrapper) using the exact camelCase names; stray \
         keys are tolerated. Only refer to clips and durations that exist in the provided state, and \
         honour the user's intent. You may optionally include taskBrief only when the user gives or \
         materially changes a video-creation goal.\n\n\
          Skills:\n\
          - get_edit_status. no args. Read the latest previous scoped Agent task and report only grounded completion status.\n\
          - get_asset_health_summary. no args. Use for questions about this project's current source-file health, counts, scan state, or unreadable/missing causes. It returns persisted counts and safe reason codes, never paths or raw operating-system errors. Do not infer a specific cause when reasonEvidenceAvailable is false.\n\
           - list_assets. no args. Use only for a compact persisted status inventory or before requesting analysis. This Agent observation never starts or reprioritizes analysis.\n\
          - search_assets. args: query (optional), kind (video|image|audio|other optional), minDurationMs/maxDurationMs (optional), minRating 0..5 (optional), favoriteOnly (optional), tag (optional), collectionId (optional), offset (optional), limit 1..20 (optional). Use for targeted candidate discovery. It excludes user-blocked assets and returns bounded safe summaries, match reason codes and nextOffset; it never returns paths, notes, OCR text or media content.\n\
         - search_asset_segments. args: query (required), assetId (optional), offset (optional), limit 1..20 (optional). Use after candidate discovery when an edit needs exact source windows. It returns evidence-bound sourceStartMs/sourceEndMs, safe visual labels and reason codes; OCR text and paths remain private. Missing, changed, unreadable and user-blocked sources are excluded.\n\
         - search_music. args: query. Search the configured Jamendo catalog. It only returns tracks whose download is allowed and whose license is CC0 or CC-BY; CC-BY attribution is retained on the music cue. Never invent a track or URL.\n\
         - download_music. args: trackId. Download exactly one eligible Jamendo track to the current local project and queue its normal media analysis. Call search_music first.\n\
         - use_online_music. args: trackId, timelineVersionId (optional). Call search_music first. It downloads exactly one eligible track, waits for its local analysis, then creates a new timeline version with that track looped across the full timeline at safe background volume. Use this when the user asks you to choose and apply music, not merely recommend it. After it succeeds, use render_preview or create_jianying_draft if requested.\n\
          - request_asset_analysis. args: assetIds [string]; use only after list_assets identifies imported queued or failed assets. This queues local analysis and does not expose paths or run filesystem commands in the model.\n\
         - For a user request to analyze or reanalyze media, observe with list_assets first, then choose request_asset_analysis for eligible assets. Do not choose generate_storyboard unless the user asks to create a storyboard.\n\
         - get_storyboard. no args.\n\
         - get_timeline. args: timelineVersionId (optional).\n\
         - get_text_capabilities. no args. Call this before authoring or revising text when the user cares about fonts, effects, or Jianying delivery. It returns the verified Jianying matrix and local-preview-only options.\n\
         - replace_music_tracks. args: timelineVersionId (optional), musicTracks: [{{id, enabled, cues}}]. Each cue needs id, assetId, sourceStartMs, sourceEndMs, timelineStartMs, timelineEndMs, volume (0..2); loopEnabled, fadeInMs, fadeOutMs are optional. Call get_timeline and list_assets first. Music must use a ready audio asset and stay in the timeline. Set loopEnabled only when a shorter source range must repeat. create_jianying_draft can create a new experimental Jianying music draft from these local assets; never claim playback has been visually reviewed in Jianying.\n\
         - generate_storyboard. args: brief. It consumes only analysis evidence already ready in this project; it never starts or reprioritizes analysis. Request analysis explicitly first only when the user permits it.\n\
         - create_timeline_draft. no args; it uses the active storyboard in this editing task.\n\
         - replace_clips. args: timelineVersionId (optional), shots: [{{shotIndex int, assetId string, sourceStartMs int, sourceEndMs int}}]. A video source range must equal the replaced shot's current duration; images use 0 and 0.\n\
         - change_clip_duration. args: timelineVersionId (optional), adjustments: [{{shotIndex int, newDurationMs int optional, newSourceStartMs int optional}}]. The new source window must stay inside the shot's verified source.\n\
         - reorder_clips. args: timelineVersionId (optional), order: [ ints ] (a complete valid permutation of the shot indexes).\n\
         - replace_text_tracks. args: timelineVersionId (optional), textTracks: [{{id, role subtitle|headline|callout|cta|label, layer, enabled, cues}}]. Each cue needs id, startMs, endMs, text. style and layout are optional: omitted fields resolve to the safe default subtitle design. Before the first text-authoring call, call get_timeline and get_text_capabilities; call get_storyboard too when the intended on-screen meaning is not clear from the timeline. Prefer the capability selectionHint: use subtitle_safe for dialogue/narration, headline_rise for progression or an opening reveal, headline_pop for a surprise/key result/contrast, headline_drop for a conclusion/rule/warning, and callout_card/cta_card only when the user explicitly accepts a local-preview-only result. Use at most one headline per visual beat; do not overlap a headline with another headline, or use a headline as ordinary subtitles. The backend resolves a template to an auditable complete recipe and ignores conflicting style/animation values. Without a templateId, use only fade, slide_up, slide_down, pop, or wipe animations. Never send jianyingCompatibility: the backend assigns it. Jianying delivery requires fontKey jianying_default, no stroke/shadow/background/loop, an optional fade exit, and only static/fade/slide_up/slide_down/pop entrance. Unicode text is written through the verified escaped nested-text path.\n\
         - render_preview. args: timelineVersionId (optional).\n\
         - create_jianying_draft. args: timelineVersionId (optional).\n\
         - ask_user. args: question (only if a needed value is genuinely missing).\n\
         - finish. args: answer (concise Chinese summary of what was done).\n\n\
         The authoritative state snapshot contains only scoped availability and artifact facts. If \
         you need storyboard shots or timeline clips, call get_storyboard or get_timeline first. \
         Deterministic prerequisite hints identify the shortest currently valid path; they do not \
         require rebuilding a storyboard when an existing timeline can be edited or rendered.\n\n\
         Return JSON with ONLY the keys: goal and isQuestion when required above; tool (exactly one of \
         the names above, or no_action if nothing should be done); reason; answer/question/taskBrief when \
         relevant; and the argument fields that tool needs.\n\n\
         Agent state snapshot: {snapshot_json}\n\
         Deterministic prerequisite hints: {prerequisite_json}\n\
         Conversation so far: {transcript_json}",
        goal = goal_label,
        history_text = history_text,
        clarification_hint = clarification_hint,
        project_fact_instruction = project_fact_instruction,
        denied_tools = denied_tools,
    ))
}
