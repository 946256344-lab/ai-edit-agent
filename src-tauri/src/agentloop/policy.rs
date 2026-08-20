//! Agent 循环的纯策略层。
//!
//! 本模块只根据用户文本和已存在的产物结果回答三个问题：本轮禁止哪些工具、
//! 目标产物是什么、当前结果是否满足完成门。它不得持有数据库、文件系统、Tauri、
//! Provider 或外部进程句柄，因此这里的判断本身不能产生任何副作用。

use super::native_policy::explicitly_requested_native_tools;
pub(super) use super::native_policy::request_requires_project_observation;

/// 不创建/修改本地产物的观察技能；`search_music` 是受控外部查询，其余只读本地状态。
pub(super) const OBSERVATION_TOOLS: &[&str] = &[
    "get_edit_status",
    "get_asset_health_summary",
    "list_assets",
    "search_assets",
    "search_asset_segments",
    "search_music",
    "list_voices",
    "get_storyboard",
    "get_timeline",
    "get_text_capabilities",
];

/// 会创建、下载或修改可审计产物的技能。只读策略会一次性关闭整组技能。
pub(super) const EDIT_TOOLS: &[&str] = &[
    "download_music",
    "use_online_music",
    "request_asset_analysis",
    "generate_storyboard",
    "create_timeline_draft",
    "replace_clips",
    "change_clip_duration",
    "reorder_clips",
    "replace_text_tracks",
    "replace_music_tracks",
    "synthesize_voiceover",
    "render_preview",
    "create_jianying_draft",
];

/// 本地权限判断所需的显式动作词；它们只扩大本轮可用工具，不选择首个工具。
pub(super) const EDIT_VERBS: &[&str] = &[
    "替换",
    "换成",
    "换掉",
    "缩短",
    "加长",
    "重排",
    "排序",
    "去掉",
    "删除",
    "删掉",
    "不要",
    "精简",
    "调整",
    "剪掉",
    "裁剪",
    "放慢",
    "加快",
    "增加",
    "减掉",
    "adjust",
    "shorten",
    "lengthen",
    "replace",
    "reorder",
    "remove",
    "delete",
    "trim",
    "edit",
    "cut",
    "slow down",
    "speed up",
];

pub(super) const CREATE_VERBS: &[&str] = &[
    "生成",
    "创建",
    "渲染",
    "制作",
    "做一个",
    "做个",
    "做一段",
    "导入",
    "generate",
    "create",
    "render",
    "make",
    "import",
];

/// 用户本轮明确声明的负向边界，只能缩小工具集合，不能替模型选择工具。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct RequestToolPolicy {
    denied_tools: Vec<&'static str>,
    pub(super) read_only: bool,
    authorized_native_tools: Vec<&'static str>,
}

impl RequestToolPolicy {
    pub(super) fn from_request(request: &str) -> Self {
        let mut denied_tools = Vec::new();
        // 按分句判断只读，避免"不是只读，请调整"被一个孤立词误判为只读请求。
        let read_only = request
            .split(|character| {
                matches!(
                    character,
                    ';' | '；'
                        | ':'
                        | '：'
                        | '—'
                        | '–'
                        | '。'
                        | '.'
                        | '!'
                        | '！'
                        | '?'
                        | '？'
                        | ','
                        | '，'
                        | '\n'
                        | '\r'
                )
            })
            .any(request_clause_requests_read_only);
        if read_only {
            denied_tools.extend(EDIT_TOOLS.iter().copied());
        }
        if explicitly_denies_target(
            request,
            &["预览", "preview"],
            &[
                "",
                "生成",
                "创建",
                "渲染",
                "制作",
                "做",
                "generate",
                "generating",
                "create",
                "creating",
                "render",
                "rendering",
                "make",
                "making",
            ],
        ) {
            denied_tools.push("render_preview");
        }
        if explicitly_denies_target(
            request,
            &["剪映草稿", "剪映", "jianyingdraft", "jianying"],
            &[
                "",
                "生成",
                "创建",
                "制作",
                "交付",
                "generate",
                "generating",
                "create",
                "creating",
                "make",
                "making",
                "deliver",
                "delivering",
            ],
        ) {
            denied_tools.push("create_jianying_draft");
        }
        if explicitly_denies_target(
            request,
            &[
                "素材",
                "media",
                "asset",
                "assets",
                "素材分析",
                "分析素材",
                "mediaanalysis",
                "assetanalysis",
                "analyzemedia",
                "reanalyzemedia",
            ],
            &[
                "",
                "请求",
                "执行",
                "重新",
                "分析",
                "重新分析",
                "request",
                "run",
                "analyze",
                "analyzing",
                "reanalyze",
                "reanalyzing",
            ],
        ) {
            // 在线音乐会先下载媒体并触发本地分析，也属于用户排除的素材分析副作用。
            denied_tools.push("request_asset_analysis");
            denied_tools.push("download_music");
            denied_tools.push("use_online_music");
        }
        if explicitly_denies_target(
            request,
            &["30秒剪辑", "30secondedit"],
            &["", "做", "制作", "创建", "make", "create"],
        ) {
            denied_tools.push("generate_storyboard");
            denied_tools.push("create_timeline_draft");
        }
        if explicitly_denies_target(
            request,
            &["字幕", "subtitle", "subtitles", "caption", "captions"],
            &["", "加", "添加", "替换", "add", "replace", "edit"],
        ) {
            denied_tools.push("replace_text_tracks");
        }
        denied_tools.sort_unstable();
        denied_tools.dedup();
        let authorized_native_tools = explicitly_requested_native_tools(request);
        Self {
            denied_tools,
            read_only,
            authorized_native_tools,
        }
    }

    pub(super) fn forbids(&self, tool: &str) -> bool {
        self.denied_tools.contains(&tool)
    }

    /// Native 主链写工具必须由用户明确请求；没有授权时默认只暴露观察目录。
    pub(super) fn native_tool_exposed(&self, tool: &str) -> bool {
        !self.forbids(tool)
            && (OBSERVATION_TOOLS.contains(&tool) || self.native_write_authorized(tool))
    }

    pub(super) fn native_write_authorized(&self, tool: &str) -> bool {
        !self.forbids(tool) && self.authorized_native_tools.contains(&tool)
    }

    pub(super) fn has_native_write_authorization(&self) -> bool {
        self.authorized_native_write_tools().next().is_some()
    }

    pub(super) fn authorized_native_write_tools(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.authorized_native_tools
            .iter()
            .copied()
            .filter(|tool| !self.forbids(tool))
    }
}

fn request_clause_requests_read_only(clause: &str) -> bool {
    let compact = clause
        .to_lowercase()
        .chars()
        .filter(|character| !character.is_whitespace() && !matches!(character, '-' | '\'' | '’'))
        .collect::<String>();
    if compact.is_empty() {
        return false;
    }
    let rejects_read_only = [
        "不是只读",
        "非只读",
        "notreadonly",
        "notareadonly",
        "isntreadonly",
        "isntareadonly",
        "isnotreadonly",
        "isnotareadonly",
    ]
    .iter()
    .any(|phrase| compact.contains(phrase))
        || explicitly_denies_target(
            clause,
            &["只读", "只读模式", "readonly", "readonlymode"],
            &[
                "用",
                "使用",
                "采用",
                "保持",
                "继续",
                "设为",
                "设置为",
                "use",
                "using",
                "in",
                "keep",
                "keeping",
                "keepit",
                "keepingit",
                "leave",
                "leaveit",
                "set",
                "setto",
            ],
        )
        || ordered_action_before_target(
            &compact,
            &[
                "不要用",
                "不用",
                "别用",
                "不使用",
                "不要保持",
                "不用保持",
                "别保持",
                "不保持",
                "不要设置",
                "不要设为",
                "donotuse",
                "dontuse",
                "notuse",
                "donotkeep",
                "dontkeep",
                "notkeep",
                "donotleave",
                "dontleave",
                "donotset",
                "dontset",
                "donotmake",
                "dontmake",
                "notin",
            ],
            &["只读", "readonly"],
        );
    let explicitly_requests_read_only = compact == "只读"
        || compact == "readonly"
        || [
            "只读检查",
            "只读查看",
            "只读查询",
            "只读确认",
            "只读审计",
            "只检查",
            "只查看",
            "仅检查",
            "仅查看",
            "只读模式",
            "保持只读",
            "仅只读",
            "以只读",
            "readonlycheck",
            "readonlyinspect",
            "readonlyquery",
            "readonlyreview",
            "readonlyaudit",
            "readonlymode",
            "keepitreadonly",
            "stayreadonly",
            "readonlyonly",
            "readonlyrequest",
        ]
        .iter()
        .any(|phrase| compact.contains(phrase))
        || ordered_action_before_target(
            &compact,
            &[
                "保持",
                "继续",
                "设为",
                "设置为",
                "以",
                "keep",
                "stay",
                "set",
                "setto",
                "make",
            ],
            &["只读", "readonly"],
        );
    explicitly_requests_read_only && !rejects_read_only
}

fn ordered_action_before_target(text: &str, actions: &[&str], targets: &[&str]) -> bool {
    actions.iter().any(|action| {
        text.find(action).is_some_and(|action_start| {
            let after_action = &text[action_start + action.len()..];
            targets.iter().any(|target| after_action.contains(target))
        })
    })
}

/// 同时支持"不要 preview"和"不要生成 preview"两种顺序，避免负向边界漏判。
pub(super) fn explicitly_denies_target(request: &str, targets: &[&str], actions: &[&str]) -> bool {
    let compact = request
        .to_lowercase()
        .chars()
        .filter(|character| !character.is_whitespace() && !matches!(character, '-' | '\'' | '’'))
        .collect::<String>();
    let direct_negators = [
        "不",
        "不要",
        "不得",
        "无需",
        "不用",
        "禁止",
        "别",
        "不允许",
        "without",
    ];
    let action_negators = [
        "不",
        "不要",
        "不得",
        "无需",
        "不用",
        "禁止",
        "别",
        "不允许",
        "donot",
        "dont",
        "mustnot",
        "not",
        "noneedto",
        "noneedfor",
        "without",
    ];
    let articles = ["", "a", "an", "the", "any"];
    let compact_targets = targets
        .iter()
        .map(|target| {
            target
                .chars()
                .filter(|character| {
                    !character.is_whitespace() && !matches!(character, '-' | '\'' | '’')
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    let direct = direct_negators.iter().any(|negator| {
        articles.iter().any(|article| {
            compact_targets
                .iter()
                .any(|target| compact.contains(&format!("{negator}{article}{target}")))
        })
    });
    direct
        || action_negators.iter().any(|negator| {
            actions
                .iter()
                .filter(|action| !action.is_empty())
                .any(|action| {
                    articles.iter().any(|article| {
                        compact_targets.iter().any(|target| {
                            compact.contains(&format!("{negator}{action}{article}{target}"))
                        })
                    })
                })
        })
}
