//! 片段证据适配与内容寻址：只读旧分析，不修改召回、策划或体裁底线。
//! 风险的正向旧标签保留；缺置信度的否定标签、缺字段与未分析都为未知。
use crate::models::*;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub(crate) const RISKS: [RiskKind; 11] = [
    RiskKind::BrandLogo,
    RiskKind::Exhibition,
    RiskKind::OutOfFocus,
    RiskKind::Shake,
    RiskKind::Clutter,
    RiskKind::OnScreenText,
    RiskKind::Crowd,
    RiskKind::MotionBlur,
    RiskKind::Staged,
    RiskKind::Advertising,
    RiskKind::EmptyShot,
];

pub(crate) fn content_id(prefix: &str, value: &impl Serialize) -> String {
    fn canonical(value: Value) -> Value {
        match value {
            Value::Object(map) => {
                let sorted = map
                    .into_iter()
                    .collect::<std::collections::BTreeMap<_, _>>();
                Value::Object(sorted.into_iter().map(|(k, v)| (k, canonical(v))).collect())
            }
            Value::Array(items) => Value::Array(items.into_iter().map(canonical).collect()),
            value => value,
        }
    }
    // 即使依赖启用 preserve_order，也不受 JSON 对象插入顺序影响。
    let canonical = canonical(serde_json::to_value(value).expect("serializable evidence"));
    format!(
        "{prefix}:{:x}",
        Sha256::digest(canonical.to_string().as_bytes())
    )
}

fn visible_labels(labels: &[String]) -> bool {
    labels.iter().any(|s| {
        !matches!(
            s.trim().trim_end_matches('.').to_lowercase().as_str(),
            "" | "none" | "unknown" | "n/a" | "无" | "未知"
        )
    })
}

/// 任务 3/5 按最终源窗消费；正向命中优先，未知或冲突绝不降为安全。
pub(crate) fn risk_state_for_window(
    evidence: &SegmentEvidence,
    risk: RiskKind,
    window: &EvidenceRange,
) -> EvidenceState {
    if window.start_ms < evidence.range.start_ms
        || window.end_ms > evidence.range.end_ms
        || window.end_ms <= window.start_ms
    {
        return EvidenceState::Unknown;
    }
    let overlaps = |r: &RiskEvidence| {
        r.risk == risk && r.range.start_ms < window.end_ms && r.range.end_ms > window.start_ms
    };
    if evidence
        .risks
        .iter()
        .any(|r| overlaps(r) && r.state == EvidenceState::Hit)
    {
        return EvidenceState::Hit;
    }
    // 明确且带置信度的核验只补齐其覆盖窗的未知，不推断窗外或拼接零散阴性。
    if evidence.risks.iter().any(|r| {
        r.risk == risk
            && r.state == EvidenceState::NotHit
            && r.confidence.is_some()
            && r.range.start_ms <= window.start_ms
            && r.range.end_ms >= window.end_ms
    }) {
        return EvidenceState::NotHit;
    }
    EvidenceState::Unknown
}

pub(crate) fn seal_risk(mut risk: RiskEvidence) -> RiskEvidence {
    risk.id.clear();
    risk.id = content_id("risk-v1", &risk);
    risk
}

pub(crate) fn seal_segment(mut evidence: SegmentEvidence) -> SegmentEvidence {
    evidence.risks.sort_by(|a, b| {
        a.risk
            .cmp(&b.risk)
            .then(a.range.start_ms.cmp(&b.range.start_ms))
            .then(a.id.cmp(&b.id))
    });
    evidence.id.clear();
    evidence.id = content_id("segment-v1", &evidence);
    evidence
}

pub(crate) fn adapt_segment(
    asset_id: &str,
    segment: &SceneSegment,
    analysis_version: u32,
    visual_version: u32,
) -> SegmentEvidence {
    let range = EvidenceRange {
        start_ms: segment.start_ms,
        end_ms: segment.end_ms,
    };
    let visual = segment.visual_evidence.as_ref();
    let snapshot_id = content_id(
        "analysis-v1",
        &json!({
            "assetId": asset_id, "segmentId": segment.id, "range": range,
            "analysisVersion": analysis_version, "visualVersion": visual_version,
            "visual": visual, "motion": segment.motion_profile,
        }),
    );
    let source = visual
        .and_then(|v| v.provenance.clone())
        .unwrap_or(EvidenceSource {
            analysis_id: snapshot_id.clone(),
            model: None,
            method: if visual.is_some() {
                "legacy_visual_detail"
            } else {
                "missing_analysis"
            }
            .to_owned(),
            analysis_version: visual_version,
        });
    let detail = visual.and_then(|v| v.detail.as_ref());
    let risks = RISKS
        .iter()
        .map(|&risk| {
            let (hit, value) = match (risk, detail) {
                (RiskKind::BrandLogo, Some(d)) => {
                    (visible_labels(&d.brand_logos), json!(d.brand_logos))
                }
                (RiskKind::Exhibition, Some(d)) => {
                    (d.exhibition == Some(true), json!(d.exhibition))
                }
                (RiskKind::OutOfFocus, Some(d)) => {
                    (d.focus.as_deref() == Some("out_of_focus"), json!(d.focus))
                }
                (RiskKind::MotionBlur, Some(d)) => {
                    (d.focus.as_deref() == Some("motion_blur"), json!(d.focus))
                }
                (RiskKind::OnScreenText, Some(d)) => {
                    (visible_labels(&d.on_screen_text), json!(d.on_screen_text))
                }
                (RiskKind::Crowd, Some(d)) => (d.crowd.as_deref() == Some("crowd"), json!(d.crowd)),
                // 只认显式质量问题；handheld/static 和运动能量都不能证明抖动或不抖动。
                (RiskKind::Shake, _) => {
                    let notes = visual.map(|v| v.quality_notes.as_slice()).unwrap_or(&[]);
                    let hit = notes.iter().any(|n| {
                        matches!(
                            n.trim().to_lowercase().as_str(),
                            "shaky" | "shake" | "camera shake" | "抖动"
                        )
                    });
                    (hit, json!(notes))
                }
                _ => (false, Value::Null),
            };
            seal_risk(RiskEvidence {
                id: String::new(),
                risk,
                state: if hit {
                    EvidenceState::Hit
                } else {
                    EvidenceState::Unknown
                },
                source: source.clone(),
                range: range.clone(),
                confidence: None,
                value,
            })
        })
        .collect();
    seal_segment(SegmentEvidence {
        schema_version: 1,
        id: String::new(),
        analysis_snapshot_id: snapshot_id,
        asset_id: asset_id.to_owned(),
        segment_id: segment.id.clone(),
        range,
        source,
        risks,
        relations: detail.cloned(),
        motion_profile: segment.motion_profile.clone(),
        visual_evidence: visual.cloned().into_iter().collect(),
        caption: visual.and_then(|v| v.caption.clone()),
        narrative_role: visual.and_then(|v| v.narrative_role.clone()),
    })
}

/// 没有真实分段时仅用整片证据；已分段的缺卡段绝不借整片标签。
pub(crate) fn adapt_asset(asset_id: &str, metadata: &TechnicalMetadata) -> Vec<SegmentEvidence> {
    let segments = if metadata.scene_segments.is_empty() {
        vec![SceneSegment {
            id: "whole".to_owned(),
            start_ms: 0,
            end_ms: metadata.duration_ms.unwrap_or(0),
            scene_duration_ms: metadata.duration_ms,
            visual_quality_score: metadata.visual_quality_score,
            frames: metadata.keyframes.clone(),
            visual_evidence: metadata.visual_evidence.first().cloned(),
            motion_score: None,
            motion_profile: None,
        }]
    } else {
        metadata.scene_segments.clone()
    };
    let mut output = segments
        .iter()
        .map(|segment| {
            adapt_segment(
                asset_id,
                segment,
                metadata.analysis_version,
                metadata.visual_analysis_version,
            )
        })
        .collect::<Vec<_>>();
    if metadata.scene_segments.is_empty() && metadata.visual_evidence.len() > 1 {
        let mut whole = output.remove(0);
        for card in metadata.visual_evidence.iter().skip(1) {
            let mut segment = segments[0].clone();
            segment.visual_evidence = Some(card.clone());
            whole.risks.extend(
                adapt_segment(
                    asset_id,
                    &segment,
                    metadata.analysis_version,
                    metadata.visual_analysis_version,
                )
                .risks,
            );
        }
        whole.visual_evidence = metadata.visual_evidence.clone();
        whole.analysis_snapshot_id = content_id(
            "analysis-whole-v1",
            &json!({"assetId":asset_id,"range":whole.range,
            "visual":whole.visual_evidence,"analysisVersion":metadata.analysis_version,"visualVersion":metadata.visual_analysis_version}),
        );
        whole.risks.sort_by(|a, b| a.id.cmp(&b.id));
        whole.risks.dedup_by(|a, b| a.id == b.id);
        output.push(seal_segment(whole));
    }
    output
}

pub(crate) fn migrate(connection: &Connection) -> Result<(), String> {
    // 只追加，不写 assets/storyboard_versions 旧行，不重建任何表。
    connection.execute_batch("CREATE TABLE IF NOT EXISTS asset_evidence_verifications (
        id TEXT PRIMARY KEY NOT NULL,
        asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
        segment_id TEXT NOT NULL, analysis_snapshot_id TEXT NOT NULL,
        evidence_json TEXT NOT NULL, created_at INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS evidence_verification_snapshot_idx
        ON asset_evidence_verifications(asset_id, segment_id, analysis_snapshot_id);
    CREATE TABLE IF NOT EXISTS storyboard_evidence_metadata (
        storyboard_version_id TEXT PRIMARY KEY NOT NULL REFERENCES storyboard_versions(id) ON DELETE CASCADE,
        metadata_json TEXT NOT NULL
    );").map_err(|e| e.to_string())
}

pub(crate) fn with_verifications(
    connection: &Connection,
    mut evidence: SegmentEvidence,
) -> Result<SegmentEvidence, String> {
    let mut statement = connection
        .prepare(
            "SELECT evidence_json FROM asset_evidence_verifications
        WHERE asset_id=?1 AND segment_id=?2 AND analysis_snapshot_id=?3 ORDER BY id",
        )
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map(
            params![
                evidence.asset_id,
                evidence.segment_id,
                evidence.analysis_snapshot_id
            ],
            |row| row.get::<_, String>(0),
        )
        .map_err(|e| e.to_string())?;
    let mut verified = Vec::<RiskEvidence>::new();
    for row in rows {
        verified.push(
            serde_json::from_str(&row.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?,
        );
    }
    // 未知可被全段核验补齐（含上次核验仍未知的记录）；历史仍存表内。
    // 旧正向命中永远保留，命中与否定的冲突不静默消失。
    let resolved = verified
        .iter()
        .filter(|v| v.state != EvidenceState::Unknown && v.range == evidence.range)
        .map(|v| v.risk)
        .collect::<std::collections::BTreeSet<_>>();
    evidence.risks.extend(verified);
    evidence
        .risks
        .retain(|old| old.state != EvidenceState::Unknown || !resolved.contains(&old.risk));
    Ok(seal_segment(evidence))
}

pub(crate) fn load_asset(
    connection: &Connection,
    asset_id: &str,
    metadata: &TechnicalMetadata,
) -> Result<Vec<SegmentEvidence>, String> {
    adapt_asset(asset_id, metadata)
        .into_iter()
        .map(|e| with_verifications(connection, e))
        .collect()
}

pub(crate) fn read_storyboard_metadata(
    connection: &Connection,
    id: &str,
) -> Result<StoryboardEvidenceMetadata, String> {
    let stored = connection
        .query_row(
            "SELECT metadata_json FROM storyboard_evidence_metadata WHERE storyboard_version_id=?1",
            [id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    stored
        .map(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
        .unwrap_or_else(|| Ok(StoryboardEvidenceMetadata::default()))
}

/// 后续生成集成在 IPC/工具返回边界调用，内部现行版本逻辑不受投影影响。
pub(crate) fn project_storyboard_version(
    connection: &Connection,
    version: StoryboardVersion,
) -> Result<StoryboardVersionWithEvidence, String> {
    let evidence = read_storyboard_metadata(connection, &version.id)?;
    Ok(StoryboardVersionWithEvidence { version, evidence })
}

/// 任务 4/6 在新故事版事务里调用；历史/已有元数据不得覆盖，引用必须指向快照且不越窗。
pub(crate) fn write_storyboard_metadata(
    connection: &Connection,
    id: &str,
    metadata: &StoryboardEvidenceMetadata,
) -> Result<(), String> {
    if metadata
        .pipeline_version
        .as_deref()
        .unwrap_or_default()
        .is_empty()
        || metadata.genre.is_none()
        || metadata
            .recipe_version
            .as_deref()
            .unwrap_or_default()
            .is_empty()
    {
        return Err("storyboard_evidence_versions_required".to_owned());
    }
    for reference in &metadata.evidence_references {
        if reference.supports.trim().is_empty()
            || !metadata.evidence_snapshot.iter().any(|e| {
                e.id == reference.evidence_id
                    && e.asset_id == reference.asset_id
                    && e.segment_id == reference.segment_id
                    && reference.range.start_ms >= e.range.start_ms
                    && reference.range.end_ms <= e.range.end_ms
                    && reference.range.end_ms > reference.range.start_ms
            })
        {
            return Err("storyboard_evidence_reference_invalid".to_owned());
        }
    }
    if metadata.evidence_snapshot.iter().any(|e| {
        e.schema_version != 1
            || e.id != seal_segment(e.clone()).id
            || e.risks.iter().any(|r| r.id != seal_risk(r.clone()).id)
    }) {
        return Err("storyboard_evidence_snapshot_invalid".to_owned());
    }
    let content = serde_json::to_string(metadata).map_err(|e| e.to_string())?;
    connection.execute("INSERT INTO storyboard_evidence_metadata(storyboard_version_id,metadata_json) VALUES(?1,?2)", params![id, content]).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn sample() -> TechnicalMetadata {
        serde_json::from_value(json!({"durationMs":9000,"analysisVersion":4,"visualAnalysisVersion":1,
            "sceneSegments":[{"id":"s001","startMs":1000,"endMs":9000,
                "visualEvidence":{"detail":{"brandLogos":["ACME"],"exhibition":false,"focus":"sharp",
                    "subjectSpans":[{"timeMs":3000,"left":0.2,"right":0.8}],
                    "subjectPositions":[{"timeMs":3000,"position":"center"}],
                    "cleanStart":false,"cleanEnd":true,"subjectDirection":"left-to-right",
                    "bestRange":{"startMs":3000,"endMs":5000,"reason":"complete action"},
                    "highlights":[{"timeMs":4000,"description":"arrival"}]}}}]})).unwrap()
    }

    #[test]
    fn evidence_contract_legacy_unknown_stable_ids_and_relations() {
        let first = adapt_asset("a", &sample());
        assert_eq!(first, adapt_asset("a", &sample()));
        let e = &first[0];
        assert_eq!(e.risks.len(), RISKS.len());
        assert_eq!(
            e.risks
                .iter()
                .find(|r| r.risk == RiskKind::BrandLogo)
                .unwrap()
                .state,
            EvidenceState::Hit
        );
        assert!(e
            .risks
            .iter()
            .filter(|r| r.risk != RiskKind::BrandLogo)
            .all(|r| r.state == EvidenceState::Unknown));
        assert!(e
            .risks
            .iter()
            .all(|r| r.source.model.is_none() && r.confidence.is_none() && r.range == e.range));
        let d = e.relations.as_ref().unwrap();
        assert_eq!(
            (d.subject_spans[0].left, d.subject_spans[0].right),
            (0.2, 0.8)
        );
        assert_eq!(d.subject_positions.len(), 1);
        assert_eq!(d.clean_start, Some(false));
        assert_eq!(d.best_range.as_ref().unwrap().start_ms, 3000);
        assert_eq!(d.highlights[0].time_ms, 4000);
        let mut changed = sample();
        changed.scene_segments[0]
            .visual_evidence
            .as_mut()
            .unwrap()
            .detail
            .as_mut()
            .unwrap()
            .exhibition = Some(true);
        assert_ne!(e.id, adapt_asset("a", &changed)[0].id);
        changed.scene_segments[0].visual_evidence = None;
        changed.visual_evidence = sample().scene_segments[0]
            .visual_evidence
            .clone()
            .into_iter()
            .collect();
        assert!(adapt_asset("a", &changed)[0]
            .risks
            .iter()
            .all(|r| r.state == EvidenceState::Unknown));
        let mut whole = sample();
        whole.visual_evidence = vec![
            VisualEvidence::default(),
            whole.scene_segments[0].visual_evidence.clone().unwrap(),
        ];
        whole.scene_segments.clear();
        let whole = adapt_asset("a", &whole).remove(0);
        assert_eq!(whole.visual_evidence.len(), 2);
        assert_eq!(
            risk_state_for_window(&whole, RiskKind::BrandLogo, &whole.range),
            EvidenceState::Hit
        );
        assert_eq!(
            content_id("test", &json!({"b":1,"a":2})),
            content_id("test", &json!({"a":2,"b":1}))
        );
    }

    #[test]
    fn evidence_contract_additive_migration_storyboard_roundtrip_and_conflicts() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE assets(id TEXT PRIMARY KEY);
            INSERT INTO assets VALUES('a'); CREATE TABLE storyboard_versions(id TEXT PRIMARY KEY,content_json TEXT);
            INSERT INTO storyboard_versions VALUES('old','{\"shots\":[]}'); INSERT INTO storyboard_versions VALUES('new','{\"shots\":[]}');").unwrap();
        migrate(&c).unwrap();
        migrate(&c).unwrap();
        assert_eq!(
            read_storyboard_metadata(&c, "old").unwrap(),
            StoryboardEvidenceMetadata::default()
        );
        let e = adapt_asset("a", &sample()).remove(0);
        let metadata = StoryboardEvidenceMetadata {
            pipeline_version: Some("footage-first-v1".to_owned()),
            genre: Some(Genre::Promotion),
            recipe_version: Some("promotion-v1".to_owned()),
            evidence_references: vec![EvidenceReference {
                evidence_id: e.id.clone(),
                asset_id: e.asset_id.clone(),
                segment_id: e.segment_id.clone(),
                range: e.range.clone(),
                supports: "visible mechanism".to_owned(),
            }],
            evidence_snapshot: vec![e.clone()],
        };
        write_storyboard_metadata(&c, "new", &metadata).unwrap();
        assert_eq!(read_storyboard_metadata(&c, "new").unwrap(), metadata);
        let version = StoryboardVersion {
            id: "new".to_owned(),
            project_id: "p".to_owned(),
            editing_task_id: "t".to_owned(),
            version_number: 1,
            brief: String::new(),
            title: String::new(),
            summary: String::new(),
            target_duration_ms: 3000,
            script_mode: "key_message".to_owned(),
            beats: vec![],
            uncovered_beat_ids: vec![],
            shots: vec![],
            created_at: 0,
            derivation: StoryboardDerivation::default(),
        };
        let projection =
            serde_json::to_value(project_storyboard_version(&c, version.clone()).unwrap()).unwrap();
        assert_eq!(projection["pipelineVersion"], "footage-first-v1");
        assert_eq!(projection["genre"], "promotion");
        assert_eq!(projection["shots"], json!([]));
        let mut legacy = version;
        legacy.id = "old".to_owned();
        let projection =
            serde_json::to_value(project_storyboard_version(&c, legacy).unwrap()).unwrap();
        assert!(projection["pipelineVersion"].is_null());
        assert_eq!(projection["evidenceSnapshot"], json!([]));
        assert!(write_storyboard_metadata(&c, "new", &metadata).is_err());
        let old: String = c
            .query_row(
                "SELECT content_json FROM storyboard_versions WHERE id='old'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(old, "{\"shots\":[]}");
        let mut risk = e
            .risks
            .iter()
            .find(|r| r.risk == RiskKind::BrandLogo)
            .unwrap()
            .clone();
        risk.state = EvidenceState::NotHit;
        risk.confidence = Some(0.9);
        risk.source.method = "candidate_multiframe_verification".to_owned();
        risk = seal_risk(risk);
        c.execute(
            "INSERT INTO asset_evidence_verifications VALUES('v','a','s001',?1,?2,0)",
            params![
                e.analysis_snapshot_id,
                serde_json::to_string(&risk).unwrap()
            ],
        )
        .unwrap();
        let merged = with_verifications(&c, e).unwrap();
        assert!(merged
            .risks
            .iter()
            .any(|r| r.risk == RiskKind::BrandLogo && r.state == EvidenceState::Hit));
        assert!(merged
            .risks
            .iter()
            .any(|r| r.risk == RiskKind::BrandLogo && r.state == EvidenceState::NotHit));
    }
}
