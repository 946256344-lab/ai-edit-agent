//! Agent 循环的纯策略层。
//!
//! 本模块只根据用户文本和已存在的产物结果回答三个问题：本轮禁止哪些工具、
//! 目标产物是什么、当前结果是否满足完成门。它不得持有数据库、文件系统、Tauri、
//! Provider 或外部进程句柄，因此这里的判断本身不能产生任何副作用。

use crate::models::AgentEditResult;

/// 不创建/修改本地产物的观察技能；`search_music` 是受控外部查询，其余只读本地状态。
pub(super) const OBSERVATION_TOOLS: &[&str] = &[
    "get_edit_status",
    "get_asset_health_summary",
    "list_assets",
    "search_assets",
    "search_asset_segments",
    "search_music",
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
    "render_preview",
    "create_jianying_draft",
];

/// 一次请求必须真实达到的目标。循环不能只凭模型声称“完成”就越过产物门。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopGoal {
    Question,
    Storyboard,
    Timeline,
    Preview,
    JianyingDraft,
}

/// 明确表达时间线修改意图的动词；普通领域名词不能单独触发编辑。
const EDIT_VERBS: &[&str] = &[
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
    "剪辑",
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

/// 明确请求创建产物的动词；它们只锁定目标，不直接选择具体执行技能。
const CREATE_VERBS: &[&str] = &[
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
}

impl RequestToolPolicy {
    pub(super) fn from_request(request: &str) -> Self {
        let mut denied_tools = Vec::new();
        // 按分句判断只读，避免“不是只读，请调整”被一个孤立词误判为只读请求。
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
        denied_tools.sort_unstable();
        denied_tools.dedup();
        Self {
            denied_tools,
            read_only,
        }
    }

    pub(super) fn forbids(&self, tool: &str) -> bool {
        self.denied_tools.contains(&tool)
    }

    pub(super) fn forbids_goal(&self, goal: LoopGoal) -> bool {
        match goal {
            LoopGoal::Question => false,
            LoopGoal::Storyboard => self.forbids("generate_storyboard"),
            LoopGoal::Timeline => self.read_only,
            LoopGoal::Preview => self.forbids("render_preview"),
            LoopGoal::JianyingDraft => self.forbids("create_jianying_draft"),
        }
    }

    pub(super) fn prompt_label(&self) -> String {
        if self.denied_tools.is_empty() {
            "none".to_owned()
        } else {
            self.denied_tools.join(", ")
        }
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

/// 同时支持“不要 preview”和“不要生成 preview”两种顺序，避免负向边界漏判。
fn explicitly_denies_target(request: &str, targets: &[&str], actions: &[&str]) -> bool {
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

/// 通常表示用户在询问解释，而不是要求修改产物的表达。
const QUESTION_PHRASES: &[&str] = &[
    "为什么",
    "为何",
    "怎么",
    "如何",
    "请告诉我",
    "告诉我",
    "解释",
    "说明",
    "介绍一下",
    "是什么",
    "逻辑",
    "原因",
    "讲讲",
    "帮我看看",
    "请问",
    "能不能",
    "可不可以",
    "应该选",
    "建议",
];

/// 只为无歧义请求锁定目标；模糊表达交给首轮模型声明，不继续堆关键词直通分支。
pub(super) fn fast_goal(request: &str) -> Option<LoopGoal> {
    let text = request.trim().to_lowercase();
    let tool_policy = RequestToolPolicy::from_request(request);
    if tool_policy.read_only {
        return Some(LoopGoal::Question);
    }
    let contains = |words: &[&str]| words.iter().any(|word| text.contains(word));
    let edit_verb = contains(EDIT_VERBS);
    let create_verb = contains(CREATE_VERBS);
    let action_verb = edit_verb || create_verb;
    let question = text.ends_with('？') || text.ends_with('?') || contains(QUESTION_PHRASES);
    if question && !action_verb {
        return Some(LoopGoal::Question);
    }
    if question {
        return None;
    }
    if create_verb && contains(&["预览", "preview"]) && !tool_policy.forbids("render_preview") {
        return Some(LoopGoal::Preview);
    }
    if action_verb && contains(&["时间线", "timeline"]) {
        return Some(LoopGoal::Timeline);
    }
    if create_verb
        && contains(&["剪映", "jianying", "draft"])
        && !tool_policy.forbids("create_jianying_draft")
    {
        return Some(LoopGoal::JianyingDraft);
    }
    if create_verb && contains(&["storyboard", "分镜"]) {
        return Some(LoopGoal::Storyboard);
    }
    if edit_verb {
        return Some(LoopGoal::Timeline);
    }
    None
}

/// Router 解析失败时的保守兜底：当前项目事实必须至少执行一次真实观察。
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

pub(super) fn parse_declared_goal(
    goal: Option<&str>,
    is_question: Option<bool>,
) -> Option<LoopGoal> {
    if is_question == Some(true) {
        return Some(LoopGoal::Question);
    }
    match goal {
        Some("question") => Some(LoopGoal::Question),
        Some("storyboard") => Some(LoopGoal::Storyboard),
        Some("timeline") => Some(LoopGoal::Timeline),
        Some("preview") => Some(LoopGoal::Preview),
        Some("jianying" | "jianying_draft") => Some(LoopGoal::JianyingDraft),
        _ => None,
    }
}

pub(super) fn pinned_goal_allows_response(pinned_goal: Option<LoopGoal>) -> bool {
    pinned_goal.is_none() || pinned_goal == Some(LoopGoal::Question)
}

impl LoopGoal {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            LoopGoal::Question => "question",
            LoopGoal::Storyboard => "storyboard",
            LoopGoal::Timeline => "timeline",
            LoopGoal::Preview => "preview",
            LoopGoal::JianyingDraft => "jianying_draft",
        }
    }

    pub(super) fn label(&self) -> &'static str {
        match self {
            LoopGoal::Question => "问答/观察",
            LoopGoal::Storyboard => "storyboard",
            LoopGoal::Timeline => "内部时间线",
            LoopGoal::Preview => "preview",
            LoopGoal::JianyingDraft => "剪映草稿",
        }
    }

    /// 产物型目标只认真实返回对象；模型文字不能替代 storyboard、timeline 或文件。
    pub(super) fn satisfied_by(&self, last: &Option<AgentEditResult>) -> bool {
        match self {
            LoopGoal::Question => true,
            LoopGoal::Storyboard => last
                .as_ref()
                .is_some_and(|result| result.storyboard.is_some()),
            LoopGoal::Timeline => last
                .as_ref()
                .is_some_and(|result| result.timeline.is_some()),
            LoopGoal::Preview => last.as_ref().is_some_and(|result| result.preview.is_some()),
            LoopGoal::JianyingDraft => last
                .as_ref()
                .is_some_and(|result| result.jianying_draft.is_some()),
        }
    }
}

/// Provider 失败时只返回固定、诚实、不会泄露技术错误的用户文案。
pub(super) fn model_unavailable_message(goal: LoopGoal) -> String {
    match goal {
        LoopGoal::Question => "模型当前没有返回，因此本轮没有给出回答，也没有改动任何 storyboard、时间线或 preview；请检查模型连接后重试。".to_owned(),
        LoopGoal::Storyboard => "模型当前没有响应，本轮没有生成 storyboard，也没有修改现有内容；请检查模型连接后重试。".to_owned(),
        LoopGoal::Timeline => "模型当前没有响应，本轮没有修改内部时间线；请检查模型连接后重试。".to_owned(),
        LoopGoal::Preview => "模型当前没有响应，本轮没有生成 preview，也没有修改现有内容；请检查模型连接后重试。".to_owned(),
        LoopGoal::JianyingDraft => "模型当前没有响应，本轮没有创建剪映草稿；请检查模型连接后重试。".to_owned(),
    }
}

pub(super) fn run_deadline_message(goal: LoopGoal) -> String {
    match goal {
        LoopGoal::Question => {
            "本轮已达到交互等待时限，因此没有继续等待模型回答，也没有修改任何产物；请重试。"
                .to_owned()
        }
        LoopGoal::Storyboard => {
            "本轮已达到交互等待时限，没有生成新的 storyboard；现有内容未被覆盖。".to_owned()
        }
        LoopGoal::Timeline => {
            "本轮已达到交互等待时限，没有完成内部时间线修改；已落地的中间版本会保留供审阅。"
                .to_owned()
        }
        LoopGoal::Preview => {
            "本轮已达到交互等待时限，没有完成新的 preview；已落地的中间版本会保留供审阅。"
                .to_owned()
        }
        LoopGoal::JianyingDraft => {
            "本轮已达到交互等待时限，没有完成新的剪映草稿；已落地的中间版本会保留供审阅。"
                .to_owned()
        }
    }
}

/// 循环结束但没有目标产物时，禁止复述模型可能捏造的“已完成”。
pub(super) fn honest_no_change(goal: LoopGoal) -> String {
    match goal {
        LoopGoal::Question => {
            "本轮没有形成可用回答，也没有修改任何 storyboard、时间线或 preview。请补充说明后重试。".to_owned()
        }
        LoopGoal::Storyboard => "本轮没有生成新的 storyboard，也没有修改现有内容；如需继续，请补充创作目标后重试。".to_owned(),
        LoopGoal::Timeline => "本轮没有修改内部时间线，也没有把已完成的改动当成成功执行；如需继续，请说明你希望保留的具体片段后重试。".to_owned(),
        LoopGoal::Preview => "本轮没有生成新的 preview，也没有修改现有 storyboard、时间线或 preview；请补充说明后重试。".to_owned(),
        LoopGoal::JianyingDraft => "本轮没有创建新的剪映草稿，也没有修改现有内容；请补充说明后重试。".to_owned(),
    }
}

/// 模型过早结束时回送纠偏信息，循环仍受父模块的最大步数限制。
pub(super) fn corrective_message(goal: LoopGoal) -> String {
    match goal {
        LoopGoal::Question => "不要在没有执行任何技能时直接声称完成了剪辑操作。可以先用观察技能（list_assets/get_storyboard/get_timeline）获取信息，再给出如实回答。".to_owned(),
        LoopGoal::Storyboard => "你选择了结束，但尚未真正生成 storyboard。请调用 generate_storyboard 产出新版本后再结束；若缺少必要输入，请改用 ask_user 向用户澄清。".to_owned(),
        LoopGoal::Timeline => "你选择了结束，但尚未真正修改或创建内部时间线。请调用 create_timeline_draft、replace_clips、change_clip_duration 或 reorder_clips 产出新版本后再结束；若缺少必要输入，请改用 ask_user 向用户澄清。".to_owned(),
        LoopGoal::Preview => "你选择了结束，但尚未真正渲染出 preview。请先确保存在时间线，再调用 render_preview 产出 preview 后再结束；若缺少必要输入，请改用 ask_user 向用户澄清。".to_owned(),
        LoopGoal::JianyingDraft => "你选择了结束，但尚未真正创建剪映草稿。请调用 create_jianying_draft 产出草稿后再结束；若缺少必要输入，请改用 ask_user 向用户澄清。".to_owned(),
    }
}
