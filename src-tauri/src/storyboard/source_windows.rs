//! 证据源窗精修：保持镜头身份和单硬切窗，高光/完整动作受代码保护，裁切按主体左右边界求解。
//! 只定源范围，不定配音/音乐/槽位时钟；没有边界证据或主体放不下时如实失败。
use super::super::inventory::{contains_range, resolve_reference};
use super::super::relations::{ask_visual, SelectedShot};
use crate::media_options::AspectRatio;
use crate::models::{
    EvidenceRange, EvidenceReference, EvidenceSource, SegmentEvidence, SubjectSpan,
};
use crate::provider::ModelAccess;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

pub(crate) const MAX_WINDOW_REPAIRS: usize = 2;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WindowObservation {
    pub best_range: EvidenceRange,
    pub highlights: Vec<i64>,
    pub actions: Vec<EvidenceRange>,
    /// 逐条复核旧 changes；连续运动不等于必须从原片头看到片尾的离散动作。
    pub change_review: Vec<ChangeReview>,
    pub subject_spans: Vec<SubjectSpan>,
    pub clean_start: bool,
    pub clean_end: bool,
    pub visible_reason: String,
    pub confidence: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChangeReview {
    pub index: usize,
    pub discrete_action: Option<bool>,
    pub complete_range: Option<EvidenceRange>,
    pub visible_reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RefinedEvidenceShot {
    pub selected: SelectedShot,
    pub source_range: EvidenceRange,
    pub crop_focus: [f64; 2],
    pub protected_highlights: Vec<i64>,
    pub protected_actions: Vec<EvidenceRange>,
    pub observation: WindowObservation,
    pub source: EvidenceSource,
}

/// 只把允许窗内的点证据和最佳窗交给模型；原快照不变，跨窗动作事实仍保留供失败裁决。
fn observation_detail(evidence: &SegmentEvidence, allowed: &EvidenceRange) -> serde_json::Value {
    let mut detail = json!(evidence.relations);
    if !detail.is_object() { return detail; }
    for (field, time_key) in [("highlights", "timeMs"), ("subjectSpans", "timeMs"), ("subjectPositions", "timeMs")] {
        if let Some(points) = detail[field].as_array_mut() {
            points.retain(|point| point[time_key].as_i64().is_some_and(|t| t >= allowed.start_ms && t < allowed.end_ms));
        }
    }
    if let Some(best) = detail["bestRange"].as_object_mut() {
        let start = best["startMs"].as_i64().unwrap_or(allowed.end_ms).max(allowed.start_ms);
        let end = best["endMs"].as_i64().unwrap_or(allowed.start_ms).min(allowed.end_ms);
        if start < end {
            best.insert("startMs".into(), json!(start));
            best.insert("endMs".into(), json!(end));
        } else { detail["bestRange"] = serde_json::Value::Null; }
    }
    detail
}

pub(crate) fn constrain_window(
    reference: &EvidenceReference,
    evidence: &SegmentEvidence,
    observation: &WindowObservation,
    geometry: (i64, i64),
    aspect: AspectRatio,
) -> Result<(EvidenceRange, [f64; 2], Vec<i64>, Vec<EvidenceRange>), String> {
    if !contains_range(&evidence.range, &reference.range)
        || !contains_range(&reference.range, &observation.best_range)
    {
        return Err("verification_range_invalid".into());
    }
    if observation.visible_reason.trim().is_empty()
        || !observation
            .confidence
            .is_some_and(|c| c.is_finite() && (0.0..=1.0).contains(&c))
    {
        return Err("verification_response_missing_boundary_evidence".into());
    }
    if !observation.clean_start || !observation.clean_end {
        return Err("refinement_unclean_cut_points".into());
    }
    let mut range = observation.best_range.clone();
    let mut highlights = observation.highlights.clone();
    let mut actions = observation.actions.clone();
    if observation
        .highlights
        .iter()
        .any(|t| *t < reference.range.start_ms || *t >= reference.range.end_ms)
        || observation
            .actions
            .iter()
            .any(|r| !contains_range(&reference.range, r))
        || observation.subject_spans.iter().any(|s| {
            s.time_ms < reference.range.start_ms
                || s.time_ms >= reference.range.end_ms
                || !s.left.is_finite()
                || !s.right.is_finite()
                || s.left < 0.0
                || s.right > 1.0
                || s.left > s.right
        })
    {
        return Err("verification_range_invalid".into());
    }
    if let Some(detail) = &evidence.relations {
        if let Some(best) = &detail.best_range {
            let best = EvidenceRange {
                start_ms: best.start_ms,
                end_ms: best.end_ms,
            };
            if best.start_ms < reference.range.end_ms
                && best.end_ms > reference.range.start_ms
                && (range.end_ms <= best.start_ms || range.start_ms >= best.end_ms)
            {
                return Err("verification_response_best_range_missed".into());
            }
        }
        highlights.extend(
            detail
                .highlights
                .iter()
                .filter(|h| {
                    h.time_ms >= reference.range.start_ms && h.time_ms < reference.range.end_ms
                })
                .map(|h| h.time_ms),
        );
        let mut seen = std::collections::HashSet::new();
        for review in &observation.change_review {
            if review.index >= detail.changes.len() || !seen.insert(review.index) {
                return Err("verification_response_change_index_invalid".into());
            }
        }
        for (index, change) in detail
            .changes
            .iter()
            .enumerate()
            .filter(|(_, c)| c.start_ms < range.end_ms && c.end_ms > range.start_ms)
        {
            let review = observation
                .change_review
                .iter()
                .find(|r| r.index == index)
                .ok_or("verification_response_change_review_missing")?;
            if review.visible_reason.trim().is_empty() {
                return Err("verification_response_change_review_missing".into());
            }
            match review.discrete_action {
                Some(true) => {
                    let complete = review
                        .complete_range
                        .as_ref()
                        .ok_or("verification_response_action_boundary_unknown")?;
                    if !contains_range(&reference.range, complete) {
                        return Err("refinement_action_outside_allowed_window".into());
                    }
                    if complete.start_ms >= change.end_ms || complete.end_ms <= change.start_ms {
                        return Err("verification_response_action_reference_invalid".into());
                    }
                    actions.push(complete.clone());
                }
                Some(false) => {}
                None => return Err("verification_response_action_boundary_unknown".into()),
            }
        }
    }
    for t in &highlights {
        range.start_ms = range.start_ms.min(*t);
        range.end_ms = range.end_ms.max(t.saturating_add(1));
    }
    for action in &actions {
        if !contains_range(&reference.range, action) {
            return Err("refinement_action_outside_allowed_window".into());
        }
        range.start_ms = range.start_ms.min(action.start_ms);
        range.end_ms = range.end_ms.max(action.end_ms);
    }
    if !contains_range(&reference.range, &range) {
        return Err("verification_range_invalid".into());
    }
    if let Some(d) = &evidence.relations {
        if (d.clean_start == Some(false) && range.start_ms == evidence.range.start_ms)
            || (d.clean_end == Some(false) && range.end_ms == evidence.range.end_ms)
        {
            return Err("refinement_unclean_evidence_edges".into());
        }
    }
    // 模型判断的干净端点被证据扩窗改变后，需重看该镜而不是把旧 clean=true 外推到新端点。
    if range != observation.best_range {
        return Err(format!(
            "verification_response_protected_range:{}",
            json!(range)
        ));
    }
    let (width, height) = geometry;
    if width <= 0 || height <= 0 {
        return Err("refinement_source_geometry_unknown".into());
    }
    let canvas = aspect.canvas();
    let source_ratio = width as f64 / height as f64;
    let target_ratio = canvas.width as f64 / canvas.height as f64;
    if source_ratio + 0.0001 < target_ratio {
        return Err("refinement_vertical_subject_bounds_unknown".into());
    }
    let fraction = (target_ratio / source_ratio).min(1.0);
    let mut spans = observation.subject_spans.clone();
    if let Some(d) = &evidence.relations {
        spans.extend(d.subject_spans.clone());
    }
    let spans: Vec<_> = spans
        .iter()
        .filter(|s| s.time_ms >= range.start_ms && s.time_ms < range.end_ms)
        .collect();
    if fraction < 0.9999 && spans.is_empty() {
        return Err("refinement_subject_bounds_unknown".into());
    }
    if spans.iter().any(|s| {
        !s.left.is_finite()
            || !s.right.is_finite()
            || s.left < 0.0
            || s.right > 1.0
            || s.left > s.right
    }) {
        return Err("refinement_subject_bounds_invalid".into());
    }
    let left = spans.iter().map(|s| s.left).fold(1.0, f64::min);
    let right = spans.iter().map(|s| s.right).fold(0.0, f64::max);
    if right - left > fraction + 0.0001 {
        return Err("refinement_subject_does_not_fit_crop".into());
    }
    // cropFocus 是裁切矩形中心；同时约束每个样本时刻的左右边界。
    let low = (right - fraction / 2.0).max(fraction / 2.0);
    let high = (left + fraction / 2.0).min(1.0 - fraction / 2.0);
    let focus = if fraction >= 0.9999 {
        0.5
    } else {
        if low > high + 0.0001 {
            return Err("refinement_subject_does_not_fit_crop".into());
        }
        ((left + right) / 2.0).clamp(low, high.max(low))
    };
    highlights.sort_unstable();
    highlights.dedup();
    Ok((range, [focus, 0.5], highlights, actions))
}

/// 当次调用成功结果留存；下一次只修未成功的镜头，模型不能返回其他镜头身份。
#[derive(Default)]
pub(crate) struct EvidenceRefinementSession {
    pub completed: HashMap<usize, RefinedEvidenceShot>,
    bindings: HashMap<usize, String>,
    repairs: HashMap<usize, String>,
}

impl EvidenceRefinementSession {
    /// 关系/最终校验失败时，仅解除指定镜头的成功缓存；原身份绑定和其他镜头继续冻结。
    pub(crate) fn retry_affected(&mut self, feedback: &[(usize, String)]) {
        for (slot, reason) in feedback {
            self.completed.remove(slot);
            self.repairs.insert(*slot, reason.clone());
        }
    }
    pub(crate) fn refine(
        &mut self,
        access: &ModelAccess,
        selected: &[SelectedShot],
        evidence: &[SegmentEvidence],
        grids: &HashMap<String, serde_json::Value>,
        geometry: &HashMap<String, (i64, i64)>,
        aspect: AspectRatio,
    ) -> Vec<(usize, String)> {
        let mut errors = Vec::new();
        let mut pending = Vec::new();
        let mut requests = Vec::new();
        for shot in selected {
            let binding = crate::assets::evidence_contract::content_id(
                "refinement-binding",
                &json!({"shot":shot,"aspect":aspect,"geometry":geometry.get(&shot.reference.asset_id)}),
            );
            if self.bindings.get(&shot.slot).is_some_and(|s| s != &binding) {
                errors.push((shot.slot, "refinement_frozen_identity_changed".into()));
                continue;
            }
            if self.completed.contains_key(&shot.slot) {
                continue;
            }
            let Ok(e) = resolve_reference(&shot.reference, evidence) else {
                errors.push((shot.slot, "refinement_reference_invalid".into()));
                continue;
            };
            let Some(grid) = grids.get(&e.id) else {
                errors.push((shot.slot, "refinement_frames_missing".into()));
                continue;
            };
            self.bindings.insert(shot.slot, binding);
            pending.push(shot);
            requests.push((json!({"task":"Find clean cut points in this SINGLE hard-cut-free allowed window. Source frame labels are milliseconds. Preserve all provided highlights inside the allowed window, prefer the original bestRange, and keep each discrete action complete. Review EVERY original detail.changes by index: discreteAction=true for a visible bounded action, false for continuous/repeating motion or camera change (explain from pictures), null if uncertain. For true provide the actual completeRange; never cut a discrete action that extends outside allowed range. Continuous machine rotation does not require using the entire original shot. Return JSON {bestRange:{startMs,endMs},highlights:[sourceMs],actions:[{startMs,endMs}],changeReview:[{index:integer,discreteAction:boolean|null,completeRange:{startMs,endMs}|null,visibleReason:string}],subjectSpans:[{timeMs,left,right}],cleanStart:true,cleanEnd:true,visibleReason:string,confidence:0..1}. bestRange must already contain all protected highlights and complete action ranges. Do not change identity or fill duration. Unknown clean points must be false.",
                "allowed":shot.reference,"detail":observation_detail(e,&shot.reference.range),"aspect":aspect,"repair":self.repairs.get(&shot.slot)}),vec![grid.clone()]));
        }
        let mut failures: HashMap<usize, String> = HashMap::new();
        for round in 0..=MAX_WINDOW_REPAIRS {
            if pending.is_empty() {
                break;
            }
            let responses = ask_visual::<WindowObservation>(
                access,
                "Phase 4 evidence windows",
                requests.clone(),
            );
            let mut retry_shots = Vec::new();
            let mut retry_requests = Vec::new();
            for ((shot, request), response) in
                pending.into_iter().zip(requests.into_iter()).zip(responses)
            {
                let observed = response.is_ok();
                let result: Result<RefinedEvidenceShot, String> = (|| {
                    let (observation, source) = response?;
                    let e = resolve_reference(&shot.reference, evidence)?;
                    let (range, crop, highlights, actions) = constrain_window(
                        &shot.reference,
                        e,
                        &observation,
                        geometry
                            .get(&e.asset_id)
                            .copied()
                            .ok_or("refinement_source_geometry_unknown")?,
                        aspect,
                    )?;
                    Ok(RefinedEvidenceShot {
                        selected: shot.clone(),
                        source_range: range,
                        crop_focus: crop,
                        protected_highlights: highlights,
                        protected_actions: actions,
                        observation,
                        source,
                    })
                })();
                match result {
                    Ok(value) => {
                        self.completed.insert(shot.slot, value);
                        self.repairs.remove(&shot.slot);
                        failures.remove(&shot.slot);
                    }
                    Err(error) => {
                        failures.insert(shot.slot, error.clone());
                        if round < MAX_WINDOW_REPAIRS
                            && observed
                            && (crate::assets::evidence_verification::retryable_verification_error(
                                &error,
                            ) || error.starts_with("refinement_unclean_")
                                || error.starts_with("refinement_subject_"))
                        {
                            let mut request = request;
                            request.0["repair"] = json!(error);
                            retry_shots.push(shot);
                            retry_requests.push(request);
                        }
                    }
                }
            }
            if retry_shots.is_empty() {
                break;
            }
            pending = retry_shots;
            requests = retry_requests;
        }
        errors.extend(failures);
        errors.sort_by_key(|e| e.0);
        errors
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    #[test]
    fn relations_contract_protects_windows_actions_highlights_and_crop() {
        let m: crate::models::TechnicalMetadata =
            serde_json::from_value(json!({"durationMs":4000})).unwrap();
        let e = crate::assets::evidence_contract::adapt_asset("a", &m).remove(0);
        let r = super::super::super::inventory::reference(&e, "action".into());
        let mut o = WindowObservation {
            best_range: EvidenceRange {
                start_ms: 500,
                end_ms: 3500,
            },
            highlights: vec![2000],
            actions: vec![EvidenceRange {
                start_ms: 1000,
                end_ms: 3000,
            }],
            change_review: vec![],
            subject_spans: vec![SubjectSpan {
                time_ms: 2000,
                left: 0.4,
                right: 0.6,
            }],
            clean_start: true,
            clean_end: true,
            visible_reason: "full action".into(),
            confidence: Some(0.9),
        };
        let (range, crop, _, _) =
            constrain_window(&r, &e, &o, (1920, 1080), AspectRatio::Portrait).unwrap();
        assert_eq!(range, o.best_range);
        assert_eq!(crop, [0.5, 0.5]);
        o.best_range.end_ms = 2500;
        assert!(
            constrain_window(&r, &e, &o, (1920, 1080), AspectRatio::Portrait)
                .unwrap_err()
                .contains("protected_range")
        );
        o.best_range.end_ms = 4500;
        assert_eq!(
            constrain_window(&r, &e, &o, (1920, 1080), AspectRatio::Portrait).unwrap_err(),
            "verification_range_invalid"
        );
        o.best_range.end_ms = 3500;
        o.subject_spans[0].left = 0.0;
        o.subject_spans[0].right = 0.8;
        assert_eq!(
            constrain_window(&r, &e, &o, (1920, 1080), AspectRatio::Portrait).unwrap_err(),
            "refinement_subject_does_not_fit_crop"
        );
        let mut session = EvidenceRefinementSession::default();
        for slot in [0, 1] {
            session.bindings.insert(slot, "frozen".into());
            session.completed.insert(
                slot,
                RefinedEvidenceShot {
                    selected: SelectedShot {
                        slot,
                        section_id: "one".into(),
                        reference: r.clone(),
                        fit_score: 90.0,
                    },
                    source_range: EvidenceRange {
                        start_ms: 500,
                        end_ms: 3500,
                    },
                    crop_focus: [0.5, 0.5],
                    protected_highlights: vec![2000],
                    protected_actions: vec![],
                    observation: o.clone(),
                    source: e.source.clone(),
                },
            );
        }
        session.retry_affected(&[(1, "only this cut failed".into())]);
        assert!(session.completed.contains_key(&0));
        assert!(!session.completed.contains_key(&1));
        assert_eq!(session.bindings[&1], "frozen");
        let mut bounded = e.clone();
        bounded.relations = Some(serde_json::from_value(json!({
            "bestRange":{"startMs":0,"endMs":4000},
            "highlights":[{"timeMs":100,"description":"outside"},{"timeMs":2000,"description":"inside"}],
            "subjectSpans":[{"timeMs":100,"left":0.1,"right":0.9},{"timeMs":2000,"left":0.4,"right":0.6}]
        })).unwrap());
        let detail = observation_detail(&bounded, &EvidenceRange { start_ms:500,end_ms:3500 });
        assert_eq!(detail["bestRange"]["startMs"], json!(500));
        assert_eq!(detail["bestRange"]["endMs"], json!(3500));
        assert_eq!(detail["highlights"].as_array().unwrap().len(), 1);
        assert_eq!(detail["subjectSpans"].as_array().unwrap().len(), 1);
        assert_eq!(bounded.relations.unwrap().highlights.len(), 2);
    }
}
