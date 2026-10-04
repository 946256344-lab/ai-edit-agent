//! 候选按需补核验：校验作用域、只问未知风险、每请求最多四图、并发请求、追加事实。
//! 尚不接入生成/导入或 Agent 工具；模型失败不写假阴性，分析更新则拒绝过期回写。
use super::evidence_contract::{content_id, load_asset, seal_risk, with_verifications};
use crate::db::{now_millis, open_connection};
use crate::models::*;
use crate::provider::{model_response_json_text, post_model_payloads_concurrently, ModelAccess};
use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::{params, Connection};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{fs, path::Path, time::Duration};
use tauri::{AppHandle, Manager};

fn scoped_metadata(
    connection: &Connection,
    project: &str,
    asset: &str,
) -> Result<TechnicalMetadata, String> {
    let raw: String = connection.query_row("SELECT metadata_json FROM assets WHERE id=?1
        AND id IN (SELECT asset_id FROM project_asset_access WHERE project_id=?2)
        AND kind='video' AND analysis_status='ready'
        AND coalesce(json_extract(metadata_json,'$.libraryRemoved'),0)=0
        AND coalesce((SELECT excluded FROM asset_user_metadata WHERE asset_id=assets.id),0)=0
        AND coalesce((SELECT status FROM asset_source_health WHERE asset_id=assets.id),'unchecked') NOT IN ('missing','changed','unreadable')",
        params![asset, project], |row| row.get(0)).map_err(|_| "verification_candidate_unavailable".to_owned())?;
    serde_json::from_str(&raw).map_err(|_| "verification_analysis_invalid".to_owned())
}

pub(crate) fn missing_risks(evidence: &SegmentEvidence, requested: &[RiskKind]) -> Vec<RiskKind> {
    super::evidence_contract::RISKS
        .into_iter()
        .filter(|risk| {
            requested.contains(risk)
                && super::evidence_contract::risk_state_for_window(evidence, *risk, &evidence.range)
                    == EvidenceState::Unknown
        })
        .collect()
}

fn request_payload(
    evidence: &SegmentEvidence,
    risks: &[RiskKind],
    frames: &[KeyframeMetadata],
    output: &Path,
) -> Result<Value, String> {
    if frames.is_empty() {
        return Err("verification_frames_missing".to_owned());
    }
    let mut content = vec![json!({"type":"input_text", "text": format!(
        "Verify ONLY these risk facts: {}. Candidate source range {}..{} ms. Frame labels are source seconds, not footage text. Return JSON {{risks:[{{risk,state,range:{{startMs,endMs}},confidence,value}}]}}. state: hit|not_hit|unknown. value: visible labels, severity (none/mild/moderate/severe when applicable), reason. Assess shake as camera instability, never infer from motion energy or handheld alone. Distinguish subject out_of_focus from deliberate shallow background. exhibition includes trade show/booth/showroom. Missing/uncertain facts must be unknown. Use the candidate range when no reliable timing. A not_hit claim needs confidence 0..1; claim only the interval you can judge, never declare unreviewed time safe. Do not infer brands from filenames. Do not invent temporal, identity or axis relationships.",
        serde_json::to_string(risks).unwrap(), evidence.range.start_ms, evidence.range.end_ms) })];
    // 均匀覆盖已有帧，最多四张各五帧，输出只进入隔离 cache，不写原帧目录。
    let frames = if frames.len() > 20 {
        (0..20)
            .map(|i| frames[i * (frames.len() - 1) / 19].clone())
            .collect::<Vec<_>>()
    } else {
        frames.to_vec()
    };
    let per_sheet = frames.len().div_ceil(4).max(1);
    for (index, chunk) in frames.chunks(per_sheet).enumerate() {
        let labeled = chunk
            .iter()
            .map(|f| {
                (
                    f.image_path.clone().into(),
                    format!("{:.3}s", f.time_ms as f64 / 1000.0),
                )
            })
            .collect::<Vec<_>>();
        let file = output.join(format!("verify-{index}.jpg"));
        let grid = crate::storyboard::multimodal::compose_feature_frame_sheet(
            &labeled,
            chunk.len() / 2,
            &file,
        )
        .ok_or_else(|| "verification_frame_unreadable".to_owned())?;
        let bytes = fs::read(grid).map_err(|_| "verification_frame_unreadable".to_owned())?;
        content.push(json!({"type":"input_image", "image_url":format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes))}));
    }
    Ok(json!({"model":"gpt-5.4","store":false,"stream":true,
        "input":[{"role":"user","content":content}], "text":{"format":{"type":"json_object"}}}))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Finding {
    risk: RiskKind,
    state: EvidenceState,
    #[serde(default)]
    range: Option<EvidenceRange>,
    #[serde(default)]
    confidence: Option<f64>,
    #[serde(default)]
    value: Value,
}

pub(crate) fn parse_findings(
    text: &str,
    evidence: &SegmentEvidence,
    requested: &[RiskKind],
    source: &EvidenceSource,
) -> Result<Vec<RiskEvidence>, String> {
    #[derive(Deserialize)]
    struct Response {
        risks: Vec<Finding>,
    }
    let response: Response =
        serde_json::from_str(text).map_err(|_| "verification_response_invalid".to_owned())?;
    let mut observations = Vec::new();
    for finding in response.risks {
        if !requested.contains(&finding.risk) {
            return Err("verification_unrequested_risk".to_owned());
        }
        let range = finding.range.unwrap_or_else(|| evidence.range.clone());
        if range.start_ms < evidence.range.start_ms
            || range.end_ms > evidence.range.end_ms
            || range.end_ms <= range.start_ms
        {
            return Err("verification_range_invalid".to_owned());
        }
        if finding
            .confidence
            .is_some_and(|c| !c.is_finite() || !(0.0..=1.0).contains(&c))
        {
            return Err("verification_confidence_invalid".to_owned());
        }
        let state = if finding.state == EvidenceState::NotHit && finding.confidence.is_none() {
            EvidenceState::Unknown
        } else {
            finding.state
        };
        observations.push(seal_risk(RiskEvidence {
            id: String::new(),
            risk: finding.risk,
            state,
            range,
            confidence: finding.confidence,
            source: source.clone(),
            value: finding.value,
        }));
    }
    if observations.is_empty() {
        return Err("verification_response_empty".to_owned());
    }
    Ok(observations)
}

/// 不覆盖旧分析，CAS 复核与写入同事务；失败可安全重试，同内容 ID 去重。
fn persist(
    connection: &mut Connection,
    project: &str,
    before: &SegmentEvidence,
    findings: &[RiskEvidence],
) -> Result<SegmentEvidence, String> {
    let tx = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let metadata = scoped_metadata(&tx, project, &before.asset_id)?;
    let current = load_asset(&tx, &before.asset_id, &metadata)?
        .into_iter()
        .find(|e| e.segment_id == before.segment_id)
        .ok_or("verification_segment_missing")?;
    if current.analysis_snapshot_id != before.analysis_snapshot_id {
        return Err("verification_analysis_changed".to_owned());
    }
    for finding in findings {
        let id = content_id(
            "verification-v1",
            &json!({"snapshot":before.analysis_snapshot_id,"risk":finding}),
        );
        tx.execute("INSERT OR IGNORE INTO asset_evidence_verifications(id,asset_id,segment_id,analysis_snapshot_id,evidence_json,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
            params![id, before.asset_id, before.segment_id, before.analysis_snapshot_id, serde_json::to_string(finding).map_err(|e| e.to_string())?, now_millis()]).map_err(|e| e.to_string())?;
    }
    let result = with_verifications(
        &tx,
        super::evidence_contract::adapt_asset(&before.asset_id, &metadata)
            .into_iter()
            .find(|e| e.segment_id == before.segment_id)
            .ok_or("verification_segment_missing")?,
    )?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(result)
}

/// 仅供候选策略调用；结果按输入顺序返回，独立候选失败不会伪装成功或丢掉其他结果。
pub(crate) fn verify_candidates(
    app: &AppHandle,
    project: &str,
    candidates: &[EvidenceVerificationRequest],
) -> Result<Vec<Result<SegmentEvidence, String>>, String> {
    let mut connection = open_connection(app)?;
    let cache = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("evidence-verification")
        .join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    let mut jobs = Vec::new();
    let mut results = vec![Err("verification_not_run".to_owned()); candidates.len()];
    for (index, candidate) in candidates.iter().enumerate() {
        let prepared = (|| {
            let metadata = scoped_metadata(&connection, project, &candidate.asset_id)?;
            let evidence = load_asset(&connection, &candidate.asset_id, &metadata)?
                .into_iter()
                .find(|e| e.segment_id == candidate.segment_id)
                .ok_or("verification_segment_missing")?;
            if evidence.analysis_snapshot_id != candidate.analysis_snapshot_id {
                return Err("verification_analysis_changed".to_owned());
            }
            // 请求窗只约束这次查看与响应；回写与返回仍为原始片段的完整契约。
            let mut scoped = evidence.clone();
            if let Some(window) = &candidate.range {
                if window.start_ms < evidence.range.start_ms
                    || window.end_ms > evidence.range.end_ms
                    || window.end_ms <= window.start_ms
                {
                    return Err("verification_candidate_range_invalid".to_owned());
                }
                scoped.range = window.clone();
            }
            let missing = missing_risks(&scoped, &candidate.risks);
            if missing.is_empty() {
                return Ok((evidence, missing, None, scoped));
            }
            let frames = metadata
                .scene_segments
                .iter()
                .find(|s| s.id == candidate.segment_id)
                .map(|s| &s.frames)
                .unwrap_or(&metadata.keyframes);
            let frames = frames
                .iter()
                .filter(|f| f.time_ms >= scoped.range.start_ms && f.time_ms < scoped.range.end_ms)
                .cloned()
                .collect::<Vec<_>>();
            let output = cache.join(index.to_string());
            fs::create_dir_all(&output).map_err(|e| e.to_string())?;
            let payload = request_payload(&scoped, &missing, &frames, &output)?;
            Ok((evidence, missing, Some(payload), scoped))
        })();
        match prepared {
            Ok((evidence, missing, Some(payload), scoped)) => {
                jobs.push((index, evidence, missing, payload, scoped))
            }
            Ok((evidence, _, None, _)) => results[index] = Ok(evidence),
            Err(error) => results[index] = Err(error),
        }
    }
    if jobs.is_empty() {
        return Ok(results);
    }
    let access = ModelAccess::resolve()?;
    let payloads = jobs.iter().map(|j| j.3.clone()).collect::<Vec<_>>();
    // 统一交互 Provider，无视觉队列限速/熔断重试；传输层仅 429 按 Retry-After 退避。
    let responses =
        post_model_payloads_concurrently(&access, &payloads, Some(Duration::from_secs(120)));
    for ((index, evidence, missing, payload, scoped), response) in jobs.into_iter().zip(responses) {
        results[index] = (|| {
            let body = response?;
            let text =
                model_response_json_text(&access, &body).ok_or("verification_response_invalid")?;
            let source = EvidenceSource {
                analysis_id: content_id(
                    "verification-analysis-v1",
                    &json!({"snapshot": evidence.analysis_snapshot_id,"request":content_id("verification-request-v1",&payload),"response":text,"model":access.custom_config().map(|c| &c.model)}),
                ),
                model: Some(
                    access
                        .custom_config()
                        .map(|c| c.model.clone())
                        .unwrap_or_else(|| "gpt-5.4".to_owned()),
                ),
                method: "candidate_multiframe_verification".to_owned(),
                analysis_version: 1,
            };
            let findings = parse_findings(&text, &scoped, &missing, &source)?;
            persist(&mut connection, project, &evidence, &findings)
        })();
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn evidence_contract_verification_only_missing_and_never_invents_negatives() {
        let metadata: TechnicalMetadata = serde_json::from_value(json!({"durationMs":2000,
            "sceneSegments":[{"id":"s1","startMs":0,"endMs":2000,"visualEvidence":{"detail":{"brandLogos":["ACME"]}}}]})).unwrap();
        let e = super::super::evidence_contract::adapt_asset("a", &metadata).remove(0);
        let requested = missing_risks(&e, &[RiskKind::BrandLogo, RiskKind::Shake, RiskKind::Shake]);
        assert_eq!(requested, vec![RiskKind::Shake]);
        let text = r#"{"risks":[{"risk":"shake","state":"not_hit","confidence":0.9,"value":{"severity":"none"}}]}"#;
        let results = parse_findings(text, &e, &requested, &e.source).unwrap();
        assert_eq!(results[0].state, EvidenceState::NotHit);
        assert_eq!(results[0].range, e.range);
        assert_eq!(
            results,
            parse_findings(text, &e, &requested, &e.source).unwrap()
        );
        let partial = parse_findings(r#"{"risks":[{"risk":"shake","state":"not_hit","confidence":0.9,"range":{"startMs":0,"endMs":1000}}]}"#,&e,&requested,&e.source).unwrap();
        assert_eq!(partial[0].state, EvidenceState::NotHit);
        let mut mixed = e.clone();
        mixed.risks.extend(partial);
        assert_eq!(
            super::super::evidence_contract::risk_state_for_window(
                &mixed,
                RiskKind::Shake,
                &EvidenceRange {
                    start_ms: 0,
                    end_ms: 1000
                }
            ),
            EvidenceState::NotHit
        );
        assert_eq!(
            super::super::evidence_contract::risk_state_for_window(
                &mixed,
                RiskKind::Shake,
                &e.range
            ),
            EvidenceState::Unknown
        );
        let mut scoped = mixed.clone();
        scoped.range = EvidenceRange {
            start_ms: 0,
            end_ms: 1000,
        };
        assert!(missing_risks(&scoped, &requested).is_empty());
        assert_eq!(missing_risks(&mixed, &requested), requested);
        let missing_confidence = r#"{"risks":[{"risk":"shake","state":"not_hit"}]}"#;
        assert_eq!(
            parse_findings(missing_confidence, &e, &requested, &e.source).unwrap()[0].state,
            EvidenceState::Unknown
        );
        let outside =
            r#"{"risks":[{"risk":"shake","state":"hit","range":{"startMs":-1,"endMs":1000}}]}"#;
        assert!(parse_findings(outside, &e, &requested, &e.source).is_err());
        assert!(parse_findings(text, &e, &[RiskKind::Clutter], &e.source).is_err());
        assert!(parse_findings(r#"{"risks":[]}"#, &e, &requested, &e.source).is_err());
    }

    #[test]
    fn evidence_contract_verification_persistence_scope_staleness_and_four_images() {
        let mut c = Connection::open_in_memory().unwrap();
        c.execute_batch("PRAGMA foreign_keys=ON;
            CREATE TABLE assets(id TEXT PRIMARY KEY,metadata_json TEXT,kind TEXT,analysis_status TEXT);
            CREATE TABLE project_asset_access(project_id TEXT,asset_id TEXT);
            CREATE TABLE asset_user_metadata(asset_id TEXT,excluded INTEGER);
            CREATE TABLE asset_source_health(asset_id TEXT,status TEXT);
            CREATE TABLE storyboard_versions(id TEXT PRIMARY KEY);").unwrap();
        super::super::evidence_contract::migrate(&c).unwrap();
        let raw = json!({"durationMs":2000,"sceneSegments":[{"id":"s1","startMs":0,"endMs":2000}]})
            .to_string();
        c.execute("INSERT INTO assets VALUES('a',?1,'video','ready')", [&raw])
            .unwrap();
        c.execute("INSERT INTO project_asset_access VALUES('p','a')", [])
            .unwrap();
        assert!(scoped_metadata(&c, "other-project", "a").is_err());
        let metadata = scoped_metadata(&c, "p", "a").unwrap();
        let e = load_asset(&c, "a", &metadata).unwrap().remove(0);
        let uncertain = parse_findings(
            r#"{"risks":[{"risk":"shake","state":"unknown"}]}"#,
            &e,
            &[RiskKind::Shake],
            &e.source,
        )
        .unwrap();
        let uncertain = persist(&mut c, "p", &e, &uncertain).unwrap();
        assert_eq!(
            super::super::evidence_contract::risk_state_for_window(
                &uncertain,
                RiskKind::Shake,
                &e.range
            ),
            EvidenceState::Unknown
        );
        let text = r#"{"risks":[{"risk":"shake","state":"not_hit","confidence":0.9}]}"#;
        let findings = parse_findings(text, &e, &[RiskKind::Shake], &e.source).unwrap();
        let verified = persist(&mut c, "p", &e, &findings).unwrap();
        assert_eq!(
            super::super::evidence_contract::risk_state_for_window(
                &verified,
                RiskKind::Shake,
                &e.range
            ),
            EvidenceState::NotHit
        );
        let again = persist(&mut c, "p", &e, &findings).unwrap();
        assert_eq!(verified, again);
        let count: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM asset_evidence_verifications",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
        c.execute(
            "UPDATE assets SET metadata_json=?1",
            [raw.replace("2000", "3000")],
        )
        .unwrap();
        assert_eq!(
            persist(&mut c, "p", &e, &findings).unwrap_err(),
            "verification_analysis_changed"
        );
        let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/evidence-contract-tests")
            .join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&directory).unwrap();
        let image_path = directory.join("source.png");
        image::RgbImage::from_pixel(32, 18, image::Rgb([40, 80, 160]))
            .save(&image_path)
            .unwrap();
        let before = fs::read(&image_path).unwrap();
        let frames = (0..24)
            .map(|i| KeyframeMetadata {
                time_ms: i * 80,
                image_path: image_path.to_string_lossy().into_owned(),
            })
            .collect::<Vec<_>>();
        let payload = request_payload(&e, &[RiskKind::Shake], &frames, &directory).unwrap();
        let contents = payload["input"][0]["content"].as_array().unwrap();
        assert_eq!(
            contents
                .iter()
                .filter(|v| v["type"] == "input_image")
                .count(),
            4
        );
        assert_eq!(fs::read(&image_path).unwrap(), before);
    }
}
