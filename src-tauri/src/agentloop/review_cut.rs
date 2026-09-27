//! 成片复查：预览渲染后像剪辑师一样看真实成片，逐镜找问题，能安全修的修一轮。
//!
//! 流程：从渲染好的预览抽帧（每镜最多 3 帧）→ 拼接触表，每格标「#镜号 成片秒数」→
//! 每个模型请求最多 4 张图，镜头多时拆成并发请求 → 模型按帧给结构化问题 →
//! 按问题调用现有局部编辑（精修切点 → 重选镜头 → 无配音无字幕时重排）→ 重渲染 → 复核一次。
//! 修复最多一轮，复核只报告不再修。素材证据与故事板只作参考，帧上看到的为准；
//! 同一素材重复出现是时间线事实，由本地直接判定。
//! 任何一步失败都写进报告，未修掉的问题转成 qualityWarnings；模型文字不算修复证据，
//! 只有局部编辑工具落地的新版本才算。
//! 只在生成流程里自动运行，不做成模型可调用工具：Voycut 一次出基本满意的初剪，精修交给编辑器。

use super::schema::LoopState;
use crate::models::{
    PreviewResult, StoryboardSource, StoryboardVersion, TimelineClip, TimelineVersion,
};
use crate::process::{hidden_command, run_hidden_command_with_timeout};
use crate::provider::{
    model_response_json_text, post_model_payloads_concurrently, ModelAccess,
    MAX_IMAGES_PER_REQUEST,
};
use crate::storyboard::local_edit::{
    self, LocalEditScope, ReselectTarget, MAX_REFINE_SHOTS, MAX_RESELECT_BEATS,
};
use crate::storyboard::multimodal::{draw_cell_label, fit_into_box, read_input_image};
use image::{ImageBuffer, Rgb, RgbImage};
use rusqlite::Connection;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

/// 每镜最多抽几帧（开头 / 中间 / 结尾）；短镜少抽，避免同一帧重复。
const FRAMES_PER_SHOT: usize = 3;
/// 每张接触表最多几行（一行一镜）。
const MAX_ROWS_PER_SHEET: usize = 8;
/// 单个请求最多看几镜：4 张图 × 每张 8 行。更多镜头拆成并发请求。
const MAX_SHOTS_PER_REQUEST: usize = MAX_IMAGES_PER_REQUEST * MAX_ROWS_PER_SHEET;
/// 一轮修复最多几次重选调用（不同替换要求分开调用，避免一条要求污染别的拍的召回）。
const MAX_RESELECT_CALLS: usize = 3;
const REVIEW_REQUEST_TIMEOUT: Duration = Duration::from_secs(150);
const BATCH_FRAME_TIMEOUT: Duration = Duration::from_secs(60);
const SINGLE_FRAME_TIMEOUT: Duration = Duration::from_secs(15);
/// 生成后自动复查至少要留的本轮时间；不够就跳过并如实说明，不拿已生成的结果冒险。
const AUTO_REVIEW_MIN_REMAINING: Duration = Duration::from_secs(300);
/// 修复后复核至少要留的本轮时间。
const VERIFY_MIN_REMAINING: Duration = Duration::from_secs(120);

const CATEGORIES: &[&str] = &[
    "repeated_footage",
    "continuity",
    "beat_order",
    "weak_shot",
    "text_logo_crowd",
];

/// 复查与修复需要的作用域；全部来自 LoopState，不接受模型参数。
pub(super) struct ReviewContext<'a> {
    pub(super) app: &'a AppHandle,
    pub(super) connection: &'a Connection,
    pub(super) project_id: &'a str,
    pub(super) editing_task_id: &'a str,
    pub(super) conversation_id: &'a str,
    pub(super) agent_task_id: &'a str,
}

impl<'a> ReviewContext<'a> {
    pub(super) fn from_state(state: &LoopState<'a>) -> Self {
        Self {
            app: state.app,
            connection: state.connection,
            project_id: state.project_id,
            editing_task_id: state.editing_task_id,
            conversation_id: state.conversation_id,
            agent_task_id: state.agent_task_id,
        }
    }
}

/// 一条复查问题。`source` 为 `timeline` 的是时间线事实，`frames` 的是模型看帧得出的判断。
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CutFinding {
    shot_indexes: Vec<i64>,
    category: String,
    severity: String,
    evidence: String,
    repair: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    instruction: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    proposed_order: Option<Vec<i64>>,
    source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    repair_status: Option<String>,
}

/// 复查结果：报告进 generate_storyboard 结果的 cutReview，未修掉的问题进 qualityWarnings，message 追加到中文回执。
pub(super) struct CutReview {
    pub(super) report: Value,
    pub(super) quality_warnings: Vec<Value>,
    pub(super) message: String,
    pub(super) repaired: Option<RepairedCut>,
}

/// 修复落地的新版本；预览可能重渲染失败，此时如实带回错误。
pub(super) struct RepairedCut {
    pub(super) storyboard: Option<StoryboardVersion>,
    pub(super) timeline: TimelineVersion,
    pub(super) preview: Option<PreviewResult>,
    pub(super) preview_error: Option<String>,
}

struct ReviewShot {
    shot_index: i64,
    asset_id: String,
    timeline_start_ms: i64,
    timeline_end_ms: i64,
    source_start_ms: i64,
    source_end_ms: i64,
    beat_id: Option<String>,
    /// 与故事板镜头一一对应才能重选 / 精修；手动插入的镜头只能报告。
    storyboard_shot: bool,
    evidence: Value,
}

struct Inspection {
    shots: Vec<ReviewShot>,
    findings: Vec<CutFinding>,
    sheet_count: usize,
    request_count: usize,
    uncovered: Vec<i64>,
    summaries: Vec<String>,
}

// ──────────────────────────────────────────────────────────────────────────────
// 入口
// ──────────────────────────────────────────────────────────────────────────────

/// `generate_storyboard` 出预览后自动复查并修一轮；失败或时间不够只报告，不挡生成结果。
pub(super) fn auto_review_after_generation(
    ctx: &ReviewContext<'_>,
    reselected: &mut HashSet<String>,
    timeline: &TimelineVersion,
    preview: &PreviewResult,
) -> CutReview {
    let remaining = crate::execution_deadline::current()
        .map(|deadline| deadline.saturating_duration_since(Instant::now()));
    if remaining.is_some_and(|left| left < AUTO_REVIEW_MIN_REMAINING) {
        return unreviewed(
            timeline,
            "skipped",
            "Not enough time left in this turn to review the rendered cut.",
            "\n成片复查已跳过：本轮剩余时间不足，本版未经复查。",
        );
    }
    match review_rendered_cut(ctx, reselected, timeline, Path::new(&preview.preview_path)) {
        Ok(review) => review,
        Err(error) => {
            log::warn!("Cut review after storyboard skipped: {error}");
            let brief = error.chars().take(200).collect::<String>();
            unreviewed(
                timeline,
                "failed",
                &brief,
                "\n成片复查未完成，本版未经复查（原因见 cutReview.reason）。",
            )
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// 复查主流程
// ──────────────────────────────────────────────────────────────────────────────

fn review_rendered_cut(
    ctx: &ReviewContext<'_>,
    reselected: &mut HashSet<String>,
    timeline: &TimelineVersion,
    preview_path: &Path,
) -> Result<CutReview, String> {
    if timeline.clips.is_empty() {
        return Err("The timeline has no clips to review.".to_owned());
    }
    let access = ModelAccess::resolve()?;
    let first = inspect_cut(ctx, &access, timeline, preview_path, "review")?;
    let mut findings = first.findings;
    let mut report = json!({
        "reviewedTimelineVersionId": timeline.id,
        "reviewedVersionNumber": timeline.version_number,
        "shotCount": first.shots.len(),
        "contactSheets": first.sheet_count,
        "modelRequests": first.request_count,
        "summary": first.summaries.join(" "),
        "basis": "Frames sampled from the rendered preview. source=timeline findings are timeline facts; source=frames findings are the reviewer model's reading of the frames.",
    });
    if !first.uncovered.is_empty() {
        report["uncoveredShotIndexes"] = json!(first.uncovered);
    }

    if findings.is_empty() {
        report["status"] = json!("clean");
        report["findings"] = json!([]);
        return Ok(CutReview {
            report,
            quality_warnings: Vec::new(),
            message: format!(
                "\n成片复查：逐镜看过 v{} 的预览，未发现需要处理的问题。",
                timeline.version_number
            ),
            repaired: None,
        });
    }

    let steps = plan_repairs(
        &mut findings,
        &first.shots,
        timed_to_shot_order(timeline),
        reselected,
    );
    if steps.is_empty() {
        report["status"] = json!("issues_found");
        report["findings"] = json!(findings);
        report["repairs"] = json!([]);
        return Ok(CutReview {
            quality_warnings: findings_to_warnings(&findings),
            report,
            message: format!(
                "\n成片复查发现 {} 处问题，都不能靠自动局部编辑安全修复，见 qualityWarnings。",
                findings.len()
            ),
            repaired: None,
        });
    }

    let run = execute_repairs(ctx, reselected, timeline, steps, &mut findings);
    report["findings"] = json!(findings);
    report["repairs"] = json!(run.records);
    let applied = run.records.iter().filter(|record| record["status"] == "applied").count();
    let Some(final_timeline) = run.timeline else {
        report["status"] = json!("repair_failed");
        return Ok(CutReview {
            quality_warnings: findings_to_warnings(&findings),
            report,
            message: format!(
                "\n成片复查发现 {} 处问题，自动修复没有落地（原因见 cutReview.repairs），时间线未改动。",
                findings.len()
            ),
            repaired: None,
        });
    };
    report["status"] = json!("repaired");
    report["finalTimelineVersionId"] = json!(final_timeline.id);
    report["finalVersionNumber"] = json!(final_timeline.version_number);

    let (preview, preview_error) =
        match crate::preview::render_preview_inner(ctx.app.clone(), final_timeline.id.clone()) {
            Ok(preview) => (Some(preview), None),
            Err(error) => (None, Some(error)),
        };
    let repair_note = repair_summary(&run.records);
    let mut message = format!(
        "\n成片复查发现 {} 处问题，已自动修复 {applied} 步并生成时间线 v{}：{repair_note}。",
        findings.len(),
        final_timeline.version_number
    );

    let verify_time_left = crate::execution_deadline::current()
        .map(|deadline| deadline.saturating_duration_since(Instant::now()))
        .is_none_or(|left| left >= VERIFY_MIN_REMAINING);
    let verification = match preview.as_ref() {
        None => Err("The repaired timeline could not be rendered, so it was not re-checked.".to_owned()),
        Some(_) if !verify_time_left => {
            Err("Not enough time left in this turn to re-check the repaired cut.".to_owned())
        }
        Some(rendered) => inspect_cut(
            ctx,
            &access,
            &final_timeline,
            Path::new(&rendered.preview_path),
            "verify",
        ),
    };
    let quality_warnings = match verification {
        Ok(check) => {
            report["verification"] = json!({
                "status": "checked",
                "timelineVersionId": final_timeline.id,
                "findings": check.findings,
                "summary": check.summaries.join(" "),
            });
            if check.findings.is_empty() {
                message.push_str("复核新预览未再发现问题。");
            } else {
                message.push_str(&format!(
                    "复核新预览仍有 {} 处问题，见 qualityWarnings。",
                    check.findings.len()
                ));
            }
            findings_to_warnings(&check.findings)
        }
        Err(reason) => {
            report["verification"] = json!({ "status": "not_checked", "reason": reason });
            message.push_str("修复后的版本未能复核。");
            let mut warnings = findings_to_warnings(
                &findings
                    .iter()
                    .filter(|finding| finding.repair_status.as_deref() != Some("applied"))
                    .cloned()
                    .collect::<Vec<_>>(),
            );
            warnings.push(json!({
                "category": "cut_review_unverified",
                "severity": "info",
                "message": "Repairs were applied but the new cut was not re-checked.",
            }));
            warnings
        }
    };
    if let Some(error) = preview_error.as_ref() {
        let brief = error.chars().take(160).collect::<String>();
        message.push_str(&format!(" 修复后的预览渲染失败：{brief}。"));
    }
    Ok(CutReview {
        report,
        quality_warnings,
        message,
        repaired: Some(RepairedCut {
            storyboard: run.storyboard,
            timeline: final_timeline,
            preview,
            preview_error,
        }),
    })
}

fn unreviewed(timeline: &TimelineVersion, status: &str, reason: &str, message: &str) -> CutReview {
    CutReview {
        report: json!({
            "status": status,
            "reviewedTimelineVersionId": timeline.id,
            "reason": reason,
        }),
        quality_warnings: vec![json!({
            "category": "cut_review_unavailable",
            "severity": "info",
            "message": reason,
        })],
        message: message.to_owned(),
        repaired: None,
    }
}

/// 抽帧 → 接触表 → 并发请求 → 解析问题，再并入本地判定的重复素材。
fn inspect_cut(
    ctx: &ReviewContext<'_>,
    access: &ModelAccess,
    timeline: &TimelineVersion,
    preview_path: &Path,
    label: &str,
) -> Result<Inspection, String> {
    let storyboard = crate::storyboard::load_storyboard_version(
        ctx.connection,
        &timeline.storyboard_version_id,
    )
    .ok()
    .filter(|storyboard| storyboard.project_id == ctx.project_id);
    let asset_ids = timeline
        .clips
        .iter()
        .map(|clip| clip.asset_id.clone())
        .collect::<HashSet<_>>();
    let sources = crate::storyboard::storyboard_sources(ctx.connection, ctx.project_id, Some(&asset_ids))
        .map(|(sources, _)| sources)
        .unwrap_or_else(|error| {
            log::warn!("Cut review evidence unavailable: {error}");
            Vec::new()
        });
    let shots = review_shots(timeline, storyboard.as_ref(), &sources);

    let directory = ctx
        .app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("derived")
        .join("cut_review")
        .join(&timeline.id)
        .join(label);
    let times = shots
        .iter()
        .map(|shot| sample_times(shot.timeline_start_ms, shot.timeline_end_ms))
        .collect::<Vec<_>>();
    let flat = times.iter().flatten().copied().collect::<Vec<_>>();
    let frames = extract_preview_frames(preview_path, &flat, &directory)?;
    let mut cursor = 0usize;
    let shot_frames = times
        .iter()
        .map(|shot_times| {
            let frames_for_shot = shot_times
                .iter()
                .zip(&frames[cursor..cursor + shot_times.len()])
                .filter_map(|(time, path)| path.clone().map(|path| (*time, path)))
                .collect::<Vec<_>>();
            cursor += shot_times.len();
            frames_for_shot
        })
        .collect::<Vec<_>>();

    let plan = plan_requests(shots.len());
    let single_request = plan.len() == 1;
    let all_indexes = shots.iter().map(|shot| shot.shot_index).collect::<Vec<_>>();
    let reuse_facts = reused_assets(&shots);
    let mut requests = Vec::new();
    let mut request_shots = Vec::new();
    let mut sheet_count = 0usize;
    let mut uncovered = Vec::new();
    for (request_index, sheets) in plan.iter().enumerate() {
        let covered = sheets.iter().flatten().copied().collect::<Vec<_>>();
        let mut blocks = vec![json!({
            "type": "input_text",
            "text": review_prompt(
                storyboard.as_ref(),
                &shots,
                &covered,
                &reuse_facts,
                single_request,
                request_index,
                plan.len(),
            ),
        })];
        let mut images = 0usize;
        for (sheet_index, rows) in sheets.iter().enumerate() {
            let sheet_rows = rows
                .iter()
                .map(|position| (shots[*position].shot_index, shot_frames[*position].clone()))
                .collect::<Vec<_>>();
            let output = directory.join(format!("sheet_{request_index}_{sheet_index}.jpg"));
            let Some(sheet) = compose_contact_sheet(&sheet_rows, &output) else {
                continue;
            };
            let Some(image) = read_input_image(&sheet) else {
                continue;
            };
            let listed = rows
                .iter()
                .map(|position| format!("#{}", shots[*position].shot_index))
                .collect::<Vec<_>>()
                .join(", ");
            blocks.push(json!({
                "type": "input_text",
                "text": format!("Contact sheet {}: rows top to bottom are shots {listed}.", sheet_index + 1),
            }));
            blocks.push(image);
            images += 1;
        }
        if images == 0 {
            uncovered.extend(covered.iter().map(|position| shots[*position].shot_index));
            continue;
        }
        sheet_count += images;
        requests.push(json!({
            "model": access.custom_config().map(|config| config.model.as_str()).unwrap_or("gpt-5.4"),
            "store": false,
            "stream": true,
            "input": [{ "role": "user", "content": blocks }],
            "text": { "format": { "type": "json_object" } }
        }));
        request_shots.push(covered);
    }
    if requests.is_empty() {
        return Err("No frames could be read from the rendered preview.".to_owned());
    }
    log::info!(
        "Cut review ({label}): {} shot(s), {sheet_count} contact sheet(s), {} request(s)",
        shots.len(),
        requests.len()
    );
    let responses = post_model_payloads_concurrently(access, &requests, Some(REVIEW_REQUEST_TIMEOUT));
    let mut findings = repeated_footage_findings(&shots);
    let mut summaries = Vec::new();
    let mut first_error = None;
    for (positions, response) in request_shots.iter().zip(responses) {
        let covered = positions
            .iter()
            .map(|position| shots[*position].shot_index)
            .collect::<HashSet<_>>();
        let parsed = response.and_then(|body| {
            model_response_json_text(access, &body)
                .ok_or_else(|| "The cut review response did not contain JSON.".to_owned())
                .and_then(|text| parse_findings(&text, &covered, single_request.then_some(all_indexes.as_slice())))
        });
        match parsed {
            Ok((model_findings, summary)) => {
                merge_model_findings(&mut findings, model_findings);
                if !summary.is_empty() {
                    summaries.push(summary);
                }
            }
            Err(error) => {
                log::warn!("Cut review request failed: {error}");
                first_error.get_or_insert(error);
                let mut missing = covered.into_iter().collect::<Vec<_>>();
                missing.sort_unstable();
                uncovered.extend(missing);
            }
        }
    }
    if uncovered.len() == shots.len() {
        return Err(first_error.unwrap_or_else(|| "The cut review request failed.".to_owned()));
    }
    Ok(Inspection {
        shots,
        findings,
        sheet_count,
        request_count: requests.len(),
        uncovered,
        summaries,
    })
}

// ──────────────────────────────────────────────────────────────────────────────
// 镜头事实与证据
// ──────────────────────────────────────────────────────────────────────────────

fn review_shots(
    timeline: &TimelineVersion,
    storyboard: Option<&StoryboardVersion>,
    sources: &[StoryboardSource],
) -> Vec<ReviewShot> {
    timeline
        .clips
        .iter()
        .map(|clip| {
            let story_shot = storyboard.and_then(|board| {
                board
                    .shots
                    .iter()
                    .find(|shot| shot.order_index == clip.shot_index && shot.asset_id == clip.asset_id)
            });
            ReviewShot {
                shot_index: clip.shot_index,
                asset_id: clip.asset_id.clone(),
                timeline_start_ms: clip.timeline_start_ms,
                timeline_end_ms: clip.timeline_end_ms,
                source_start_ms: clip.source_start_ms,
                source_end_ms: clip.source_end_ms,
                beat_id: story_shot
                    .map(|shot| shot.beat_id.clone())
                    .filter(|beat| !beat.is_empty()),
                storyboard_shot: story_shot.is_some() && clip.clip_kind == "source",
                evidence: shot_evidence(
                    sources,
                    clip,
                    story_shot.and_then(|shot| shot.segment_id.as_deref()),
                ),
            }
        })
        .collect()
}

/// 取与该镜源区间最贴近的片段证据，只留复查用得上的字段；证据可能过时，帧上看到的为准。
fn shot_evidence(sources: &[StoryboardSource], clip: &TimelineClip, segment_id: Option<&str>) -> Value {
    let best = sources
        .iter()
        .filter(|source| source.asset_id == clip.asset_id)
        .max_by_key(|source| {
            let segment = source.segment.as_ref();
            let same_id = segment_id.is_some() && segment.map(|segment| segment.id.as_str()) == segment_id;
            let overlap = segment.map_or(0, |segment| {
                overlap_ms(
                    (segment.start_ms, segment.end_ms),
                    (clip.source_start_ms, clip.source_end_ms),
                )
            });
            (same_id, overlap)
        });
    let Some(evidence) = best.and_then(|source| source.visual_evidence.first()) else {
        return Value::Null;
    };
    let mut out = serde_json::Map::new();
    let mut put = |key: &str, value: Option<&String>| {
        if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
            out.insert(key.to_owned(), json!(value.chars().take(160).collect::<String>()));
        }
    };
    put("caption", evidence.caption.as_ref());
    put("scene", evidence.scene.as_ref());
    if let Some(detail) = evidence.detail.as_ref() {
        put("timeOfDay", detail.time_of_day.as_ref());
        put("setting", detail.setting.as_ref());
        put("brightness", detail.brightness.as_ref());
        put("focus", detail.focus.as_ref());
        put("crowd", detail.crowd.as_ref());
        if !detail.on_screen_text.is_empty() {
            out.insert("sourceText".to_owned(), json!(detail.on_screen_text.iter().take(3).collect::<Vec<_>>()));
        }
        if !detail.brand_logos.is_empty() {
            out.insert("brandLogos".to_owned(), json!(detail.brand_logos.iter().take(3).collect::<Vec<_>>()));
        }
        if detail.exhibition == Some(true) {
            out.insert("exhibition".to_owned(), json!(true));
        }
    }
    Value::Object(out)
}

fn overlap_ms(a: (i64, i64), b: (i64, i64)) -> i64 {
    (a.1.min(b.1) - a.0.max(b.0)).max(0)
}

/// 同一素材出现在多镜：按素材首次出现的顺序给出镜号。
fn reused_assets(shots: &[ReviewShot]) -> Vec<(String, Vec<usize>)> {
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for (position, shot) in shots.iter().enumerate() {
        match groups.iter_mut().find(|(asset, _)| *asset == shot.asset_id) {
            Some((_, positions)) => positions.push(position),
            None => groups.push((shot.asset_id.clone(), vec![position])),
        }
    }
    groups.retain(|(_, positions)| positions.len() > 1);
    groups
}

/// 重复用同一素材是时间线事实，不等模型判断：源区间重叠为 high，不重叠为 medium；修复只换后出现的镜头。
fn repeated_footage_findings(shots: &[ReviewShot]) -> Vec<CutFinding> {
    reused_assets(shots)
        .into_iter()
        .map(|(asset_id, positions)| {
            let overlapping = positions.iter().enumerate().any(|(index, a)| {
                positions[index + 1..].iter().any(|b| {
                    overlap_ms(
                        (shots[*a].source_start_ms, shots[*a].source_end_ms),
                        (shots[*b].source_start_ms, shots[*b].source_end_ms),
                    ) > 0
                })
            });
            let uses = positions
                .iter()
                .map(|position| {
                    let shot = &shots[*position];
                    format!(
                        "shot {} (source {}-{} ms)",
                        shot.shot_index, shot.source_start_ms, shot.source_end_ms
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            CutFinding {
                shot_indexes: positions.iter().map(|position| shots[*position].shot_index).collect(),
                category: "repeated_footage".to_owned(),
                severity: if overlapping { "high" } else { "medium" }.to_owned(),
                evidence: format!(
                    "Asset {asset_id} is used {} times: {uses}{}.",
                    positions.len(),
                    if overlapping { "; the source ranges overlap, so the same moment plays twice" } else { "" }
                ),
                repair: "reselect".to_owned(),
                instruction: String::new(),
                proposed_order: None,
                source: "timeline",
                repair_status: None,
            }
        })
        .collect()
}

/// 模型也报了同一组镜头的重复时以时间线事实为准，不重复计数。
fn merge_model_findings(findings: &mut Vec<CutFinding>, model: Vec<CutFinding>) {
    for finding in model {
        let covered = finding.category == "repeated_footage"
            && findings.iter().any(|existing| {
                existing.source == "timeline"
                    && finding
                        .shot_indexes
                        .iter()
                        .all(|shot| existing.shot_indexes.contains(shot))
            });
        if !covered {
            findings.push(finding);
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// 抽帧与接触表
// ──────────────────────────────────────────────────────────────────────────────

/// 每镜抽帧时刻（成片时间）：够长抽开头 / 中间 / 结尾，短镜少抽以免落在同一帧。
fn sample_times(start_ms: i64, end_ms: i64) -> Vec<i64> {
    let span = (end_ms - start_ms).max(0);
    let fractions: &[f64] = if span < 700 {
        &[0.5]
    } else if span < 1_400 {
        &[0.25, 0.75]
    } else {
        &[0.12, 0.5, 0.88]
    };
    fractions
        .iter()
        .take(FRAMES_PER_SHOT)
        .map(|fraction| start_ms + (span as f64 * fraction).round() as i64)
        .collect()
}

/// 请求 → 图 → 行（镜头位置）：每请求最多 `MAX_SHOTS_PER_REQUEST` 镜，均摊到最多 4 张图。
fn plan_requests(shot_count: usize) -> Vec<Vec<Vec<usize>>> {
    let positions = (0..shot_count).collect::<Vec<_>>();
    positions
        .chunks(MAX_SHOTS_PER_REQUEST)
        .map(|chunk| {
            let rows = chunk.len().div_ceil(MAX_IMAGES_PER_REQUEST).max(1);
            chunk.chunks(rows).map(<[usize]>::to_vec).collect()
        })
        .collect()
}

/// 从预览按时刻抽帧，返回与 `times` 一一对应的路径；一次解码整条预览，数目对不上再逐帧补。
fn extract_preview_frames(
    preview: &Path,
    times: &[i64],
    directory: &Path,
) -> Result<Vec<Option<PathBuf>>, String> {
    if !preview.is_file() {
        return Err("The rendered preview file is missing.".to_owned());
    }
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    if let Ok(entries) = std::fs::read_dir(directory) {
        for entry in entries.flatten() {
            if entry.path().extension().is_some_and(|ext| ext == "jpg") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    let select = times
        .iter()
        .map(|time| {
            let seconds = *time as f64 / 1000.0;
            format!("lt(prev_pts*TB\\,{seconds:.3})*gte(pts*TB\\,{seconds:.3})")
        })
        .collect::<Vec<_>>()
        .join("+");
    let mut command = hidden_command("ffmpeg");
    command
        .args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(preview)
        .args([
            "-vf",
            &format!("select='{select}',scale=360:-2"),
            "-vsync",
            "vfr",
            "-q:v",
            "3",
        ])
        .arg(directory.join("batch_%04d.jpg"));
    let _ = run_hidden_command_with_timeout(&mut command, BATCH_FRAME_TIMEOUT);
    let batch = (1..=times.len())
        .map(|index| directory.join(format!("batch_{index:04}.jpg")))
        .collect::<Vec<_>>();
    let produced = std::fs::read_dir(directory)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.file_name().to_string_lossy().starts_with("batch_"))
                .count()
        })
        .unwrap_or(0);
    if produced == times.len() && batch.iter().all(|path| path.is_file()) {
        return Ok(batch.into_iter().map(Some).collect());
    }
    log::warn!(
        "Cut review batch extract produced {produced}/{} frames; extracting one by one.",
        times.len()
    );
    let mut frames = Vec::with_capacity(times.len());
    for (index, time) in times.iter().enumerate() {
        crate::execution_deadline::check()?;
        let destination = directory.join(format!("frame_{:04}.jpg", index + 1));
        let mut command = hidden_command("ffmpeg");
        command
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-ss",
                &format!("{:.3}", *time as f64 / 1000.0),
                "-i",
            ])
            .arg(preview)
            .args(["-frames:v", "1", "-vf", "scale=360:-2", "-q:v", "3"])
            .arg(&destination);
        let ok = matches!(
            run_hidden_command_with_timeout(&mut command, SINGLE_FRAME_TIMEOUT),
            Ok(output) if output.status.success()
        ) && destination.is_file();
        frames.push(ok.then_some(destination));
    }
    if frames.iter().all(Option::is_none) {
        return Err("FFmpeg could not read frames from the rendered preview.".to_owned());
    }
    Ok(frames)
}

/// 一行一镜，行内按时间从左到右；每格左上角标「#镜号 成片秒数」。格子按成片画幅定，不变形。
fn compose_contact_sheet(rows: &[(i64, Vec<(i64, PathBuf)>)], output: &Path) -> Option<PathBuf> {
    const GAP: u32 = 6;
    let first = rows
        .iter()
        .flat_map(|(_, frames)| frames.iter())
        .find_map(|(_, path)| image::open(path).ok())?
        .to_rgb8();
    let (cell_w, cell_h) = if first.height() * 10 > first.width() * 11 {
        (180, 320)
    } else if first.width() * 10 > first.height() * 11 {
        (320, 180)
    } else {
        (240, 240)
    };
    let columns = FRAMES_PER_SHOT as u32;
    let width = columns * cell_w + (columns - 1) * GAP;
    let height = rows.len() as u32 * cell_h + (rows.len() as u32).saturating_sub(1) * GAP;
    let mut sheet: RgbImage = ImageBuffer::from_pixel(width, height, Rgb([96, 96, 96]));
    for (row, (shot_index, frames)) in rows.iter().enumerate() {
        for column in 0..columns {
            let mut cell: RgbImage = ImageBuffer::from_pixel(cell_w, cell_h, Rgb([0, 0, 0]));
            if let Some((time, path)) = frames.get(column as usize) {
                if let Ok(frame) = image::open(path) {
                    cell = fit_into_box(&frame.to_rgb8(), cell_w, cell_h);
                }
                draw_cell_label(&mut cell, &format!("#{shot_index} {:.1}s", *time as f64 / 1000.0));
            }
            image::imageops::replace(
                &mut sheet,
                &cell,
                (column * (cell_w + GAP)).into(),
                (row as u32 * (cell_h + GAP)).into(),
            );
        }
    }
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    sheet.save(output).ok()?;
    output.is_file().then(|| output.to_path_buf())
}

// ──────────────────────────────────────────────────────────────────────────────
// 模型请求与解析
// ──────────────────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn review_prompt(
    storyboard: Option<&StoryboardVersion>,
    shots: &[ReviewShot],
    covered: &[usize],
    reuse: &[(String, Vec<usize>)],
    single_request: bool,
    request_index: usize,
    request_count: usize,
) -> String {
    let brief = storyboard
        .map(|board| board.brief.chars().take(1_200).collect::<String>())
        .unwrap_or_default();
    let beats = storyboard
        .map(|board| {
            board
                .beats
                .iter()
                .map(|beat| {
                    json!({
                        "beatId": beat.id,
                        "purpose": beat.purpose.chars().take(160).collect::<String>(),
                        "requiredVisual": beat.required_visual.chars().take(160).collect::<String>(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let shot_facts = shots
        .iter()
        .map(|shot| {
            json!({
                "shotIndex": shot.shot_index,
                "timelineMs": [shot.timeline_start_ms, shot.timeline_end_ms],
                "beatId": shot.beat_id,
                "assetId": shot.asset_id,
                "sourceMs": [shot.source_start_ms, shot.source_end_ms],
                "priorEvidence": shot.evidence,
            })
        })
        .collect::<Vec<_>>();
    let reuse_lines = reuse
        .iter()
        .map(|(asset, positions)| {
            let list = positions
                .iter()
                .map(|position| shots[*position].shot_index.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            format!("- asset {asset} is used in shots {list}")
        })
        .collect::<Vec<_>>();
    let covered_list = covered
        .iter()
        .map(|position| shots[*position].shot_index.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let scope = if single_request {
        "The contact sheets below cover every shot of the cut.".to_owned()
    } else {
        format!(
            "This is request {} of {request_count}; its contact sheets cover shots {covered_list}. Report findings only for those shots. Do not propose a reorder (proposedOrder must be null).",
            request_index + 1
        )
    };
    format!(
        "You are a senior video editor reviewing the ACTUAL rendered cut of a short video before it goes to the client.\n\
Brief: {brief}\n\
Beats in brief order: {beats}\n\
{scope}\n\
Each contact-sheet row is one shot; its cells are frames sampled from the start, middle and end of that shot. \
The label in each cell reads \"#<shotIndex> <seconds into the cut>s\".\n\
Shots of the whole cut in playback order (timeline facts; priorEvidence comes from earlier footage analysis and may be wrong - what you see in the frames wins): {shots}\n\
Timeline facts about reused footage:\n{reuse}\n\
Subtitles and titles burned in by the edit are expected; flag text, logos, watermarks or crowds only when they come from the source footage and hurt the video.\n\
Check every shot you can see for:\n\
- repeated_footage: the same or nearly the same footage appears in more than one shot.\n\
- continuity: day/night jumps, unexplained location jumps, or screen-direction flips between adjacent shots.\n\
- beat_order: shots whose order contradicts the brief (for example night scenes before daytime beats the brief places first).\n\
- weak_shot: blurred, out of focus, shaky, too dark, badly framed, or the subject is cut off.\n\
- text_logo_crowd: distracting source text, brand logos, watermarks, trade-show booths or crowds.\n\
Report only real problems you can see; do not list shots that are fine.\n\
Return JSON only: {{\"findings\":[{{\"shotIndexes\":[3],\"category\":\"continuity\",\"severity\":\"high\",\"evidence\":\"what the frames show, one sentence\",\"repair\":\"reselect\",\"instruction\":\"daytime lakeshore with the van in sunlight\",\"proposedOrder\":null}}],\"summary\":\"one sentence about the cut\"}}\n\
category is one of repeated_footage, continuity, beat_order, weak_shot, text_logo_crowd. \
severity: high = any viewer notices at once; medium = an editor would fix it; low = minor polish.\n\
repair: reselect = the shot needs different footage; refine = the same footage with different in/out points or crop would fix it (for example start after the blur); \
reorder = the shots are fine but in the wrong order, then proposedOrder lists every shotIndex of the cut exactly once in the new order; none = editing cannot fix it.\n\
instruction: for reselect, describe positively what the replacement should show, without negations; for refine, say how the cut should move; otherwise an empty string.",
        beats = Value::Array(beats),
        shots = Value::Array(shot_facts),
        reuse = if reuse_lines.is_empty() { "- none".to_owned() } else { reuse_lines.join("\n") },
    )
}

/// 解析模型问题：镜号必须在本请求覆盖范围内，未知取值按保守默认处理；重排只在看过全片时接受完整排列。
fn parse_findings(
    text: &str,
    covered: &HashSet<i64>,
    all_shots: Option<&[i64]>,
) -> Result<(Vec<CutFinding>, String), String> {
    let json_text = match (text.find('{'), text.rfind('}')) {
        (Some(start), Some(end)) if end > start => &text[start..=end],
        _ => text,
    };
    let value: Value = serde_json::from_str(json_text)
        .map_err(|_| "The cut review response was not valid JSON.".to_owned())?;
    let items = value["findings"]
        .as_array()
        .ok_or_else(|| "The cut review response has no findings array.".to_owned())?;
    let mut findings = Vec::new();
    for item in items {
        let mut shot_indexes = item["shotIndexes"]
            .as_array()
            .map(|values| values.iter().filter_map(Value::as_i64).collect::<Vec<_>>())
            .unwrap_or_default();
        shot_indexes.retain(|shot| covered.contains(shot));
        shot_indexes.dedup();
        let category = item["category"].as_str().unwrap_or("").trim().to_lowercase();
        if shot_indexes.is_empty() || !CATEGORIES.contains(&category.as_str()) {
            continue;
        }
        let severity = match item["severity"].as_str().map(str::to_lowercase).as_deref() {
            Some("high") => "high",
            Some("low") => "low",
            _ => "medium",
        };
        let mut repair = match item["repair"].as_str().map(str::to_lowercase).as_deref() {
            Some("reselect") => "reselect",
            Some("refine") => "refine",
            Some("reorder") => "reorder",
            _ => "none",
        };
        let proposed_order = if repair == "reorder" {
            let order = item["proposedOrder"]
                .as_array()
                .map(|values| values.iter().filter_map(Value::as_i64).collect::<Vec<_>>());
            match (order, all_shots) {
                (Some(order), Some(all)) if is_permutation(&order, all) => Some(order),
                _ => {
                    repair = "none";
                    None
                }
            }
        } else {
            None
        };
        findings.push(CutFinding {
            shot_indexes,
            category,
            severity: severity.to_owned(),
            evidence: item["evidence"]
                .as_str()
                .unwrap_or("")
                .trim()
                .chars()
                .take(300)
                .collect(),
            repair: repair.to_owned(),
            instruction: item["instruction"]
                .as_str()
                .unwrap_or("")
                .trim()
                .chars()
                .take(200)
                .collect(),
            proposed_order,
            source: "frames",
            repair_status: None,
        });
    }
    let summary = value["summary"]
        .as_str()
        .unwrap_or("")
        .trim()
        .chars()
        .take(300)
        .collect();
    Ok((findings, summary))
}

fn is_permutation(order: &[i64], all: &[i64]) -> bool {
    let mut left = order.to_vec();
    let mut right = all.to_vec();
    left.sort_unstable();
    right.sort_unstable();
    left == right && order != all
}

// ──────────────────────────────────────────────────────────────────────────────
// 修复计划与执行
// ──────────────────────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
enum RepairKind {
    Refine { shots: Vec<i64>, instruction: Option<String> },
    Reselect { beats: Vec<String>, instruction: Option<String> },
    Reorder { order: Vec<i64> },
}

#[derive(Debug, PartialEq)]
struct PlannedRepair {
    kind: RepairKind,
    findings: Vec<usize>,
}

/// 旁白或字幕按当前镜头顺序对时：重排会让声画错位，不自动做。
fn timed_to_shot_order(timeline: &TimelineVersion) -> bool {
    timeline
        .voiceover_tracks
        .iter()
        .any(|track| track.enabled && !track.cues.is_empty())
        || timeline
            .text_tracks
            .iter()
            .any(|track| track.enabled && !track.cues.is_empty())
}

fn severity_rank(severity: &str) -> u8 {
    match severity {
        "high" => 0,
        "medium" => 1,
        _ => 2,
    }
}

/// 只修 high / medium：先精修切点（不改镜号），再重选（可能改镜号），最后在没有重选时重排。
/// 同一拍只进一个重选；重选覆盖的镜头不再精修；每条问题都写明计划或未尝试的原因。
fn plan_repairs(
    findings: &mut [CutFinding],
    shots: &[ReviewShot],
    timed_to_order: bool,
    already_reselected: &HashSet<String>,
) -> Vec<PlannedRepair> {
    let beat_of = |index: i64| {
        shots
            .iter()
            .find(|shot| shot.shot_index == index && shot.storyboard_shot)
            .and_then(|shot| shot.beat_id.clone())
    };
    let mut order = (0..findings.len()).collect::<Vec<_>>();
    order.sort_by_key(|index| severity_rank(&findings[*index].severity));
    let mut refine: Vec<(i64, Option<String>, Vec<usize>)> = Vec::new();
    let mut reselect: Vec<(String, Option<String>, Vec<usize>)> = Vec::new();
    let mut reorder: Option<(Vec<i64>, usize)> = None;
    for index in order {
        let finding = &mut findings[index];
        if finding.severity == "low" {
            finding.repair_status = Some("not_attempted: low severity, reported only".to_owned());
            continue;
        }
        let instruction = Some(finding.instruction.clone()).filter(|text| !text.is_empty());
        let mut reasons = Vec::new();
        let mut planned = false;
        match finding.repair.as_str() {
            "reselect" => {
                let targets = if finding.category == "repeated_footage" {
                    finding.shot_indexes.iter().skip(1).copied().collect::<Vec<_>>()
                } else {
                    finding.shot_indexes.clone()
                };
                for shot in targets {
                    let Some(beat) = beat_of(shot) else {
                        reasons.push(format!("shot {shot} is not a storyboard shot"));
                        continue;
                    };
                    if already_reselected.contains(&beat) {
                        reasons.push(format!("beat {beat} was already re-picked this turn"));
                        continue;
                    }
                    if let Some(entry) = reselect.iter_mut().find(|entry| entry.0 == beat) {
                        entry.2.push(index);
                        planned = true;
                        continue;
                    }
                    if reselect.len() >= MAX_RESELECT_BEATS {
                        reasons.push(format!("at most {MAX_RESELECT_BEATS} beats are re-picked per round"));
                        continue;
                    }
                    reselect.push((beat, instruction.clone(), vec![index]));
                    planned = true;
                }
            }
            "refine" => {
                for shot in finding.shot_indexes.clone() {
                    if beat_of(shot).is_none() {
                        reasons.push(format!("shot {shot} is not a storyboard shot"));
                        continue;
                    }
                    if let Some(entry) = refine.iter_mut().find(|entry| entry.0 == shot) {
                        entry.2.push(index);
                        planned = true;
                        continue;
                    }
                    if refine.len() >= MAX_REFINE_SHOTS {
                        reasons.push(format!("at most {MAX_REFINE_SHOTS} shots are refined per round"));
                        continue;
                    }
                    refine.push((shot, instruction.clone(), vec![index]));
                    planned = true;
                }
            }
            "reorder" => match finding.proposed_order.clone() {
                _ if timed_to_order => reasons
                    .push("narration or subtitles are timed to the current shot order".to_owned()),
                Some(_) if reorder.is_some() => {
                    reasons.push("another reorder is already planned".to_owned())
                }
                Some(proposed) => {
                    reorder = Some((proposed, index));
                    planned = true;
                }
                None => reasons.push("no complete shot order was proposed".to_owned()),
            },
            _ => reasons.push("editing alone cannot fix it".to_owned()),
        }
        finding.repair_status = Some(if planned {
            "planned".to_owned()
        } else {
            format!("not_attempted: {}", reasons.join("; "))
        });
    }

    // 重选会整拍换片，同拍镜头的精修并入该重选。
    refine.retain(|(shot, _, finding_ids)| {
        let Some(beat) = beat_of(*shot) else {
            return true;
        };
        match reselect.iter_mut().find(|entry| entry.0 == beat) {
            Some(entry) => {
                entry.2.extend(finding_ids.iter().copied());
                false
            }
            None => true,
        }
    });
    if let Some((_, finding)) = reorder.as_ref().filter(|_| !reselect.is_empty()) {
        findings[*finding].repair_status = Some(
            "not_attempted: other shots were re-picked this round, so the proposed order no longer applies"
                .to_owned(),
        );
        reorder = None;
    }

    let mut steps = Vec::new();
    if !refine.is_empty() {
        let instruction = joined_instruction(refine.iter().map(|(shot, text, _)| (*shot, text.as_deref())));
        steps.push(PlannedRepair {
            kind: RepairKind::Refine {
                shots: refine.iter().map(|(shot, _, _)| *shot).collect(),
                instruction,
            },
            findings: unique(refine.iter().flat_map(|(_, _, ids)| ids.iter().copied())),
        });
    }
    // 不同替换要求分开重选：要求会拼进该拍的召回文本，混在一起会把 A 拍的描述带进 B 拍。
    let mut groups: Vec<(Option<String>, Vec<String>, Vec<usize>)> = Vec::new();
    for (beat, instruction, ids) in reselect {
        match groups.iter_mut().find(|group| group.0 == instruction) {
            Some(group) => {
                group.1.push(beat);
                group.2.extend(ids);
            }
            None => groups.push((instruction, vec![beat], ids)),
        }
    }
    for (position, (instruction, beats, ids)) in groups.into_iter().enumerate() {
        if position >= MAX_RESELECT_CALLS {
            for id in ids {
                findings[id].repair_status = Some(format!(
                    "not_attempted: at most {MAX_RESELECT_CALLS} re-pick calls per round"
                ));
            }
            continue;
        }
        steps.push(PlannedRepair {
            kind: RepairKind::Reselect { beats, instruction },
            findings: unique(ids.into_iter()),
        });
    }
    if let Some((order, finding)) = reorder {
        steps.push(PlannedRepair {
            kind: RepairKind::Reorder { order },
            findings: vec![finding],
        });
    }
    steps
}

fn joined_instruction<'a>(items: impl Iterator<Item = (i64, Option<&'a str>)>) -> Option<String> {
    let parts = items
        .filter_map(|(shot, text)| text.map(|text| format!("shot {shot}: {text}")))
        .collect::<Vec<_>>();
    (!parts.is_empty()).then(|| parts.join("; ").chars().take(500).collect())
}

fn unique(ids: impl Iterator<Item = usize>) -> Vec<usize> {
    let mut out = Vec::new();
    for id in ids {
        if !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

struct RepairRun {
    timeline: Option<TimelineVersion>,
    storyboard: Option<StoryboardVersion>,
    records: Vec<Value>,
}

/// 按计划依次调用现有局部编辑；每步基于上一步落地的版本，失败只记原因、不回滚已落地的步骤。
fn execute_repairs(
    ctx: &ReviewContext<'_>,
    reselected: &mut HashSet<String>,
    base: &TimelineVersion,
    steps: Vec<PlannedRepair>,
    findings: &mut [CutFinding],
) -> RepairRun {
    let scope = LocalEditScope {
        project_id: ctx.project_id,
        editing_task_id: ctx.editing_task_id,
        conversation_id: ctx.conversation_id,
        agent_task_id: ctx.agent_task_id,
    };
    let mut current = base.clone();
    let mut storyboard = None;
    let mut changed = false;
    let mut records = Vec::new();
    let mut failed_findings = HashSet::new();
    for step in steps {
        let (tool, target) = match &step.kind {
            RepairKind::Refine { shots, .. } => ("refine_shot_ranges", json!({ "shotIndexes": shots })),
            RepairKind::Reselect { beats, .. } => ("reselect_shots", json!({ "beatIds": beats })),
            RepairKind::Reorder { order } => ("reorder_clips", json!({ "order": order })),
        };
        let outcome = crate::execution_deadline::check().and_then(|_| match &step.kind {
            RepairKind::Refine { shots, instruction } => local_edit::refine_shot_ranges(
                ctx.app,
                &scope,
                &current.id,
                shots,
                instruction.as_deref(),
            )
            .map(|done| (Some(done.storyboard), done.timeline, json!(done.changes))),
            RepairKind::Reselect { beats, instruction } => local_edit::reselect_shots(
                ctx.app,
                &scope,
                &current.id,
                ReselectTarget::Beats(beats.clone()),
                instruction.as_deref(),
                false,
                reselected,
            )
            .map(|done| {
                reselected.extend(done.changes.iter().map(|change| change.beat_id.clone()));
                (Some(done.storyboard), done.timeline, json!(done.changes))
            }),
            RepairKind::Reorder { order } => crate::timeline::reorder_clips(
                ctx.connection,
                ctx.project_id,
                ctx.editing_task_id,
                ctx.conversation_id,
                ctx.agent_task_id,
                &current,
                order,
            )
            .map(|timeline| (None, timeline, Value::Null)),
        });
        match outcome {
            Ok((new_storyboard, timeline, changes)) => {
                let mut record = json!({
                    "tool": tool,
                    "target": target,
                    "status": "applied",
                    "timelineVersionId": timeline.id,
                    "versionNumber": timeline.version_number,
                });
                if !changes.is_null() {
                    record["changes"] = changes;
                }
                records.push(record);
                for id in &step.findings {
                    if !failed_findings.contains(id) {
                        findings[*id].repair_status = Some("applied".to_owned());
                    }
                }
                if new_storyboard.is_some() {
                    storyboard = new_storyboard;
                }
                current = timeline;
                changed = true;
            }
            Err(error) => {
                log::warn!("Cut review repair {tool} failed: {error}");
                let brief = error.chars().take(200).collect::<String>();
                records.push(json!({
                    "tool": tool,
                    "target": target,
                    "status": "failed",
                    "error": brief,
                }));
                for id in &step.findings {
                    failed_findings.insert(*id);
                    findings[*id].repair_status = Some(format!("failed: {brief}"));
                }
            }
        }
    }
    RepairRun {
        timeline: changed.then_some(current),
        storyboard,
        records,
    }
}

fn repair_summary(records: &[Value]) -> String {
    let parts = records
        .iter()
        .filter(|record| record["status"] == "applied")
        .map(|record| match record["tool"].as_str() {
            Some("reselect_shots") => format!("重选 {}", join_values(&record["target"]["beatIds"])),
            Some("refine_shot_ranges") => format!("精修镜头 {}", join_values(&record["target"]["shotIndexes"])),
            _ => "调整镜头顺序".to_owned(),
        })
        .collect::<Vec<_>>();
    let failed = records.iter().filter(|record| record["status"] == "failed").count();
    let mut summary = parts.join("，");
    if failed > 0 {
        summary.push_str(&format!("；另有 {failed} 步修复失败，原因见 cutReview.repairs"));
    }
    summary
}

fn join_values(value: &Value) -> String {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| item.as_str().map(str::to_owned).unwrap_or_else(|| item.to_string()))
                .collect::<Vec<_>>()
                .join("、")
        })
        .unwrap_or_default()
}

/// 未修掉的问题转成 qualityWarnings：high / medium 为 warning，low 为 info。
fn findings_to_warnings(findings: &[CutFinding]) -> Vec<Value> {
    findings
        .iter()
        .filter(|finding| finding.repair_status.as_deref() != Some("applied"))
        .map(|finding| {
            json!({
                "category": format!("cut_review_{}", finding.category),
                "severity": if finding.severity == "low" { "info" } else { "warning" },
                "shotIndexes": finding.shot_indexes,
                "message": finding.evidence,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot(index: i64, asset: &str, source: (i64, i64), beat: &str) -> ReviewShot {
        ReviewShot {
            shot_index: index,
            asset_id: asset.into(),
            timeline_start_ms: (index - 1) * 2_500,
            timeline_end_ms: index * 2_500,
            source_start_ms: source.0,
            source_end_ms: source.1,
            beat_id: Some(beat.into()),
            storyboard_shot: true,
            evidence: Value::Null,
        }
    }

    /// 2026-09-27「Weekend Road Trip」：素材 8973588 在同一条成片里用了两次。
    /// 重复是时间线事实，必须报出来，且只重选后出现的那一拍。
    #[test]
    fn reused_asset_is_flagged_and_only_the_later_shot_is_repicked() {
        let shots = vec![
            shot(1, "8973588", (0, 2_500), "b1"),
            shot(2, "8976543", (1_000, 3_500), "b2"),
            shot(3, "8973588", (4_000, 6_500), "b3"),
        ];
        let mut findings = repeated_footage_findings(&shots);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].shot_indexes, vec![1, 3]);
        assert_eq!(findings[0].severity, "medium");
        let steps = plan_repairs(&mut findings, &shots, true, &HashSet::new());
        assert_eq!(
            steps,
            vec![PlannedRepair {
                kind: RepairKind::Reselect { beats: vec!["b3".into()], instruction: None },
                findings: vec![0],
            }]
        );
    }

    #[test]
    fn contact_sheets_never_exceed_the_image_limit_per_request() {
        for count in [1, 5, 12, 32, 33, 70] {
            let plan = plan_requests(count);
            assert!(plan.iter().all(|sheets| sheets.len() <= MAX_IMAGES_PER_REQUEST));
            assert!(plan.iter().flatten().all(|rows| rows.len() <= MAX_ROWS_PER_SHEET));
            let covered = plan.iter().flatten().flatten().copied().collect::<Vec<_>>();
            assert_eq!(covered, (0..count).collect::<Vec<_>>());
        }
    }
}
