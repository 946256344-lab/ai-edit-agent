//! Agent 请求的路由、状态快照、有界模型循环与技能执行器。
//!
//! 纯请求策略和真实产物完成门位于 `agentloop/policy.rs`；路由决策与有界循环位于
//! `agentloop/runtime.rs`；状态快照与提示构建位于 `agentloop/prompt.rs`；技能执行
//! 与参数校验位于 `agentloop/skills.rs`；首批原生 Function Tool 定义位于
//! `agentloop/tools.rs`。本文件只负责模块声明、公开接口重导出和测试。

mod policy;
mod prompt;
mod runtime;
mod schema;
mod skills;
mod tools;

// 重导出公开接口供外部模块使用
pub(crate) use runtime::{
    decide_conversation_route, run_agent_loop, run_agent_loop_with_initial_skill,
    run_explicit_command, ConversationRouteDecision,
};
pub(crate) use schema::InitialAgentSkill;
pub(crate) use skills::read_scoped_edit_status;

#[cfg(test)]
mod tests {
    use super::policy::*;
    use super::prompt::{project_fact_completion_instruction, unmet_conditions};
    use super::runtime::{
        clarification_resolution, finalize_result, finalize_terminal, first_model_step,
        question_scope_allows_route,
    };
    use super::schema::*;
    use super::skills::should_redirect_storyboard_after_failed_generation;
    use crate::models::{AgentEditResult, PendingClarificationSnapshot};
    use serde_json::json;

    #[test]
    fn storyboard_generation_failure_redirects_without_reasking_for_a_brief() {
        assert!(should_redirect_storyboard_after_failed_generation(
            LoopGoal::Storyboard,
            Some("skill_execution_failed")
        ));
        assert!(!should_redirect_storyboard_after_failed_generation(
            LoopGoal::Storyboard,
            Some("missing_or_invalid_prerequisite")
        ));
    }

    fn test_snapshot(goal: &str) -> AgentStateSnapshot {
        AgentStateSnapshot {
            scope: AgentScopeSnapshot {
                project_id: "project-1".to_owned(),
                editing_task_id: "task-1".to_owned(),
                conversation_id: "conversation-1".to_owned(),
            },
            assets: AssetAvailabilitySnapshot {
                total_count: 2,
                usable_count: 2,
                pending_analysis_count: 0,
                failed_analysis_count: 0,
                unavailable_source_count: 0,
            },
            artifacts: ArtifactPresenceSnapshot {
                storyboard: VersionArtifactSnapshot {
                    exists: false,
                    version_id: None,
                    version_number: None,
                },
                timeline: VersionArtifactSnapshot {
                    exists: false,
                    version_id: None,
                    version_number: None,
                },
                preview: TimelineArtifactSnapshot {
                    exists: false,
                    timeline_version_id: None,
                },
                jianying_draft: JianyingArtifactSnapshot {
                    exists: false,
                    timeline_version_id: None,
                    registration_status: None,
                },
            },
            executed_steps: vec![ExecutedStepSummary {
                step_number: 1,
                tool: "list_assets".to_owned(),
                status: "succeeded".to_owned(),
                produced_artifact: None,
            }],
            remaining_steps: 5,
            goal: goal.to_owned(),
            pending_clarification: None,
            unmet_conditions: Vec::new(),
        }
    }

    #[test]
    fn step_args_removes_meta_keys() {
        let raw = json!({
            "goal": "timeline",
            "isQuestion": false,
            "tool": "replace_clips",
            "reason": "swap",
            "taskBrief": "new goal",
            "clarificationAction": "resolve",
            "informationScope": "project",
            "shots": [{"shotIndex": 1, "assetId": "a", "sourceStartMs": 0, "sourceEndMs": 2000}]
        });
        let args = step_args(&raw);
        assert!(args.get("tool").is_none());
        assert!(args.get("goal").is_none());
        assert!(args.get("isQuestion").is_none());
        assert!(args.get("reason").is_none());
        assert!(args.get("taskBrief").is_none());
        assert!(args.get("clarificationAction").is_none());
        assert!(args.get("informationScope").is_none());
        assert!(args.get("shots").is_some());
    }

    #[test]
    fn step_args_survives_non_object_decisions() {
        let args = step_args(&json!(["not", "an", "object"]));
        assert!(args.is_array());
    }

    #[test]
    fn finalize_result_keeps_the_last_concrete_outcome() {
        let result = AgentEditResult {
            agent_task_id: "task-1".to_owned(),
            message: "done".to_owned(),
            storyboard: None,
            timeline: None,
            preview: None,
            jianying_draft: None,
        };
        let finalized = finalize_result("task-1", Some(result), "fallback");
        assert_eq!(finalized.message, "done");
        let empty = finalize_result("task-1", None, "fallback");
        assert_eq!(empty.message, "fallback");
        assert!(empty.storyboard.is_none());
    }

    #[test]
    fn fast_goal_pins_unambiguous_requests() {
        assert_eq!(fast_goal("请生成一个新的预览"), Some(LoopGoal::Preview));
        assert_eq!(fast_goal("把镜头1换成视频素材"), Some(LoopGoal::Timeline));
        assert_eq!(fast_goal("不要这么多警报的镜头"), Some(LoopGoal::Timeline));
        assert_eq!(fast_goal("创建剪映草稿"), Some(LoopGoal::JianyingDraft));
        assert_eq!(fast_goal("生成一个分镜脚本"), Some(LoopGoal::Storyboard));
        assert_eq!(
            fast_goal("你好，介绍一下这个项目"),
            Some(LoopGoal::Question)
        );
    }

    #[test]
    fn explicit_negative_side_effects_narrow_the_tool_set_and_goal() {
        let request = "仅调整当前内部时间线：缩短第 2 个镜头，不生成 preview，不创建 Jianying draft，不分析素材。";
        let policy = RequestToolPolicy::from_request(request);

        assert!(policy.forbids("render_preview"));
        assert!(policy.forbids("create_jianying_draft"));
        assert!(policy.forbids("request_asset_analysis"));
        assert!(policy.forbids("download_music"));
        assert!(policy.forbids("use_online_music"));
        assert!(!policy.forbids("change_clip_duration"));
        assert!(policy.forbids_goal(LoopGoal::Preview));
        assert!(!policy.forbids_goal(LoopGoal::Timeline));
        assert_eq!(fast_goal(request), Some(LoopGoal::Timeline));
    }

    #[test]
    fn positive_preview_requests_remain_available() {
        let policy = RequestToolPolicy::from_request("请为当前时间线生成 preview");
        assert!(!policy.forbids("render_preview"));
        assert_eq!(
            fast_goal("请为当前时间线生成 preview"),
            Some(LoopGoal::Preview)
        );
        assert!(
            !RequestToolPolicy::from_request("No preview exists; please generate one")
                .forbids("render_preview")
        );
        assert!(
            RequestToolPolicy::from_request("Do not generate a preview").forbids("render_preview")
        );
        assert!(
            RequestToolPolicy::from_request("Without creating a preview, adjust the timeline")
                .forbids("render_preview")
        );
        assert!(RequestToolPolicy::from_request("Don't render a preview").forbids("render_preview"));
        let no_analysis = RequestToolPolicy::from_request("Do not analyze media or assets");
        assert!(no_analysis.forbids("request_asset_analysis"));
        assert!(no_analysis.forbids("download_music"));
        assert!(no_analysis.forbids("use_online_music"));
        assert!(RequestToolPolicy::from_request("Do not reanalyze assets")
            .forbids("request_asset_analysis"));
    }

    #[test]
    fn explicit_read_only_requests_block_every_edit_tool() {
        let request = "只读检查当前 timeline 版本，不生成 preview，也不要修改任何产物。";
        let policy = RequestToolPolicy::from_request(request);

        assert!(policy.read_only);
        assert!(EDIT_TOOLS.iter().all(|tool| policy.forbids(tool)));
        assert!(!policy.forbids("get_timeline"));
        assert!(policy.forbids_goal(LoopGoal::Storyboard));
        assert!(policy.forbids_goal(LoopGoal::Timeline));
        assert!(policy.forbids_goal(LoopGoal::Preview));
        assert!(policy.forbids_goal(LoopGoal::JianyingDraft));
        assert!(!policy.forbids_goal(LoopGoal::Question));
        assert_eq!(fast_goal(request), Some(LoopGoal::Question));
        assert!(!RequestToolPolicy::from_request("不是只读，请调整 timeline").read_only);
        assert_eq!(
            fast_goal("不是只读，请调整 timeline"),
            Some(LoopGoal::Timeline)
        );
        let english_edit = "This isn't readonly; adjust the timeline";
        assert!(!RequestToolPolicy::from_request(english_edit).read_only);
        assert_eq!(fast_goal(english_edit), Some(LoopGoal::Timeline));
        let chinese_mode_edit = "不要用只读模式，请调整 timeline";
        assert!(!RequestToolPolicy::from_request(chinese_mode_edit).read_only);
        assert_eq!(fast_goal(chinese_mode_edit), Some(LoopGoal::Timeline));
        let english_mode_edit = "Don't use readonly mode; adjust the timeline";
        assert!(!RequestToolPolicy::from_request(english_mode_edit).read_only);
        assert_eq!(fast_goal(english_mode_edit), Some(LoopGoal::Timeline));
        for request in [
            "This is not in readonly mode; adjust the timeline",
            "Don't keep it readonly; adjust the timeline",
            "不要保持只读模式，请调整 timeline",
        ] {
            assert!(!RequestToolPolicy::from_request(request).read_only);
            assert_eq!(fast_goal(request), Some(LoopGoal::Timeline));
        }
        for request in [
            "Keep the current timeline readonly",
            "保持当前时间线只读",
            "Don't keep the intro; keep the current timeline readonly",
            "不要保持片头；保持当前时间线只读",
            "Don't keep the intro: keep the current timeline readonly",
            "不要保持片头：保持当前时间线只读",
            "Don't keep the intro — keep the current timeline readonly",
        ] {
            let policy = RequestToolPolicy::from_request(request);
            assert!(policy.read_only);
            assert!(EDIT_TOOLS.iter().all(|tool| policy.forbids(tool)));
            assert_eq!(fast_goal(request), Some(LoopGoal::Question));
        }
    }

    #[test]
    fn current_project_questions_require_observation_when_routing_falls_back() {
        assert!(request_requires_project_observation(
            "只读检查当前 timeline 是 v几、包含多少片段？"
        ));
        assert!(request_requires_project_observation(
            "How many clips are in the current timeline?"
        ));
        assert_eq!(fast_goal("当前 preview 状态"), None);
        assert_eq!(fast_goal("当前 timeline 版本"), None);
        assert_eq!(
            fast_goal("Adjust the current timeline"),
            Some(LoopGoal::Timeline)
        );
        assert_eq!(
            fast_goal("Shorten the current clip"),
            Some(LoopGoal::Timeline)
        );
        assert_eq!(
            fast_goal("Render the current preview"),
            Some(LoopGoal::Preview)
        );
        assert_eq!(fast_goal("Update the current timeline"), None);
        assert_eq!(fast_goal("Modify the current clip"), None);
        assert_eq!(fast_goal("Extend the current clip"), None);
        assert!(!request_requires_project_observation(
            "请解释 timeline 是什么？"
        ));
    }

    #[test]
    fn pinned_goal_allows_response_is_always_true() {
        // fast_goal 已降级为提示；纠偏逻辑在 try_build_route_decision 处理。
        assert!(pinned_goal_allows_response(None));
        assert!(pinned_goal_allows_response(Some(LoopGoal::Question)));
        assert!(pinned_goal_allows_response(Some(LoopGoal::Preview)));
        assert!(pinned_goal_allows_response(Some(LoopGoal::Timeline)));
    }

    #[test]
    fn project_questions_cannot_bypass_observation_with_respond() {
        assert!(question_scope_allows_route(Some("general"), "respond"));
        assert!(question_scope_allows_route(Some("general"), "run"));
        assert!(question_scope_allows_route(Some("project"), "run"));
        assert!(!question_scope_allows_route(Some("project"), "respond"));
        assert!(!question_scope_allows_route(None, "respond"));
    }

    #[test]
    fn grounded_project_question_finishes_without_redundant_confirmation() {
        let instruction = project_fact_completion_instruction(true, true);
        assert!(instruction.contains("choose finish now"));
        assert!(instruction.contains("Do not call a semantically overlapping observation tool"));
        assert!(!project_fact_completion_instruction(true, false).contains("choose finish now"));
    }

    #[test]
    fn clarification_resolution_targets_the_observed_record() {
        let pending = PendingClarificationSnapshot {
            id: "clarification-1".to_owned(),
            source_kind: "router".to_owned(),
            source_agent_task_id: None,
            goal: Some("storyboard".to_owned()),
            question: "请补充目标。".to_owned(),
            created_at: 1,
        };
        assert_eq!(
            clarification_resolution(Some(&pending), Some("resolve"))
                .expect("resolve clarification"),
            Some("clarification-1".to_owned())
        );
        assert_eq!(
            clarification_resolution(Some(&pending), Some("keep")).expect("keep clarification"),
            None
        );
        assert!(clarification_resolution(Some(&pending), None).is_err());
    }

    #[test]
    fn an_attempted_initial_skill_advances_the_next_model_step() {
        assert_eq!(first_model_step(&[]), 0);
        let attempted = vec![ExecutedStepSummary {
            step_number: 1,
            tool: "generate_storyboard".to_owned(),
            status: "failed".to_owned(),
            produced_artifact: None,
        }];
        assert_eq!(first_model_step(&attempted), 1);
    }

    #[test]
    fn fast_goal_answers_questions_instead_of_forcing_edits() {
        assert_eq!(
            fast_goal("请告诉我选择每个镜头的逻辑"),
            Some(LoopGoal::Question)
        );
        assert_eq!(fast_goal("草稿为什么没出现"), Some(LoopGoal::Question));
        assert_eq!(fast_goal("为什么预览是黑的"), Some(LoopGoal::Question));
    }

    #[test]
    fn fast_goal_leaves_ambiguous_requests_for_the_model() {
        assert_eq!(fast_goal("你好"), None);
        assert_eq!(fast_goal("怎么把镜头1换成另一个素材"), None);
    }

    #[test]
    fn declared_goal_prefers_a_truthful_question_flag() {
        assert_eq!(
            parse_declared_goal(Some("timeline"), Some(true)),
            Some(LoopGoal::Question)
        );
        assert_eq!(
            parse_declared_goal(Some("timeline"), Some(false)),
            Some(LoopGoal::Timeline)
        );
        assert_eq!(
            parse_declared_goal(Some("preview"), None),
            Some(LoopGoal::Preview)
        );
        assert_eq!(
            parse_declared_goal(Some("storyboard"), Some(false)),
            Some(LoopGoal::Storyboard)
        );
    }

    #[test]
    fn finalize_terminal_returns_real_artifact_messages() {
        let existing = AgentEditResult {
            agent_task_id: "task-1".to_owned(),
            message: "已创建 storyboard v1。".to_owned(),
            storyboard: None,
            timeline: None,
            preview: None,
            jianying_draft: None,
        };
        let finalized = finalize_terminal(
            "task-1",
            LoopGoal::Storyboard,
            Some(existing),
            "model says: done",
        );
        assert_eq!(finalized.message, "已创建 storyboard v1。");
        let question_answer = finalize_terminal(
            "task-1",
            LoopGoal::Question,
            None,
            "The model is currently unavailable.",
        );
        assert_eq!(
            question_answer.message,
            "The model is currently unavailable."
        );
        let no_artifact = finalize_terminal("task-1", LoopGoal::Preview, None, "done");
        assert!(no_artifact.message.contains("没"));
    }

    #[test]
    fn unmet_conditions_highlights_missing_prerequisites() {
        let conditions = unmet_conditions(
            LoopGoal::Preview,
            &AssetAvailabilitySnapshot {
                total_count: 0,
                usable_count: 0,
                pending_analysis_count: 0,
                failed_analysis_count: 0,
                unavailable_source_count: 0,
            },
            &ArtifactPresenceSnapshot {
                storyboard: VersionArtifactSnapshot {
                    exists: false,
                    version_id: None,
                    version_number: None,
                },
                timeline: VersionArtifactSnapshot {
                    exists: false,
                    version_id: None,
                    version_number: None,
                },
                preview: TimelineArtifactSnapshot {
                    exists: false,
                    timeline_version_id: None,
                },
                jianying_draft: JianyingArtifactSnapshot {
                    exists: false,
                    timeline_version_id: None,
                    registration_status: None,
                },
            },
            false,
            false,
        );
        assert!(conditions
            .iter()
            .any(|condition| condition.contains("imported_media")));
        assert!(conditions
            .iter()
            .any(|condition| condition.contains("storyboard")));
        assert!(conditions
            .iter()
            .any(|condition| condition.contains("timeline")));
    }
}
