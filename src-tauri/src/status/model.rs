use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Claude,
    Codex,
}

impl Provider {
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "claude" | "claude-code" => Some(Self::Claude),
            "codex" | "codex-cli" => Some(Self::Codex),
            _ => None,
        }
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

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatusSource {
    Hook,
    ProcessFallback,
}

/// Hook 归一化后的原始事件。
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
    /// 原始 hook 事件名（可被 normalize_event_name 归一化）
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
    /// Notification 的 notification_type，用于区分是否等人操作。
    /// 旧 inbox 事件可能缺此字段，缺省反序列化为 None。
    #[serde(default)]
    pub notification_type: Option<String>,
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
    let provider = match provider {
        Provider::Claude => "claude",
        Provider::Codex => "codex",
    };
    format!("{provider}:{session_id}")
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
            event: "start".to_string(),
            sequence: Some(42),
            occurred_at_ms: Some(1000),
            received_at_ms: 2000,
            pid: Some(1234),
            error_code: None,
            outcome: None,
            notification_type: Some("idle_prompt".to_string()),
        };
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: AgentEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.event_id, "test-event");
        assert_eq!(deserialized.provider, Provider::Claude);
        assert_eq!(deserialized.session_id, "sess-123");
        assert_eq!(deserialized.sequence, Some(42));
        assert_eq!(deserialized.pid, Some(1234));
        assert_eq!(
            deserialized.notification_type.as_deref(),
            Some("idle_prompt")
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
}
