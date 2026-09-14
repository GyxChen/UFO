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
use super::reducer::normalize_event_name;

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

    // 项目级 Claude 配置以会话的 cwd 为基准；payload 未带 cwd 时只读用户级配置。
    let cwd = string_field(&payload, &["cwd"]).map(PathBuf::from);
    // 只认用户手输的提问：展开后的模板文本不能覆盖它。
    let prompt = user_prompt(&event, &payload);

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
        base_url: provider_base_url(provider, cwd.as_deref()),
        prompt,
        transcript_path: transcript_path(&payload),
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

/// Claude 的 base_url 变量名。
///
/// 既用于 settings.json 的 `env` 块 key，也是进程环境变量名 —— 两者同一个名字。
const CLAUDE_BASE_URL_KEY: &str = "ANTHROPIC_BASE_URL";

/// 读取当前 provider 的 API endpoint。
///
/// 取值顺序：**Claude Code 配置文件优先，环境变量兜底**。
/// - 配置文件：解析 Claude Code 的 settings.json（Codex 的配置是 TOML，格式不同，暂不解析）。
/// - 环境变量：hook 由 agent 进程派生、继承其环境，能反映该会话实际的启动环境。
///
/// 只读 endpoint 类变量，绝不读取 token / api key 类变量。
/// 按需求不做脱敏：原样上报（含 userinfo / query），仅去掉首尾空白。
fn provider_base_url(provider: Provider, cwd: Option<&Path>) -> Option<String> {
    let from_settings = match provider {
        Provider::Claude => read_settings_base_url(
            &claude_settings_paths(home_dir().as_deref(), cwd),
            CLAUDE_BASE_URL_KEY,
        ),
        Provider::Codex => None,
    };

    from_settings
        .or_else(|| {
            base_url_env_names(provider)
                .iter()
                .find_map(|name| env::var(name).ok())
        })
        .and_then(|value| normalize_base_url(&value))
}

/// 各 provider 对应的 base_url 环境变量名，按优先级排列。
fn base_url_env_names(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Claude => &[CLAUDE_BASE_URL_KEY],
        Provider::Codex => &["OPENAI_BASE_URL", "OPENAI_API_BASE"],
    }
}

/// Claude Code 配置文件路径，按优先级**从低到高**排列（后者覆盖前者）。
///
/// 与 Claude Code 自身的优先级一致：用户级 < 项目级 < 项目本地。
/// home / cwd 缺失时对应路径直接跳过，不报错。
fn claude_settings_paths(home: Option<&Path>, cwd: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = home {
        paths.push(home.join(".claude").join("settings.json"));
    }
    if let Some(cwd) = cwd {
        paths.push(cwd.join(".claude").join("settings.json"));
        paths.push(cwd.join(".claude").join("settings.local.json"));
    }
    paths
}

/// 从 settings JSON 的 `env` 块中读取一个变量。
///
/// 只按名字取单个 key —— settings.json 的 env 块通常同时存放 token，
/// 绝不能整块读取或上报。缺失 / 非字符串 / 空白一律视为未设置。
fn env_from_settings(settings: &Value, key: &str) -> Option<String> {
    settings
        .get("env")?
        .get(key)?
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string)
}

/// 依次读取配置文件，后面的文件覆盖前面的（用户级 → 项目级 → 项目本地）。
///
/// 文件缺失、JSON 非法、env 块缺失都静默跳过：hook 必须快速返回，
/// 也不能因为一个损坏的配置文件让整个状态上报失败。
fn read_settings_base_url(paths: &[PathBuf], key: &str) -> Option<String> {
    let mut found = None;
    for path in paths {
        let Ok(raw) = fs::read_to_string(path) else {
            continue;
        };
        let Ok(settings) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        if let Some(value) = env_from_settings(&settings, key) {
            found = Some(value);
        }
    }
    found
}

/// 解析用户主目录，用于定位用户级 Claude 配置。
///
/// 与 default_status_dir 一样直接读环境变量，不引入 dirs 依赖。
fn home_dir() -> Option<PathBuf> {
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

/// base_url 长度上限。
///
/// 这不是脱敏，只是防止异常环境变量把超大字符串写进事件文件（事件整体有 64KiB 上限）。
const MAX_BASE_URL_LEN: usize = 4096;

/// 归一化 base_url：仅去掉首尾空白。
///
/// 不剥离 userinfo / query —— 按需求保留原值，便于排查实际使用的 endpoint。
fn normalize_base_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_BASE_URL_LEN {
        return None;
    }
    Some(trimmed.to_string())
}

/// 从 hook payload 中提取「用户手输」的提问。
///
/// 只认 `UserPromptSubmit`。`UserPromptExpansion` 紧随其后触发、同样携带 `prompt`，
/// 但那是斜杠命令展开后的模板文本，不是用户输入 —— 放行它会把用户真正的提问覆盖掉。
/// 其余事件本就不带 `prompt`，返回 None 后由 reducer 继承上一次的值。
fn user_prompt(event_name: &str, payload: &Value) -> Option<String> {
    if normalize_event_name(event_name) != "user_prompt_submit" {
        return None;
    }
    string_field(payload, &["prompt", "user_prompt", "prompt_text"])
        .and_then(|value| normalize_prompt(&value))
}

/// 路径长度上限，防止异常 payload 把超长字符串写进事件文件。
const MAX_TRANSCRIPT_PATH_LEN: usize = 4096;

/// 提取该会话 transcript 的路径，供 monitor 增量统计 token。
///
/// 这里**只透传路径**，不读文件：hook 每次事件启动一次、必须快速返回，
/// 而 transcript 会增长到 MB 级，解析必须放在常驻的 monitor 线程里做。
fn transcript_path(payload: &Value) -> Option<String> {
    string_field(payload, &["transcript_path", "transcriptPath"])
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && value.len() <= MAX_TRANSCRIPT_PATH_LEN)
}

/// 提问内容长度上限（字符数）。
///
/// 仅防止超大 prompt 撑爆事件文件（事件整体有 64KiB 上限），超出部分截断并加省略号。
const MAX_PROMPT_CHARS: usize = 4000;

/// 归一化提问内容：去掉首尾空白，超长截断。
///
/// 注意：提问内容会以明文写入状态目录的 inbox 事件文件（消费后删除）。
/// 这是「展示当前提问」功能的必要代价，已在文档中标注。
fn normalize_prompt(raw: &str) -> Option<String> {
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

    use crate::status::test_support::TempDir;

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
    fn claude_settings_paths_orders_user_then_project_then_local() {
        let home = Path::new("/home/u");
        let project = Path::new("/proj");
        // 顺序即优先级：后面的文件覆盖前面的。
        assert_eq!(
            claude_settings_paths(Some(home), Some(project)),
            vec![
                home.join(".claude").join("settings.json"),
                project.join(".claude").join("settings.json"),
                project.join(".claude").join("settings.local.json"),
            ]
        );
    }

    #[test]
    fn claude_settings_paths_skips_missing_base_dirs() {
        // 没有 home / cwd 时不应报错，只是少读几个文件。
        assert!(claude_settings_paths(None, None).is_empty());
        assert_eq!(
            claude_settings_paths(None, Some(Path::new("/proj"))).len(),
            2
        );
    }

    #[test]
    fn env_from_settings_reads_env_block() {
        let settings: Value =
            serde_json::json!({"env": {"ANTHROPIC_BASE_URL": "http://127.0.0.1:15721"}});
        assert_eq!(
            env_from_settings(&settings, CLAUDE_BASE_URL_KEY).as_deref(),
            Some("http://127.0.0.1:15721")
        );
    }

    #[test]
    fn env_from_settings_rejects_missing_non_string_and_blank() {
        let settings: Value = serde_json::json!({
            "env": {"NUM": 1, "BLANK": "   ", "NULL": null}
        });
        assert_eq!(env_from_settings(&settings, "NUM"), None);
        assert_eq!(env_from_settings(&settings, "BLANK"), None);
        assert_eq!(env_from_settings(&settings, "NULL"), None);
        assert_eq!(env_from_settings(&settings, "MISSING"), None);
        // 完全没有 env 块
        assert_eq!(
            env_from_settings(&serde_json::json!({}), CLAUDE_BASE_URL_KEY),
            None
        );
    }

    #[test]
    fn env_from_settings_only_reads_the_named_key() {
        // settings.json 的 env 块常同时存放 token，只能按名字取 endpoint。
        let settings: Value = serde_json::json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "secret",
                "ANTHROPIC_BASE_URL": "http://only-this",
            }
        });
        assert_eq!(
            env_from_settings(&settings, CLAUDE_BASE_URL_KEY).as_deref(),
            Some("http://only-this")
        );
        assert_eq!(env_from_settings(&settings, "ANTHROPIC_API_KEY"), None);
    }

    #[test]
    fn read_settings_base_url_later_file_overrides_earlier() {
        let temp = TempDir::new("override");
        let user = temp.write(
            ".claude/settings.json",
            r#"{"env":{"ANTHROPIC_BASE_URL":"http://user"}}"#,
        );
        let project = temp.write(
            "proj/.claude/settings.json",
            r#"{"env":{"ANTHROPIC_BASE_URL":"http://project"}}"#,
        );
        let local = temp.write(
            "proj/.claude/settings.local.json",
            r#"{"env":{"ANTHROPIC_BASE_URL":"http://local"}}"#,
        );

        assert_eq!(
            read_settings_base_url(&[user.clone()], CLAUDE_BASE_URL_KEY).as_deref(),
            Some("http://user")
        );
        // 项目级覆盖用户级
        assert_eq!(
            read_settings_base_url(&[user.clone(), project.clone()], CLAUDE_BASE_URL_KEY).as_deref(),
            Some("http://project")
        );
        // 项目本地覆盖项目级
        assert_eq!(
            read_settings_base_url(&[user, project, local], CLAUDE_BASE_URL_KEY).as_deref(),
            Some("http://local")
        );
    }

    #[test]
    fn read_settings_base_url_skips_missing_and_invalid_files() {
        let temp = TempDir::new("invalid");
        let missing = temp.path().join("nope").join("settings.json");
        let invalid = temp.write("bad/.claude/settings.json", "{ this is not json");
        let valid = temp.write(
            "ok/.claude/settings.json",
            r#"{"env":{"ANTHROPIC_BASE_URL":"http://ok"}}"#,
        );

        // 缺失与损坏的文件静默跳过，不影响后续文件生效。
        assert_eq!(
            read_settings_base_url(&[missing, invalid, valid], CLAUDE_BASE_URL_KEY).as_deref(),
            Some("http://ok")
        );
    }

    #[test]
    fn provider_base_url_prefers_settings_over_env() {
        let temp = TempDir::new("prefer");
        let project = temp.path().join("proj");
        temp.write(
            "proj/.claude/settings.json",
            r#"{"env":{"ANTHROPIC_BASE_URL":"http://from-settings"}}"#,
        );

        // 配置表命中时不再看环境变量，因此结果与测试进程的环境无关。
        assert_eq!(
            provider_base_url(Provider::Claude, Some(&project)).as_deref(),
            Some("http://from-settings")
        );
    }

    #[test]
    fn provider_base_url_ignores_claude_settings_for_codex() {
        let temp = TempDir::new("codex");
        let project = temp.path().join("proj");
        temp.write(
            "proj/.claude/settings.json",
            r#"{"env":{"ANTHROPIC_BASE_URL":"http://from-settings"}}"#,
        );

        // Codex 的配置是 TOML，不走 Claude 的 settings.json；
        // 结果只取决于环境变量，因此这里只断言不会误取 Claude 的值。
        assert_ne!(
            provider_base_url(Provider::Codex, Some(&project)).as_deref(),
            Some("http://from-settings")
        );
    }

    #[test]
    fn normalize_base_url_keeps_value_verbatim() {
        // 按需求不做脱敏：userinfo / query / fragment 原样保留。
        assert_eq!(
            normalize_base_url("http://127.0.0.1:15721").as_deref(),
            Some("http://127.0.0.1:15721")
        );
        assert_eq!(
            normalize_base_url("https://user:pass@api.example.com/v1?token=abc").as_deref(),
            Some("https://user:pass@api.example.com/v1?token=abc")
        );
        assert_eq!(
            normalize_base_url("https://api.example.com/").as_deref(),
            Some("https://api.example.com/")
        );
    }

    #[test]
    fn normalize_base_url_trims_whitespace() {
        assert_eq!(
            normalize_base_url("  http://127.0.0.1:15721  ").as_deref(),
            Some("http://127.0.0.1:15721")
        );
    }

    #[test]
    fn normalize_base_url_rejects_empty_and_oversized() {
        assert_eq!(normalize_base_url(""), None);
        assert_eq!(normalize_base_url("   "), None);
        // 超长仍然丢弃：事件整体有 64KiB 上限，避免异常环境变量撑爆事件文件。
        let long = format!("http://example.com/{}", "a".repeat(MAX_BASE_URL_LEN));
        assert_eq!(normalize_base_url(&long), None);
    }

    #[test]
    fn normalize_prompt_trims_and_keeps_content() {
        assert_eq!(normalize_prompt("  帮我看看这个 bug  ").as_deref(), Some("帮我看看这个 bug"));
        assert_eq!(normalize_prompt("hello").as_deref(), Some("hello"));
    }

    #[test]
    fn normalize_prompt_rejects_empty() {
        assert_eq!(normalize_prompt(""), None);
        assert_eq!(normalize_prompt("   \n\t "), None);
    }

    #[test]
    fn normalize_prompt_truncates_oversized_input() {
        let long = "字".repeat(MAX_PROMPT_CHARS + 500);
        let result = normalize_prompt(&long).expect("prompt kept");
        // 截断到上限并追加省略号。
        assert_eq!(result.chars().count(), MAX_PROMPT_CHARS + 1);
        assert!(result.ends_with('…'));
    }

    #[test]
    fn normalize_prompt_counts_characters_not_bytes() {
        // 多字节字符按「字符」计数，避免把 UTF-8 序列从中间截断。
        let long = "汉".repeat(MAX_PROMPT_CHARS);
        let result = normalize_prompt(&long).expect("prompt kept");
        assert_eq!(result.chars().count(), MAX_PROMPT_CHARS);
        assert!(!result.ends_with('…'));
    }

    #[test]
    fn user_prompt_accepts_only_user_prompt_submit() {
        let payload: Value = serde_json::json!({"prompt": "帮我看看这个 bug"});
        // 用户手输：认
        assert_eq!(
            user_prompt("UserPromptSubmit", &payload).as_deref(),
            Some("帮我看看这个 bug")
        );
        // 下划线写法同样认
        assert_eq!(
            user_prompt("user_prompt_submit", &payload).as_deref(),
            Some("帮我看看这个 bug")
        );
        // 展开后的模板文本：不认，否则会覆盖用户真正的提问
        assert_eq!(user_prompt("UserPromptExpansion", &payload), None);
        // 其它事件即使带 prompt 也不认
        assert_eq!(user_prompt("PreToolUse", &payload), None);
        assert_eq!(user_prompt("Stop", &payload), None);
    }

    #[test]
    fn user_prompt_returns_none_without_a_usable_prompt() {
        let missing: Value = serde_json::json!({"session_id": "s1"});
        assert_eq!(user_prompt("UserPromptSubmit", &missing), None);
        // 空 / 空白同样视为未提供，交给 reducer 继承上一次的值
        let blank: Value = serde_json::json!({"prompt": "   "});
        assert_eq!(user_prompt("UserPromptSubmit", &blank), None);
    }

    #[test]
    fn user_prompt_accepts_alternate_field_names() {
        let payload: Value = serde_json::json!({"user_prompt": "换个字段名"});
        assert_eq!(
            user_prompt("UserPromptSubmit", &payload).as_deref(),
            Some("换个字段名")
        );
    }

    #[test]
    fn transcript_path_reads_and_trims() {
        let payload: Value = serde_json::json!({"transcript_path": "  C:\\x\\sess.jsonl  "});
        assert_eq!(
            transcript_path(&payload).as_deref(),
            Some("C:\\x\\sess.jsonl")
        );
        // 驼峰别名同样认
        let camel: Value = serde_json::json!({"transcriptPath": "/tmp/sess.jsonl"});
        assert_eq!(transcript_path(&camel).as_deref(), Some("/tmp/sess.jsonl"));
    }

    #[test]
    fn transcript_path_rejects_missing_blank_and_oversized() {
        assert_eq!(transcript_path(&serde_json::json!({})), None);
        assert_eq!(transcript_path(&serde_json::json!({"transcript_path": "   "})), None);
        assert_eq!(transcript_path(&serde_json::json!({"transcript_path": ""})), None);
        // 超长路径丢弃，避免撑大事件文件
        let huge = "a".repeat(MAX_TRANSCRIPT_PATH_LEN + 1);
        assert_eq!(
            transcript_path(&serde_json::json!({ "transcript_path": huge })),
            None
        );
    }
}
