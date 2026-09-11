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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentEvent {
    pub schema_version: u16,
    pub event_id: String,
    pub provider: Provider,
    pub session_id: String,
    pub event: String,
    pub sequence: Option<u64>,
    pub occurred_at_ms: Option<i64>,
    pub received_at_ms: i64,
    pub pid: Option<u32>,
    pub error_code: Option<String>,
    pub outcome: Option<String>,
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
        };
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: AgentEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.event_id, "test-event");
        assert_eq!(deserialized.provider, Provider::Claude);
        assert_eq!(deserialized.session_id, "sess-123");
        assert_eq!(deserialized.sequence, Some(42));
        assert_eq!(deserialized.pid, Some(1234));
    }
}
