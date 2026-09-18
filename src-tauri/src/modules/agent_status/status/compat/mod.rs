//! 平台兼容层：把各平台 hook 的「原始负载」归一化成内核事件。
//!
//! 分层约定（见 CLAUDE.md「平台兼容层 / 内核层」）：
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
//! **接入新平台 = 新增一个 `compat/<platform>.rs` + 在 [`adapter_for`] 里加一行。**
//! 适配器负责平台的全部知识（字段别名、事件名表、base_url 来源、提问提取…），
//! hook 外壳（`super::hook`）只做与平台无关的信封活：读 stdin、补事件 id / pid / 接收时刻、落盘。

pub(crate) mod claude;
pub(crate) mod codex;

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
    pub received_at_ms: i64,
    pub pid: Option<u32>,
}

impl AdaptedEvent {
    /// 补上信封字段，得到可写盘、可入内核的 [`AgentEvent`]。
    pub(crate) fn into_event(self, envelope: EventEnvelope) -> AgentEvent {
        AgentEvent {
            schema_version: SCHEMA_VERSION,
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
/// 仅防止超大 prompt 撑爆事件文件（事件整体有 64KiB 上限），超出部分截断并加省略号。
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
/// 这不是脱敏，只是防止异常环境变量把超大字符串写进事件文件（事件整体有 64KiB 上限）。
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
/// 分开（Claude 与 Codex 均为主线程不带、子代理事件带 `agent_id`）。内核靠它把子代理事件
/// 限制为「只刷存活时间、不驱灯色」，否则主线程 Waiting 会被仍在后台跑的子代理顶回
/// Running（灯一直绿）。只透传标识，不做任何语义解释。
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
    }
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
