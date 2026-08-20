//! timeline.rs 的独立测试模块：镜头、文字和音乐编辑回归，通过 #[path] 挂载。
use super::*;
use crate::models::{TextAnimation, TextCue, TextLayout, TextStyle};

fn text_cue(
    font_key: &str,
    entrance: Option<TextAnimation>,
    exit: Option<TextAnimation>,
) -> TextCue {
    TextCue {
        id: "cue-1".to_owned(),
        template_id: None,
        start_ms: 0,
        end_ms: 2_000,
        text: "Verified text".to_owned(),
        style: TextStyle {
            font_key: font_key.to_owned(),
            font_size: 0.06,
            bold: true,
            color: "#FFFFFF".to_owned(),
            stroke_color: None,
            stroke_width: 0.0,
            shadow: false,
            background_color: None,
            alignment: "center".to_owned(),
            letter_spacing: 0,
            line_spacing: 0,
        },
        layout: TextLayout {
            anchor: "bottom".to_owned(),
            x: 0.5,
            y: 0.82,
            max_width: 0.8,
            safe_area: "title_safe".to_owned(),
        },
        entrance,
        exit,
        loop_animation: None,
        jianying_compatibility: "verified".to_owned(),
    }
}

#[test]
fn text_compatibility_is_assigned_by_the_verified_delivery_matrix() {
    let fade = TextAnimation {
        template_id: "fade".to_owned(),
        duration_ms: 250,
        intensity: 0.6,
    };
    let slide_down = TextAnimation {
        template_id: "slide_down".to_owned(),
        duration_ms: 250,
        intensity: 0.6,
    };
    let mut tracks = vec![TextTrack {
        id: "text-1".to_owned(),
        role: "subtitle".to_owned(),
        layer: 1,
        enabled: true,
        origin: "storyboard_generated".to_owned(),
        generation_id: None,
        editable: true,
        locked: false,
        cues: vec![
            text_cue("jianying_default", Some(fade), None),
            text_cue("sans_bold", None, None),
            text_cue("jianying_default", Some(slide_down), None),
        ],
    }];
    tracks[0].cues[1].id = "cue-2".to_owned();
    tracks[0].cues[2].id = "cue-3".to_owned();
    tracks[0].cues[1].start_ms = 2_000;
    tracks[0].cues[1].end_ms = 4_000;
    tracks[0].cues[2].start_ms = 4_000;
    tracks[0].cues[2].end_ms = 6_000;

    validate_text_tracks(&mut tracks, 6_000).expect("validate text tracks");

    assert_eq!(tracks[0].cues[0].jianying_compatibility, "verified");
    assert_eq!(
        tracks[0].cues[1].jianying_compatibility,
        "local_preview_only"
    );
    assert_eq!(tracks[0].cues[2].jianying_compatibility, "verified");
}

#[test]
fn validated_pop_entrance_and_fade_exit_are_jianying_deliverable() {
    let mut cue = text_cue(
        "jianying_default",
        Some(TextAnimation {
            template_id: "pop".to_owned(),
            duration_ms: 250,
            intensity: 0.7,
        }),
        Some(TextAnimation {
            template_id: "fade".to_owned(),
            duration_ms: 250,
            intensity: 0.7,
        }),
    );
    cue.id = "cue-pop-out".to_owned();
    let mut tracks = vec![TextTrack {
        id: "text-1".to_owned(),
        role: "headline".to_owned(),
        layer: 1,
        enabled: true,
        origin: "storyboard_generated".to_owned(),
        generation_id: None,
        editable: true,
        locked: false,
        cues: vec![cue],
    }];

    validate_text_tracks(&mut tracks, 2_000).expect("validate confirmed dynamics");

    assert_eq!(tracks[0].cues[0].jianying_compatibility, "verified");
}

#[test]
fn unicode_text_uses_the_verified_jianying_delivery_matrix() {
    let mut cue = text_cue("jianying_default", None, None);
    cue.text = "\u{5b57}\u{5e55}".to_owned();
    let mut tracks = vec![TextTrack {
        id: "text-1".to_owned(),
        role: "subtitle".to_owned(),
        layer: 1,
        enabled: true,
        origin: "storyboard_generated".to_owned(),
        generation_id: None,
        editable: true,
        locked: false,
        cues: vec![cue],
    }];

    validate_text_tracks(&mut tracks, 2_000).expect("validate Unicode cue");

    assert_eq!(tracks[0].cues[0].jianying_compatibility, "verified");
}

#[test]
fn text_recipe_overrides_conflicting_model_style_with_a_verified_delivery_recipe() {
    let mut cue = text_cue(
        "sans_clean",
        Some(TextAnimation {
            template_id: "pop".to_owned(),
            duration_ms: 400,
            intensity: 1.0,
        }),
        Some(TextAnimation {
            template_id: "fade".to_owned(),
            duration_ms: 400,
            intensity: 1.0,
        }),
    );
    cue.template_id = Some("headline_rise".to_owned());
    let mut tracks = vec![TextTrack {
        id: "text-1".to_owned(),
        role: "headline".to_owned(),
        layer: 1,
        enabled: true,
        origin: "storyboard_generated".to_owned(),
        generation_id: None,
        editable: true,
        locked: false,
        cues: vec![cue],
    }];

    validate_text_tracks(&mut tracks, 2_000).expect("apply text recipe");

    let resolved = &tracks[0].cues[0];
    assert_eq!(resolved.style.font_key, "jianying_default");
    assert_eq!(resolved.layout.anchor, "center");
    assert_eq!(
        resolved
            .entrance
            .as_ref()
            .map(|animation| animation.template_id.as_str()),
        Some("slide_up")
    );
    assert_eq!(
        resolved
            .exit
            .as_ref()
            .map(|animation| animation.template_id.as_str()),
        Some("fade")
    );
    assert_eq!(resolved.jianying_compatibility, "verified");
}

#[test]
fn legacy_text_cue_without_template_id_still_deserializes() {
    let cue: TextCue = serde_json::from_str(
        r##"{
            "id":"cue-1","startMs":0,"endMs":1000,"text":"Legacy text",
            "style":{"fontKey":"jianying_default","fontSize":0.06,"bold":true,"color":"#FFFFFF","strokeColor":null,"strokeWidth":0.0,"shadow":false,"backgroundColor":null,"alignment":"center","letterSpacing":0,"lineSpacing":0},
            "layout":{"anchor":"bottom","x":0.5,"y":0.82,"maxWidth":0.8,"safeArea":"title_safe"},
            "entrance":null,"exit":null,"loopAnimation":null,"jianyingCompatibility":"verified"
        }"##,
    )
    .expect("load legacy text cue");

    assert!(cue.template_id.is_none());
}

#[test]
fn legacy_timeline_content_defaults_music_tracks_and_preserves_music_cues() {
    let legacy: TimelineContent = serde_json::from_str(r#"{"clips":[],"textTracks":[]}"#)
        .expect("parse legacy timeline content");
    assert!(legacy.music_tracks.is_empty());
    let content: TimelineContent = serde_json::from_str(r#"{"clips":[],"textTracks":[],"musicTracks":[{"id":"music-1","enabled":true,"cues":[{"id":"cue-1","assetId":"audio-1","sourceStartMs":0,"sourceEndMs":1000,"timelineStartMs":0,"timelineEndMs":3000,"loopEnabled":true,"volume":0.4,"fadeInMs":120,"fadeOutMs":120}]}]}"#)
        .expect("parse music timeline content");
    assert!(content.music_tracks[0].cues[0].loop_enabled);
    assert_eq!(
        content.music_tracks[0].cues[0].jianying_compatibility,
        "not_deliverable"
    );
}

#[test]
fn minimal_agent_text_cue_uses_safe_defaults_before_validation() {
    let mut cue: TextCue = serde_json::from_str(
        r#"{"id":"cue-1","templateId":"subtitle_safe","startMs":0,"endMs":1000,"text":"Minimal cue"}"#,
    )
    .expect("parse minimal agent cue");
    let mut tracks = vec![TextTrack {
        id: "text-1".to_owned(),
        role: "subtitle".to_owned(),
        layer: 1,
        enabled: true,
        origin: "storyboard_generated".to_owned(),
        generation_id: None,
        editable: true,
        locked: false,
        cues: vec![cue.clone()],
    }];

    validate_text_tracks(&mut tracks, 1_000).expect("resolve minimal text cue");
    cue = tracks[0].cues[0].clone();
    assert_eq!(cue.style.font_key, "jianying_default");
    assert_eq!(cue.layout.anchor, "bottom");
    assert_eq!(cue.jianying_compatibility, "verified");
}

#[test]
fn every_advertised_text_recipe_is_accepted_by_the_text_track_validator() {
    for recipe in text_recipe_capabilities() {
        let template_id = recipe["templateId"].as_str().expect("recipe ID");
        let role = recipe["role"].as_str().expect("recipe role");
        let mut cue = text_cue("sans_clean", None, None);
        cue.template_id = Some(template_id.to_owned());
        let mut tracks = vec![TextTrack {
            id: format!("track-{template_id}"),
            role: role.to_owned(),
            layer: 1,
            enabled: true,
            origin: "storyboard_generated".to_owned(),
            generation_id: None,
            editable: true,
            locked: false,
            cues: vec![cue],
        }];

        validate_text_tracks(&mut tracks, 2_000).unwrap_or_else(|error| {
            panic!("recipe {template_id} was advertised but rejected: {error}")
        });
    }
}

#[test]
fn text_recipe_capabilities_expose_a_semantic_selection_hint() {
    for recipe in text_recipe_capabilities() {
        assert!(recipe["selectionHint"]
            .as_str()
            .is_some_and(|hint| !hint.trim().is_empty()));
    }
    let headline_pop = text_recipe_capabilities()
        .into_iter()
        .find(|recipe| recipe["templateId"] == "headline_pop")
        .expect("headline pop recipe");
    assert!(headline_pop["selectionHint"]
        .as_str()
        .expect("headline pop hint")
        .contains("surprise"));
}

#[test]
fn text_track_quality_warns_about_readability_and_animation_without_rejecting_the_cue() {
    let mut cue = text_cue(
        "jianying_default",
        Some(TextAnimation {
            template_id: "fade".to_owned(),
            duration_ms: 600,
            intensity: 0.5,
        }),
        Some(TextAnimation {
            template_id: "fade".to_owned(),
            duration_ms: 500,
            intensity: 0.5,
        }),
    );
    cue.end_ms = 1_000;
    cue.text = "这是一段在一秒钟内无法舒适读完的过长文本内容".to_owned();
    let tracks = vec![TextTrack {
        id: "text-1".to_owned(),
        role: "subtitle".to_owned(),
        layer: 1,
        enabled: true,
        origin: "storyboard_generated".to_owned(),
        generation_id: None,
        editable: true,
        locked: false,
        cues: vec![cue],
    }];
    let warnings = text_track_quality_warnings(&tracks);
    assert!(warnings
        .iter()
        .any(|warning| warning.ends_with("readability_density")));
    assert!(warnings
        .iter()
        .any(|warning| warning.ends_with("animation_dominates_cue")));
}

#[test]
fn headline_cues_cannot_overlap_across_tracks() {
    let first = text_cue("jianying_default", None, None);
    let mut second = text_cue("jianying_default", None, None);
    second.id = "cue-2".to_owned();
    second.start_ms = 500;
    let mut tracks = vec![
        TextTrack {
            id: "headline-1".to_owned(),
            role: "headline".to_owned(),
            layer: 1,
            enabled: true,
            origin: "storyboard_generated".to_owned(),
            generation_id: None,
            editable: true,
            locked: false,
            cues: vec![first],
        },
        TextTrack {
            id: "headline-2".to_owned(),
            role: "headline".to_owned(),
            layer: 2,
            enabled: true,
            origin: "storyboard_generated".to_owned(),
            generation_id: None,
            editable: true,
            locked: false,
            cues: vec![second],
        },
    ];
    assert!(validate_text_tracks(&mut tracks, 2_000).is_err());
}

#[test]
fn text_cues_on_the_same_track_cannot_overlap() {
    let mut second_cue = text_cue("jianying_default", None, None);
    second_cue.id = "cue-2".to_owned();
    second_cue.start_ms = 1_500;
    second_cue.end_ms = 2_500;
    let mut tracks = vec![TextTrack {
        id: "text-1".to_owned(),
        role: "subtitle".to_owned(),
        layer: 1,
        enabled: true,
        origin: "storyboard_generated".to_owned(),
        generation_id: None,
        editable: true,
        locked: false,
        cues: vec![text_cue("jianying_default", None, None), second_cue],
    }];

    assert!(validate_text_tracks(&mut tracks, 3_000).is_err());
}

#[test]
fn editing_task_candidates_include_only_its_timelines() {
    let connection = Connection::open_in_memory().expect("open test database");
    connection
        .execute_batch(
            "
            CREATE TABLE storyboard_versions (id TEXT, project_id TEXT, editing_task_id TEXT);
            CREATE TABLE timeline_versions (id TEXT, project_id TEXT, storyboard_version_id TEXT, version_number INTEGER, content_json TEXT, created_at INTEGER);
            INSERT INTO storyboard_versions VALUES ('storyboard-a', 'project-1', 'edit-a');
            INSERT INTO storyboard_versions VALUES ('storyboard-b', 'project-1', 'edit-b');
            INSERT INTO storyboard_versions VALUES ('storyboard-other', 'project-2', 'edit-a');
            INSERT INTO timeline_versions VALUES ('timeline-a', 'project-1', 'storyboard-a', 1, '{\"clips\":[]}', 1);
            INSERT INTO timeline_versions VALUES ('timeline-b', 'project-1', 'storyboard-b', 1, '{\"clips\":[]}', 2);
            INSERT INTO timeline_versions VALUES ('timeline-other', 'project-2', 'storyboard-other', 1, '{\"clips\":[]}', 3);
            ",
        )
        .expect("create scoped timeline test data");

    let candidates = timeline_candidates_for_editing_task(&connection, "project-1", "edit-a")
        .expect("load scoped candidates");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].id, "timeline-a");
}

#[test]
fn requested_timeline_is_used_when_multiple_candidates_exist() {
    let timeline = |id: &str, version_number| TimelineVersion {
        id: id.to_owned(),
        project_id: "project-1".to_owned(),
        storyboard_version_id: "storyboard-1".to_owned(),
        version_number,
        clips: Vec::new(),
        text_tracks: Vec::new(),
        music_tracks: Vec::new(),
        voiceover_tracks: Vec::new(),
        quality_report: None,
        created_at: version_number,
    };
    let timelines = vec![timeline("timeline-2", 2), timeline("timeline-1", 1)];

    let selected =
        select_timeline_candidate(&timelines, None, Some("timeline-1")).expect("selection");
    assert_eq!(selected.id, "timeline-1");
}

#[test]
fn explicit_unknown_timeline_never_falls_back_to_the_only_candidate() {
    let timeline = TimelineVersion {
        id: "timeline-1".to_owned(),
        project_id: "project-1".to_owned(),
        storyboard_version_id: "storyboard-1".to_owned(),
        version_number: 1,
        clips: Vec::new(),
        text_tracks: Vec::new(),
        music_tracks: Vec::new(),
        voiceover_tracks: Vec::new(),
        quality_report: None,
        created_at: 1,
    };

    assert!(select_timeline_candidate(&[timeline.clone()], Some("unknown"), None).is_none());
    assert!(select_timeline_candidate(&[timeline], None, Some("unknown")).is_none());
}

#[test]
fn replacing_clips_creates_a_new_version_without_moving_timeline_bounds() {
    let connection = Connection::open_in_memory().expect("open test database");
    connection
        .execute_batch(
            "
            CREATE TABLE assets (id TEXT, project_id TEXT, kind TEXT, analysis_status TEXT, metadata_json TEXT);
            CREATE TABLE timeline_versions (id TEXT, project_id TEXT, storyboard_version_id TEXT, version_number INTEGER, status TEXT, content_json TEXT, created_at INTEGER);
            CREATE TABLE operation_logs (id TEXT, project_id TEXT, editing_task_id TEXT, conversation_id TEXT, agent_task_id TEXT, actor TEXT, operation_type TEXT, entity_type TEXT, entity_id TEXT, before_json TEXT, after_json TEXT, created_at INTEGER);
            ",
        )
        .expect("create test tables");
    let replacement_metadata = serde_json::to_string(&TechnicalMetadata {
        duration_ms: Some(10_000),
        ..TechnicalMetadata::default()
    })
    .expect("serialize replacement metadata");
    connection
        .execute(
            "INSERT INTO assets VALUES ('replacement-video', 'project-1', 'video', 'ready', ?1)",
            params![replacement_metadata],
        )
        .expect("insert replacement asset");
    connection
        .execute(
            "INSERT INTO timeline_versions VALUES ('existing', 'project-1', 'storyboard-1', 1, 'draft', '{}', 1)",
            [],
        )
        .expect("insert existing version");
    let existing = TimelineVersion {
        id: "existing".to_owned(),
        project_id: "project-1".to_owned(),
        storyboard_version_id: "storyboard-1".to_owned(),
        version_number: 1,
        clips: vec![TimelineClip {
            shot_index: 1,
            asset_id: "original-video".to_owned(),
            source_start_ms: 0,
            source_end_ms: 2_000,
            timeline_start_ms: 0,
            timeline_end_ms: 2_000,
            on_screen_text: "Quality".to_owned(),
            ..Default::default()
        }],
        text_tracks: Vec::new(),
        music_tracks: Vec::new(),
        voiceover_tracks: Vec::new(),
        quality_report: None,
        created_at: 1,
    };
    let replacement = replace_clips(
        &connection,
        "project-1",
        "editing-task-1",
        "conversation-1",
        "agent-task-1",
        &existing,
        &[ClipReplacement {
            shot_index: 1,
            asset_id: "replacement-video".to_owned(),
            source_start_ms: 3_000,
            source_end_ms: 5_000,
        }],
    )
    .expect("replace scoped clip");
    assert_eq!(replacement.version_number, 2);
    assert_eq!(replacement.clips[0].asset_id, "replacement-video");
    assert_eq!(replacement.clips[0].source_start_ms, 3_000);
    assert_eq!(replacement.clips[0].source_end_ms, 5_000);
    assert_eq!(replacement.clips[0].timeline_start_ms, 0);
    assert_eq!(replacement.clips[0].timeline_end_ms, 2_000);
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM operation_logs", [], |row| row
                .get::<_, i64>(0))
            .expect("count operation logs"),
        1
    );
}

#[test]
fn changing_clip_duration_shifts_following_shots_and_stays_in_source_range() {
    let connection = Connection::open_in_memory().expect("open test database");
    connection
        .execute_batch(
            "
            CREATE TABLE assets (id TEXT, project_id TEXT, kind TEXT, analysis_status TEXT, metadata_json TEXT);
            CREATE TABLE timeline_versions (id TEXT, project_id TEXT, storyboard_version_id TEXT, version_number INTEGER, status TEXT, content_json TEXT, created_at INTEGER);
            CREATE TABLE operation_logs (id TEXT, project_id TEXT, editing_task_id TEXT, conversation_id TEXT, agent_task_id TEXT, actor TEXT, operation_type TEXT, entity_type TEXT, entity_id TEXT, before_json TEXT, after_json TEXT, created_at INTEGER);
            ",
        )
        .expect("create test tables");
    let metadata = serde_json::to_string(&TechnicalMetadata {
        duration_ms: Some(10_000),
        ..TechnicalMetadata::default()
    })
    .expect("serialize metadata");
    connection
        .execute(
            "INSERT INTO assets VALUES ('video-a', 'project-1', 'video', 'ready', ?1)",
            params![metadata],
        )
        .expect("insert asset");
    connection
        .execute(
            "INSERT INTO timeline_versions VALUES ('existing', 'project-1', 'storyboard-1', 1, 'draft', '{}', 1)",
            [],
        )
        .expect("insert existing version");
    let existing = TimelineVersion {
        id: "existing".to_owned(),
        project_id: "project-1".to_owned(),
        storyboard_version_id: "storyboard-1".to_owned(),
        version_number: 1,
        clips: vec![
            TimelineClip {
                shot_index: 1,
                asset_id: "video-a".to_owned(),
                source_start_ms: 0,
                source_end_ms: 3_000,
                timeline_start_ms: 0,
                timeline_end_ms: 3_000,
                on_screen_text: String::new(),
                ..Default::default()
            },
            TimelineClip {
                shot_index: 2,
                asset_id: "video-a".to_owned(),
                source_start_ms: 3_000,
                source_end_ms: 5_000,
                timeline_start_ms: 3_000,
                timeline_end_ms: 5_000,
                on_screen_text: String::new(),
                ..Default::default()
            },
        ],
        text_tracks: Vec::new(),
        music_tracks: Vec::new(),
        voiceover_tracks: Vec::new(),
        quality_report: None,
        created_at: 1,
    };
    let adjusted = change_clip_duration(
        &connection,
        "project-1",
        "editing-task-1",
        "conversation-1",
        "agent-task-1",
        &existing,
        &[ClipAdjustment {
            shot_index: 1,
            new_duration_ms: Some(1_000),
            new_source_start_ms: Some(2_000),
        }],
    )
    .expect("retime first clip");
    assert_eq!(adjusted.clips[0].timeline_start_ms, 0);
    assert_eq!(adjusted.clips[0].timeline_end_ms, 1_000);
    assert_eq!(adjusted.clips[1].timeline_start_ms, 1_000);
    assert_eq!(adjusted.clips[1].timeline_end_ms, 3_000);
    assert_eq!(
        adjusted.clips[0].source_start_ms, 2_000,
        "new source start is honored"
    );
    assert_eq!(
        adjusted.clips[0].source_end_ms, 3_000,
        "source end follows the retimed video window"
    );

    let shortened = change_clip_duration(
        &connection,
        "project-1",
        "editing-task-1",
        "conversation-1",
        "agent-task-1",
        &existing,
        &[ClipAdjustment {
            shot_index: 1,
            new_duration_ms: Some(1_000),
            new_source_start_ms: None,
        }],
    )
    .expect("shorten first clip without moving its source start");
    assert_eq!(shortened.clips[0].source_start_ms, 0);
    assert_eq!(shortened.clips[0].source_end_ms, 1_000);
    assert_eq!(shortened.clips[0].timeline_end_ms, 1_000);

    let invalid = change_clip_duration(
        &connection,
        "project-1",
        "editing-task-1",
        "conversation-1",
        "agent-task-1",
        &existing,
        &[ClipAdjustment {
            shot_index: 1,
            new_duration_ms: Some(4_000),
            new_source_start_ms: None,
        }],
    );
    assert!(
        invalid.is_err(),
        "retiming beyond the verified source range must fail"
    );

    let before_verified_start = change_clip_duration(
        &connection,
        "project-1",
        "editing-task-1",
        "conversation-1",
        "agent-task-1",
        &existing,
        &[ClipAdjustment {
            shot_index: 2,
            new_duration_ms: Some(1_000),
            new_source_start_ms: Some(0),
        }],
    );
    assert!(
        before_verified_start.is_err(),
        "retiming before the verified source range must fail"
    );
}

#[test]
fn reordering_clips_requires_a_full_permutation() {
    let connection = Connection::open_in_memory().expect("open test database");
    connection
        .execute_batch(
            "
            CREATE TABLE timeline_versions (id TEXT, project_id TEXT, storyboard_version_id TEXT, version_number INTEGER, status TEXT, content_json TEXT, created_at INTEGER);
            CREATE TABLE operation_logs (id TEXT, project_id TEXT, editing_task_id TEXT, conversation_id TEXT, agent_task_id TEXT, actor TEXT, operation_type TEXT, entity_type TEXT, entity_id TEXT, before_json TEXT, after_json TEXT, created_at INTEGER);
            ",
        )
        .expect("create test tables");
    connection
        .execute(
            "INSERT INTO timeline_versions VALUES ('existing', 'project-1', 'storyboard-1', 1, 'draft', '{}', 1)",
            [],
        )
        .expect("insert existing version");
    let existing = TimelineVersion {
        id: "existing".to_owned(),
        project_id: "project-1".to_owned(),
        storyboard_version_id: "storyboard-1".to_owned(),
        version_number: 1,
        clips: vec![
            TimelineClip {
                shot_index: 1,
                asset_id: "a".to_owned(),
                source_start_ms: 0,
                source_end_ms: 1_000,
                timeline_start_ms: 0,
                timeline_end_ms: 1_000,
                on_screen_text: String::new(),
                ..Default::default()
            },
            TimelineClip {
                shot_index: 2,
                asset_id: "b".to_owned(),
                source_start_ms: 0,
                source_end_ms: 1_000,
                timeline_start_ms: 1_000,
                timeline_end_ms: 2_000,
                on_screen_text: String::new(),
                ..Default::default()
            },
            TimelineClip {
                shot_index: 3,
                asset_id: "c".to_owned(),
                source_start_ms: 0,
                source_end_ms: 1_000,
                timeline_start_ms: 2_000,
                timeline_end_ms: 3_000,
                on_screen_text: String::new(),
                ..Default::default()
            },
        ],
        text_tracks: Vec::new(),
        music_tracks: Vec::new(),
        voiceover_tracks: Vec::new(),
        quality_report: None,
        created_at: 1,
    };
    let reordered = reorder_clips(
        &connection,
        "project-1",
        "editing-task-1",
        "conversation-1",
        "agent-task-1",
        &existing,
        &[3, 1, 2],
    )
    .expect("reorder shots");
    assert_eq!(reordered.clips[0].shot_index, 3);
    assert_eq!(reordered.clips[0].timeline_start_ms, 0);
    assert_eq!(reordered.clips[1].shot_index, 1);
    assert_eq!(reordered.clips[1].timeline_start_ms, 1_000);
    assert_eq!(reordered.clips[2].shot_index, 2);
    assert_eq!(reordered.clips[2].timeline_start_ms, 2_000);

    let incomplete = reorder_clips(
        &connection,
        "project-1",
        "editing-task-1",
        "conversation-1",
        "agent-task-1",
        &existing,
        &[1, 2],
    );
    assert!(incomplete.is_err(), "a partial order must be rejected");
    let unknown = reorder_clips(
        &connection,
        "project-1",
        "editing-task-1",
        "conversation-1",
        "agent-task-1",
        &existing,
        &[4, 1, 2],
    );
    assert!(unknown.is_err(), "unknown shot indexes must be rejected");
}

#[test]
fn select_timeline_candidate_picks_latest_when_multiple_versions_exist() {
    let v2 = TimelineVersion {
        id: "v2".to_owned(),
        project_id: "p1".to_owned(),
        storyboard_version_id: "sb1".to_owned(),
        version_number: 2,
        clips: vec![],
        text_tracks: vec![],
        music_tracks: vec![],
        voiceover_tracks: vec![],
        quality_report: None,
        created_at: 2000,
    };
    let v1 = TimelineVersion {
        id: "v1".to_owned(),
        version_number: 1,
        created_at: 1000,
        ..v2.clone()
    };

    // 候选按 version_number DESC 排列，首条是最新版
    assert_eq!(
        select_timeline_candidate(&[v2.clone(), v1], None, None).map(|t| t.id),
        Some("v2".to_owned())
    );
}
