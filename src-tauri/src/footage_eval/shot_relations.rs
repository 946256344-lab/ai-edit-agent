//! 无 GUI 的策划→全片组合→证据精修评测；核验、帧缓存和输出只在本次副本写入。
use crate::media_options::AspectRatio;
use crate::models::*;
use crate::storyboard::{eligibility, inventory, phase4, planning, relations};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};
use tauri::AppHandle;

fn save(directory: &Path, name: &str, value: &impl serde::Serialize) -> Result<(), String> {
    fs::write(
        directory.join(name),
        serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// 全部窗内样本按行展示，数字标签单位为毫秒；不复用只保留五帧的整段识别图。
fn compose_window_grid(frames: &[(std::path::PathBuf, String)], output: &Path) -> Option<std::path::PathBuf> {
    if frames.is_empty() || frames.len() > 32 { return None; }
    let columns = 4;
    let rows = (frames.len() as u32).div_ceil(columns);
    let mut sheet = image::RgbImage::from_pixel(columns * 320, rows * 180, image::Rgb([0, 0, 0]));
    for (index, (path, label)) in frames.iter().enumerate() {
        let frame = image::open(path).ok()?.thumbnail(320, 180).to_rgb8();
        let mut cell = image::RgbImage::from_pixel(320, 180, image::Rgb([0, 0, 0]));
        image::imageops::replace(&mut cell, &frame, ((320-frame.width())/2).into(), ((180-frame.height())/2).into());
        crate::storyboard::multimodal::draw_cell_label(&mut cell, label);
        image::imageops::replace(&mut sheet, &cell, (index as u32 % columns * 320).into(), (index as u32 / columns * 180).into());
    }
    sheet.save(output).ok()?;
    Some(output.to_path_buf())
}

pub(super) fn verify(app: &AppHandle, job: &mut Value, directory: &Path) -> Result<(), String> {
    let aspect: AspectRatio = serde_json::from_value(job.get("aspectRatio").cloned().unwrap_or(json!("16:9"))).map_err(|e|e.to_string())?;
    let input: Vec<SegmentEvidence> =
        serde_json::from_value(job["eligibleEvidence"].clone()).map_err(|e| e.to_string())?;
    let selection: crate::storyboard::genre::GenreSelection =
        serde_json::from_value(job["selection"].clone()).map_err(|e| e.to_string())?;
    let initial_inventory: inventory::Inventory = if let Some(value) = job.get("frozenInventory") {
        serde_json::from_value(value.clone()).map_err(|e| e.to_string())?
    } else {
        inventory::Inventory {
            items: vec![],
            talkable_content: vec![],
            gaps: vec![],
            causal_links: vec![],
            causal_chain_complete: false,
            causal_reason: "工业快照尚无经核验的同一事件因果链。".into(),
            request_fulfillable: true,
            coverage_count: 0,
            requirements: vec![],
            rejected_causal_links: vec![],
        }
    };
    let decision = crate::storyboard::genre::decide_genre(
        &super::model_access()?,
        selection,
        job["request"].as_str().ok_or("shot_eval_missing_request")?,
        &initial_inventory,
        None,
    )?;
    let genre = decision.genre;
    job["verifiedGenreDecision"] = json!(decision);
    let project = job["projectId"]
        .as_str()
        .ok_or("shot_eval_project_missing")?;
    let mut requests = Vec::new();
    let mut windows = serde_json::Map::new();
    for e in &input {
        let key = format!("{}:{}", e.asset_id, e.segment_id);
        let range = job
            .get("candidateWindows")
            .and_then(|w| w.get(&key))
            .cloned()
            .map(serde_json::from_value::<EvidenceRange>)
            .transpose()
            .map_err(|e| e.to_string())?
            .unwrap_or_else(|| {
                e.motion_profile
                    .as_ref()
                    .map(|m| EvidenceRange {
                        start_ms: m.usable_start_ms,
                        end_ms: m.usable_end_ms,
                    })
                    .filter(|r| inventory::contains_range(&e.range, r))
                    .unwrap_or_else(|| e.range.clone())
            });
        let decision = eligibility::evaluate(
            e,
            genre,
            aspect,
            &eligibility::BrandIdentity::default(),
            &range,
            &[],
            false,
        );
        let mut required = decision.required_verification;
        if genre == Genre::Bts
            && crate::assets::evidence_contract::risk_state_for_window(
                e,
                RiskKind::EmptyShot,
                &range,
            ) == EvidenceState::Unknown
        {
            required.push(RiskKind::EmptyShot);
        }
        windows.insert(key, json!(range));
        if !required.is_empty()
            && !decision
                .reasons
                .iter()
                .any(|r| r.state == EvidenceState::Hit)
        {
            requests.push(EvidenceVerificationRequest {
                asset_id: e.asset_id.clone(),
                segment_id: e.segment_id.clone(),
                analysis_snapshot_id: e.analysis_snapshot_id.clone(),
                risks: required,
                range: Some(range),
            });
        }
    }
    let results = crate::assets::evidence_verification::verify_candidates(app, project, &requests)?;
    let mut candidates = input.clone();
    for (request, result) in requests.iter().zip(&results) {
        if let Ok(e) = result {
            if let Some(slot) = candidates
                .iter_mut()
                .find(|e| e.asset_id == request.asset_id && e.segment_id == request.segment_id)
            {
                *slot = e.clone();
            }
        }
    }
    job["eligibleEvidence"] = json!(candidates);
    job["candidateWindows"] = Value::Object(windows);
    save(
        directory,
        "verification-result.json",
        &json!({"requests":requests,"results":results,"maxRetries":crate::assets::evidence_verification::MAX_VERIFICATION_RETRIES}),
    )?;
    save(
        directory,
        "verified-candidates.json",
        &job["eligibleEvidence"],
    )
}

fn grids(
    app: &AppHandle,
    directory: &Path,
    evidence: &[SegmentEvidence],
) -> Result<(HashMap<String, Value>, HashMap<String, (i64, i64)>), String> {
    let c = crate::db::open_connection(app)?;
    let mut images = HashMap::new();
    let mut geometry = HashMap::new();
    for e in evidence {
        let (path, raw): (String, String) = c
            .query_row(
                "SELECT source_reference,metadata_json FROM assets WHERE id=?1",
                [&e.asset_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        let m: TechnicalMetadata = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        geometry.insert(
            e.asset_id.clone(),
            (m.width.unwrap_or(0), m.height.unwrap_or(0)),
        );
        let length = e.range.end_ms - e.range.start_ms;
        let n = (length / 700 + 2).clamp(6, 32);
        // seek/select 返回请求时刻之后的帧，尾样本留一帧余量，避免借到下一硬切的画面。
        let frame_ms = m
            .fps
            .filter(|f| f.is_finite() && *f > 0.0)
            .map(|f| (1000.0 / f).ceil() as i64)
            .unwrap_or(100);
        let sample_span = (length - frame_ms - 1).max(0);
        let times: Vec<_> = (0..n)
            .map(|i| e.range.start_ms + i * sample_span / (n - 1))
            .collect();
        let frames = crate::storyboard::multimodal::extract_frames_at_times(
            app,
            &e.asset_id,
            Path::new(&path),
            &times,
            &crate::assets::evidence_contract::content_id(
                "relations-grid",
                &json!({"range":e.range,"segment":e.segment_id}),
            )
            .replace(':', "-"),
        );
        if frames.len() != times.len() {
            continue;
        }
        let labels: Vec<_> = frames
            .iter()
            .map(|(t, p)| (p.clone(), t.to_string()))
            .collect();
        let file = directory.join(format!("grid-{}.jpg", e.id.replace(':', "-")));
        if let Some(grid) = compose_window_grid(&labels, &file)
        .and_then(|p| crate::storyboard::multimodal::read_input_image(&p))
        {
            images.insert(e.id.clone(), grid);
        }
    }
    Ok((images, geometry))
}

#[cfg(test)]
mod grid_regression {
    use super::*;
    #[test]
    fn shot_grid_preserves_every_sample_including_tail() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join(".footage-eval").join(format!("grid-regression-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let mut frames = Vec::new();
        for index in 0..7_u8 {
            let path = directory.join(format!("{index}.png"));
            image::RgbImage::from_pixel(64,36,image::Rgb([index*30,20,40])).save(&path).unwrap();
            frames.push((path,(index as i64 * 1000).to_string()));
        }
        let output = directory.join("grid.png");
        compose_window_grid(&frames,&output).unwrap();
        let grid = image::open(output).unwrap().to_rgb8();
        assert_eq!(grid.dimensions(),(1280,360));
        for index in 0..7_u32 {
            assert_eq!(*grid.get_pixel(index%4*320+160,index/4*180+90),image::Rgb([index as u8*30,20,40]));
        }
    }
}

pub(super) fn run(app: &AppHandle, job: &Value, directory: &Path) -> Result<(), String> {
    let value: Value = serde_json::from_slice(
        &fs::read(directory.join("planning-result.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if value["result"]["proposal"].is_null() {
        return save(
            directory,
            "shot-result.json",
            &json!({"status":"no_plan","planningStatus":value["result"]["status"],"error":value["error"],"metrics":null}),
        );
    }
    let plan: planning::PlanningResult =
        serde_json::from_value(value["result"].clone()).map_err(|e| e.to_string())?;
    let eligible: Vec<SegmentEvidence> = serde_json::from_slice(
        &fs::read(directory.join("eligible-evidence.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let slots = relations::slots(&plan, &eligible)?;
    let ids: HashSet<_> = slots
        .iter()
        .flat_map(|s| s.candidates.iter())
        .map(|r| r.evidence_id.clone())
        .collect();
    let references: Vec<_> = eligible.iter().filter(|e| ids.contains(&e.id)).map(|e| {
        slots.iter().flat_map(|s| &s.candidates).find(|r| r.evidence_id == e.id).unwrap().clone()
    }).collect();
    let evidence: Vec<_> = eligible
        .iter()
        .filter(|e| ids.contains(&e.id))
        .map(|e| {
            let mut window = e.clone();
            window.range = references.iter().find(|r| r.evidence_id == e.id).unwrap().range.clone();
            window
        })
        .collect();
    let (images, geometry) = grids(app, directory, &evidence)?;
    let access = super::model_access()?;
    let (fits, mut model_errors) =
        relations::score_slots(&access, &slots, &images, plan.genre.genre);
    let (pairs, pair_errors) = relations::observe_pairs(&access, &references, &images);
    model_errors.extend(pair_errors);
    save(
        directory,
        "relations-input.json",
        &json!({"slots":slots,"fits":fits,"pairs":pairs,"modelErrors":model_errors}),
    )?;
    let aspect: AspectRatio =
        serde_json::from_value(job.get("aspectRatio").cloned().unwrap_or(json!("16:9")))
            .map_err(|e| e.to_string())?;
    let outcome = (|| -> Result<Value, String> {
        let combination = relations::combine(
            &plan,
            &eligible,
            &slots,
            &fits,
            &pairs,
            aspect,
            &eligibility::BrandIdentity::default(),
            &[],
        )?;
        save(directory, "combination.json", &combination)?;
        let mut session = phase4::evidence::EvidenceRefinementSession::default();
        let mut errors = session.refine(
            &access,
            &combination.shots,
            &eligible,
            &images,
            &geometry,
            aspect,
        );
        let mut refined: Vec<_> = combination
            .shots
            .iter()
            .filter_map(|s| session.completed.get(&s.slot))
            .cloned()
            .collect();
        save(directory, "refined-shots.json", &refined)?;
        let mut final_relation_errors = Vec::new();
        let mut pair_cache = pairs.clone();
        for repair in 0..=phase4::evidence::MAX_WINDOW_REPAIRS {
            if refined.len() != combination.shots.len() {
                break;
            }
            let final_evidence: Vec<_> = refined
                .iter()
                .map(|shot| {
                    let mut e = eligible
                        .iter()
                        .find(|e| e.id == shot.selected.reference.evidence_id)
                        .unwrap()
                        .clone();
                    e.range = shot.source_range.clone();
                    e
                })
                .collect();
            let (final_images, _) = grids(app, directory, &final_evidence)?;
            let final_refs: Vec<_> = refined
                .iter()
                .map(|s| {
                    let mut r = s.selected.reference.clone();
                    r.range = s.source_range.clone();
                    r
                })
                .collect();
            let (final_pairs, final_errors) =
                relations::observe_pairs_cached(&access, &final_refs, &final_images, &pair_cache);
            let observation_failed = !final_errors.is_empty();
            let final_selections: Vec<_> = refined
                .iter()
                .map(|s| {
                    let mut selected = s.selected.clone();
                    selected.reference.range = s.source_range.clone();
                    selected
                })
                .collect();
            final_relation_errors = relations::validate_sequence(
                plan.genre.genre,
                &final_selections,
                &final_pairs,
                &eligible,
                aspect,
                &eligibility::BrandIdentity::default(),
            );
            model_errors.extend(final_errors);
            save(
                directory,
                "final-relations.json",
                &json!({"pairs":final_pairs,"errors":final_relation_errors,"repair":repair}),
            )?;
            save(
                directory,
                &format!("final-relations-{repair}.json"),
                &json!({"pairs":final_pairs,"errors":final_relation_errors}),
            )?;
            // ask_visual 已耗尽响应/传输预算；不能借修窗为失败请求重新开启 429/传输预算。
            if observation_failed
                || final_relation_errors.is_empty()
                || repair == phase4::evidence::MAX_WINDOW_REPAIRS
            {
                break;
            }
            // 每条关系错误只修后镜；其他镜头与未改变的邻镜事实留在 session/cache。
            let feedback: Vec<_> = final_relation_errors
                .iter()
                .filter_map(|error| {
                    error
                        .rsplit(':')
                        .next()?
                        .parse::<usize>()
                        .ok()
                        .map(|slot| (slot, error.clone()))
                })
                .collect();
            session.retry_affected(&feedback);
            pair_cache = final_pairs;
            errors = session.refine(
                &access,
                &combination.shots,
                &eligible,
                &images,
                &geometry,
                aspect,
            );
            refined = combination
                .shots
                .iter()
                .filter_map(|s| session.completed.get(&s.slot))
                .cloned()
                .collect();
            save(directory, "refined-shots.json", &refined)?;
        }
        Ok(
            json!({"status":if !errors.is_empty(){"partial"}else if !final_relation_errors.is_empty(){"relation_recheck_failed"}else{"refined"},
            "combination":combination,"refinementErrors":errors,"modelErrors":model_errors,"finalRelationErrors":final_relation_errors,
            "metricsAuthority":"shot-metrics.json independently computed from saved windows, facts and SQLite"}),
        )
    })();
    save(
        directory,
        "shot-result.json",
        &match outcome {
            Ok(v) => v,
            Err(e) => {
                json!({"status":"combination_failed","error":e,"metrics":null,"modelErrors":model_errors})
            }
        },
    )
}
