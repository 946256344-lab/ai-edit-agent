//! Native Function Tool 的本地显式授权识别。
//!
//! 本模块只把明确的创建、分析或修改表达映射为可暴露的主链工具名称；它不选择
//! 首个工具、不读取项目状态，也不拥有 Provider、SQLite 或副作用执行能力。

use super::policy::{RequestToolPolicy, CREATE_VERBS, EDIT_VERBS};

/// Router 解析失败时的保守事实策略只决定是否需要观察，不选择具体工具。
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
    let mut tools = Vec::new();
    if analysis {
        tools.push("request_asset_analysis");
    }
    if has_action(&["storyboard", "分镜"]) {
        tools.push("generate_storyboard");
    }
    if has_action(&["timeline", "时间线"]) {
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
    if [
        "添加字幕",
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
        if !unique_tools.contains(&tool) {
            unique_tools.push(tool);
        }
    }
    unique_tools
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
}
