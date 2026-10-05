//! 隔离策划评测：先过任务 3 底线门，冻结需求定义，三次独立盘点与策划；无选镜/时间线副作用。
use crate::models::SegmentEvidence;
use crate::storyboard::{eligibility, genre, inventory, planning};
use serde_json::{json, Value};
use std::{fs, path::Path};

fn save(directory: &Path, name: &str, value: &impl serde::Serialize) -> Result<(), String> {
    fs::write(directory.join(name), serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

pub(super) fn run(job: &Value, directory: &Path, data: &Path) -> Result<(), String> {
    let input: Vec<SegmentEvidence> = serde_json::from_value(job["evidence"].clone()).map_err(|e| e.to_string())?;
    let mut candidates: Vec<SegmentEvidence> = serde_json::from_value(job["eligibleEvidence"].clone()).map_err(|e| e.to_string())?;
    if input.iter().chain(&candidates).any(|e| e.id != crate::assets::evidence_contract::seal_segment(e.clone()).id) {
        return Err("planning_eval_unsealed_evidence".into());
    }
    if let Some(windows) = job.get("candidateWindows") {
        for segment in &mut candidates {
            let key = format!("{}:{}",segment.asset_id,segment.segment_id);
            if let Some(window) = windows.get(&key) {
                let range: crate::models::EvidenceRange = serde_json::from_value(window.clone()).map_err(|e|e.to_string())?;
                if !inventory::contains_range(&segment.range,&range) { return Err("planning_eval_candidate_window_outside_snapshot".into()); }
                segment.range = range;
                *segment = crate::assets::evidence_contract::seal_segment(segment.clone());
            }
        }
    }
    let selection: genre::GenreSelection = serde_json::from_value(job["selection"].clone()).map_err(|e| e.to_string())?;
    let request = job["request"].as_str().ok_or("planning_eval_missing_request")?;
    let facts: Vec<planning::UserFact> = serde_json::from_value(job.get("userFacts").cloned().unwrap_or(json!([]))).map_err(|e| e.to_string())?;
    let access = super::model_access()?;
    let outcome = (|| -> Result<Value, String> {
        let mut inventory = inventory::Inventory { items:vec![], talkable_content:vec![], gaps:vec![], causal_links:vec![],
            causal_chain_complete:false, causal_reason:"工业快照尚无经核验的同一事件因果链。".into(),
            request_fulfillable:true, coverage_count:0, requirements:vec![], rejected_causal_links:vec![] };
        if let Some(frozen) = job.get("frozenInventory") {
            inventory = serde_json::from_value(frozen.clone()).map_err(|e| e.to_string())?;
        }
        let preliminary = genre::decide_genre(&access, selection, request, &inventory, None)?;
        save(directory,"genre-before-eligibility.json",&preliminary)?;
        let decisions: Vec<_> = candidates.iter().map(|s| eligibility::evaluate(s, preliminary.genre,
            crate::media_options::AspectRatio::Landscape, &eligibility::BrandIdentity::default(), &s.range, &[], true)).collect();
        let eligible: Vec<_> = candidates.iter().zip(&decisions).filter(|(_,d)| d.status == eligibility::EligibilityStatus::Eligible)
            .map(|(s,_)| s.clone()).collect();
        save(directory,"eligibility.json",&json!({"policyVersion":eligibility::POLICY_VERSION,"genre":preliminary.genre,
            "inputCount":candidates.len(),"eligibleCount":eligible.len(),"decisions":decisions}))?;
        save(directory,"eligible-evidence.json",&eligible)?;
        if !eligible.is_empty() {
            inventory = if job.get("frozenInventory").is_some() {
                inventory::build_inventory_with_requirements(&access, request, &eligible, Some(&inventory.requirements))?
            } else { inventory::build_inventory(&access, request, &eligible)? };
        }
        // 调用方不得把另一底线集合的盘点拿来当本轮证据支持。
        if inventory.items.iter().any(|item| !eligible.iter().any(|e| inventory::item_matches(item,e))) {
            return Err("planning_eval_inventory_outside_eligible_input".into());
        }
        save(directory,"inventory.json",&inventory)?;
        let decision = genre::bind_genre_to_inventory(&preliminary, request, &inventory)?;
        save(directory,"genre.json",&decision)?;
        if job["prepareOnly"].as_bool() == Some(true) {
            return Ok(json!({"status":"prepared","eligibleCount":eligible.len()}));
        }
        let result = if eligible.is_empty() {
            planning::no_eligible_result(&decision, job["durationMs"].as_i64(),
                format!("所选体裁没有合格片段：{} 个输入均未通过底线；未知风险或邻镜关系不能当安全/因果证明。",candidates.len()))
        } else {
            let recipe = genre::build_recipe(&decision,job["durationMs"].as_i64(),&eligible,&inventory)?;
            save(directory,"recipe.json",&recipe)?;
            planning::plan_with_evidence(&access,request,&inventory,&decision,&recipe,&eligible,&facts)?
        };
        Ok(json!({"result":result,"eligibleCount":eligible.len()}))
    })();
    let mut result = match outcome {
        Ok(value) => value,
        Err(error) => {
            let failure = crate::provider::classify_model_request_failure(&error).code;
            let category = if failure != "provider_unknown" || error.contains("TLS") || error.contains("连接") {
                "external_service" } else { "contract_or_runner" };
            json!({"status":"runner_failed","error":error,"failureCategory":category})
        }
    };
    result["isolation"] = json!({"dataDirectory":data,"windows":0});
    result["modelEvaluation"] = json!("live_planning_only_frozen_requirements_independent_inventory");
    result["eligibilityScope"] = json!("genre_eligibility_evaluate_v1");
    save(directory,"planning-result.json",&result)
}
