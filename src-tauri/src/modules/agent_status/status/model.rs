use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Claude,
    Codex,
    Qoder,
}

impl Provider {
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "claude" | "claude-code" => Some(Self::Claude),
            "codex" | "codex-cli" => Some(Self::Codex),
            "qoder" | "qoder-cli" => Some(Self::Qoder),
            _ => None,
        }
    }

    /// provider 的稳定字符串写法（`"claude"` / `"codex"` / `"qoder"`）。
    ///
    /// 会话 key、存档表里的 `provider` 列都用它，避免各处各写一份 match。
    pub fn as_str(&self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
            Provider::Qoder => "qoder",
        }
    }

    /// 该平台的会话日志是否与 Claude Code 的 transcript 同构（能直接喂给 `TokenTracker`）。
    ///
    /// Qoder 实测共用同一套记录字段（`type` = assistant、`message.usage` 的四个 token 键名）
    /// 与同一套目录布局（`<项目>/<session-id>.jsonl` + `<session-id>/subagents/*.jsonl`），
    /// 所以 token 统计复用同一个解析器，不必另写一套。
    /// Codex 的会话是 rollout 文件、格式不同，由 `codex_rollout` 自己读，**不得**登记进来。
    pub fn uses_claude_transcript(&self) -> bool {
        matches!(self, Provider::Claude | Provider::Qoder)
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionPhase {
    Running,
    Waiting,
    Error,
    Unknown,
    Completed,
}

/// 兼容层归一化后的「事件种类」：内核只认这些语义。
///
/// 平台（Claude / Codex / …）的原始事件名（`PreToolUse`、`TurnCompleted` …）在
/// `status::compat` 里就被翻译成这里的取值，内核不解析任何平台词汇 —— 加平台只加适配器。
///
/// ⚠️ 它是**线协议字段**：缺省值必须是「无法识别」（老二进制写的事件没有 `kind`），
/// 且只能新增取值、不得改语义，`SCHEMA_VERSION` 不因此递增。
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentEventKind {
    /// 正在执行（工具调用、生成中、会话启动…）→ 绿灯常亮。
    Running,
    /// 一轮执行**正常**结束、等下一条用户输入（Claude / Codex 的 `Stop`）→ 绿闪后落黄。
    TurnFinished,
    /// 正在等用户操作（权限弹窗 / elicitation / 用户取消本轮后等下一条指令）→ 直接黄灯，不闪。
    ///
    /// 刻意不闪：绿闪在本项目里是「这一轮顺利跑完」的信号，权限等待与 Esc 取消都不属于它。
    WaitingForUser,
    /// 会话确认结束（Claude / Codex 的 `SessionEnd`）→ 绿闪后移除。
    SessionFinished,
    /// 子代理开始执行；与主会话共用 `session_id`，靠 `agent_id` 区分。
    ///
    /// 它不直接等于主线程正在执行：主线程可能已经 `Stop`、正在等用户输入，
    /// 而 Plan / Agent 子对话仍在后台跑。内核用该语义维护活跃子代理集合。
    SubagentStarted,
    /// 子代理结束；只有最后一个活跃子代理结束时，后台运行态才会退出。
    SubagentStopped,
    /// 本轮或会话出错 → 红灯。
    Failed,
    /// 无法识别；内核保守处理（不构成转灯信号，见 `kernel::reduce_state`）。
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatusSource {
    Hook,
    ProcessFallback,
}

/// 兼容层归一化后的原始事件。
///
/// `outcome` 同时承载 SessionEnd 的 `reason` 以及其它结果字段别名；
/// `notification_type` 单独存放 Notification 的类型，避免污染 outcome 判定。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentEvent {
    /// 协议版本，必须与 SCHEMA_VERSION 一致
    pub schema_version: u16,
    /// 事件去重 ID
    pub event_id: String,
    /// 来源 provider
    pub provider: Provider,
    /// 会话标识
    pub session_id: String,
    /// 兼容层归一化出的事件种类：**内核唯一据以转灯的语义**。
    /// 旧事件缺此字段时反序列化为 `Unknown`（保守：不驱灯色）。
    #[serde(default)]
    pub kind: AgentEventKind,
    /// 平台原始事件名（如 `PreToolUse`），**仅供诊断 / 排障**：
    /// 内核不得据此转灯，否则平台词汇会重新漏回内核层。
    pub event: String,
    /// 可选序号，用于乱序过滤
    pub sequence: Option<u64>,
    /// 事件发生时间（毫秒）
    pub occurred_at_ms: Option<i64>,
    /// 被 UFO 接收的时间（毫秒）
    pub received_at_ms: i64,
    /// 可选进程 ID，用于存活检查
    pub pid: Option<u32>,
    /// 错误码；存在时优先映射为 Error
    pub error_code: Option<String>,
    /// 结果/退出原因（SessionEnd.reason 等）
    pub outcome: Option<String>,
    /// Notification 的 notification_type；**仅供诊断 / 排障**。
    /// 「是否等人操作」的判定在兼容层（`compat::claude`）就做完并落成 `kind`，内核不再看它。
    /// 旧 inbox 事件可能缺此字段，缺省反序列化为 None。
    #[serde(default)]
    pub notification_type: Option<String>,
    /// 该会话实际使用的 API endpoint（如 ANTHROPIC_BASE_URL）。
    /// 由 hook 从自身环境变量读取，旧事件缺此字段时反序列化为 None。
    /// 注意：只读 endpoint 类变量，不读取 token / api key。
    #[serde(default)]
    pub base_url: Option<String>,
    /// 用户最近一次提交的提问内容（UserPromptSubmit 的 `prompt` 字段）。
    ///
    /// 字段名已对 CC 2.1.269 实测确认。旧事件缺此字段时反序列化为 None。
    /// 只认 UserPromptSubmit；UserPromptExpansion 的展开模板不覆盖它。
    #[serde(default)]
    pub prompt: Option<String>,
    /// 该会话 transcript（JSONL）的绝对路径，供 monitor 线程增量统计 token。
    ///
    /// hook 只透传路径，解析在常驻 monitor 里做 —— hook 每次事件启动一次，必须快速返回。
    /// 旧事件缺此字段时反序列化为 None。
    #[serde(default)]
    pub transcript_path: Option<String>,
    /// 「正在等你操作」的原因原文（Codex 审批理由等）；非等待态为 None。
    /// 旧事件缺此字段时反序列化为 None。
    #[serde(default)]
    pub waiting_reason: Option<String>,
    /// 子代理（subagent）事件的来源标识；主线程事件缺省为 None。
    ///
    /// ⚠️ 子代理事件与主会话**共用同一个 `session_id`**（实测确认），只有本字段能区分。
    /// 内核用本字段维护「活跃子代理集合」，并按主线程所处状态分流（见 `kernel::reduce_state`）：
    /// 主线程在等用户操作、或会话已结束 / 已出错时，子代理事件只记账、不转灯 —— 否则
    /// 「Claude 已经在问我了，灯却一直是绿色」；只有主线程已停、处于「等 prompt」空闲时，
    /// 子代理活动才把灯拉回后台运行（对外绿常亮）。子代理自己的 `Failed` 也不染红父灯。
    /// 旧事件缺此字段时反序列化为 None。
    #[serde(default)]
    pub agent_id: Option<String>,
}

/// 一个会话累计消耗的 token。
///
/// 由 transcript 增量解析得出，按 `message.id` 去重后累加：
/// 同一条 API 响应会在 transcript 里写成多条 assistant 记录，每条都带同一份 usage，
/// 不去重会翻 2–3 倍。
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct TokenTotals {
    /// 新增输入 token（不含缓存部分）
    pub input_tokens: u64,
    /// 写入缓存的 token
    pub cache_creation_input_tokens: u64,
    /// 命中缓存读取的 token
    pub cache_read_input_tokens: u64,
    /// 模型生成的 token（已含 thinking，不重复相加）
    pub output_tokens: u64,
}

impl TokenTotals {
    /// 累计总量：输入 + 缓存写 + 缓存读 + 输出。
    ///
    /// 仅供测试断言使用 —— 生产的「总量」由前端 `tokenTotal()` 计算，
    /// 因此标为 `cfg(test)` 以免在正式构建里变成 dead code。
    #[cfg(test)]
    pub fn total(&self) -> u64 {
        self.input_tokens
            + self.cache_creation_input_tokens
            + self.cache_read_input_tokens
            + self.output_tokens
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionSnapshot {
    pub key: String,
    pub provider: Provider,
    pub session_id: String,
    pub phase: SessionPhase,
    pub source: StatusSource,
    pub confidence: u8,
    pub started_at_ms: i64,
    pub last_seen_at_ms: i64,
    pub completed_at_ms: Option<i64>,
    pub remove_at_ms: Option<i64>,
    pub is_flashing: bool,
    pub pid: Option<u32>,
    pub error_code: Option<String>,
    /// 会话使用的 API endpoint；未知时为 None，供悬浮窗展示。
    pub base_url: Option<String>,
    /// `base_url` 经所选代理解析器反查出的**上游 endpoint**（如 cc-switch 代理后的 `https://api.b.ai`）。
    ///
    /// 由 `status` 在生成快照时按当前选中的解析器填充，不是事件派生字段，
    /// 因此**不得升 SCHEMA_VERSION**；旧事件缺此字段反序列化为 None。
    /// 未启用解析 / 未命中时为 None，前端回落到 `base_url`。
    #[serde(default)]
    pub upstream_url: Option<String>,
    /// 用户最近一次提问内容；未知时为 None，供悬浮窗展示。
    pub prompt: Option<String>,
    /// 「正在等你操作」的原因原文（如 Codex 的审批理由）；非等待态为 None。
    /// 与 `error_code` 同策略：只在 Waiting 期间保留，离开即清空。
    #[serde(default)]
    pub waiting_reason: Option<String>,
    /// 会话累计消耗的 token；尚未统计到时为 None，供悬浮窗展示。
    /// 由 StatusRegistry 在 snapshot() 时合并进来，不属于事件派生字段。
    #[serde(default)]
    pub tokens: Option<TokenTotals>,
    pub last_sequence: Option<u64>,
    pub termination_confirmed: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct StatusCounts {
    pub running: usize,
    pub waiting: usize,
    pub error: usize,
    pub unknown: usize,
    pub completed: usize,
}

/// 会话存档的**回填摘要**：`session_id` 重新出现（跨重启 / 跨清理，如 `claude --resume`）时，
/// 内核用它把「跨重启仍有意义」的字段续上。
///
/// 只含回填需要的四项，是持久化行（`session_store::SessionRecord`）到领域类型的边界转换 ——
/// 内核因此不依赖存储层。灯色 / pid / sequence 等存活态字段刻意不在这里：
/// 它们必须由本次事件决定。
#[derive(Debug, Clone)]
pub struct StoredSeed {
    /// 会话最早创建时刻：回填它让指示灯排序位置不跳。
    pub started_at_ms: i64,
    /// 最近一次用户提问（同轮内后续事件继续继承它）。
    pub prompt: Option<String>,
    /// 会话的 API endpoint（同样参与继承阶梯）。
    pub base_url: Option<String>,
    /// 已消耗的 token 累计，作为**地板**灌进统计器（重扫值只会更大不会回退）。
    pub tokens: TokenTotals,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StatusSnapshot {
    pub schema_version: u16,
    pub revision: u64,
    pub generated_at_ms: i64,
    pub sessions: Vec<SessionSnapshot>,
    pub counts: StatusCounts,
    pub source_health: String,
}

pub fn session_key(provider: Provider, session_id: &str) -> String {
    format!("{}:{session_id}", provider.as_str())
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_parse_known_values() {
        assert_eq!(Provider::parse("claude"), Some(Provider::Claude));
        assert_eq!(Provider::parse("claude-code"), Some(Provider::Claude));
        assert_eq!(Provider::parse("CLAUDE"), Some(Provider::Claude));
        assert_eq!(Provider::parse("codex"), Some(Provider::Codex));
        assert_eq!(Provider::parse("codex-cli"), Some(Provider::Codex));
        assert_eq!(Provider::parse("CODEX"), Some(Provider::Codex));
        assert_eq!(Provider::parse("qoder"), Some(Provider::Qoder));
        assert_eq!(Provider::parse("qoder-cli"), Some(Provider::Qoder));
        assert_eq!(Provider::parse("QODER"), Some(Provider::Qoder));
    }

    #[test]
    fn provider_parse_rejects_unknown() {
        assert_eq!(Provider::parse("unknown"), None);
        assert_eq!(Provider::parse(""), None);
        assert_eq!(Provider::parse("gpt"), None);
    }

    #[test]
    fn session_key_includes_provider_namespace() {
        assert_eq!(session_key(Provider::Claude, "abc"), "claude:abc");
        assert_eq!(session_key(Provider::Codex, "abc"), "codex:abc");
        assert_eq!(session_key(Provider::Qoder, "abc"), "qoder:abc");
    }

    #[test]
    fn only_claude_shaped_transcripts_go_to_the_token_tracker() {
        // Qoder 的会话日志与 Claude 同构，复用 TokenTracker；Codex 是 rollout，格式不同。
        assert!(Provider::Claude.uses_claude_transcript());
        assert!(Provider::Qoder.uses_claude_transcript());
        assert!(!Provider::Codex.uses_claude_transcript());
    }

    #[test]
    fn now_ms_returns_positive_value() {
        let now = now_ms();
        assert!(now > 0);
    }

    #[test]
    fn agent_event_serialization_roundtrip() {
        let event = AgentEvent {
            schema_version: SCHEMA_VERSION,
            event_id: "test-event".to_string(),
            provider: Provider::Claude,
            session_id: "sess-123".to_string(),
            kind: AgentEventKind::TurnFinished,
            event: "stop".to_string(),
            sequence: Some(42),
            occurred_at_ms: Some(1000),
            received_at_ms: 2000,
            pid: Some(1234),
            error_code: None,
            outcome: None,
            notification_type: Some("idle_prompt".to_string()),
            base_url: Some("http://127.0.0.1:15721".to_string()),
            prompt: Some("修一下这个 bug".to_string()),
            transcript_path: Some("/tmp/sess-123.jsonl".to_string()),
            waiting_reason: Some("是否允许我访问官网".to_string()),
            agent_id: Some("agent-abc".to_string()),
        };
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: AgentEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.event_id, "test-event");
        assert_eq!(deserialized.provider, Provider::Claude);
        assert_eq!(deserialized.session_id, "sess-123");
        assert_eq!(deserialized.kind, AgentEventKind::TurnFinished);
        assert_eq!(deserialized.sequence, Some(42));
        assert_eq!(deserialized.pid, Some(1234));
        assert_eq!(
            deserialized.notification_type.as_deref(),
            Some("idle_prompt")
        );
        assert_eq!(
            deserialized.base_url.as_deref(),
            Some("http://127.0.0.1:15721")
        );
        assert_eq!(deserialized.prompt.as_deref(), Some("修一下这个 bug"));
        assert_eq!(
            deserialized.transcript_path.as_deref(),
            Some("/tmp/sess-123.jsonl")
        );
        assert_eq!(deserialized.agent_id.as_deref(), Some("agent-abc"));
        assert_eq!(
            deserialized.waiting_reason.as_deref(),
            Some("是否允许我访问官网")
        );
    }

    #[test]
    fn agent_event_missing_kind_deserializes_as_unknown() {
        // 老二进制写的事件没有 kind：必须兼容为 Unknown，而不是解析失败落 quarantine。
        // 语义上「无法识别」= 不驱灯色，正是保守该有的行为。
        let json = r#"{
            "schema_version": 1,
            "event_id": "legacy",
            "provider": "claude",
            "session_id": "s1",
            "event": "Stop",
            "sequence": null,
            "occurred_at_ms": null,
            "received_at_ms": 1,
            "pid": null,
            "error_code": null,
            "outcome": null
        }"#;
        let deserialized: AgentEvent = serde_json::from_str(json).unwrap();
        assert_eq!(deserialized.kind, AgentEventKind::Unknown);
    }

    #[test]
    fn agent_event_kind_uses_snake_case_on_the_wire() {
        // 取值名是线协议的一部分，改名会静默丢掉前端/旧进程的语义。
        assert_eq!(
            serde_json::to_string(&AgentEventKind::WaitingForUser).unwrap(),
            "\"waiting_for_user\""
        );
        assert_eq!(
            serde_json::from_str::<AgentEventKind>("\"session_finished\"").unwrap(),
            AgentEventKind::SessionFinished
        );
    }

    #[test]
    fn agent_event_missing_notification_type_deserializes_as_none() {
        // 旧 inbox 事件缺字段时应兼容为 None，不破坏消费。
        let json = r#"{
            "schema_version": 1,
            "event_id": "legacy",
            "provider": "claude",
            "session_id": "s1",
            "event": "Stop",
            "sequence": null,
            "occurred_at_ms": null,
            "received_at_ms": 1,
            "pid": null,
            "error_code": null,
            "outcome": null
        }"#;
        let deserialized: AgentEvent = serde_json::from_str(json).unwrap();
        assert_eq!(deserialized.notification_type, None);
    }

    #[test]
    fn agent_event_missing_base_url_deserializes_as_none() {
        // 旧 hook 二进制写出的事件没有 base_url，必须仍能解析而不是落 quarantine。
        let json = r#"{
            "schema_version": 1,
            "event_id": "legacy",
            "provider": "claude",
            "session_id": "s1",
            "event": "Stop",
            "sequence": null,
            "occurred_at_ms": null,
            "received_at_ms": 1,
            "pid": null,
            "error_code": null,
            "outcome": null,
            "notification_type": null
        }"#;
        let deserialized: AgentEvent = serde_json::from_str(json).unwrap();
        assert_eq!(deserialized.base_url, None);
        assert_eq!(deserialized.prompt, None);
        // 旧事件同样没有 transcript_path，缺省为 None，token 行不显示。
        assert_eq!(deserialized.transcript_path, None);
        // 也没有 agent_id：缺省 None 即视为主线程事件（沿用旧行为）。
        assert_eq!(deserialized.agent_id, None);
    }

    #[test]
    fn token_totals_total_sums_all_four_fields() {
        let totals = TokenTotals {
            input_tokens: 100,
            cache_creation_input_tokens: 20,
            cache_read_input_tokens: 3000,
            output_tokens: 50,
        };
        // 含缓存的总量：缓存读通常占比最大，漏掉它会显著低估消耗。
        assert_eq!(totals.total(), 3170);
    }

    #[test]
    fn token_totals_default_is_zero() {
        let totals = TokenTotals::default();
        assert_eq!(totals.total(), 0);
    }

    #[test]
    fn session_snapshot_missing_tokens_deserializes_as_none() {
        // 旧快照没有 tokens 字段，必须兼容为 None 而不是解析失败。
        let json = r#"{
            "key": "claude:s1",
            "provider": "claude",
            "session_id": "s1",
            "phase": "running",
            "source": "hook",
            "confidence": 100,
            "started_at_ms": 1,
            "last_seen_at_ms": 2,
            "completed_at_ms": null,
            "remove_at_ms": null,
            "is_flashing": false,
            "pid": null,
            "error_code": null,
            "base_url": null,
            "prompt": null,
            "last_sequence": null,
            "termination_confirmed": false
        }"#;
        let snapshot: SessionSnapshot = serde_json::from_str(json).unwrap();
        assert_eq!(snapshot.tokens, None);
    }
}
