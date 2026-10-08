//! 平台兼容层：把各平台 hook 的「原始负载」归一化成内核事件。
//!
//! 分层约定（见 `reference/doc/guide/agent-status-platform-onboarding-guide.md` §7–§9）：
//!
//! ```text
//! claude hook payload ─┐
//!                      ├─▸ compat（这里）：payload → AgentEvent{ kind, … }
//! 其它平台 payload  ───┘
//!                                      │
//!                                      ▼
//!             kernel：只认 AgentEventKind，不解析任何平台事件名
//! ```
//!
//! **接入新平台：新建一个 `compat/<platform>.rs` + 在 [`adapter_for`] 里加一行即可跑通事件链。**
//! 适配器负责平台的全部知识（字段别名、事件名表、base_url 来源、提问提取…），
//! hook 外壳（`super::hook`）只做与平台无关的信封活：读 stdin、补事件 id / pid / 接收时刻、落盘。
//!
//! ⚠️ 但「只改这两处」是不够的：`Provider` 是闭包枚举，另有 `base_url_env_names`（本文件）、
//! `claude::provider_base_url`、`proxy::AgentApp::from_provider` 三处**穷尽 match 会编译报错**，
//! 逐个点名要登记的地方（这是特性不是缺陷 —— 漏登记编译不过）。前端展示名/图标走
//! `src/modules/agent-status/lib/provider-registry.ts` 加一行。
//! 完整改动清单与验收见 `reference/doc/guide/agent-status-platform-onboarding-guide.md`。

pub(crate) mod claude;
pub(crate) mod codex;
pub(crate) mod qoder;

use std::{env, path::PathBuf};

use serde_json::Value;

use crate::modules::agent_status::status::model::{
    AgentEvent, AgentEventKind, Provider, SCHEMA_VERSION,
};

/// Claude 的 base_url 变量名（settings.json 的 `env` 块 key，也是进程环境变量名）。
///
/// 放在共享层而非 claude.rs，是因为 [`base_url_env_names`]（通用 helper）也要引用它。
pub(crate) const CLAUDE_BASE_URL_KEY: &str = "ANTHROPIC_BASE_URL";

/// 适配上下文：与平台无关的运行环境，由 hook 外壳在调用适配器前收集好。
pub(crate) struct AdaptContext<'a> {
    /// hook payload 里的 `cwd`，用于定位项目级配置文件。
    pub cwd: Option<&'a std::path::Path>,
}

/// 兼容层归一化产出的**语义字段**；信封字段（schema_version / event_id / provider /
/// received_at_ms / pid）由 hook 外壳补，适配器不用管。
pub(crate) struct AdaptedEvent {
    /// 会话标识（归一化后的字符串）。
    pub session_id: String,
    /// 归一化出的事件种类。
    pub kind: AgentEventKind,
    /// 平台原始事件名，仅诊断用。
    pub event: String,
    /// 子代理（subagent）事件来源标识；主线程事件为 None。
    pub agent_id: Option<String>,
    /// 错误码 / 结果字段别名。
    pub error_code: Option<String>,
    pub outcome: Option<String>,
    /// Notification 的 notification_type，仅诊断用。
    pub notification_type: Option<String>,
    /// 会话实际使用的 API endpoint。
    pub base_url: Option<String>,
    /// 用户最近一次提交的提问。
    pub prompt: Option<String>,
    /// 「正在等你操作」的原因原文（如审批理由）。
    pub waiting_reason: Option<String>,
    /// transcript（JSONL）的绝对路径，供 monitor 增量统计 token。
    pub transcript_path: Option<String>,
    /// 可选序号，用于乱序过滤。
    pub sequence: Option<u64>,
    /// 事件发生时间（毫秒）。
    pub occurred_at_ms: Option<i64>,
}

/// 事件信封：与平台无关、由 hook 外壳填充的字段。
pub(crate) struct EventEnvelope {
    pub provider: Provider,
    pub event_id: String,
    /// 贯穿各环节的日志关联 id（由 hook 生成，**不来自 payload**，见 [`AgentEvent::trace_id`]）。
    pub trace_id: String,
    pub received_at_ms: i64,
    pub pid: Option<u32>,
}

impl AdaptedEvent {
    /// 补上信封字段，得到可写盘、可入内核的 [`AgentEvent`]。
    pub(crate) fn into_event(self, envelope: EventEnvelope) -> AgentEvent {
        AgentEvent {
            schema_version: SCHEMA_VERSION,
            trace_id: Some(envelope.trace_id),
            event_id: envelope.event_id,
            provider: envelope.provider,
            session_id: self.session_id,
            kind: self.kind,
            event: self.event,
            sequence: self.sequence,
            occurred_at_ms: self.occurred_at_ms,
            received_at_ms: envelope.received_at_ms,
            pid: envelope.pid,
            error_code: self.error_code,
            outcome: self.outcome,
            notification_type: self.notification_type,
            base_url: self.base_url,
            prompt: self.prompt,
            transcript_path: self.transcript_path,
            waiting_reason: self.waiting_reason,
            agent_id: self.agent_id,
        }
    }
}

/// 平台 hook 适配器：把某平台的原始负载翻译成内核事件。
///
/// 三个返回值各有含义，别混：
/// - `Ok(Some(event))`：归一化成功，落盘喂内核；
/// - `Ok(None)`：**这条事件本适配器不关心，静默忽略**（不落盘、不算失败）。
///   Codex 用它把「只要 PermissionRequest、其余全部忽略」表达出来 —— 若返回 Err，
///   hook 会记失败日志并把噪声写进 hook-debug.log；
/// - `Err(_)`：真的坏了（缺 session id 等），hook 外壳记日志并非零退出。
pub(crate) trait HookAdapter: Sync {
    fn adapt(&self, payload: &Value, ctx: &AdaptContext<'_>) -> Result<Option<AdaptedEvent>, String>;
}

/// 按 provider 取适配器；尚未接入的平台返回 None（hook 外壳给出明确报错）。
///
/// 接入新平台时，除了加 `compat/<platform>.rs`，在这里加一条 match 分支即可。
pub(crate) fn adapter_for(provider: Provider) -> Option<&'static dyn HookAdapter> {
    match provider {
        Provider::Claude => Some(&claude::ClaudeAdapter),
        // Codex 走 rollout（`status::codex_rollout`），hook 只保留一个极窄的补充：
        // `PermissionRequest` —— 那是 rollout 里唯一拿不到的状态。见 `compat::codex` 的模块文档。
        Provider::Codex => Some(&codex::CodexPermissionAdapter),
        // Qoder（CLI 与 IDE 共用同一份 hook 配置）走完整 hook 链路，不需要第二个事件源。
        Provider::Qoder => Some(&qoder::QoderAdapter),
    }
}

/// 从 payload 里按多别名取第一个字符串字段。
pub(crate) fn string_field(payload: &Value, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        payload
            .get(*name)
            .and_then(Value::as_str)
            .map(ToString::to_string)
    })
}

/// 从 payload 里按多别名取第一个 u64 字段。
pub(crate) fn number_field(payload: &Value, names: &[&str]) -> Option<u64> {
    names
        .iter()
        .find_map(|name| payload.get(*name).and_then(Value::as_u64))
}

/// 归一化事件名：大小写、横线、点号和冒号差异都收敛成 snake_case。
///
/// 各适配器在查自己的事件名表前先过一遍这里，避免同一语义的多种写法各占一行。
pub(crate) fn normalize_event_name(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len() + 4);
    let mut prev = '\0';
    for character in value.chars() {
        if character.is_ascii_uppercase() && prev.is_ascii_lowercase() {
            normalized.push('_');
        }
        if matches!(character, '-' | '.' | ':') {
            normalized.push('_');
        } else {
            normalized.push(character.to_ascii_lowercase());
        }
        prev = character;
    }
    normalized
}

/// 对事件名表未覆盖的事件名做保守兜底，避免适配器漏项时完全失明。
pub(crate) fn fallback_kind(event_name: &str) -> AgentEventKind {
    if event_name.contains("error") || event_name.contains("fail") {
        return AgentEventKind::Failed;
    }
    if event_name.contains("start") || event_name.contains("tool") {
        return AgentEventKind::Running;
    }
    if event_name.contains("stop") || event_name.contains("wait") {
        return AgentEventKind::WaitingForUser;
    }
    AgentEventKind::Unknown
}

/// 判断 payload 的字段是否已明确表示错误。
///
/// 语义：`error_code` 有值，或 `outcome` 落在失败/取消/拒绝类取值。各适配器在分类前
/// 先用它短路 —— 错误字段优先于事件名，与内核旧行为一致。
pub(crate) fn payload_error(error_code: Option<&str>, outcome: Option<&str>) -> bool {
    error_code.is_some()
        || outcome.is_some_and(|outcome| {
            matches!(
                outcome.to_ascii_lowercase().as_str(),
                "error" | "failed" | "failure" | "cancelled" | "denied"
            )
        })
}

/// 该事件是否为「用户提交了提问」。
///
/// 两个平台的提问都**只**在这一刻采集：后续事件（工具调用等）本就不带提问字段，
/// 由内核继承上一次的值。Claude 另有 `UserPromptExpansion`（斜杠命令展开的模板文本），
/// 刻意不认；Codex 没有该事件。
pub(crate) fn is_user_prompt_submit(event_name: &str) -> bool {
    normalize_event_name(event_name) == "user_prompt_submit"
}

/// 提问内容长度上限（字符数）。
///
/// 仅防止超大 prompt 撑爆事件文件（hook 侧唯一的整体额度是 stdin 的 `MAX_HOOK_INPUT` = 4 MiB，
/// 落盘的事件文件本身没有整体上限），超出部分截断并加省略号。
pub(crate) const MAX_PROMPT_CHARS: usize = 4000;

/// 归一化提问内容：去首尾空白，超长按**字符**截断并追加省略号；空白内容视为未提供。
///
/// 注意：提问内容会以明文写入状态目录的 inbox 事件文件（消费后删除）。
/// 这是「展示当前提问」功能的必要代价，已在文档中标注。
pub(crate) fn truncate_prompt(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.chars().count() <= MAX_PROMPT_CHARS {
        return Some(trimmed.to_string());
    }
    let truncated: String = trimmed.chars().take(MAX_PROMPT_CHARS).collect();
    Some(format!("{truncated}…"))
}

/// base_url 长度上限。
///
/// 这不是脱敏，只是防止异常环境变量把超大字符串写进事件文件（整体额度只有 hook stdin 的
/// `MAX_HOOK_INPUT` = 4 MiB，事件文件本身不再另设上限 —— 与 `MAX_PROMPT_CHARS` 同一口径）。
pub(crate) const MAX_BASE_URL_LEN: usize = 4096;

/// 归一化 base_url：仅去掉首尾空白。
///
/// 不剥离 userinfo / query —— 按需求保留原值，便于排查实际使用的 endpoint。
pub(crate) fn normalize_base_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_BASE_URL_LEN {
        return None;
    }
    Some(trimmed.to_string())
}

/// 子代理标识的长度上限：只防异常 payload 撑大事件文件，不做语义校验。
pub(crate) const MAX_AGENT_ID_LEN: usize = 256;

/// 提取子代理事件的来源标识。主线程事件不带此字段，返回 None。
///
/// ⚠️ 两个平台都确认：子代理事件与主会话**共用同一个 `session_id`**，只有本字段能把两者
/// 分开（Claude 与 Codex 均为主线程不带、子代理事件带 `agent_id`）。内核靠它维护「活跃子代理
/// 集合」并按主线程所处状态分流：主线程在等用户操作（WaitingForUser）、或会话已结束 / 已出错
/// 时，子代理事件**只**记账并刷新存活时间、不得转灯（否则审批黄灯会被顶回 Running）；只有主线程
/// 已停、处于「等 prompt」空闲时，子代理活动才把灯拉回后台运行（对外绿常亮）。
/// 子代理自己的 `Failed` 也不染红父灯。只透传标识，语义全在内核 `reduce_state`。
pub(crate) fn agent_id(payload: &Value) -> Option<String> {
    string_field(payload, &["agent_id", "agentId"])
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && value.len() <= MAX_AGENT_ID_LEN)
}

/// 路径长度上限，防止异常 payload 把超长字符串写进事件文件。
pub(crate) const MAX_TRANSCRIPT_PATH_LEN: usize = 4096;

/// 提取该会话 transcript 的路径，供 monitor 增量统计 token。
///
/// 这里**只透传路径**，不读文件：hook 每次事件启动一次、必须快速返回，
/// 而 transcript 会增长到 MB 级，解析必须放在常驻的 monitor 线程里做。
///
/// 两个平台的字段名一致（`transcript_path`）；Codex 侧可能为 null → None。
pub(crate) fn transcript_path(payload: &Value) -> Option<String> {
    string_field(payload, &["transcript_path", "transcriptPath"])
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && value.len() <= MAX_TRANSCRIPT_PATH_LEN)
}

/// 各平台对应的 base_url 环境变量名，按优先级排列。
pub(crate) fn base_url_env_names(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Claude => &[CLAUDE_BASE_URL_KEY],
        Provider::Codex => &["OPENAI_BASE_URL", "OPENAI_API_BASE"],
        // Qoder 由云端网关按登录态路由：`settings.json` 没有 `env` 块，环境变量里也没有
        // endpoint 类变量（实测）。它的 Endpoint 行来自 `compat::qoder` 自己的身份常量。
        Provider::Qoder => &[],
    }
}

/// 提问里标签的处理动作 —— 两种语义覆盖已观测到的全部形态。
///
/// 引擎本身不认识任何平台：动作、标签名、命中门槛都由规则表给（见 [`apply_tag_rules`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TagAction {
    /// 删掉 `<t>…</t>` **连同内文**：平台注入的元信息（提醒、环境说明、命令模板）。
    Drop,
    /// 只保留 `<t>…</t>` **内的文本**，标签外一律丢弃：用户正文被平台包进标签里的形态。
    ///
    /// ⚠️ 与 `Drop` 恰好相反 —— 拿它删标签会连用户的提问一起丢掉，配错方向是看得见的事故。
    Extract,
}

/// 一条标签规则。字段用 `String`：规则表要从 `.env` 注入的列表构造。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TagRule {
    /// 标签名（不含尖括号），只允许 `[A-Za-z0-9_-]`。
    pub name: String,
    /// 命中后怎么处理。
    pub action: TagAction,
    /// 命中门槛：`None` = 任何时候都适用；`Some(前缀)` = 仅当文本以该前缀开头才适用。
    ///
    /// 门槛是给「正文被包进标签」那类形态用的：平台只在特定轮次（如设置目标）改写提问，
    /// 普通轮次根本没有那个标签。没有门槛就会把普通提问整个判成「无正文」。
    pub when_prefix: Option<String>,
}

#[cfg(test)]
impl TagRule {
    /// 无条件删除某标签及其内容。
    pub(crate) fn drop(name: &str) -> Self {
        Self { name: name.to_string(), action: TagAction::Drop, when_prefix: None }
    }

    /// 只保留某标签内的正文（不带门槛）；要门槛用 [`TagRule::when_starts_with`] 续上。
    pub(crate) fn extract(name: &str) -> Self {
        Self { name: name.to_string(), action: TagAction::Extract, when_prefix: None }
    }

    /// 挂上命中门槛：规则只处理以该前缀开头的文本。
    pub(crate) fn when_starts_with(mut self, prefix: &str) -> Self {
        self.when_prefix = Some(prefix.to_string());
        self
    }
}

/// 一次标签命中的结果（偏移都相对**传入的整段文本**，且恒落在字符边界上）。
struct TagSpan {
    /// 起始 `<` 的位置。
    open_at: usize,
    /// 起始标记结束后的位置（正文从这里开始）。
    body_at: usize,
    /// 显式闭合 `</name…>` 的起点；`None` = 没找到闭合。
    close_at: Option<usize>,
    /// 闭合标记之后的位置；`None` = 没找到闭合。
    close_end: Option<usize>,
    /// 自闭合（`<t/>`）：本来就没有正文。
    self_closing: bool,
}

/// 标签名的合法字符集：ASCII 字母数字与 `_` `-`。
fn is_tag_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|character| {
                character.is_ascii_alphanumeric() || character == '_' || character == '-'
            })
}

/// 判断 `rest`（以 `<` 开头）是否为 `name` 的开始标签，命中返回 `name` 之后的字节偏移。
///
/// 边界要求：`<name` 之后必须紧跟 ASCII 标点（`>`、`/`）或空白（属性位）。
/// 这样 `<environment>` 不会被规则里的 `env` 误命中（`env` 之后是 `i`）。
/// 用 `get` 而非裸切片：`<` 之后可能是多字节字符，按字节切会 panic。
fn matches_open_tag(rest: &str, name: &str) -> Option<usize> {
    let body = rest.get(1..1 + name.len())?;
    if body != name {
        return None;
    }
    let after = rest.get(1 + name.len()..)?;
    if after
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_whitespace() || character.is_ascii_punctuation())
    {
        Some(1 + name.len())
    } else {
        None
    }
}

/// 找到 `text` 里第一个 `<name…>` 标签，标出正文边界。
///
/// 起始标记以第一个 `>` 结束（`/>` 即自闭合）。「没有闭合」怎么办留给调用方，两家要的相反：
/// - `Drop`：只跳过起始标记 —— 归属不明的正文不能顺手吞掉（删标签的既有语义）；
/// - `Extract`：把起始标记之后全部当正文 —— 模板被截断时这是唯一一份正文。
///
/// ⚠️ 不支持标签内再套同名标签（实测无此形态）。`<` `>` `/` 与标签名都是 ASCII，
/// 因此下面所有字节偏移都落在字符边界上；多字节正文只整体搬运，不按字节切。
fn find_tag_span(text: &str, name: &str) -> Option<TagSpan> {
    let mut offset = 0usize;
    while let Some(relative) = text[offset..].find('<') {
        let open_at = offset + relative;
        let rest = &text[open_at..];
        if let Some(after_name) = matches_open_tag(rest, name) {
            let head = match rest[after_name..].find('>') {
                Some(at) => after_name + at + 1,
                // 起始标记本身被截断（没有 `>`）：后面全是正文。
                None => rest.len(),
            };
            let self_closing = rest.as_bytes().get(head.saturating_sub(2)) == Some(&b'/');
            let closing = format!("</{name}");
            // 下面两个偏移一律换算回 text 的坐标系：调用方直接拿它们切 text 才保证字符边界。
            let close_at = rest[head..].find(&closing).map(|at| open_at + head + at);
            // 闭合标记自身没有 `>`（畸形输入）时停在它前面，至少标签外的正文还在。
            let close_end = close_at.map(|at| match text[at..].find('>') {
                Some(gt) => at + gt + 1,
                None => at,
            });
            return Some(TagSpan { open_at, body_at: open_at + head, close_at, close_end, self_closing });
        }
        offset = open_at + 1;
    }
    None
}

/// 按规则表处理一段文本里的标签，返回「处理后的正文」；`None` = 没有可用正文。
///
/// 规则**按顺序**应用（例如先 `Drop` 掉提醒标签，再 `Extract` 正文标签）。
/// `Extract` 是终局动作 —— 它定义了「正文在哪」，命中就直接返回，后面的规则不再跑。
/// 规则名非法（空 / 含标签字符集外的字符）整条跳过，绝不 panic（release 是 `panic = "abort"`）。
pub(crate) fn apply_tag_rules(text: &str, rules: &[TagRule]) -> Option<String> {
    let mut current = text.to_string();
    for rule in rules {
        if !is_tag_name(&rule.name) {
            continue;
        }
        // 命中门槛不满足 → 整条跳过（普通提问不能被 Extract 判成「无正文」）。
        if rule
            .when_prefix
            .as_ref()
            .is_some_and(|prefix| !current.trim_start().starts_with(prefix.as_str()))
        {
            continue;
        }
        match rule.action {
            TagAction::Drop => current = drop_tag(&current, &rule.name),
            TagAction::Extract => {
                // 门槛命中却找不到标签：模板改版或被截断到只剩脚手架，
                // 这时上报的不会是用户的话，判为「没有可用正文」。
                return extract_tag(&current, &rule.name).filter(|body| !body.trim().is_empty());
            }
        }
    }
    (!current.trim().is_empty()).then_some(current)
}

/// 删掉文本里所有 `<name…>…</name>`（连同内文），其余原样保留。
fn drop_tag(text: &str, name: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(span) = find_tag_span(rest, name) {
        out.push_str(&rest[..span.open_at]);
        // 没有显式闭合时只跳过起始标记（`body_at`），把归属不明的正文留给下一轮。
        let next = span.close_end.unwrap_or(span.body_at);
        rest = &rest[next.min(rest.len())..];
    }
    out.push_str(rest);
    out
}

/// 取出文本里所有 `<name…>…</name>` 的内文，按出现顺序用换行拼接；一个都没有返回 None。
fn extract_tag(text: &str, name: &str) -> Option<String> {
    let mut pieces: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(span) = find_tag_span(rest, name) {
        let body = match (span.close_at, span.self_closing) {
            // 自闭合：本来就没有正文。
            (_, true) => "",
            (Some(close_at), false) => rest[span.body_at..close_at].trim(),
            // 没有闭合：起始标记之后全是正文（被截断的模板）。
            (None, false) => rest[span.body_at..].trim(),
        };
        if !body.is_empty() {
            pieces.push(body.to_string());
        }
        let Some(next) = span.close_end else {
            break; // 没有闭合就没有下一个标签了。
        };
        if next >= rest.len() {
            break;
        }
        rest = &rest[next..];
    }
    (!pieces.is_empty()).then(|| pieces.join("\n"))
}

/// 解析规则表文本：`name`（默认 `drop`）或 `name:drop` / `name:extract`。
///
/// 逗号分隔、忽略空白、剥掉误写的尖括号；非法项（动作名不认识、名字含标签字符集外的字符）
/// 整项丢弃但不影响其它项 —— 一个坏配置不该让提问采集整个失效。
pub(crate) fn parse_tag_rules(raw: &str) -> Vec<TagRule> {
    let mut rules = Vec::new();
    for item in raw.split(',') {
        let item = item.trim().trim_matches(|c| c == '<' || c == '>').trim();
        if item.is_empty() {
            continue;
        }
        let (name, action) = match item.split_once(':') {
            Some((name, action)) => match action.trim() {
                "" | "drop" => (name.trim(), TagAction::Drop),
                "extract" => (name.trim(), TagAction::Extract),
                // 动作名写错：丢掉整项，别猜用户想干什么。
                _ => continue,
            },
            None => (item, TagAction::Drop),
        };
        if is_tag_name(name) {
            rules.push(TagRule { name: name.to_string(), action, when_prefix: None });
        }
    }
    rules
}

/// 由「构建期注入的值 + 本平台内置默认」拼出规则表。
///
/// 注入值为空（没配 `.env` 里的键，或值为空串）时用默认表；解析不出任何合法项时也回落默认 ——
/// 一份写坏的配置不该让提问采集整个失效。默认表按 `Drop` 在前、`Extract` 在后排序：
/// `Extract` 是终局动作，排在前面会挡住本该先做的删除。
pub(crate) fn build_tag_rules(from_env: Option<&str>, defaults: &str) -> Vec<TagRule> {
    let mut rules = from_env.map(parse_tag_rules).unwrap_or_default();
    if rules.is_empty() {
        rules = parse_tag_rules(defaults);
    }
    rules.sort_by_key(|rule| match rule.action {
        TagAction::Drop => 0,
        TagAction::Extract => 1,
    });
    rules
}

/// 解析用户主目录，用于定位用户级配置与代理解析器（`~/.cc-switch`）。
///
/// 与 `default_status_dir` 一样直接读环境变量，不引入 dirs 依赖。
pub(crate) fn home_dir() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        env::var_os("USERPROFILE").map(PathBuf::from).or_else(|| {
            // 少数环境下只有 HOMEDRIVE + HOMEPATH（如 C: + \Users\me）。
            let mut combined = env::var_os("HOMEDRIVE")?;
            combined.push(env::var_os("HOMEPATH")?);
            Some(PathBuf::from(combined))
        })
    } else {
        env::var_os("HOME").map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_field_reads_first_matching_name() {
        let payload: Value = serde_json::json!({"session_id": "abc", "sessionId": "def"});
        assert_eq!(string_field(&payload, &["session_id"]), Some("abc".to_string()));
        assert_eq!(
            string_field(&payload, &["sessionId", "session_id"]),
            Some("def".to_string())
        );
    }

    #[test]
    fn string_field_returns_none_for_missing() {
        let payload: Value = serde_json::json!({});
        assert_eq!(string_field(&payload, &["missing"]), None);
    }

    #[test]
    fn number_field_reads_u64_value() {
        let payload: Value = serde_json::json!({"sequence": 42});
        assert_eq!(number_field(&payload, &["sequence"]), Some(42));
    }

    #[test]
    fn number_field_returns_none_for_missing() {
        let payload: Value = serde_json::json!({});
        assert_eq!(number_field(&payload, &["seq"]), None);
    }

    #[test]
    fn normalize_event_name_handles_camel_case() {
        assert_eq!(normalize_event_name("TurnCompleted"), "turn_completed");
        assert_eq!(normalize_event_name("SessionEnd"), "session_end");
    }

    #[test]
    fn normalize_event_name_handles_separators() {
        assert_eq!(normalize_event_name("session-end"), "session_end");
        assert_eq!(normalize_event_name("session.end"), "session_end");
        assert_eq!(normalize_event_name("session:end"), "session_end");
    }

    #[test]
    fn normalize_event_name_lowercases() {
        assert_eq!(normalize_event_name("STOP"), "stop");
        assert_eq!(normalize_event_name("Start"), "start");
    }

    /// 适配器注册表：两个平台都有，但 Codex 那个**极窄** —— 只认 `PermissionRequest`。
    ///
    /// 这条断言防的是「顺手把 Codex 的其它事件也接进来」：多认一个事件，就多一处与
    /// rollout 抢同一盏灯的地方（rollout 说绿、hook 说黄，灯就会跳）。
    #[test]
    fn codex_adapter_only_accepts_permission_request() {
        let adapter = adapter_for(Provider::Codex).expect("Codex 有极窄适配器");
        let ctx = AdaptContext { cwd: None };

        // 其余事件一律静默忽略（Ok(None)），不算失败、不落盘。
        for name in ["PreToolUse", "PostToolUse", "Stop", "SessionStart", "Interrupt"] {
            let payload = serde_json::json!({"session_id": "s1", "hook_event_name": name});
            assert!(
                adapter.adapt(&payload, &ctx).expect("不报错").is_none(),
                "{name} 不该被 Codex 适配器接受"
            );
        }

        // 只认这一个：等人授权 → 黄灯，且带上审批理由。
        let payload = serde_json::json!({
            "session_id": "s1",
            "hook_event_name": "PermissionRequest",
            "tool_name": "Bash",
            "tool_input": {"command": "curl …", "description": "是否允许我访问官网"},
        });
        let adapted = adapter
            .adapt(&payload, &ctx)
            .expect("不报错")
            .expect("PermissionRequest 必须被接受");
        assert_eq!(adapted.kind, AgentEventKind::WaitingForUser);
        assert_eq!(adapted.waiting_reason.as_deref(), Some("是否允许我访问官网"));
        // ⚠️ 理由只走 waiting_reason，绝不能污染用户的提问。
        assert_eq!(adapted.prompt, None);
    }

    #[test]
    fn base_url_env_names_are_provider_specific() {
        assert_eq!(base_url_env_names(Provider::Claude), &[CLAUDE_BASE_URL_KEY]);
        // Codex 先试 OPENAI_BASE_URL，再退 OPENAI_API_BASE。
        assert_eq!(
            base_url_env_names(Provider::Codex),
            &["OPENAI_BASE_URL", "OPENAI_API_BASE"]
        );
        // Qoder 没有 endpoint 类环境变量（云端网关按登录态路由），故为空。
        assert!(base_url_env_names(Provider::Qoder).is_empty());
    }

    /// Qoder 与 Claude 同形：**每个事件都归一出结果**，没有一个要像 Codex 那样被忽略。
    ///
    /// 反向护栏 —— 哪天有人给 Qoder 也加「只认几个事件」的收窄，未列出的事件会静默不落盘，
    /// 表现是灯在某些节点死活不动。
    #[test]
    fn qoder_adapter_is_registered_and_accepts_every_event() {
        let adapter = adapter_for(Provider::Qoder).expect("Qoder 有适配器");
        let ctx = AdaptContext { cwd: None };
        for name in [
            "SessionStart",
            "UserPromptSubmit",
            "PreToolUse",
            "PostToolUse",
            "Notification",
            "Stop",
            "SessionEnd",
        ] {
            let payload = serde_json::json!({"session_id": "s1", "hook_event_name": name});
            assert!(
                adapter.adapt(&payload, &ctx).expect("不报错").is_some(),
                "{name} 不该被 Qoder 适配器忽略"
            );
        }
    }

    #[test]
    fn drop_removes_tag_and_its_wrapped_content() {
        let rules = [TagRule::drop("system-reminder")];
        assert_eq!(
            apply_tag_rules("A<system-reminder>x</system-reminder>B", &rules).as_deref(),
            Some("AB")
        );
        // 同名多个标签全部删掉。
        assert_eq!(
            apply_tag_rules("<system-reminder>a</system-reminder>keep<system-reminder>b</system-reminder>", &rules).as_deref(),
            Some("keep")
        );
    }

    #[test]
    fn drop_requires_a_boundary_so_a_name_prefix_does_not_over_match() {
        // 规则 `env` 不该命中 `<environment>`：`env` 之后是 `i`，不是边界字符。
        let rules = [TagRule::drop("env")];
        let text = "<environment>x</environment>";
        assert_eq!(apply_tag_rules(text, &rules).as_deref(), Some(text));
    }

    #[test]
    fn drop_handles_self_closing_and_unclosed_open_tag() {
        let rules = [TagRule::drop("tag")];
        assert_eq!(apply_tag_rules("<tag/>keep", &rules).as_deref(), Some("keep"));
        assert_eq!(apply_tag_rules("<tag a=1>keep", &rules).as_deref(), Some("keep"));
        assert_eq!(apply_tag_rules("<tag>keep me", &rules).as_deref(), Some("keep me"));
    }

    #[test]
    fn drop_is_safe_on_multibyte_content() {
        // 偏移全按 ASCII 标记算：中文再多也不能把切片切到非字符边界上（panic 会带走整个进程）。
        let rules = [TagRule::drop("tag")];
        assert_eq!(
            apply_tag_rules("中文 <tag>内嵌</tag> 更多 汉字", &rules).as_deref(),
            Some("中文  更多 汉字")
        );
    }

    #[test]
    fn extract_keeps_only_the_tagged_body() {
        let text = "平台前言\n\n<obj>\n真正的提问\n</obj>\n\n后面的模板说明";
        assert_eq!(apply_tag_rules(text, &[TagRule::extract("obj")]).as_deref(), Some("真正的提问"));
    }

    #[test]
    fn extract_joins_multiple_tags_in_order() {
        let text = "前<a>一</a>中<a>二</a>后";
        assert_eq!(apply_tag_rules(text, &[TagRule::extract("a")]).as_deref(), Some("一\n二"));
    }

    #[test]
    fn extract_gate_prefix_decides_whether_the_rule_applies() {
        let rule = TagRule::extract("obj").when_starts_with("A goal has been set.");
        // 前缀不命中 → 原样返回（普通提问不能被判成「无正文」）。
        let plain = "帮我改个 bug";
        assert_eq!(apply_tag_rules(plain, std::slice::from_ref(&rule)).as_deref(), Some(plain));
        // 前缀命中且标签在 → 只留标签内正文。
        let wrapped = "A goal has been set.\n<obj>目标正文</obj>\nturns used 0/20";
        assert_eq!(apply_tag_rules(wrapped, std::slice::from_ref(&rule)).as_deref(), Some("目标正文"));
        // 前缀命中但标签没了（模板改版 / 被截断）→ 判为没有可用正文，不把脚手架当提问。
        assert_eq!(apply_tag_rules("A goal has been set.\n只剩模板", &[rule]), None);
    }

    #[test]
    fn extract_treats_unclosed_open_tag_as_body_to_the_end() {
        // 提问被上游额度截断时闭合标签会消失：取开标签之后全部，比丢掉整条提问更有用。
        assert_eq!(
            apply_tag_rules("模板\n<obj>正文开头", &[TagRule::extract("obj")]).as_deref(),
            Some("正文开头")
        );
        // 只有闭标签：边界判不出，返回 None（调用方沿用上一次的值）。
        assert_eq!(apply_tag_rules("正文</obj>", &[TagRule::extract("obj")]), None);
    }

    #[test]
    fn rules_apply_in_order_and_extract_is_terminal() {
        // Drop 先**整体**清扫（正文里同名的标签也一并删掉），Extract 最后取正文；
        // 反过来排就会错：Extract 一命中即返回，后面的 Drop 永远轮不到。
        let rules = [TagRule::drop("note"), TagRule::extract("obj")];
        assert_eq!(
            apply_tag_rules("<note>提醒</note><obj>正文 <note>也该删</note></obj>", &rules).as_deref(),
            Some("正文")
        );
    }

    #[test]
    fn invalid_tag_names_are_skipped_without_panicking() {
        let junk = [
            TagRule::drop(""),
            TagRule { name: "a<b".to_string(), action: TagAction::Drop, when_prefix: None },
            TagRule { name: "中文".to_string(), action: TagAction::Extract, when_prefix: None },
        ];
        let text = "原样返回";
        assert_eq!(apply_tag_rules(text, &junk).as_deref(), Some(text));
    }

    #[test]
    fn empty_rules_pass_the_text_through() {
        assert_eq!(apply_tag_rules("  提问  ", &[]).as_deref(), Some("  提问  "));
        // 只剩空白 → 没有可用正文。
        assert_eq!(apply_tag_rules("   \n ", &[]), None);
    }

    #[test]
    fn parse_tag_rules_strips_brackets_and_defaults_to_drop() {
        // `.env` 里习惯写 <name>：尖括号与空白都是分隔噪声，只留标签名本体。
        let rules = parse_tag_rules(" <ide_opened_file> , system-reminder ,, <env> ");
        assert_eq!(
            rules,
            vec![TagRule::drop("ide_opened_file"), TagRule::drop("system-reminder"), TagRule::drop("env")]
        );
        assert!(parse_tag_rules("").is_empty());
        assert!(parse_tag_rules("  ,,  ").is_empty());
    }

    #[test]
    fn parse_tag_rules_reads_actions_and_discards_bad_items() {
        let rules = parse_tag_rules("note:extract, junk:shred, <env>, 中文tag:drop");
        // 认得的动作生效、不认得的整项丢掉、非法标签名也丢掉，但**不影响同表其它项**。
        assert_eq!(rules, vec![TagRule::extract("note"), TagRule::drop("env")]);
    }

    #[test]
    fn engine_holds_no_platform_vocabulary() {
        // 边界护栏：机制在共享层、策略在平台文件里 —— 引擎源码里出现任何平台名都说明搬错了。
        let source = include_str!("mod.rs");
        let engine = source
            .split_once("/// 提问里标签的处理动作")
            .and_then(|(_, rest)| rest.split_once("/// 解析用户主目录").map(|(body, _)| body))
            .expect("标签引擎段落存在");
        for platform in ["claude", "codex", "qoder", "Claude", "Qoder"] {
            assert!(!engine.contains(platform), "引擎里不该出现平台名 {platform}");
        }
    }

    #[test]
    fn payload_error_short_circuits_on_error_code() {
        assert!(payload_error(Some("overloaded"), None));
        assert!(!payload_error(None, Some("clear")));
    }

    #[test]
    fn payload_error_matches_failure_outcomes() {
        for outcome in ["error", "failed", "failure", "cancelled", "denied", "FAILED"] {
            assert!(payload_error(None, Some(outcome)), "{outcome}");
        }
        for outcome in ["clear", "success", "ok", ""] {
            assert!(!payload_error(None, Some(outcome)), "{outcome}");
        }
    }

    #[test]
    fn fallback_kind_guesses_from_event_name() {
        assert_eq!(fallback_kind("stream_error"), AgentEventKind::Failed);
        assert_eq!(fallback_kind("tool_xyz"), AgentEventKind::Running);
        assert_eq!(fallback_kind("start_something"), AgentEventKind::Running);
        assert_eq!(fallback_kind("stop_something"), AgentEventKind::WaitingForUser);
        assert_eq!(fallback_kind("wait_input"), AgentEventKind::WaitingForUser);
        assert_eq!(fallback_kind("completely_unrecognized"), AgentEventKind::Unknown);
    }
}
