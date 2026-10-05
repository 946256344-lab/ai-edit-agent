//! 体裁快照与版本化配方：手选不可改，自动只判一次；段数、预算、可读镜长由代码决定。

use super::inventory::{ask_many, Inventory};
use crate::models::{Genre, SegmentEvidence};
use crate::provider::ModelAccess;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

pub(crate) const PIPELINE_VERSION: &str = "footage-first-v1";
pub(crate) const RECIPE_VERSION: &str = "genre-recipe-2026-10-05-v1";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GenreSelection {
    Auto,
    Narrative,
    Promotion,
    Bts,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GenreDecision {
    pub selection: GenreSelection,
    pub genre: Genre,
    pub reason: String,
    pub limited: bool,
    pub snapshot_id: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SectionRole {
    Hook,
    SellingPoint,
    Closing,
    Opening,
    Development,
    Turn,
    Ending,
    Moment,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecipeSection {
    pub id: String,
    pub role: SectionRole,
    pub budget_ms: i64,
    pub min_readable_ms: i64,
    pub max_shots: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GenreRecipe {
    pub version: String,
    pub genre: Genre,
    pub requested_duration_ms: i64,
    pub planned_duration_ms: i64,
    pub limited: bool,
    pub limitations: Vec<String>,
    pub sections: Vec<RecipeSection>,
}

pub(crate) fn decide_genre(
    access: &ModelAccess,
    selection: GenreSelection,
    request: &str,
    inventory: &Inventory,
    previous: Option<&GenreDecision>,
) -> Result<GenreDecision, String> {
    let bytes =
        serde_json::to_vec(&json!({"selection":selection,"request":request,"inventory":inventory}))
            .map_err(|e| e.to_string())?;
    let snapshot_id = format!("genre-{:x}", Sha256::digest(bytes));
    if let Some(previous) = previous {
        if previous.snapshot_id != snapshot_id || previous.selection != selection {
            return Err("genre_snapshot_input_changed".into());
        }
        let manual_genre = match selection {
            GenreSelection::Narrative => Some(Genre::Narrative),
            GenreSelection::Promotion => Some(Genre::Promotion),
            GenreSelection::Bts => Some(Genre::Bts),
            GenreSelection::Auto => None,
        };
        if manual_genre.is_some_and(|genre| genre != previous.genre) {
            return Err("genre_snapshot_overrides_manual_selection".into());
        }
        return Ok(previous.clone());
    }
    let manual = match selection {
        GenreSelection::Narrative => Some(Genre::Narrative),
        GenreSelection::Promotion => Some(Genre::Promotion),
        GenreSelection::Bts => Some(Genre::Bts),
        GenreSelection::Auto => None,
    };
    let (mut genre, mut reason) = if let Some(genre) = manual {
        (genre, "采用用户手选体裁。".to_owned())
    } else if request.trim().is_empty() || inventory.talkable_content.is_empty() {
        (Genre::Promotion, "信息不足，默认宣传体裁。".to_owned())
    } else {
        #[derive(Deserialize)]
        struct Proposal {
            genre: Genre,
            reason: String,
            #[serde(rename = "informationSufficient")]
            information_sufficient: bool,
        }
        let mut response: Vec<Proposal> = ask_many(
            access,
            "genre-once",
            vec![json!({
                "task":"Choose genre from narrative/promotion/bts using the request AND the complete inventory. Narrative requires a causal story; promotion proves visible selling points; bts collects independent real moments. Explain the choice. If information is insufficient set informationSufficient=false; code will default to promotion. This is a single decision, not a repeated routing step.",
                "request":request,"inventory":inventory,"schema":{"genre":"promotion","reason":"","informationSufficient":true}
            })],
        )?;
        let response = response.remove(0);
        if response.reason.trim().is_empty() {
            return Err("genre_missing_reason".into());
        }
        if response.information_sufficient {
            (response.genre, response.reason)
        } else {
            (
                Genre::Promotion,
                format!("信息不足，默认宣传体裁。{}", response.reason),
            )
        }
    };
    if genre == Genre::Narrative && !inventory.causal_chain_complete {
        if selection == GenreSelection::Auto {
            genre = Genre::Bts;
            reason = format!(
                "{} 自动叙事缺完整因果证据，改用花絮时刻集合：{}",
                reason, inventory.causal_reason
            );
        } else {
            reason.push_str(&format!(
                " 手选叙事缺完整因果证据，只交缺口：{}",
                inventory.causal_reason
            ));
        }
    }
    let limited = genre == Genre::Narrative && !inventory.causal_chain_complete;
    Ok(GenreDecision {
        selection,
        genre,
        reason,
        limited,
        snapshot_id,
    })
}

pub(crate) fn build_recipe(
    decision: &GenreDecision,
    duration_ms: Option<i64>,
    eligible: &[SegmentEvidence],
    inventory: &Inventory,
) -> Result<GenreRecipe, String> {
    super::inventory::validate_input(eligible)?;
    if eligible.is_empty() {
        return Err("planning_no_eligible_segments".into());
    }
    let target = duration_ms.unwrap_or(match decision.genre {
        Genre::Bts => 20_000,
        _ => 30_000,
    });
    if target <= 0 {
        return Err("recipe_invalid_duration".into());
    }
    let min_readable_ms = match decision.genre {
        Genre::Narrative => 2_000,
        _ => 1_500,
    };
    let available: i64 = eligible
        .iter()
        .filter(|s| s.range.end_ms - s.range.start_ms >= min_readable_ms)
        .fold(0i64, |sum, s| sum.saturating_add(s.range.end_ms - s.range.start_ms));
    let duration = target.min(available);
    if duration < min_readable_ms {
        return Err("planning_no_readable_eligible_window".into());
    }
    let strengths: Vec<i64> = inventory
        .items
        .iter()
        .filter(|item| eligible.iter().any(|s| super::inventory::item_matches(item, s)))
        .map(|item| item.statements.iter().filter(|s| s.direct).count() as i64)
        .filter(|n| *n > 0)
        .collect();
    let mut strengths = strengths;
    strengths.sort_by(|a, b| b.cmp(a));
    let (roles, weights): (Vec<SectionRole>, Vec<i64>) = match decision.genre {
        Genre::Narrative => {
            if duration < 4 * min_readable_ms {
                return Err("recipe_narrative_too_short_for_evidenced_chain".into());
            }
            (
                vec![
                    SectionRole::Opening,
                    SectionRole::Development,
                    SectionRole::Turn,
                    SectionRole::Ending,
                ],
                vec![20, 35, 25, 20],
            )
        }
        Genre::Promotion => {
            let count = strengths
                .len()
                .clamp(2, 4)
                .min((duration / min_readable_ms).saturating_sub(2).max(1) as usize);
            if duration < 3 * min_readable_ms {
                (vec![SectionRole::SellingPoint], vec![100])
            } else {
                let mut roles = vec![SectionRole::Hook];
                roles.extend(vec![SectionRole::SellingPoint; count]);
                roles.push(SectionRole::Closing);
                // 每个卖点预算按可用直证数量分配，代码确定 15/70/15 总比例。
                let ss: Vec<i64> = (0..count)
                    .map(|i| strengths.get(i).copied().unwrap_or(1))
                    .collect();
                let sum: i64 = ss.iter().sum();
                let mut weights = vec![15 * sum];
                weights.extend(ss.iter().map(|n| 70 * n));
                weights.push(15 * sum);
                (roles, weights)
            }
        }
        Genre::Bts => {
            if duration < 3 * min_readable_ms {
                (vec![SectionRole::Moment], vec![100])
            } else {
                let count = eligible
                    .len()
                    .clamp(1, 3)
                    .min((duration / min_readable_ms).saturating_sub(2).max(1) as usize);
                let mut roles = vec![SectionRole::Opening];
                roles.extend(vec![SectionRole::Moment; count]);
                roles.push(SectionRole::Closing);
                let mut weights = vec![10 * count as i64];
                weights.extend(vec![80; count]);
                weights.push(10 * count as i64);
                (roles, weights)
            }
        }
    };
    let sum: i64 = weights.iter().sum();
    let mut budgets: Vec<i64> = weights.iter().map(|w| (duration as i128 * *w as i128 / sum as i128) as i64).collect();
    // 极短版本优先可读性：从富余段移给不足段，总时钟不变。
    for i in 0..budgets.len() {
        while budgets[i] < min_readable_ms {
            let donor = (0..budgets.len())
                .max_by_key(|j| budgets[*j])
                .ok_or("recipe_missing_budget")?;
            if budgets[donor] <= min_readable_ms {
                return Err("recipe_insufficient_readable_budget".into());
            }
            let transfer = (min_readable_ms - budgets[i]).min(budgets[donor] - min_readable_ms);
            budgets[donor] -= transfer;
            budgets[i] += transfer;
        }
    }
    let remainder = duration - budgets.iter().sum::<i64>();
    *budgets.last_mut().ok_or("recipe_no_sections")? += remainder;
    let sections = roles
        .into_iter()
        .zip(budgets)
        .enumerate()
        .map(|(i, (role, budget_ms))| RecipeSection {
            id: format!("section-{}", i + 1),
            role,
            budget_ms,
            min_readable_ms,
            max_shots: (budget_ms / min_readable_ms) as usize,
        })
        .collect();
    let mut limitations = Vec::new();
    if duration < target {
        limitations.push(format!(
            "合格可读源窗不足目标时长，配方从 {}ms 收缩为 {}ms。",
            target, duration
        ));
    }
    if decision.limited {
        limitations.push(decision.reason.clone());
    }
    Ok(GenreRecipe {
        version: RECIPE_VERSION.into(),
        genre: decision.genre,
        requested_duration_ms: target,
        planned_duration_ms: duration,
        limited: !limitations.is_empty(),
        limitations,
        sections,
    })
}
