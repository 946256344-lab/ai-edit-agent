//! 隔离策划评测：只运行盘点→体裁→配方→策划，不调用 Agent、选镜、时间线或渲染。
use crate::models::SegmentEvidence;
use crate::storyboard::{genre, inventory, planning};
use serde_json::{json, Value};
use std::{fs, path::Path};

pub(super) fn run(job: &Value, directory: &Path, data: &Path) -> Result<(), String> {
    let input: Vec<SegmentEvidence> =
        serde_json::from_value(job["evidence"].clone()).map_err(|e| e.to_string())?;
    let eligible: Vec<SegmentEvidence> =
        serde_json::from_value(job["eligibleEvidence"].clone()).map_err(|e| e.to_string())?;
    if input.iter().chain(&eligible).any(|e| e.id != crate::assets::evidence_contract::seal_segment(e.clone()).id) {
        return Err("planning_eval_unsealed_evidence".into());
    }
    let selection: genre::GenreSelection =
        serde_json::from_value(job["selection"].clone()).map_err(|e| e.to_string())?;
    let request = job["request"]
        .as_str()
        .ok_or("planning_eval_missing_request")?;
    let facts: Vec<planning::UserFact> =
        serde_json::from_value(job.get("userFacts").cloned().unwrap_or(json!([])))
            .map_err(|e| e.to_string())?;
    let access = super::model_access()?;
    let outcome = (|| -> Result<Value, String> {
        let inventory = inventory::build_inventory(&access, request, &input)?;
        fs::write(
            directory.join("inventory.json"),
            serde_json::to_vec_pretty(&inventory).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let decision = genre::decide_genre(&access, selection, request, &inventory, None)?;
        let saved = genre::decide_genre(&access, selection, request, &inventory, Some(&decision))?;
        fs::write(
            directory.join("genre.json"),
            serde_json::to_vec_pretty(&saved).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let recipe =
            genre::build_recipe(&decision, job["durationMs"].as_i64(), &eligible, &inventory)?;
        fs::write(
            directory.join("recipe.json"),
            serde_json::to_vec_pretty(&recipe).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let result = planning::plan_with_evidence(
            &access, request, &inventory, &decision, &recipe, &eligible, &facts,
        )?;
        Ok(json!({"result":result}))
    })();
    let mut result = match outcome {
        Ok(value) => value,
        Err(error) => json!({"status":"runner_failed","error":error}),
    };
    result["isolation"] = json!({"dataDirectory":data,"windows":0});
    result["modelEvaluation"] = json!("live_planning_only");
    result["eligibilityScope"] = job["eligibilityScope"].clone();
    fs::write(
        directory.join("planning-result.json"),
        serde_json::to_vec_pretty(&result).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
