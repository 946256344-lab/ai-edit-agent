//! Native Function Tool 的本地敏感能力授权识别。
//!
//! 可逆本地编辑能力默认交给模型选择。本模块只识别会下载外部媒体、调用付费语音
//! Provider 或创建交付草稿的明确请求；它不选择首个工具、不读取项目状态，也不
//! 拥有 Provider、SQLite 或副作用执行能力。

use super::policy::{explicitly_denies_target, RequestToolPolicy};

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

pub(super) fn explicitly_requested_sensitive_tools(request: &str) -> Vec<&'static str> {
    let compact = request
        .to_lowercase()
        .chars()
        .filter(|character| !character.is_whitespace() && !matches!(character, '-' | '\'' | '’'))
        .collect::<String>();
    let mut tools = Vec::new();
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
        "配音",
        "旁白",
        "voiceover",
        "narration",
        "texttospeech",
        "synthesizevoiceover",
    ]
    .iter()
    .any(|phrase| compact.contains(phrase))
    {
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
        if !explicitly_denies_sensitive_tool(request, tool) && !unique_tools.contains(&tool) {
            unique_tools.push(tool);
        }
    }
    unique_tools
}

/// 终态验真所需的最低产物集合。它不参与工具暴露或路径选择；即使这里漏掉新说法，
/// 模型仍能调用全部可逆工具。命中时只防止“没有对应收据却声称完成”。
pub(super) fn expected_native_write_tools(request: &str) -> Vec<&'static str> {
    let compact = request
        .to_lowercase()
        .chars()
        .filter(|character| !character.is_whitespace() && !matches!(character, '-' | '\'' | '’'))
        .collect::<String>();
    let contains_any = |phrases: &[&str]| phrases.iter().any(|phrase| compact.contains(phrase));
    let production_denied = explicitly_denies_target(
        request,
        &["视频", "影片", "video"],
        &["剪辑", "剪", "生成", "制作", "做", "edit", "create", "make"],
    );
    let wants_production = !production_denied
        && contains_any(&[
            "剪辑一个视频",
            "剪辑视频",
            "剪个视频",
            "做一个视频",
            "制作视频",
            "生成视频",
            "editavideo",
            "editvideo",
            "makeavideo",
            "createavideo",
        ]);
    let mut tools = Vec::new();
    if contains_any(&[
        "分析素材",
        "素材分析",
        "analyzemedia",
        "analyzeassets",
        "requestassetanalysis",
    ]) {
        tools.push("request_asset_analysis");
    }
    if wants_production
        || contains_any(&[
            "生成storyboard",
            "创建storyboard",
            "制作storyboard",
            "生成分镜",
            "创建分镜",
            "generatestoryboard",
            "createstoryboard",
        ])
    {
        tools.push("generate_storyboard");
    }
    if wants_production
        || contains_any(&[
            "创建时间线",
            "生成时间线",
            "制作时间线",
            "createtimeline",
            "generatetimeline",
        ])
    {
        tools.push("create_timeline_draft");
    }
    if contains_any(&["替换片段", "replaceclips", "swapclips"]) {
        tools.push("replace_clips");
    }
    if contains_any(&[
        "调整片段时长",
        "缩短片段",
        "加长片段",
        "changeclipduration",
        "adjustclipduration",
    ]) {
        tools.push("change_clip_duration");
    }
    if contains_any(&["重排片段", "排序片段", "reorderclips", "sortclips"]) {
        tools.push("reorder_clips");
    }
    if contains_any(&[
        "添加字幕",
        "加字幕",
        "替换字幕",
        "替换文本轨",
        "addsubtitles",
        "addcaptions",
        "replacetexttracks",
    ]) {
        tools.push("replace_text_tracks");
    }
    if contains_any(&[
        "替换音乐",
        "替换背景音乐",
        "编辑音乐",
        "replacemusictracks",
        "replacebackgroundmusic",
    ]) {
        tools.push("replace_music_tracks");
    }
    if contains_any(&[
        "生成预览",
        "生成一个预览",
        "创建预览",
        "创建一个预览",
        "渲染预览",
        "制作预览",
        "generatepreview",
        "createpreview",
        "renderpreview",
    ]) {
        tools.push("render_preview");
    }
    tools.extend(explicitly_requested_sensitive_tools(request));
    tools.sort_unstable();
    tools.dedup();
    tools
}

fn explicitly_denies_sensitive_tool(request: &str, tool: &str) -> bool {
    let denied =
        |targets: &[&str], actions: &[&str]| explicitly_denies_target(request, targets, actions);
    match tool {
        "download_music" => denied(
            &["音乐", "背景音乐", "music", "backgroundmusic"],
            &["下载", "download"],
        ),
        "use_online_music" => denied(
            &["在线音乐", "onlinemusic"],
            &["使用", "添加", "用", "use", "add"],
        ),
        "synthesize_voiceover" => denied(
            &["配音", "旁白", "voiceover", "tts", "narration"],
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
    use super::{expected_native_write_tools, explicitly_requested_sensitive_tools};

    #[test]
    fn ordinary_and_local_edit_requests_do_not_need_sensitive_authorization() {
        for request in [
            "你好",
            "这些素材适合怎么剪？",
            "给我剪辑一个视频，体现专业、负责、供应链强大。",
            "生成 storyboard 并创建时间线、字幕和预览",
            "这个应用支持运行日志吗？",
            "Does this application have logs?",
            "运行日志不用读取",
        ] {
            assert!(explicitly_requested_sensitive_tools(request).is_empty());
        }
    }

    #[test]
    fn sensitive_requests_authorize_only_matching_capabilities() {
        assert_eq!(
            explicitly_requested_sensitive_tools(
                "下载音乐、生成配音并创建剪映草稿，不要使用在线音乐"
            ),
            [
                "download_music",
                "synthesize_voiceover",
                "create_jianying_draft"
            ]
        );
        assert_eq!(
            explicitly_requested_sensitive_tools("Use online music"),
            ["use_online_music"]
        );
        assert!(explicitly_requested_sensitive_tools("读取运行日志第 10 到 20 行").is_empty());
    }

    #[test]
    fn a_video_request_does_not_imply_paid_voiceover() {
        assert!(explicitly_requested_sensitive_tools("用这个文案生成视频").is_empty());
        assert_eq!(
            explicitly_requested_sensitive_tools("用这个文案生成视频并配音"),
            ["synthesize_voiceover"]
        );
    }

    #[test]
    fn negated_sensitive_requests_do_not_authorize_tools() {
        for (request, tool) in [
            ("不要下载音乐", "download_music"),
            ("不要使用在线音乐", "use_online_music"),
            ("不要配音", "synthesize_voiceover"),
            ("不要创建剪映草稿", "create_jianying_draft"),
            ("Do not download music", "download_music"),
            ("不要读取日志", "read_logs"),
            ("运行日志不用读取", "read_logs"),
        ] {
            assert!(!explicitly_requested_sensitive_tools(request).contains(&tool));
        }
    }

    #[test]
    fn edit_wording_sets_truth_expectations_without_sensitive_authorization() {
        assert_eq!(
            expected_native_write_tools("给我剪辑一个视频，体现专业、负责、供应链强大。"),
            ["create_timeline_draft", "generate_storyboard"]
        );
        assert!(expected_native_write_tools("这些素材适合怎么剪？").is_empty());
        assert!(expected_native_write_tools("不要剪辑视频").is_empty());
    }
}
