//! 有引用策划的全片组合：模型提供适配和邻镜事实，代码搜索、去重、排序并保留不足证明。
//! 不修改策划、体裁底线或时钟；搜索预算耗尽不能冒充无解，也不能借此允许复用。
use super::eligibility::{self, BrandIdentity, EligibilityStatus, RelationEvidence, RelationRisk};
use super::inventory::resolve_reference;
use super::planning::{PlanningResult, SupportMode};
use crate::media_options::AspectRatio;
use crate::models::{
    EvidenceRange, EvidenceReference, EvidenceSource, EvidenceState, Genre, SegmentEvidence,
};
use crate::provider::{model_response_json_text, post_model_payloads_concurrently, ModelAccess};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

pub(crate) const RELATIONS_VERSION: &str = "shot-relations-v1";
pub(crate) const MAX_SEARCH_NODES: usize = 1_000_000;

/// 每个片段对每条主张单独评分，只有模型看图证实支持才进入该槽位；分数不能解除底线。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FitFact {
    pub slot: usize,
    pub evidence_id: String,
    pub score: f64,
    pub supports: bool,
    pub visible_reason: String,
    pub source: EvidenceSource,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PairFacts {
    pub before: String,
    pub after: String,
    pub before_range: EvidenceRange,
    pub after_range: EvidenceRange,
    pub similar: EvidenceState,
    pub same_scene: EvidenceState,
    pub same_person: EvidenceState,
    pub action_compatible: EvidenceState,
    pub gaze_compatible: EvidenceState,
    pub axis_crossing: EvidenceState,
    pub direction_reversed: bool,
    pub action_before: String,
    pub action_after: String,
    pub person_evidence: String,
    pub scene_evidence: String,
    pub gaze_evidence: String,
    /// 两机位相对于同一动作轴线的位置；没有该证据时轴线保持未知。
    pub camera_evidence: String,
    /// before/after 两条可见时刻/动作先后证据；空时只能按主题，不能称真实时间顺序。
    pub chronology_evidence: String,
    pub theme_evidence: String,
    pub confidence: Option<f64>,
    pub source: EvidenceSource,
}

impl PairFacts {
    pub(crate) fn narrative_evidence(&self, range: &EvidenceRange) -> Vec<RelationEvidence> {
        let known = range == &self.after_range
            && !self.source.analysis_id.trim().is_empty()
            && !self.source.method.trim().is_empty()
            && self
                .confidence
                .is_some_and(|c| c.is_finite() && (0.0..=1.0).contains(&c));
        let action_known =
            known && !self.action_before.trim().is_empty() && !self.action_after.trim().is_empty();
        let contradiction = if action_known
            && (self.action_compatible == EvidenceState::NotHit
                || (self.gaze_compatible == EvidenceState::NotHit
                    && !self.gaze_evidence.trim().is_empty()))
        {
            EvidenceState::Hit
        } else if action_known
            && self.action_compatible == EvidenceState::Hit
            && self.gaze_compatible == EvidenceState::Hit
            && !self.gaze_evidence.trim().is_empty()
        {
            EvidenceState::NotHit
        } else {
            EvidenceState::Unknown
        };
        let axis = if known && !self.camera_evidence.trim().is_empty() {
            self.axis_crossing
        } else {
            EvidenceState::Unknown
        };
        // 方向反转本身不产生跳轴 Hit；仍须两机位证据。
        let related = known
            && ((self.same_person == EvidenceState::Hit
                && !self.person_evidence.trim().is_empty())
                || (self.same_scene == EvidenceState::Hit
                    && !self.scene_evidence.trim().is_empty()));
        let unrelated = if related && action_known {
            EvidenceState::NotHit
        } else if known
            && self.same_person == EvidenceState::NotHit
            && self.same_scene == EvidenceState::NotHit
            && !self.person_evidence.trim().is_empty()
            && !self.scene_evidence.trim().is_empty()
        {
            EvidenceState::Hit
        } else {
            EvidenceState::Unknown
        };
        [
            (RelationRisk::Contradiction, contradiction),
            (RelationRisk::AxisCrossing, axis),
            (RelationRisk::UnrelatedInsert, unrelated),
        ]
        .into_iter()
        .map(|(kind, state)| RelationEvidence {
            kind,
            state,
            source: self.source.clone(),
            range: range.clone(),
            confidence: self.confidence,
        })
        .collect()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShotSlot {
    pub section_id: String,
    pub expression: String,
    pub support_mode: SupportMode,
    pub reference: EvidenceReference,
    pub candidates: Vec<EvidenceReference>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectedShot {
    pub slot: usize,
    pub section_id: String,
    pub reference: EvidenceReference,
    pub fit_score: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CombinationResult {
    pub version: String,
    pub shots: Vec<SelectedShot>,
    pub duplicate_assets: usize,
    pub explored_nodes: usize,
    pub minimum_duplicate_assets: usize,
    /// 同一语义/源窗/关系约束下，穷举低复用层无解的机器证明；不使用全库数量代替。
    pub availability_proof: Value,
}

pub(crate) fn slots(
    plan: &PlanningResult,
    eligible: &[SegmentEvidence],
) -> Result<Vec<ShotSlot>, String> {
    let proposal = plan.proposal.as_ref().ok_or("relations_no_accepted_plan")?;
    let mut slots = Vec::new();
    let mut windows = HashMap::new();
    // 看图和事实均以 evidenceId 绑定一个允许窗；不能悄悄借同 ID 的另一范围。
    for claim in proposal.sections.iter().flat_map(|s| s.claims.iter().chain(&s.alternatives)) {
        resolve_reference(&claim.reference, eligible)?;
        if windows.insert(claim.reference.evidence_id.clone(), claim.reference.range.clone())
            .is_some_and(|range| range != claim.reference.range) {
            return Err("relations_conflicting_candidate_windows".into());
        }
    }
    for section in &proposal.sections {
        if section.claims.len() > section.max_shots {
            return Err("relations_claims_exceed_recipe_capacity".into());
        }
        for claim in &section.claims {
            resolve_reference(&claim.reference, eligible)?;
            let mut seen = HashSet::new();
            let candidates = section
                .claims
                .iter()
                .chain(&section.alternatives)
                .map(|c| c.reference.clone())
                .filter(|r| seen.insert(r.evidence_id.clone()))
                .collect::<Vec<_>>();
            for r in &candidates {
                resolve_reference(r, eligible)?;
            }
            slots.push(ShotSlot {
                section_id: section.recipe_section_id.clone(),
                expression: claim.expression.clone(),
                support_mode: claim.support_mode,
                reference: claim.reference.clone(),
                candidates,
            });
        }
    }
    if slots.is_empty() {
        return Err("relations_empty_plan".into());
    }
    Ok(slots)
}

/// 由调用方提供窗内网格，无本地路径进入模型；失败仅重发相应请求，不回退到文字猜关系。
pub(crate) fn ask_visual<T: serde::de::DeserializeOwned>(
    access: &ModelAccess,
    stage: &str,
    requests: Vec<(Value, Vec<Value>)>,
) -> Vec<Result<(T, EvidenceSource), String>> {
    ask_visual_checked(access, stage, requests, |_, _| Ok(()))
}

fn strict_object(properties: Value) -> Value {
    let required = properties.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

/// 固定每个返回键和批次大小；真实响应曾连续漏动作字段，不能靠提示词或默认阴性补齐。
fn visual_response_format(access: &ModelAccess, request: &Value) -> Value {
    let state = json!({"type":"string","enum":["hit","not_hit","unknown"]});
    let text = json!({"type":"string"});
    let boolean = json!({"type":"boolean"});
    let integer = json!({"type":"integer"});
    let confidence = json!({"type":["number","null"],"minimum":0,"maximum":1});
    let range = strict_object(json!({"startMs":integer,"endMs":integer}));
    let schema = if let Some(candidates) = request["candidates"].as_array() {
        strict_object(json!({"fits":{"type":"array","minItems":candidates.len(),"maxItems":candidates.len(),
            "items":strict_object(json!({"index":{"type":"integer","enum":(0..candidates.len()).collect::<Vec<_>>()},
                "score":{"type":"number","minimum":0,"maximum":100},"supports":boolean,"visibleReason":text}))}}))
    } else if let Some(comparisons) = request["comparisons"].as_array() {
        let fact = strict_object(json!({"similar":state,"sameScene":state,"samePerson":state,
            "actionCompatible":state,"gazeCompatible":state,"axisCrossing":state,
            "directionReversed":boolean,"actionBefore":text,"actionAfter":text,"personEvidence":text,
            "sceneEvidence":text,"gazeEvidence":text,"cameraEvidence":text,"chronologyEvidence":text,
            "themeEvidence":text,"confidence":confidence}));
        strict_object(json!({"pairs":{"type":"array","minItems":comparisons.len(),"maxItems":comparisons.len(),
            "items":strict_object(json!({"index":{"type":"integer","enum":(0..comparisons.len()).collect::<Vec<_>>()},
                "forward":fact,"reverse":fact}))}}))
    } else {
        let start = request["allowed"]["range"]["startMs"].as_i64().unwrap_or(0);
        let end = request["allowed"]["range"]["endMs"].as_i64().unwrap_or(i64::MAX);
        let time = json!({"type":"integer","minimum":start,"maximum":end.saturating_sub(1)});
        let best_range = strict_object(json!({"startMs":time,
            "endMs":{"type":"integer","minimum":start.saturating_add(1),"maximum":end}}));
        let mut nullable_range = range.clone();
        nullable_range["type"] = json!(["object","null"]);
        strict_object(json!({"bestRange":best_range,"highlights":{"type":"array","items":time},
            "actions":{"type":"array","items":range},
            "changeReview":{"type":"array","items":strict_object(json!({"index":integer,
                "discreteAction":{"type":["boolean","null"]},"completeRange":nullable_range,"visibleReason":text}))},
            "subjectSpans":{"type":"array","items":strict_object(json!({"timeMs":time,
                "left":{"type":"number","minimum":0,"maximum":1},"right":{"type":"number","minimum":0,"maximum":1}}))},
            "cleanStart":boolean,"cleanEnd":boolean,"visibleReason":text,"confidence":confidence}))
    };
    match access {
        ModelAccess::OAuth(_) => json!({"type":"json_schema","name":"shot_observation","schema":schema,"strict":true}),
        _ => json!({"type":"json_schema","json_schema":{"name":"shot_observation","schema":schema,"strict":true}}),
    }
}

fn ask_visual_checked<T: serde::de::DeserializeOwned>(
    access: &ModelAccess,
    stage: &str,
    requests: Vec<(Value, Vec<Value>)>,
    validate: impl Fn(&T, &Value) -> Result<(), String>,
) -> Vec<Result<(T, EvidenceSource), String>> {
    let payloads: Vec<_> = requests.iter().map(|(data,images)| {
        let mut content = vec![json!({"type":"input_text","text":data.to_string()})];
        content.extend(images.clone());
        json!({"model":access.custom_config().map(|c|c.model.as_str()).unwrap_or("gpt-5.4"),"store":false,"stream":true,
            "input":[{"role":"user","content":content}],"text":{"format":visual_response_format(access,data)},"max_output_tokens":8000})
    }).collect();
    let mut output: Vec<_> = payloads
        .iter()
        .map(|_| Err("relations_not_run".to_owned()))
        .collect();
    let mut pending: Vec<_> = (0..payloads.len())
        .filter(|i| {
            let valid = (1..=4).contains(&requests[*i].1.len());
            if !valid {
                output[*i] = Err("relations_images_required_max_four".into());
            }
            valid
        })
        .collect();
    for attempt in 0..=2 {
        let selected: Vec<_> = pending.iter().map(|i| payloads[*i].clone()).collect();
        let responses =
            post_model_payloads_concurrently(access, &selected, Some(Duration::from_secs(120)));
        let mut retry = Vec::new();
        for (index, response) in pending.into_iter().zip(responses) {
            super::provider_trace::append_storyboard_trace(
                stage,
                Some(&index.to_string()),
                attempt + 1,
                "request",
                &json!({"input":requests[index].0,"imageCount":requests[index].1.len()}),
            );
            let parsed: Result<(T, EvidenceSource), String> = (|| {
                if requests[index].1.is_empty() || requests[index].1.len() > 4 {
                    return Err("relations_images_required_max_four".into());
                }
                let body = response?;
                super::provider_trace::append_storyboard_trace(
                    stage,
                    Some(&index.to_string()),
                    attempt + 1,
                    "response",
                    &json!(body),
                );
                let text = model_response_json_text(access, &body)
                    .ok_or("verification_response_invalid")?;
                let object: Value = serde_json::from_str(&text)
                    .map_err(|_| "verification_response_invalid".to_owned())?;
                if !object.is_object() {
                    return Err("verification_response_invalid".into());
                }
                let value = serde_json::from_value::<T>(object)
                    .map_err(|_| "verification_response_invalid".to_owned())?;
                validate(&value, &requests[index].0)?;
                let source = EvidenceSource {
                    analysis_id: crate::assets::evidence_contract::content_id(
                        stage,
                        &json!({"input":requests[index].0,"response":text}),
                    ),
                    model: Some(
                        access
                            .custom_config()
                            .map(|c| c.model.clone())
                            .unwrap_or("gpt-5.4".into()),
                    ),
                    method: stage.into(),
                    analysis_version: 1,
                };
                Ok((value, source))
            })();
            super::provider_trace::append_pool_trace(
                stage,
                &index.to_string(),
                &json!({"attempt":attempt+1,"input":requests[index].0,"result":parsed.as_ref().ok().map(|(_,source)|source),"error":parsed.as_ref().err()}),
            );
            if attempt < 2
                && parsed.as_ref().err().is_some_and(|e| {
                    crate::assets::evidence_verification::retryable_verification_error(e)
                })
            {
                retry.push(index);
            }
            output[index] = parsed;
        }
        if retry.is_empty() {
            break;
        }
        pending = retry;
    }
    output
}

/// 固定键完整返回；漏项/重号都重试，不把一次批次里的结果套给另一候选。
fn batch_items<'a>(value: &'a Value, field: &str, count: usize) -> Result<Vec<&'a Value>, String> {
    let items = value[field]
        .as_array()
        .ok_or("verification_response_batch_missing")?;
    if count == 0 || items.len() != count {
        return Err("verification_response_batch_incomplete".into());
    }
    let mut ordered = vec![None; count];
    for item in items {
        let index = item["index"]
            .as_u64()
            .filter(|i| *i < count as u64)
            .ok_or("verification_response_batch_index_invalid")? as usize;
        if ordered[index].replace(item).is_some() {
            return Err("verification_response_batch_duplicate".into());
        }
    }
    ordered
        .into_iter()
        .map(|item| item.ok_or_else(|| "verification_response_batch_incomplete".into()))
        .collect()
}

/// 逐槽位看图评分；每批最多四个独立候选，身份由代码绑定，模型不能返回分配。
pub(crate) fn score_slots(
    access: &ModelAccess,
    slots: &[ShotSlot],
    images: &HashMap<String, Value>,
    genre: Genre,
) -> (Vec<FitFact>, Vec<String>) {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Observation {
        score: f64,
        supports: bool,
        visible_reason: String,
    }
    let mut flat = Vec::new();
    for (slot, s) in slots.iter().enumerate() {
        for r in &s.candidates {
            if let Some(image) = images.get(&r.evidence_id) {
                flat.push((
                    slot,
                    r.evidence_id.clone(),
                    json!({"expression":s.expression,"plannedSupport":s.support_mode,"allowed":r}),
                    image.clone(),
                ));
            }
        }
    }
    let batches:Vec<_>=flat.chunks(4).map(|batch| {
        (json!({"task":"Score each candidate independently from its corresponding image. Image order equals candidate index. Direct support is required unless plannedSupport explicitly permits atmosphere. Return JSON {fits:[{index,score:0..100,supports:boolean,visibleReason:string}]} with EVERY index exactly once. Never allocate shots, infer from filenames or mix evidence between candidates.",
            "genre":genre,"candidates":batch.iter().enumerate().map(|(index,(_,_,v,_))|json!({"index":index,"candidate":v})).collect::<Vec<_>>()}),
            batch.iter().map(|(_,_,_,image)|image.clone()).collect::<Vec<_>>())
    }).collect();
    let responses = ask_visual_checked::<Value>(access, "relations-fit", batches, |value, data| {
        for row in batch_items(
            value,
            "fits",
            data["candidates"].as_array().map(Vec::len).unwrap_or(0),
        )? {
            let observation: Observation = serde_json::from_value(row.clone())
                .map_err(|_| "verification_response_invalid_fit".to_owned())?;
            if !observation.score.is_finite()
                || !(0.0..=100.0).contains(&observation.score)
                || observation.visible_reason.trim().is_empty()
            {
                return Err("verification_response_invalid_fit".into());
            }
        }
        Ok(())
    });
    let mut fits = Vec::new();
    let mut errors = Vec::new();
    for (batch, response) in flat.chunks(4).zip(responses) {
        match response {
            Ok((value, source)) => {
                for ((slot, id, _, _), row) in batch
                    .iter()
                    .zip(batch_items(&value, "fits", batch.len()).unwrap())
                {
                    let observation: Observation = serde_json::from_value(row.clone()).unwrap();
                    fits.push(FitFact {
                        slot: *slot,
                        evidence_id: id.clone(),
                        score: observation.score,
                        supports: observation.supports,
                        visible_reason: observation.visible_reason,
                        source: source.clone(),
                    });
                }
            }
            Err(error) => errors.push(error),
        }
    }
    (fits, errors)
}

/// 在指定源范围比较有向邻镜事实；精修后调用方必须以新范围重看相邻关系。
pub(crate) fn observe_pairs(
    access: &ModelAccess,
    references: &[EvidenceReference],
    images: &HashMap<String, Value>,
) -> (Vec<PairFacts>, Vec<String>) {
    observe_pairs_cached(access, references, images, &[])
}

/// 只重看源窗改变或前次失败的片段对；已成功且端点未改的事实冻结。
pub(crate) fn observe_pairs_cached(
    access: &ModelAccess,
    references: &[EvidenceReference],
    images: &HashMap<String, Value>,
    previous: &[PairFacts],
) -> (Vec<PairFacts>, Vec<String>) {
    let mut model_errors = Vec::new();
    let mut pairing = Vec::new();
    let mut pair_keys = Vec::new();
    let mut pairs = Vec::new();
    for (i, a) in references.iter().enumerate() {
        for b in references.iter().skip(i + 1) {
            let cached: Vec<_> = [(a, b), (b, a)]
                .into_iter()
                .filter_map(|(before, after)| {
                    pair(previous, &before.evidence_id, &after.evidence_id)
                        .filter(|p| p.before_range == before.range && p.after_range == after.range)
                })
                .collect();
            if cached.len() == 2 {
                pairs.extend(cached.into_iter().cloned());
                continue;
            }
            if let (Some(ai), Some(bi)) = (images.get(&a.evidence_id), images.get(&b.evidence_id)) {
                let schema = json!({"similar":"hit|not_hit|unknown","sameScene":"hit|not_hit|unknown","samePerson":"hit|not_hit|unknown",
                "actionCompatible":"hit|not_hit|unknown","gazeCompatible":"hit|not_hit|unknown","axisCrossing":"hit|not_hit|unknown",
                "directionReversed":false,"actionBefore":"visible end state of before","actionAfter":"visible start state of after",
                "personEvidence":"identity clues or uncertainty","sceneEvidence":"observed setting clues","gazeEvidence":"visible gaze continuity or N/A only when visibly no gaze",
                "cameraEvidence":"relative positions of BOTH cameras to the SAME action axis; empty if unknown",
                "chronologyEvidence":"visible evidence before precedes after; empty if unknown","themeEvidence":"this run's observed theme association; empty if none","confidence":0.9});
                pairing.push((json!({"task":"Compare these TWO candidate windows. Provide forward and reverse FACT using the schema; the outer indexed batch defines the result JSON. forward=A then B; reverse=B then A. Frame labels are local source time, NOT chronological date across assets. Do not infer same person/event from similar machinery. Similar=near-duplicate composition/moment, not merely same subject. Direction reversal alone NEVER proves crossing axis: cameraEvidence must locate both cameras relative to one identified action axis, otherwise axisCrossing=unknown. Facts are observational; do not choose or allocate shots.",
                "A":a,"B":b,"schema":schema}),vec![ai.clone(),bi.clone()]));
                pair_keys.push((a.clone(), b.clone()));
            }
        }
    }
    let batches:Vec<_>=pairing.chunks(2).map(|batch| {
        (json!({"task":"Compare each indexed pair INDEPENDENTLY. Images are pair0 A, pair0 B, then pair1 A, pair1 B. Return ONLY JSON {pairs:[{index,forward:FACT,reverse:FACT}]} for every index exactly once; use that comparison's schema. Never mix people/scenes/cameras across pairs or allocate shots.",
            "comparisons":batch.iter().enumerate().map(|(index,(v,_))|json!({"index":index,"comparison":v})).collect::<Vec<_>>()}),
            batch.iter().flat_map(|(_,images)|images.clone()).collect::<Vec<_>>())
    }).collect();
    let responses =
        ask_visual_checked::<Value>(access, "relations-pair", batches, |value, data| {
            for row in batch_items(
                value,
                "pairs",
                data["comparisons"].as_array().map(Vec::len).unwrap_or(0),
            )? {
                for field in ["forward", "reverse"] {
                    let mut fact = row[field].clone();
                    if !fact.is_object() {
                        return Err("verification_response_invalid_pair".into());
                    }
                    fact["before"] = json!("");
                    fact["after"] = json!("");
                    fact["beforeRange"] = json!({"startMs":0,"endMs":1});
                    fact["afterRange"] = json!({"startMs":0,"endMs":1});
                    fact["source"] = json!(EvidenceSource {
                        analysis_id: "response-validation".into(),
                        model: None,
                        method: "relations-pair".into(),
                        analysis_version: 1
                    });
                    let parsed: PairFacts = serde_json::from_value(fact)
                        .map_err(|_| "verification_response_invalid_pair".to_owned())?;
                    if parsed
                        .confidence
                        .is_some_and(|c| !c.is_finite() || !(0.0..=1.0).contains(&c))
                    {
                        return Err("verification_confidence_invalid".into());
                    }
                }
            }
            Ok(())
        });
    for (batch, response) in pair_keys.chunks(2).zip(responses) {
        match response {
            Ok((value, source)) => {
                for ((a, b), row) in batch
                    .iter()
                    .zip(batch_items(&value, "pairs", batch.len()).unwrap())
                {
                    for (field, before, after) in [("forward", a, b), ("reverse", b, a)] {
                        let mut fact = row[field].clone();
                        fact["before"] = json!(before.evidence_id);
                        fact["after"] = json!(after.evidence_id);
                        fact["beforeRange"] = json!(before.range);
                        fact["afterRange"] = json!(after.range);
                        fact["source"] = json!(source);
                        pairs.push(serde_json::from_value::<PairFacts>(fact).unwrap());
                    }
                }
            }
            Err(error) => model_errors.push(error),
        }
    }
    (pairs, model_errors)
}

/// 源窗精修后再次检查实际范围；邻镜动作/机位事实不得从旧端点外推。
pub(crate) fn validate_sequence(
    genre: Genre,
    shots: &[SelectedShot],
    pairs: &[PairFacts],
    evidence: &[SegmentEvidence],
    aspect: AspectRatio,
    brand: &BrandIdentity,
) -> Vec<String> {
    let mut errors = Vec::new();
    for (i, a) in shots.iter().enumerate() {
        if genre != Genre::Narrative
            && !resolve_reference(&a.reference, evidence)
                .ok()
                .is_some_and(|e| {
                    eligibility::evaluate(e, genre, aspect, brand, &a.reference.range, &[], true)
                        .status
                        == EligibilityStatus::Eligible
                })
        {
            errors.push(format!("relation_final_floor_rejected:{}", a.slot));
        }
        for b in shots.iter().skip(i + 1) {
            if !verified_pair(pairs, &a.reference, &b.reference)
                .is_some_and(|p| p.similar == EvidenceState::NotHit)
                || !verified_pair(pairs, &b.reference, &a.reference)
                    .is_some_and(|p| p.similar == EvidenceState::NotHit)
            {
                errors.push(format!(
                    "relation_similarity_unverified_or_hit:{}:{}",
                    a.slot, b.slot
                ));
            }
        }
    }
    for adjacent in shots.windows(2) {
        let (a, b) = (&adjacent[0], &adjacent[1]);
        let p = verified_pair(pairs, &a.reference, &b.reference);
        if genre == Genre::Narrative {
            let valid = resolve_reference(&b.reference, evidence)
                .ok()
                .zip(p)
                .is_some_and(|(e, p)| {
                    eligibility::evaluate(
                        e,
                        genre,
                        aspect,
                        brand,
                        &b.reference.range,
                        &p.narrative_evidence(&b.reference.range),
                        true,
                    )
                    .status
                        == EligibilityStatus::Eligible
                });
            if !valid {
                errors.push(format!(
                    "relation_narrative_pending_or_rejected:{}:{}",
                    a.slot, b.slot
                ));
            }
        } else if genre == Genre::Bts
            && !p.is_some_and(|p| {
                !p.chronology_evidence.trim().is_empty() || !p.theme_evidence.trim().is_empty()
            })
        {
            errors.push(format!(
                "relation_bts_order_unverified:{}:{}",
                a.slot, b.slot
            ));
        }
    }
    errors
}

fn pair<'a>(pairs: &'a [PairFacts], a: &str, b: &str) -> Option<&'a PairFacts> {
    pairs.iter().find(|p| p.before == a && p.after == b)
}

fn verified_pair<'a>(
    pairs: &'a [PairFacts],
    before: &EvidenceReference,
    after: &EvidenceReference,
) -> Option<&'a PairFacts> {
    pair(pairs, &before.evidence_id, &after.evidence_id).filter(|p| {
        p.before_range == before.range
            && p.after_range == after.range
            && p.confidence
                .is_some_and(|c| c.is_finite() && (0.0..=1.0).contains(&c))
            && !p.source.analysis_id.trim().is_empty()
            && !p.source.method.trim().is_empty()
    })
}

fn scale(e: &SegmentEvidence) -> Option<&str> {
    e.visual_evidence
        .iter()
        .find_map(|v| v.shot_type.as_deref())
}

pub(crate) fn combine(
    plan: &PlanningResult,
    eligible: &[SegmentEvidence],
    slots: &[ShotSlot],
    fits: &[FitFact],
    pairs: &[PairFacts],
    aspect: AspectRatio,
    brand: &BrandIdentity,
    frozen: &[EvidenceReference],
) -> Result<CombinationResult, String> {
    if !matches!(
        plan.status,
        super::planning::PlanningStatus::Accepted | super::planning::PlanningStatus::Limited
    ) || json!(self::slots(plan, eligible)?) != json!(slots)
    {
        return Err("relations_unapproved_or_changed_plan_slots".into());
    }
    let genre = plan.genre.genre;
    let mut choices = Vec::new();
    let mut rejected = Vec::new();
    for (i, slot) in slots.iter().enumerate() {
        let mut allowed = Vec::new();
        for reference in &slot.candidates {
            let evidence = resolve_reference(reference, eligible)?;
            let fit = fits.iter().find(|f| {
                f.slot == i
                    && f.evidence_id == reference.evidence_id
                    && f.supports
                    && f.score.is_finite()
                    && (0.0..=100.0).contains(&f.score)
                    && !f.visible_reason.trim().is_empty()
                    && !f.source.analysis_id.trim().is_empty()
                    && !f.source.method.trim().is_empty()
            });
            // 叙事的邻镜门在组合边上执行；不伪造未知阴性来令单片段过门。
            let decision =
                eligibility::evaluate(evidence, genre, aspect, brand, &reference.range, &[], true);
            let floor_ok = if genre == Genre::Narrative {
                decision
                    .reasons
                    .iter()
                    .all(|r| r.code.starts_with("relation_"))
            } else {
                decision.status == EligibilityStatus::Eligible
            };
            let conflict = frozen.iter().any(|f| {
                f.asset_id == reference.asset_id
                    || !verified_pair(pairs, f, reference)
                        .is_some_and(|p| p.similar == EvidenceState::NotHit)
                    || !verified_pair(pairs, reference, f)
                        .is_some_and(|p| p.similar == EvidenceState::NotHit)
            });
            if let Some(fit) = fit.filter(|_| floor_ok && !conflict) {
                allowed.push((reference.clone(), fit.score));
            } else {
                rejected.push(json!({"slot":i,"reference":reference,"floor":decision,"fitMissing":fit.is_none(),"frozenConflict":conflict}));
            }
        }
        allowed.sort_by(|a, b| {
            b.1.total_cmp(&a.1)
                .then_with(|| a.0.evidence_id.cmp(&b.0.evidence_id))
        });
        choices.push(allowed);
    }
    struct Search<'a> {
        slots: &'a [ShotSlot],
        choices: &'a [Vec<(EvidenceReference, f64)>],
        pairs: &'a [PairFacts],
        plan: &'a PlanningResult,
        eligible: &'a [SegmentEvidence],
        genre: Genre,
        aspect: AspectRatio,
        brand: &'a BrandIdentity,
        nodes: usize,
        exhausted: bool,
    }
    impl Search<'_> {
        fn visit(
            &mut self,
            path: &mut Vec<SelectedShot>,
            used: &mut HashSet<usize>,
            duplicates: usize,
            max_duplicates: usize,
        ) -> bool {
            self.nodes += 1;
            if self.nodes > MAX_SEARCH_NODES {
                self.exhausted = true;
                return false;
            }
            if path.len() == self.slots.len() {
                if self.genre == Genre::Narrative && path.len() < 2 {
                    return false;
                }
                if self.genre == Genre::Bts {
                    let mut empty = 0.0;
                    let mut total = 0.0;
                    for s in path.iter() {
                        let e = resolve_reference(&s.reference, self.eligible).unwrap();
                        let state = crate::assets::evidence_contract::risk_state_for_window(
                            e,
                            crate::models::RiskKind::EmptyShot,
                            &s.reference.range,
                        );
                        if state == EvidenceState::Unknown {
                            return false;
                        }
                        let budget = self
                            .plan
                            .recipe
                            .sections
                            .iter()
                            .find(|r| r.id == s.section_id)
                            .map(|r| r.budget_ms)
                            .unwrap_or(0) as f64;
                        let count =
                            path.iter().filter(|p| p.section_id == s.section_id).count() as f64;
                        let exposure = budget / count.max(1.0);
                        total += exposure;
                        if state == EvidenceState::Hit {
                            empty += exposure;
                        }
                    }
                    if total <= 0.0 || empty / total > 0.2 + 0.000001 {
                        return false;
                    }
                }
                return true;
            }
            // 每段内部自由组合，段序由配方冻结。叙事关系/花絮时序仍在有向边上裁决。
            let next = (0..self.slots.len()).find(|i| !used.contains(i)).unwrap();
            let section = &self.slots[next].section_id;
            let remaining: Vec<_> = (0..self.slots.len())
                .filter(|i| !used.contains(i) && self.slots[*i].section_id == *section)
                .collect();
            let mut options = Vec::new();
            for i in remaining {
                for (r, score) in &self.choices[i] {
                    let repeat = path.iter().any(|s| s.reference.asset_id == r.asset_id) as usize;
                    if duplicates + repeat > max_duplicates {
                        continue;
                    }
                    if path.iter().any(|s| {
                        (s.reference.asset_id == r.asset_id
                            && s.reference.segment_id == r.segment_id)
                            || pair(self.pairs, &s.reference.evidence_id, &r.evidence_id)
                                .is_some_and(|p| p.similar == EvidenceState::Hit)
                            || pair(self.pairs, &r.evidence_id, &s.reference.evidence_id)
                                .is_some_and(|p| p.similar == EvidenceState::Hit)
                            || !verified_pair(self.pairs, &s.reference, r)
                                .is_some_and(|p| p.similar == EvidenceState::NotHit)
                            || !verified_pair(self.pairs, r, &s.reference)
                                .is_some_and(|p| p.similar == EvidenceState::NotHit)
                    }) {
                        continue;
                    }
                    let mut variation = 0.0;
                    if let Some(prev) = path.last() {
                        let p = verified_pair(self.pairs, &prev.reference, r);
                        match self.genre {
                            Genre::Narrative => {
                                let Some(p) = p else {
                                    continue;
                                };
                                if p.before_range != prev.reference.range
                                    || p.after_range != r.range
                                {
                                    continue;
                                }
                                let e = resolve_reference(r, self.eligible).unwrap();
                                if eligibility::evaluate(
                                    e,
                                    self.genre,
                                    self.aspect,
                                    self.brand,
                                    &r.range,
                                    &p.narrative_evidence(&r.range),
                                    true,
                                )
                                .status
                                    != EligibilityStatus::Eligible
                                {
                                    continue;
                                }
                            }
                            Genre::Bts => {
                                let Some(p) = p else {
                                    continue;
                                };
                                if p.chronology_evidence.trim().is_empty()
                                    && p.theme_evidence.trim().is_empty()
                                {
                                    continue;
                                }
                            }
                            Genre::Promotion => {
                                if p.is_some_and(|p| {
                                    p.same_scene == EvidenceState::Hit
                                        && p.similar == EvidenceState::Hit
                                }) {
                                    continue;
                                }
                                if prev.section_id == *section {
                                    let a =
                                        resolve_reference(&prev.reference, self.eligible).unwrap();
                                    let b = resolve_reference(r, self.eligible).unwrap();
                                    if scale(a).is_some()
                                        && scale(b).is_some()
                                        && scale(a) != scale(b)
                                    {
                                        variation += 110.0;
                                    }
                                }
                                if p.is_some_and(|p| p.same_scene == EvidenceState::Hit) {
                                    variation -= 5.0;
                                }
                            }
                        }
                    }
                    options.push((i, r.clone(), *score, *score + variation, repeat));
                }
            }
            options.sort_by(|a, b| {
                b.3.total_cmp(&a.3)
                    .then_with(|| a.0.cmp(&b.0))
                    .then_with(|| a.1.evidence_id.cmp(&b.1.evidence_id))
            });
            for (i, r, score, _, repeat) in options {
                used.insert(i);
                path.push(SelectedShot {
                    slot: i,
                    section_id: self.slots[i].section_id.clone(),
                    reference: r,
                    fit_score: score,
                });
                if self.visit(path, used, duplicates + repeat, max_duplicates) {
                    return true;
                }
                path.pop();
                used.remove(&i);
                if self.exhausted {
                    return false;
                }
            }
            false
        }
    }
    let mut search = Search {
        slots,
        choices: &choices,
        pairs,
        plan,
        eligible,
        genre,
        aspect,
        brand,
        nodes: 0,
        exhausted: false,
    };
    let mut failed_levels = Vec::new();
    for limit in 0..slots.len() {
        let mut path = Vec::new();
        if search.visit(&mut path, &mut HashSet::new(), 0, limit) {
            return Ok(CombinationResult {
                version: RELATIONS_VERSION.into(),
                shots: path,
                duplicate_assets: limit,
                minimum_duplicate_assets: limit,
                explored_nodes: search.nodes,
                availability_proof: json!({"sameConstraints":true,"exhaustiveFailedDuplicateLimits":failed_levels,
                    "candidateSets":choices.iter().map(|c|c.iter().map(|(r,_)|r).collect::<Vec<_>>()).collect::<Vec<_>>(),"rejected":rejected,
                    "searchExhausted":false,"scope":"plan_references_and_alternatives_only"}),
            });
        }
        if search.exhausted {
            return Err(format!(
                "relations_search_budget_exhausted:{}; no insufficiency proof; reuse forbidden",
                search.nodes
            ));
        }
        failed_levels.push(limit);
    }
    Err(format!(
        "relations_no_valid_sequence:{}",
        json!({"candidateSets":choices,"rejected":rejected,"exploredNodes":search.nodes,"exhaustive":true})
    ))
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    use crate::assets::evidence_contract::{adapt_asset, seal_risk, seal_segment};

    #[test]
    fn relations_contract_global_search_and_axis_evidence() {
        let access = ModelAccess::Custom(crate::custom_api::CustomApiConfig {
            base_url: "http://unused.invalid".into(), model: "unused".into(),
            coarse_visual_model: String::new(), api_key: String::new(),
        });
        let format = visual_response_format(&access, &json!({"comparisons":[{},{}]}));
        let schema = &format["json_schema"]["schema"]["properties"]["pairs"];
        assert_eq!(schema["minItems"], json!(2));
        assert_eq!(schema["maxItems"], json!(2));
        let fact = &schema["items"]["properties"]["forward"];
        assert!(fact["required"].as_array().unwrap().contains(&json!("actionCompatible")));
        assert_eq!(fact["additionalProperties"], json!(false));
        assert_eq!(fact["properties"]["axisCrossing"]["enum"], json!(["hit","not_hit","unknown"]));
        let metadata:crate::models::TechnicalMetadata=serde_json::from_value(json!({"durationMs":2000,
            "sceneSegments":[{"id":"s1","startMs":0,"endMs":2000},{"id":"s2","startMs":2000,"endMs":4000}]})).unwrap();
        let mut evidence = Vec::new();
        for asset in ["a", "b"] {
            for mut e in adapt_asset(asset, &metadata) {
                for risk in eligibility::critical_risks(Genre::Promotion) {
                    e.risks.push(seal_risk(crate::models::RiskEvidence {
                        id: String::new(),
                        risk: *risk,
                        state: EvidenceState::NotHit,
                        source: e.source.clone(),
                        range: e.range.clone(),
                        confidence: Some(0.9),
                        value: json!({}),
                    }));
                }
                evidence.push(seal_segment(e));
            }
        }
        let a = super::super::inventory::reference(&evidence[0], "same proof".into());
        let a2 = super::super::inventory::reference(&evidence[1], "same proof".into());
        let b = super::super::inventory::reference(&evidence[2], "same proof".into());
        let section = |id: &str, r: &EvidenceReference, alternatives: Vec<EvidenceReference>| {
            json!({"recipeSectionId":id,"content":"same proof","claims":[{
            "expression":"same proof","reference":r,"evidenceQuote":"visible","supportMode":"direct"}],
            "alternatives":alternatives.iter().map(|r|json!({"expression":"same proof","reference":r,"evidenceQuote":"visible","supportMode":"direct"})).collect::<Vec<_>>(),"budgetMs":2000,"maxShots":1})
        };
        let make_plan = |alternative: Vec<EvidenceReference>| {
            serde_json::from_value::<PlanningResult>(json!({"status":"accepted",
            "genre":{"selection":"promotion","genre":"promotion","reason":"manual","limited":false,"snapshotId":"test"},
            "recipe":{"version":"test","genre":"promotion","requestedDurationMs":4000,"plannedDurationMs":4000,"limited":false,"limitations":[],"sections":[]},
            "proposal":{"title":"test","sections":[section("one",&a,alternative),section("two",&a2,vec![])],"gaps":[]},
            "actualDurationMs":4000,"gaps":[],"rejectionReasons":[],"rejectionMessages":[],"rejectedAttempts":[],
            "metadata":{"pipelineVersion":"test","genre":"promotion","recipeVersion":"test","evidenceSnapshot":[],"evidenceReferences":[]}})).unwrap()
        };
        let mut pairs = Vec::new();
        for before in &evidence {
            for after in &evidence {
                if before.id != after.id {
                    pairs.push(PairFacts {
                        before: before.id.clone(),
                        after: after.id.clone(),
                        before_range: before.range.clone(),
                        after_range: after.range.clone(),
                        similar: EvidenceState::NotHit,
                        same_scene: EvidenceState::Hit,
                        same_person: EvidenceState::Hit,
                        action_compatible: EvidenceState::Hit,
                        gaze_compatible: EvidenceState::Hit,
                        axis_crossing: EvidenceState::Hit,
                        direction_reversed: true,
                        action_before: "object moving".into(),
                        action_after: "object stopped".into(),
                        person_evidence: "same actor".into(),
                        scene_evidence: "same desk".into(),
                        gaze_evidence: "same eye line".into(),
                        camera_evidence: String::new(),
                        chronology_evidence: String::new(),
                        theme_evidence: String::new(),
                        confidence: Some(0.9),
                        source: before.source.clone(),
                    });
                }
            }
        }
        let source = &evidence[0].source;
        let make_fits = |slots: &[ShotSlot]| {
            slots
                .iter()
                .enumerate()
                .flat_map(|(i, s)| {
                    s.candidates.iter().map(move |r| FitFact {
                        slot: i,
                        evidence_id: r.evidence_id.clone(),
                        score: if r.asset_id == "a" { 100.0 } else { 50.0 },
                        supports: true,
                        visible_reason: "visible".into(),
                        source: (*source).clone(),
                    })
                })
                .collect::<Vec<_>>()
        };
        let plan = make_plan(vec![b]);
        let slots = slots(&plan, &evidence).unwrap();
        let fits = make_fits(&slots);
        let result = combine(
            &plan,
            &evidence,
            &slots,
            &fits,
            &pairs,
            AspectRatio::Landscape,
            &BrandIdentity::default(),
            &[],
        )
        .unwrap();
        assert_eq!(result.duplicate_assets, 0);
        assert_eq!(result.shots[0].reference.asset_id, "b");
        let plan = make_plan(vec![]);
        let slots = self::slots(&plan, &evidence).unwrap();
        let fits = make_fits(&slots);
        let result = combine(
            &plan,
            &evidence,
            &slots,
            &fits,
            &pairs,
            AspectRatio::Landscape,
            &BrandIdentity::default(),
            &[],
        )
        .unwrap();
        assert_eq!(result.duplicate_assets, 1);
        assert_eq!(
            result.availability_proof["exhaustiveFailedDuplicateLimits"],
            json!([0])
        );
        let facts = pairs[0].narrative_evidence(&pairs[0].after_range);
        assert_eq!(
            facts
                .iter()
                .find(|f| f.kind == RelationRisk::AxisCrossing)
                .unwrap()
                .state,
            EvidenceState::Unknown
        );
        let mut observed = pairs[0].clone();
        observed.camera_evidence = "camera A north of desk axis, camera B south".into();
        assert_eq!(
            observed
                .narrative_evidence(&observed.after_range)
                .iter()
                .find(|f| f.kind == RelationRisk::AxisCrossing)
                .unwrap()
                .state,
            EvidenceState::Hit
        );
        observed.axis_crossing = EvidenceState::NotHit;
        assert!(observed
            .narrative_evidence(&observed.after_range)
            .iter()
            .all(|f| f.state == EvidenceState::NotHit));
        observed.gaze_compatible = EvidenceState::NotHit;
        assert_eq!(
            observed.narrative_evidence(&observed.after_range)[0].state,
            EvidenceState::Hit
        );
        let mut narrative = serde_json::to_value(&plan).unwrap();
        narrative["genre"]["genre"] = json!("narrative");
        narrative["recipe"]["genre"] = json!("narrative");
        narrative["metadata"]["genre"] = json!("narrative");
        let narrative: PlanningResult = serde_json::from_value(narrative).unwrap();
        assert!(combine(
            &narrative,
            &evidence,
            &slots,
            &fits,
            &pairs,
            AspectRatio::Landscape,
            &BrandIdentity::default(),
            &[]
        )
        .is_err());
        let mut narrative_pairs = pairs.clone();
        for p in &mut narrative_pairs {
            p.axis_crossing = EvidenceState::NotHit;
            p.camera_evidence = "both cameras on north side of the same desk axis".into();
        }
        let selected = combine(
            &narrative,
            &evidence,
            &slots,
            &fits,
            &narrative_pairs,
            AspectRatio::Landscape,
            &BrandIdentity::default(),
            &[],
        )
        .unwrap();
        assert!(validate_sequence(
            Genre::Narrative,
            &selected.shots,
            &narrative_pairs,
            &evidence,
            AspectRatio::Landscape,
            &BrandIdentity::default()
        )
        .is_empty());
    }
}
