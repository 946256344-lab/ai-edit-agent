//! Native Function Tool 的本地显式授权识别。
//!
//! 本模块只把明确的创建、分析或修改表达映射为可暴露的主链工具名称；它不选择
//! 首个工具、不读取项目状态，也不拥有 Provider、SQLite 或副作用执行能力。

use super::policy::{explicitly_denies_target, RequestToolPolicy, CREATE_VERBS, EDIT_VERBS};

/// 保守事实策略只决定是否需要观察，不选择具体工具。
pub(super) fn request_requires_project_observation(request: &str) -> bool {
    let policy = RequestToolPolicy::from_request(request);
    if policy.read_only {
        return true;
    }
    let text = request.trim().to_lowercase();
    let project_subject = [
        "当前项目",
        "本项目",
        "这个项目",
        "剪辑任务",
        "时间线",
        "timeline",
        "storyboard",
        "分镜",
        "preview",
        "预览",
        "素材",
        "asset",
        "片段",
        "clip",
        "镜头",
        "shot",
        "版本",
        "version",
    ]
    .iter()
    .any(|term| text.contains(term));
    let current_fact = [
        "当前",
        "现在",
        "现有",
        "已有",
        "最新",
        "多少",
        "几个",
        "状态",
        "是否已经",
        "有没有",
        "v几",
        "current",
        "existing",
        "latest",
        "how many",
        "count",
        "status",
        "which version",
        "检查",
        "inspect",
    ]
    .iter()
    .any(|term| text.contains(term));
    project_subject && current_fact
}

pub(super) fn explicitly_requested_native_tools(request: &str) -> Vec<&'static str> {
    let compact = request
        .to_lowercase()
        .chars()
        .filter(|character| !character.is_whitespace() && !matches!(character, '-' | '\'' | '’'))
        .collect::<String>();
    let action_target_matches = |verb: &str, target: &str| {
        ["", "a", "an", "the"]
            .iter()
            .any(|article| compact.contains(&format!("{verb}{article}{target}")))
    };
    let has_action = |targets: &[&str]| {
        CREATE_VERBS.iter().any(|verb| {
            let verb = verb.to_lowercase();
            targets.iter().any(|target| {
                action_target_matches(&verb, target) || compact.contains(&format!("{target}{verb}"))
            })
        }) || EDIT_VERBS.iter().any(|verb| {
            let verb = verb.to_lowercase();
            targets.iter().any(|target| {
                action_target_matches(&verb, target) || compact.contains(&format!("{target}{verb}"))
            })
        })
    };
    let analysis = compact.contains("分析素材")
        || compact.contains("素材分析")
        || (compact.contains("分析") && compact.contains("素材"))
        || compact.contains("analyzemedia")
        || compact.contains("analyzeassets")
        || (compact.contains("analyze")
            && (compact.contains("asset") || compact.contains("media")))
        || compact.contains("requestassetanalysis");
    let negates_edit = explicitly_denies_target(
        request,
        &["30秒剪辑", "30secondedit"],
        &["", "做", "制作", "创建", "make", "create"],
    );
    let negates_text = explicitly_denies_target(
        request,
        &["字幕", "subtitle", "subtitles", "caption", "captions"],
        &["", "加", "添加", "替换", "add", "replace", "edit"],
    );
    let negates_voiceover = explicitly_denies_target(
        request,
        &["配音", "旁白", "voiceover", "tts", "narration"],
        &[
            "", "加", "添加", "生成", "做", "add", "create", "generate", "make",
        ],
    );
    let negates_preview = explicitly_denies_target(
        request,
        &["预览", "preview"],
        &[
            "", "生成", "创建", "渲染", "制作", "generate", "create", "render", "make",
        ],
    );
    let creates_edit = !negates_edit
        && (compact.contains("做30秒剪辑")
            || compact.contains("做一个30秒剪辑")
            || compact.contains("制作30秒剪辑")
            || compact.contains("makea30secondedit")
            || compact.contains("createa30secondedit"));
    let creates_video = has_action(&["视频", "video", "影片"]);
    let requests_voiceover = [
        "配音",
        "旁白",
        "加旁白",
        "加配音",
        "生成配音",
        "voiceover",
        "narration",
        "texttospeech",
        "synthesizevoiceover",
    ]
    .iter()
    .any(|phrase| compact.contains(phrase));
    let wants_voiceover = !negates_voiceover
        && (creates_edit
            || creates_video
            || has_action(&["timeline", "时间线"])
            || requests_voiceover);
    let wants_production = creates_edit || creates_video || wants_voiceover;
    let explains_preview = compact.starts_with("解释")
        || compact.starts_with("说明")
        || compact.starts_with("explain")
        || compact.starts_with("whatis")
        || compact.starts_with("howdoes");
    let mut tools = Vec::new();
    if analysis {
        tools.push("request_asset_analysis");
    }
    if wants_production || has_action(&["storyboard", "分镜"]) {
        tools.push("generate_storyboard");
    }
    if wants_production || has_action(&["timeline", "时间线"]) {
        tools.push("create_timeline_draft");
    }
    if compact.contains("replaceclips") || compact.contains("替换片段") {
        tools.push("replace_clips");
    }
    if compact.contains("changeduration")
        || compact.contains("changeclipduration")
        || compact.contains("调整片段时长")
        || compact.contains("缩短片段")
        || compact.contains("加长片段")
    {
        tools.push("change_clip_duration");
    }
    if compact.contains("reorderclips")
        || compact.contains("重排片段")
        || compact.contains("排序片段")
    {
        tools.push("reorder_clips");
    }
    if !negates_text
        && [
            "添加字幕",
            "加字幕",
            "替换字幕",
            "替换文本轨",
            "编辑字幕",
            "replacetexttracks",
            "addcaptions",
            "addsubtitles",
            "editsubtitles",
        ]
        .iter()
        .any(|phrase| compact.contains(phrase))
    {
        tools.push("replace_text_tracks");
    }
    if !negates_preview
        && !explains_preview
        && [
            "生成预览",
            "生成一个预览",
            "创建预览",
            "创建一个预览",
            "渲染预览",
            "制作预览",
            "generatepreview",
            "createpreview",
            "renderpreview",
            "makepreview",
        ]
        .iter()
        .any(|phrase| compact.contains(phrase))
    {
        tools.push("render_preview");
    }
    if ["下载音乐", "下载背景音乐", "downloadmusic"]
        .iter()
        .any(|phrase| compact.contains(phrase))
    {
        tools.push("download_music");
    }
    if [
        "使用在线音乐",
        "添加在线音乐",
        "用在线音乐",
        "useonlinemusic",
    ]
    .iter()
    .any(|phrase| compact.contains(phrase))
    {
        tools.push("use_online_music");
    }
    if [
        "替换音乐",
        "替换背景音乐",
        "编辑音乐",
        "replacemusictracks",
        "replacebackgroundmusic",
        "editmusic",
    ]
    .iter()
    .any(|phrase| compact.contains(phrase))
    {
        tools.push("replace_music_tracks");
    }
    if wants_voiceover {
        tools.push("synthesize_voiceover");
    }
    if [
        "创建剪映草稿",
        "生成剪映草稿",
        "制作剪映草稿",
        "createjianyingdraft",
        "generatejianyingdraft",
    ]
    .iter()
    .any(|phrase| compact.contains(phrase))
        || ((compact.contains("create") || compact.contains("generate"))
            && compact.contains("jianyingdraft"))
    {
        tools.push("create_jianying_draft");
    }
    let mut unique_tools = Vec::new();
    for tool in tools {
        if !explicitly_denies_native_tool(request, tool) && !unique_tools.contains(&tool) {
            unique_tools.push(tool);
        }
    }
    unique_tools
}

fn explicitly_denies_native_tool(request: &str, tool: &str) -> bool {
    let denied =
        |targets: &[&str], actions: &[&str]| explicitly_denies_target(request, targets, actions);
    match tool {
        "request_asset_analysis" => denied(
            &[
                "素材",
                "media",
                "asset",
                "assets",
                "素材分析",
                "分析素材",
                "mediaanalysis",
                "assetanalysis",
            ],
            &["分析", "重新分析", "analyze", "reanalyze", "request", "run"],
        ),
        "generate_storyboard" => denied(
            &["storyboard", "分镜", "视频", "video", "影片"],
            &["生成", "创建", "制作", "generate", "create", "make"],
        ),
        "create_timeline_draft" => denied(
            &["timeline", "时间线", "视频", "video", "影片"],
            &["生成", "创建", "制作", "generate", "create", "make"],
        ),
        "replace_clips" => denied(&["片段", "clips"], &["替换", "replace", "swap"]),
        "change_clip_duration" => denied(
            &["片段时长", "clipduration", "duration"],
            &[
                "调整", "改变", "缩短", "加长", "change", "adjust", "shorten", "extend",
            ],
        ),
        "reorder_clips" => denied(&["片段", "clips"], &["重排", "排序", "reorder", "sort"]),
        "replace_text_tracks" => denied(
            &[
                "字幕",
                "文本轨",
                "subtitle",
                "subtitles",
                "caption",
                "captions",
                "texttracks",
            ],
            &["加", "添加", "替换", "编辑", "add", "replace", "edit"],
        ),
        "download_music" => denied(
            &["音乐", "背景音乐", "music", "backgroundmusic"],
            &["下载", "download"],
        ),
        "use_online_music" => denied(
            &["在线音乐", "onlinemusic"],
            &["使用", "添加", "用", "use", "add"],
        ),
        "replace_music_tracks" => denied(
            &["音乐", "背景音乐", "music", "backgroundmusic"],
            &["替换", "编辑", "replace", "edit"],
        ),
        "synthesize_voiceover" => denied(
            &[
                "配音",
                "旁白",
                "voiceover",
                "tts",
                "narration",
                "视频",
                "video",
                "影片",
            ],
            &[
                "加",
                "添加",
                "生成",
                "做",
                "合成",
                "add",
                "create",
                "generate",
                "make",
                "synthesize",
            ],
        ),
        "render_preview" => denied(
            &["预览", "preview"],
            &[
                "生成", "创建", "渲染", "制作", "generate", "create", "render", "make",
            ],
        ),
        "create_jianying_draft" => denied(
            &["剪映草稿", "剪映", "jianyingdraft", "jianying"],
            &[
                "生成", "创建", "制作", "交付", "generate", "create", "make", "deliver",
            ],
        ),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::explicitly_requested_native_tools;

    #[test]
    fn ambiguous_requests_remain_read_only() {
        for request in [
            "你好",
            "这些素材适合怎么剪？",
            "How should I edit these assets?",
        ] {
            assert!(explicitly_requested_native_tools(request).is_empty());
        }
    }

    #[test]
    fn explicit_requests_only_authorize_named_capabilities() {
        assert_eq!(
            explicitly_requested_native_tools("分析素材并生成 storyboard"),
            ["request_asset_analysis", "generate_storyboard"]
        );
        assert_eq!(
            explicitly_requested_native_tools("Analyze these assets and generate a storyboard"),
            ["request_asset_analysis", "generate_storyboard"]
        );
    }

    #[test]
    fn delivery_requests_authorize_only_the_matching_native_tools() {
        assert_eq!(
            explicitly_requested_native_tools("添加字幕并替换背景音乐"),
            ["replace_text_tracks", "replace_music_tracks"]
        );
        assert_eq!(
            explicitly_requested_native_tools("Download music and create a Jianying draft"),
            ["download_music", "create_jianying_draft"]
        );
        assert_eq!(
            explicitly_requested_native_tools("Use online music"),
            ["use_online_music"]
        );
    }

    #[test]
    fn composite_edit_request_authorizes_each_named_stage() {
        assert_eq!(
            explicitly_requested_native_tools("检查素材，做 30 秒剪辑，加字幕并生成预览。"),
            [
                "generate_storyboard",
                "create_timeline_draft",
                "replace_text_tracks",
                "render_preview",
                "synthesize_voiceover"
            ]
        );
        assert!(super::request_requires_project_observation(
            "检查素材，做 30 秒剪辑，加字幕并生成预览。"
        ));
    }

    #[test]
    fn negated_composite_actions_never_authorize_write_tools() {
        for request in [
            "检查素材，但不要做 30 秒剪辑，不要加字幕，也不要生成预览。",
            "Inspect the assets; do not make a 30 second edit; do not add subtitles; do not generate a preview.",
        ] {
            assert!(
                explicitly_requested_native_tools(request).is_empty(),
                "negated request authorized a write tool: {request}"
            );
        }
    }

    #[test]
    fn each_negated_write_action_is_excluded_from_native_authorization() {
        for (request, denied_tool) in [
            ("不要分析素材", "request_asset_analysis"),
            ("不要生成 storyboard", "generate_storyboard"),
            ("不要创建时间线", "create_timeline_draft"),
            ("不要替换片段", "replace_clips"),
            ("不要调整片段时长", "change_clip_duration"),
            ("不要重排片段", "reorder_clips"),
            ("不要添加字幕", "replace_text_tracks"),
            ("不要下载音乐", "download_music"),
            ("不要使用在线音乐", "use_online_music"),
            ("不要替换背景音乐", "replace_music_tracks"),
            ("不要生成预览", "render_preview"),
            ("不要创建剪映草稿", "create_jianying_draft"),
            ("不要配音", "synthesize_voiceover"),
            ("Do not generate a storyboard", "generate_storyboard"),
            ("Do not create a timeline", "create_timeline_draft"),
            ("Do not download music", "download_music"),
        ] {
            assert!(
                !explicitly_requested_native_tools(request).contains(&denied_tool),
                "negated request authorized {denied_tool}: {request}"
            );
        }
    }

    #[test]
    fn voiceover_is_authorized_for_edits_unless_denied() {
        assert!(
            explicitly_requested_native_tools("把这段文案配音").contains(&"synthesize_voiceover")
        );
        assert!(explicitly_requested_native_tools("做 30 秒剪辑").contains(&"synthesize_voiceover"));
        assert!(!explicitly_requested_native_tools("做 30 秒剪辑，不要配音")
            .contains(&"synthesize_voiceover"));
        assert!(
            !explicitly_requested_native_tools("素材有多少个").contains(&"synthesize_voiceover")
        );
    }

    #[test]
    fn generating_a_video_or_voiceover_authorizes_the_main_chain() {
        assert_eq!(
            explicitly_requested_native_tools("用这个文案生成视频"),
            [
                "generate_storyboard",
                "create_timeline_draft",
                "synthesize_voiceover"
            ]
        );
        assert_eq!(
            explicitly_requested_native_tools("用这个文案生成配音 Hello factory."),
            [
                "generate_storyboard",
                "create_timeline_draft",
                "synthesize_voiceover"
            ]
        );
        assert_eq!(
            explicitly_requested_native_tools("生成视频，不要配音"),
            ["generate_storyboard", "create_timeline_draft"]
        );
        assert!(explicitly_requested_native_tools("不要生成视频").is_empty());
        assert!(explicitly_requested_native_tools("不要配音").is_empty());
    }
}
