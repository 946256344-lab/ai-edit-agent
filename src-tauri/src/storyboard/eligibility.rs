//! 体裁底线裁决与候选补核验：事实先过硬门，未知不放行，匹配分无权恢复淘汰项。
//! 只追加候选核验事实；不重分析原库，不凭运动方向猜轴线，不裁掉整段风险标签。
use crate::assets::{evidence_contract, evidence_verification};
use crate::media_options::AspectRatio;
use crate::models::*;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use tauri::AppHandle;

pub(crate) const POLICY_VERSION: &str = "genre-eligibility-v1";
const CANDIDATES_PER_BEAT: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EligibilityStatus {
    Eligible,
    Rejected,
    PendingVerification,
}

/// 只接受品牌套件/用户给出的明确名称；logo 文件路径不是可见品牌身份。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrandIdentity {
    pub allowed_labels: Vec<String>,
}

/// 关系事实由任务 5 根据相邻镜头提供；缺项 Unknown，不把方向反转当跳轴。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RelationEvidence {
    pub kind: RelationRisk,
    pub state: EvidenceState,
    pub source: EvidenceSource,
    pub range: EvidenceRange,
    pub confidence: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RelationRisk {
    Contradiction,
    AxisCrossing,
    UnrelatedInsert,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EligibilityReason {
    pub code: String,
    pub state: EvidenceState,
    pub evidence_ids: Vec<String>,
    pub range: EvidenceRange,
    pub explanation: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EligibilityDecision {
    pub policy_version: String,
    pub asset_id: String,
    pub segment_id: String,
    pub genre: Genre,
    pub aspect_ratio: AspectRatio,
    pub status: EligibilityStatus,
    pub reasons: Vec<EligibilityReason>,
    pub required_verification: Vec<RiskKind>,
    pub usable_windows: Vec<EvidenceRange>,
}

pub(crate) fn critical_risks(genre: Genre) -> &'static [RiskKind] {
    match genre {
        Genre::Promotion => &[
            RiskKind::OutOfFocus,
            RiskKind::Shake,
            RiskKind::BrandLogo,
            RiskKind::Exhibition,
            RiskKind::Clutter,
        ],
        Genre::Bts => &[RiskKind::Staged, RiskKind::Advertising],
        Genre::Narrative => &[],
    }
}

fn labels(value: &serde_json::Value) -> Vec<&str> {
    match value {
        serde_json::Value::String(s) => vec![s],
        serde_json::Value::Array(items) => items.iter().filter_map(|s| s.as_str()).collect(),
        serde_json::Value::Object(map) => map
            .get("labels")
            .or_else(|| map.get("brandLogos"))
            .or_else(|| map.get("label"))
            .map(labels)
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn brand_is_allowed(facts: &[&RiskEvidence], brand: &BrandIdentity) -> bool {
    !facts.is_empty()
        && facts.iter().all(|fact| {
            let visible = labels(&fact.value);
            !visible.is_empty()
                && visible.iter().all(|label| {
                    let label = label.trim();
                    !matches!(
                        label.to_lowercase().as_str(),
                        "" | "unknown" | "none" | "n/a" | "未知" | "无"
                    ) && brand.allowed_labels.iter().any(|allowed| {
                        !allowed.trim().is_empty()
                            && label.to_lowercase() == allowed.trim().to_lowercase()
                    })
                })
        })
}

// 已确认是本项目标识/轻微抖动时，也不能用部分正向窗证明其余时段安全。
fn permitted_hit_covers_window(
    evidence: &SegmentEvidence,
    risk: RiskKind,
    window: &EvidenceRange,
) -> bool {
    evidence.risks.iter().any(|fact| {
        fact.risk == risk
            && fact.range.start_ms <= window.start_ms
            && fact.range.end_ms >= window.end_ms
            && (fact.state == EvidenceState::Hit
                || (fact.state == EvidenceState::NotHit
                    && fact
                        .confidence
                        .is_some_and(|c| c.is_finite() && (0.0..=1.0).contains(&c))))
    })
}

/// 单窗纯裁决。默认调用整片段窗；只接受显式、有证据的源窗，不自动裁切风险。
/// final_check=true 时宣传的关键未知按不合格；其他体裁保持待核验，绝不贴合格标签。
pub(crate) fn evaluate(
    evidence: &SegmentEvidence,
    genre: Genre,
    aspect_ratio: AspectRatio,
    brand: &BrandIdentity,
    window: &EvidenceRange,
    relations: &[RelationEvidence],
    final_check: bool,
) -> EligibilityDecision {
    let mut decision = EligibilityDecision {
        policy_version: POLICY_VERSION.to_owned(),
        asset_id: evidence.asset_id.clone(),
        segment_id: evidence.segment_id.clone(),
        genre,
        aspect_ratio,
        status: EligibilityStatus::Eligible,
        reasons: Vec::new(),
        required_verification: Vec::new(),
        usable_windows: vec![window.clone()],
    };
    let mut rejected = false;
    let mut unknown = false;
    if window.start_ms < evidence.range.start_ms
        || window.end_ms > evidence.range.end_ms
        || window.end_ms <= window.start_ms
    {
        rejected = true;
        decision.reasons.push(EligibilityReason {
            code: "invalid_source_window".to_owned(),
            state: EvidenceState::Unknown,
            evidence_ids: vec![evidence.id.clone()],
            range: window.clone(),
            explanation: "Source window is empty or outside the evidence segment.".to_owned(),
        });
    } else {
        for &risk in critical_risks(genre) {
            let mut state = evidence_contract::risk_state_for_window(evidence, risk, window);
            let facts: Vec<_> = evidence
                .risks
                .iter()
                .filter(|f| {
                    f.risk == risk
                        && f.range.start_ms < window.end_ms
                        && f.range.end_ms > window.start_ms
                        && f.state == EvidenceState::Hit
                })
                .collect();
            if state == EvidenceState::Hit
                && risk == RiskKind::BrandLogo
                && brand_is_allowed(&facts, brand)
            {
                if permitted_hit_covers_window(evidence, risk, window) {
                    continue;
                }
                state = EvidenceState::Unknown;
            }
            // 稳定的轻微手持不是不可接受抖动；旧正向但无程度仍保守淘汰。
            if state == EvidenceState::Hit
                && risk == RiskKind::Shake
                && !facts.is_empty()
                && facts.iter().all(|f| {
                    f.confidence
                        .is_some_and(|c| c.is_finite() && (0.0..=1.0).contains(&c))
                        && f.value["severity"] == "mild"
                })
            {
                if permitted_hit_covers_window(evidence, risk, window) {
                    continue;
                }
                state = EvidenceState::Unknown;
            }
            if state == EvidenceState::NotHit {
                continue;
            }
            let code = serde_json::to_value(risk)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned();
            if state == EvidenceState::Hit {
                rejected = true;
            } else {
                unknown = true;
                decision.required_verification.push(risk);
            }
            decision.reasons.push(EligibilityReason {
                code: if state == EvidenceState::Unknown {
                    format!("unknown_{code}")
                } else {
                    code.clone()
                },
                state,
                range: window.clone(),
                evidence_ids: evidence
                    .risks
                    .iter()
                    .filter(|f| {
                        f.risk == risk
                            && f.range.start_ms < window.end_ms
                            && f.range.end_ms > window.start_ms
                    })
                    .map(|f| f.id.clone())
                    .collect(),
                explanation: if state == EvidenceState::Unknown {
                    format!(
                        "Critical {code} is unknown{}.",
                        if final_check {
                            " after candidate verification; excluded"
                        } else {
                            "; verify before admission"
                        }
                    )
                } else if risk == RiskKind::BrandLogo {
                    if brand.allowed_labels.iter().all(|s| s.trim().is_empty()) {
                        "Visible logo excluded because the project has no explicit brand identity."
                            .to_owned()
                    } else {
                        "Visible logo is not fully identified as an allowed project brand."
                            .to_owned()
                    }
                } else {
                    format!("Confirmed {code} violates the selected genre floor.")
                },
            });
        }
        if genre == Genre::Narrative {
            for kind in [
                RelationRisk::Contradiction,
                RelationRisk::AxisCrossing,
                RelationRisk::UnrelatedInsert,
            ] {
                let facts: Vec<_> = relations
                    .iter()
                    .filter(|f| {
                        f.kind == kind
                            && f.range.start_ms < window.end_ms
                            && f.range.end_ms > window.start_ms
                    })
                    .collect();
                let state = if facts.iter().any(|f| f.state == EvidenceState::Hit) {
                    EvidenceState::Hit
                } else if facts.iter().any(|f| {
                    f.state == EvidenceState::NotHit
                        && f.confidence
                            .is_some_and(|c| c.is_finite() && (0.0..=1.0).contains(&c))
                        && f.range.start_ms <= window.start_ms
                        && f.range.end_ms >= window.end_ms
                }) {
                    EvidenceState::NotHit
                } else {
                    EvidenceState::Unknown
                };
                if state == EvidenceState::NotHit {
                    continue;
                }
                rejected |= state == EvidenceState::Hit;
                unknown |= state == EvidenceState::Unknown;
                decision.reasons.push(EligibilityReason { code: format!("relation_{kind:?}"), state,
                    evidence_ids: facts.iter().map(|f| evidence_contract::content_id("relation-risk-v1", f)).collect(), range: window.clone(),
                    explanation: format!("Narrative {kind:?}: {state:?}; needs neighboring-shot relation evidence, not motion direction.") });
            }
        }
    }
    decision.status = if rejected || (unknown && final_check && genre == Genre::Promotion) {
        EligibilityStatus::Rejected
    } else if unknown {
        EligibilityStatus::PendingVerification
    } else {
        EligibilityStatus::Eligible
    };
    if decision.status != EligibilityStatus::Eligible {
        decision.usable_windows.clear();
    }
    decision
}

/// 供任务 4/5/6 消费的合格清单；输入快照与逐条原因同时可审计。
pub(crate) struct EligibleInventory {
    pub sources: Vec<StoryboardSource>,
    pub evidence_snapshot: Vec<SegmentEvidence>,
    pub decisions: Vec<EligibilityDecision>,
    pub usability: HashMap<String, super::scoring::UsabilityScore>,
}

pub(crate) fn source_key(source: &StoryboardSource) -> String {
    format!(
        "{}:{}",
        source.asset_id,
        source
            .segment
            .as_ref()
            .map(|s| s.id.as_str())
            .unwrap_or("whole")
    )
}

fn source_window(source: &StoryboardSource, evidence: &SegmentEvidence) -> EvidenceRange {
    source
        .segment
        .as_ref()
        .map(|s| EvidenceRange {
            start_ms: s.start_ms,
            end_ms: s.end_ms,
        })
        .unwrap_or_else(|| evidence.range.clone())
}

/// 入池前按需求召回待核验候选，失败后继续向下取；不因数量不足撤销任何底线。
/// 目前旧生成入口显式传 Promotion；真实体裁和关系上下文由任务 6/7 接入。
pub(crate) fn prepare_candidates(
    app: &AppHandle,
    connection: &Connection,
    project: &str,
    sources: &[StoryboardSource],
    beats: &[StoryboardBeat],
    embeddings: &[Vec<f32>],
    clip_embeddings: &[Vec<f32>],
    usage: &HashMap<String, i32>,
    target_each: i64,
    genre: Genre,
    aspect: AspectRatio,
) -> Result<EligibleInventory, String> {
    let kit = crate::brand_kit::read_brand_kit(connection, project);
    let brand = BrandIdentity {
        allowed_labels: vec![kit.name]
            .into_iter()
            .filter(|s| !s.trim().is_empty())
            .collect(),
    };
    let mut facts = HashMap::new();
    let mut dimensions = HashMap::new();
    for asset in sources
        .iter()
        .map(|s| s.asset_id.clone())
        .collect::<HashSet<_>>()
    {
        let raw: String = connection.query_row("SELECT metadata_json FROM assets WHERE id=?1 AND id IN (SELECT asset_id FROM project_asset_access WHERE project_id=?2)",
            params![asset, project], |r| r.get(0)).map_err(|e| e.to_string())?;
        let metadata: TechnicalMetadata = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        dimensions.insert(asset.clone(), (metadata.width, metadata.height));
        for evidence in evidence_contract::load_asset(connection, &asset, &metadata)? {
            facts.insert(format!("{}:{}", asset, evidence.segment_id), evidence);
        }
    }
    let mut decisions = HashMap::new();
    let mut attempted = HashSet::new();
    let mut verification_errors = HashMap::new();
    let shared = super::scoring::shared_lexical_terms(beats);
    loop {
        crate::execution_deadline::check()?;
        for source in sources {
            let key = source_key(source);
            if let Some(evidence) = facts.get(&key) {
                decisions.insert(
                    key.clone(),
                    evaluate(
                        evidence,
                        genre,
                        aspect,
                        &brand,
                        &source_window(source, evidence),
                        &[],
                        attempted.contains(&key),
                    ),
                );
            }
        }
        let viable: Vec<_> = sources
            .iter()
            .filter(|s| {
                decisions.get(&source_key(s)).is_some_and(|d| {
                    d.status == EligibilityStatus::Eligible
                        || (d.status == EligibilityStatus::PendingVerification
                            && !attempted.contains(&source_key(s))
                            && !d.required_verification.is_empty())
                })
            })
            .cloned()
            .collect();
        let mut requested = HashSet::new();
        for (index, beat) in beats.iter().enumerate() {
            let ranked = super::scoring::rank_segment_candidates(
                viable.clone(),
                beat,
                target_each,
                &[],
                usage,
                embeddings.get(index).map(Vec::as_slice),
                clip_embeddings.get(index).map(Vec::as_slice),
                &shared,
            );
            let mut per_asset = HashMap::new();
            let mut count = 0;
            for item in ranked {
                let uses = per_asset.entry(item.source.asset_id.clone()).or_insert(0);
                if *uses >= 2 {
                    continue;
                }
                *uses += 1;
                count += 1;
                let key = source_key(&item.source);
                if decisions[&key].status == EligibilityStatus::PendingVerification
                    && !attempted.contains(&key)
                    && !decisions[&key].required_verification.is_empty()
                {
                    requested.insert(key);
                }
                if count >= CANDIDATES_PER_BEAT {
                    break;
                }
            }
        }
        if requested.is_empty() {
            break;
        }
        let mut keys: Vec<_> = requested.into_iter().collect();
        keys.sort();
        let requests: Vec<_> = keys
            .iter()
            .map(|key| {
                let fact = &facts[key];
                EvidenceVerificationRequest {
                    asset_id: fact.asset_id.clone(),
                    segment_id: fact.segment_id.clone(),
                    analysis_snapshot_id: fact.analysis_snapshot_id.clone(),
                    risks: decisions[key].required_verification.clone(),
                    range: Some(
                        decisions[key]
                            .reasons
                            .first()
                            .map(|r| r.range.clone())
                            .unwrap_or_else(|| fact.range.clone()),
                    ),
                }
            })
            .collect();
        let results = evidence_verification::verify_candidates(app, project, &requests)
            .unwrap_or_else(|error| vec![Err(error); requests.len()]);
        for (key, result) in keys.into_iter().zip(results) {
            attempted.insert(key.clone());
            match result {
                Ok(evidence) => {
                    facts.insert(key, evidence);
                }
                Err(error) => {
                    verification_errors.insert(key.clone(), error.clone());
                    super::provider_trace::append_pool_trace(
                        "Genre verification failure",
                        &key,
                        &json!({"error":error}),
                    );
                    log::warn!("Candidate evidence verification failed: {error}");
                }
            }
        }
    }
    let mut inventory = EligibleInventory {
        sources: vec![],
        evidence_snapshot: vec![],
        decisions: vec![],
        usability: HashMap::new(),
    };
    for source in sources {
        let key = source_key(source);
        let Some(evidence) = facts.get(&key) else {
            // 旧相邻双段与整片回退无单一片段契约：不把跨硬切组合借成合格单镜。
            inventory.decisions.push(EligibilityDecision { policy_version: POLICY_VERSION.to_owned(),
                asset_id: source.asset_id.clone(), segment_id: source.segment.as_ref().map(|s| s.id.clone()).unwrap_or("whole".to_owned()),
                genre, aspect_ratio: aspect, status: EligibilityStatus::Rejected,
                reasons: vec![EligibilityReason { code: "single_segment_evidence_required".to_owned(), state: EvidenceState::Unknown,
                    evidence_ids: vec![], range: EvidenceRange { start_ms: 0, end_ms: source.duration_ms.unwrap_or(0) },
                    explanation: "Candidate has no single matching evidence segment; cross-cut combinations cannot enter as one shot.".to_owned() }],
                required_verification: vec![], usable_windows: vec![] });
            continue;
        };
        let mut decision = decisions.remove(&key).unwrap();
        if let Some(error) = verification_errors.get(&key) {
            decision.reasons.push(EligibilityReason {
                code: "verification_failed".to_owned(),
                state: EvidenceState::Unknown,
                evidence_ids: vec![evidence.id.clone()],
                range: evidence.range.clone(),
                explanation: format!(
                    "Candidate verification failed: {error}; no negative fact was invented."
                ),
            });
        }
        if decision.status == EligibilityStatus::Eligible {
            let window = source_window(source, evidence);
            if window.start_ms < evidence.range.start_ms
                || window.end_ms > evidence.range.end_ms
                || window.end_ms <= window.start_ms
            {
                decision = evaluate(evidence, genre, aspect, &brand, &window, &[], true);
            } else {
                decision.usable_windows = vec![window.clone()];
                let (width, height) = dimensions[&source.asset_id];
                inventory.usability.insert(
                    key,
                    super::scoring::usability_score(evidence, &window, aspect, width, height),
                );
                inventory.sources.push(source.clone());
            }
        }
        inventory.evidence_snapshot.push(evidence.clone());
        inventory.decisions.push(decision);
    }
    super::provider_trace::append_pool_trace(
        "Genre eligibility",
        "-",
        &json!({"policyVersion":POLICY_VERSION,
        "genre":genre,"decisions":inventory.decisions,"usability":inventory.usability,"eligibleCount":inventory.sources.len()}),
    );
    if inventory.sources.is_empty() {
        let counts = inventory
            .decisions
            .iter()
            .flat_map(|d| d.reasons.iter())
            .fold(std::collections::BTreeMap::new(), |mut counts, r| {
                *counts.entry(r.code.clone()).or_insert(0usize) += 1;
                counts
            });
        return Err(format!(
            "storyboard_no_eligible_footage: genre={genre:?}; hard floors retained; reasons={}",
            json!(counts)
        ));
    }
    Ok(inventory)
}

/// 评测只读导出的契约，输出三体裁同片裁决；不调用模型、不访问用户数据库。
#[cfg(feature = "footage-eval")]
pub(crate) fn judge_contract_file() -> Result<(), String> {
    let input = std::env::args_os().nth(2).ok_or("missing evidence input")?;
    let output = std::env::args_os()
        .nth(3)
        .ok_or("missing judgment output")?;
    let evidence: Vec<SegmentEvidence> =
        serde_json::from_slice(&std::fs::read(input).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let decisions: Vec<_> = evidence
        .iter()
        .flat_map(|e| {
            [Genre::Promotion, Genre::Narrative, Genre::Bts]
                .into_iter()
                .map(|genre| {
                    evaluate(
                        e,
                        genre,
                        AspectRatio::Landscape,
                        &BrandIdentity::default(),
                        &e.range,
                        &[],
                        true,
                    )
                })
        })
        .collect();
    std::fs::write(
        output,
        serde_json::to_vec_pretty(&json!({"policyVersion":POLICY_VERSION,
        "liveGeneration":false,"brandIdentity":null,"relationContext":null,"decisions":decisions}))
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence() -> SegmentEvidence {
        let metadata: TechnicalMetadata = serde_json::from_value(json!({"durationMs":5000,"visualEvidence":[{
            "subjects":["machine"],"detail":{"focus":"sharp","subjectSpans":[{"timeMs":1000,"left":0.1,"right":0.9}],
                "changes":[{"startMs":500,"endMs":4500,"description":"complete movement"}],
                "highlights":[{"timeMs":3000,"description":"completion"}]}}]})).unwrap();
        let mut evidence = evidence_contract::adapt_asset("asset", &metadata).remove(0);
        for risk in &mut evidence.risks {
            risk.state = EvidenceState::NotHit;
            risk.confidence = Some(0.9);
        }
        evidence
    }

    #[test]
    fn genre_floor_contract_keeps_hits_unknowns_and_brand_identity_distinct() {
        let mut evidence = evidence();
        let brand = BrandIdentity::default();
        let assess = |e: &SegmentEvidence, genre, final_check| {
            evaluate(
                e,
                genre,
                AspectRatio::Landscape,
                &brand,
                &e.range,
                &[],
                final_check,
            )
        };
        let focus = evidence
            .risks
            .iter_mut()
            .find(|r| r.risk == RiskKind::OutOfFocus)
            .unwrap();
        focus.state = EvidenceState::Hit;
        assert_eq!(
            assess(&evidence, Genre::Promotion, true).status,
            EligibilityStatus::Rejected
        );
        assert_eq!(
            assess(&evidence, Genre::Bts, true).status,
            EligibilityStatus::Eligible
        );
        assert_eq!(
            assess(&evidence, Genre::Narrative, true).status,
            EligibilityStatus::PendingVerification
        );
        evidence
            .risks
            .iter_mut()
            .find(|r| r.risk == RiskKind::OutOfFocus)
            .unwrap()
            .state = EvidenceState::Unknown;
        assert_eq!(
            assess(&evidence, Genre::Promotion, false).status,
            EligibilityStatus::PendingVerification
        );
        assert_eq!(
            assess(&evidence, Genre::Promotion, true).status,
            EligibilityStatus::Rejected
        );
        let logo = evidence
            .risks
            .iter_mut()
            .find(|r| r.risk == RiskKind::BrandLogo)
            .unwrap();
        logo.state = EvidenceState::Hit;
        logo.value = json!(["OurBrand", "OtherBrand"]);
        let own = BrandIdentity {
            allowed_labels: vec!["OurBrand".into()],
        };
        let decision = evaluate(
            &evidence,
            Genre::Promotion,
            AspectRatio::Landscape,
            &own,
            &evidence.range,
            &[],
            true,
        );
        assert!(decision.reasons.iter().any(|r| r.code == "brand_logo"));
        // 整段展会标记没有可裁去的安全窗；匹配分和只剩一条素材都没有入口改变此结果。
        let exhibition = evidence
            .risks
            .iter_mut()
            .find(|r| r.risk == RiskKind::Exhibition)
            .unwrap();
        exhibition.state = EvidenceState::Hit;
        let subset = EvidenceRange {
            start_ms: 2000,
            end_ms: 3000,
        };
        let decision = evaluate(
            &evidence,
            Genre::Promotion,
            AspectRatio::Landscape,
            &own,
            &subset,
            &[],
            true,
        );
        assert!(decision.reasons.iter().any(|r| r.code == "exhibition"));
        assert!(decision.usable_windows.is_empty());
        // 只含本项目名称才允许品牌；无身份的同一条片段仍淘汰。
        for risk in &mut evidence.risks {
            risk.state = EvidenceState::NotHit;
        }
        let logo = evidence
            .risks
            .iter_mut()
            .find(|r| r.risk == RiskKind::BrandLogo)
            .unwrap();
        logo.state = EvidenceState::Hit;
        logo.value = json!(["OurBrand"]);
        assert_eq!(
            evaluate(
                &evidence,
                Genre::Promotion,
                AspectRatio::Landscape,
                &own,
                &evidence.range,
                &[],
                true
            )
            .status,
            EligibilityStatus::Eligible
        );
        assert_eq!(
            assess(&evidence, Genre::Promotion, true).status,
            EligibilityStatus::Rejected
        );
        // 允许品牌只覆盖部分时段，不能把未查看的其他时间放行。
        evidence
            .risks
            .iter_mut()
            .find(|r| r.risk == RiskKind::BrandLogo)
            .unwrap()
            .range
            .end_ms = 2000;
        assert_eq!(
            evaluate(
                &evidence,
                Genre::Promotion,
                AspectRatio::Landscape,
                &own,
                &evidence.range,
                &[],
                true
            )
            .status,
            EligibilityStatus::Rejected
        );
        // 花絮允许轻微瑕疵，但已证实广告感仍硬拒。
        evidence
            .risks
            .iter_mut()
            .find(|r| r.risk == RiskKind::Advertising)
            .unwrap()
            .state = EvidenceState::Hit;
        assert_eq!(
            assess(&evidence, Genre::Bts, true).status,
            EligibilityStatus::Rejected
        );
    }

    #[test]
    fn genre_floor_contract_requires_relation_facts_and_uses_selected_canvas() {
        let evidence = evidence();
        let brand = BrandIdentity::default();
        let mut relations: Vec<_> = [
            RelationRisk::Contradiction,
            RelationRisk::AxisCrossing,
            RelationRisk::UnrelatedInsert,
        ]
        .into_iter()
        .map(|kind| RelationEvidence {
            kind,
            state: EvidenceState::NotHit,
            source: evidence.source.clone(),
            range: evidence.range.clone(),
            confidence: Some(0.9),
        })
        .collect();
        assert_eq!(
            evaluate(
                &evidence,
                Genre::Narrative,
                AspectRatio::Landscape,
                &brand,
                &evidence.range,
                &relations,
                true
            )
            .status,
            EligibilityStatus::Eligible
        );
        relations[1].state = EvidenceState::Hit;
        assert_eq!(
            evaluate(
                &evidence,
                Genre::Narrative,
                AspectRatio::Landscape,
                &brand,
                &evidence.range,
                &relations,
                true
            )
            .status,
            EligibilityStatus::Rejected
        );
        let wide = super::super::scoring::usability_score(
            &evidence,
            &evidence.range,
            AspectRatio::Landscape,
            Some(1920),
            Some(1080),
        );
        let tall = super::super::scoring::usability_score(
            &evidence,
            &evidence.range,
            AspectRatio::Portrait,
            Some(1920),
            Some(1080),
        );
        assert_eq!(wide.crop_retention, Some(1.0));
        assert!(tall.crop_retention.unwrap() < 0.4);
        let vertical_source = super::super::scoring::usability_score(
            &evidence,
            &evidence.range,
            AspectRatio::Landscape,
            Some(1080),
            Some(1920),
        );
        assert_eq!(vertical_source.crop_retention, None);
        let cut = EvidenceRange {
            start_ms: 2000,
            end_ms: 3500,
        };
        let score = super::super::scoring::usability_score(
            &evidence,
            &cut,
            AspectRatio::Landscape,
            Some(1920),
            Some(1080),
        );
        assert_eq!(score.action_completeness, Some(0.0));
        assert_eq!(score.highlight_coverage, Some(1.0));
        assert_eq!(score.source_capacity_ms, 1500);
    }
}
