use std::{
    env,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

use super::model::{now_ms, AgentEvent, Provider, SCHEMA_VERSION};

const MAX_HOOK_INPUT: u64 = 64 * 1024;
const APP_IDENTIFIER: &str = "com.tauri-app.my";

pub fn try_run_status_hook(args: &[String]) -> Option<i32> {
    if !args.iter().any(|arg| arg == "--status-hook") {
        return None;
    }

    // Debug: log that hook was invoked
    let debug_path = default_status_dir().join("hook-debug.log");
    let _ = fs::create_dir_all(debug_path.parent().unwrap_or(Path::new(".")));
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&debug_path) {
        let _ = writeln!(f, "[{}] hook invoked, args: {:?}", chrono_local_now(), args);
    }

    let result = run_status_hook(args);
    Some(match result {
        Ok(()) => {
            let _ = fs::OpenOptions::new().create(true).append(true).open(&debug_path)
                .and_then(|mut f| writeln!(f, "[{}] hook OK", chrono_local_now()));
            0
        }
        Err(error) => {
            let _ = fs::OpenOptions::new().create(true).append(true).open(&debug_path)
                .and_then(|mut f| writeln!(f, "[{}] hook FAILED: {error}", chrono_local_now()));
            eprintln!("UFO status hook failed: {error}");
            1
        }
    })
}

fn chrono_local_now() -> String {
    use std::time::SystemTime;
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

fn run_status_hook(args: &[String]) -> Result<(), String> {
    let provider = argument_value(args, "--provider")
        .or_else(|| argument_value(args, "--status-hook"))
        .and_then(|value| Provider::parse(&value))
        .ok_or_else(|| "missing --provider claude|codex".to_string())?;
    let status_dir = argument_value(args, "--status-dir")
        .map(PathBuf::from)
        .or_else(|| env::var_os("UFO_STATUS_DIR").map(PathBuf::from))
        .unwrap_or_else(default_status_dir);

    let mut input = Vec::new();
    let mut stdin = io::stdin().take(MAX_HOOK_INPUT + 1);
    stdin
        .read_to_end(&mut input)
        .map_err(|error| format!("cannot read stdin: {error}"))?;
    if input.len() as u64 > MAX_HOOK_INPUT {
        return Err("hook input is too large".to_string());
    }

    let payload: Value =
        serde_json::from_slice(&input).map_err(|error| format!("invalid hook JSON: {error}"))?;
    let session_id = string_field(&payload, &["session_id", "sessionId", "conversation_id"])
        .ok_or_else(|| "hook payload has no session id".to_string())?;
    if session_id.len() > 256 {
        return Err("session id is too long".to_string());
    }
    let event = string_field(
        &payload,
        &[
            "hook_event_name",
            "event",
            "event_name",
            "event_type",
            "type",
        ],
    )
    .unwrap_or_else(|| "unknown".to_string());
    let pid = string_field(&payload, &["pid", "process_id"])
        .and_then(|value| value.parse::<u32>().ok())
        .or_else(|| env::var("UFO_AGENT_PID").ok()?.parse::<u32>().ok());
    let event_id = string_field(&payload, &["event_id", "eventId", "id"])
        .filter(|value| !value.is_empty() && value.len() <= 256)
        .unwrap_or_else(unique_event_id);

    // outcome 同时承接 SessionEnd.reason 等结果别名；
    // notification_type 单独提取，避免与 outcome 混用污染状态判定。
    let normalized = AgentEvent {
        schema_version: SCHEMA_VERSION,
        event_id,
        provider,
        session_id,
        event,
        sequence: number_field(&payload, &["sequence", "seq"]),
        occurred_at_ms: number_field(&payload, &["occurred_at_ms", "timestamp_ms"])
            .and_then(|value| i64::try_from(value).ok()),
        received_at_ms: now_ms(),
        pid,
        error_code: string_field(&payload, &["error_code", "error", "failure_reason"]),
        outcome: string_field(&payload, &["outcome", "reason", "status", "stop_reason"]),
        notification_type: string_field(&payload, &["notification_type", "notificationType"]),
    };

    let inbox = status_dir.join("inbox");
    fs::create_dir_all(&inbox).map_err(|error| format!("cannot create status inbox: {error}"))?;
    let final_path = inbox.join(format!("{}.json", unique_event_id()));
    let temporary_path = inbox.join(format!(".{}.tmp", unique_event_id()));
    let encoded = serde_json::to_vec(&normalized).map_err(|error| error.to_string())?;
    let mut file = File::create(&temporary_path)
        .map_err(|error| format!("cannot create status event: {error}"))?;
    file.write_all(&encoded)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("cannot write status event: {error}"))?;
    fs::rename(&temporary_path, &final_path)
        .map_err(|error| format!("cannot publish status event: {error}"))?;
    Ok(())
}

fn argument_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .find_map(|argument| {
            argument
                .strip_prefix(&format!("{name}="))
                .map(ToString::to_string)
        })
        .or_else(|| {
            args.windows(2)
                .find(|pair| pair[0] == name)
                .map(|pair| pair[1].clone())
        })
        .filter(|value| !value.starts_with('-'))
}

fn string_field(payload: &Value, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        payload
            .get(*name)
            .and_then(Value::as_str)
            .map(ToString::to_string)
    })
}

fn number_field(payload: &Value, names: &[&str]) -> Option<u64> {
    names
        .iter()
        .find_map(|name| payload.get(*name).and_then(Value::as_u64))
}

fn unique_event_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{}-{}", process::id(), nanos)
}

pub fn default_status_dir() -> PathBuf {
    let base = if cfg!(target_os = "windows") {
        env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"))
    } else {
        env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
    };

    base.unwrap_or_else(|| Path::new(".").to_path_buf())
        .join(APP_IDENTIFIER)
        .join("status")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(value: &str) -> String {
        value.to_string()
    }

    #[test]
    fn argument_value_from_equals_syntax() {
        let args = vec![s("--provider=claude"), s("--other")];
        assert_eq!(argument_value(&args, "--provider"), Some(s("claude")));
    }

    #[test]
    fn argument_value_from_separate_args() {
        let args = vec![s("--provider"), s("claude")];
        assert_eq!(argument_value(&args, "--provider"), Some(s("claude")));
    }

    #[test]
    fn argument_value_returns_none_for_missing() {
        let args = vec![s("--other=value")];
        assert_eq!(argument_value(&args, "--provider"), None);
    }

    #[test]
    fn argument_value_rejects_flag_as_value() {
        let args = vec![s("--provider"), s("--other")];
        assert_eq!(argument_value(&args, "--provider"), None);
    }

    #[test]
    fn string_field_reads_first_matching_name() {
        let payload: Value = serde_json::json!({"session_id": "abc", "sessionId": "def"});
        assert_eq!(string_field(&payload, &["session_id"]), Some(s("abc")));
        assert_eq!(
            string_field(&payload, &["sessionId", "session_id"]),
            Some(s("def"))
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
    fn try_run_status_hook_returns_none_without_flag() {
        assert!(try_run_status_hook(&[s("ufo")]).is_none());
    }

    #[test]
    fn default_status_dir_contains_app_identifier() {
        let dir = default_status_dir();
        let dir_str = dir.to_string_lossy();
        assert!(dir_str.contains(APP_IDENTIFIER));
        assert!(dir_str.ends_with("status"));
    }

    #[test]
    fn unique_event_ids_are_unique() {
        let id1 = unique_event_id();
        let id2 = unique_event_id();
        assert_ne!(id1, id2);
    }
}
