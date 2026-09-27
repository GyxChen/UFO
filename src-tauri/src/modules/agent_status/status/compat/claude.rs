//! Claude Code 平台适配器。
//!
//! 汇集 Claude Code 的全部平台知识：字段别名、事件名表、base_url 来源、提问提取、
//! 元标签剔除、子代理标识…… 内核对这些一概不知，只看它产出的 [`AgentEventKind`]。

use std::{env, fs, path::{Path, PathBuf}};

use serde_json::Value;

use super::{
    agent_id, apply_tag_rules, base_url_env_names, build_tag_rules, fallback_kind, home_dir,
    is_user_prompt_submit, normalize_base_url, normalize_event_name, number_field, payload_error,
    string_field, transcript_path, truncate_prompt, AdaptContext, AdaptedEvent, HookAdapter,
    TagRule, CLAUDE_BASE_URL_KEY,
};
use crate::modules::agent_status::status::model::{AgentEventKind, Provider};

/// Claude 适配器单例；`compat::adapter_for(Provider::Claude)` 直接返回它。
pub(crate) struct ClaudeAdapter;

impl HookAdapter for ClaudeAdapter {
    /// 把一次 Claude Code hook 的原始 JSON 负载归一化成内核事件。
    fn adapt(&self, payload: &Value, ctx: &AdaptContext<'_>) -> Result<Option<AdaptedEvent>, String> {
        let session_id = string_field(payload, &["session_id", "sessionId", "conversation_id"])
            .ok_or_else(|| "hook payload has no session id".to_string())?;
        if session_id.len() > 256 {
            return Err("session id is too long".to_string());
        }

        let event = string_field(
            payload,
            &[
                "hook_event_name",
                "event",
                "event_name",
                "event_type",
                "type",
            ],
        )
        .unwrap_or_else(|| "unknown".to_string());

        // outcome 同时承接 SessionEnd.reason 等结果别名；notification_type 单独提取，
        // 避免与 outcome 混用污染状态判定。
        let error_code = string_field(payload, &["error_code", "error", "failure_reason"]);
        let outcome = string_field(payload, &["outcome", "reason", "status", "stop_reason"]);
        let notification_type = string_field(payload, &["notification_type", "notificationType"]);

        // 只认用户手输的提问：展开后的模板文本不能覆盖它。
        let prompt = user_prompt(&event, payload);

        // 错误字段优先于事件名；否则查 Claude 的事件名表。
        let kind = if payload_error(error_code.as_deref(), outcome.as_deref()) {
            AgentEventKind::Failed
        } else {
            classify(&event, notification_type.as_deref())
        };

        // Claude 的适配器每个事件都归一出结果（它的事件名表是完备的），不返回「忽略」。
        Ok(Some(AdaptedEvent {
            session_id,
            kind,
            event,
            agent_id: agent_id(payload),
            error_code,
            outcome,
            notification_type,
            base_url: provider_base_url(Provider::Claude, ctx.cwd),
            prompt,
            // Claude 的「等人操作」由事件名决定（PermissionRequest / Elicitation），
            // payload 里没有像 Codex 那样的自然语言理由，所以恒为空。
            waiting_reason: None,
            transcript_path: transcript_path(payload),
            sequence: number_field(payload, &["sequence", "seq"]),
            occurred_at_ms: number_field(payload, &["occurred_at_ms", "timestamp_ms"])
                .and_then(|value| i64::try_from(value).ok()),
        }))
    }
}

/// 把 Claude Code 的事件名归一化成内核事件种类。
///
/// 事件名表与 `reference/doc/agent-status-hooks.md` 的灯色语义一一对应：
/// 绿常亮 = 执行中；黄 = 等人操作；绿闪落黄 = Stop；绿闪移除 = SessionEnd；红 = StopFailure。
fn classify(event_name: &str, notification_type: Option<&str>) -> AgentEventKind {
    let normalized = normalize_event_name(event_name);
    match normalized.as_str() {
        // ── 正在执行（绿常亮） ──
        "session_start" | "session_started" | "setup" | "user_prompt_submit"
        | "user_prompt_expansion" | "pre_tool_use" | "post_tool_use" | "post_tool_use_failure"
        | "post_tool_batch" | "task_created"
        | "message_display" | "pre_compact" | "post_compact" | "pre_model_switch"
        | "post_model_switch" | "elicit_result" | "elicitation_result"
        | "instructions_loaded" | "config_change" | "cwd_changed" | "directory_added"
        | "file_changed" | "worktree_create" | "worktree_remove"
        // 非「等人操作」信号，保持执行链路语义
        | "permission_denied" | "task_completed" | "teammate_idle" => AgentEventKind::Running,

        // ── 子代理生命周期 ──
        // 子代理与主会话共用 session_id；内核按 agent_id 维护活跃集合，
        // 从而支持 Plan 模式下主线程 Stop 后仍有后台子代理运行。
        "subagent_start" => AgentEventKind::SubagentStarted,
        "subagent_stop" => AgentEventKind::SubagentStopped,

        // ── 等待用户操作 ──
        // Stop：一轮回复结束，等下一条 prompt → 内核先绿闪再落黄
        "stop" => AgentEventKind::TurnFinished,
        // PermissionRequest / Elicitation：弹窗等人选择或输入 → 直接黄，不闪
        "permission_request" | "elicit" | "elicitation" => AgentEventKind::WaitingForUser,

        // Notification：仅等待类 notification_type 进黄灯，其它保持 Running
        "notification" => {
            if is_waiting_notification(notification_type) {
                AgentEventKind::WaitingForUser
            } else {
                AgentEventKind::Running
            }
        }

        // ── 对话结束；SessionEnd 只有 reason，不再按 success 分流 ──
        "session_end" | "session_ended" | "session_completed" | "session_done" => {
            AgentEventKind::SessionFinished
        }

        // ── 本轮 API/模型错误 ──
        "stop_failure" => AgentEventKind::Failed,

        // ── 未知事件名：靠共享兜底猜测，猜不出就是 Unknown ──
        _ => fallback_kind(&normalized),
    }
}

/// 判断 Notification 是否属于「等待用户操作」类型。
///
/// 枚举值与 Claude Code 的 `notification_type` 完整清单对照（从 CLI 2.1.270 二进制常量
/// 提取，共 16 个）；这里只列**需要用户操作**的那几个。漏列的值会退化成 Running，
/// 表现就是「该等用户了灯还是绿色」。
fn is_waiting_notification(notification_type: Option<&str>) -> bool {
    notification_type
        .map(|value| value.to_ascii_lowercase())
        .is_some_and(|value| {
            matches!(
                value.as_str(),
                "idle_prompt"
                    | "permission_prompt"
                    | "elicitation_dialog"
                    | "agent_needs_input"
                    // MCP 弹链接让用户确认/选择，同样在等人操作。
                    | "elicitation_url_dialog"
                    // 团队成员请求权限（worker 会话需要主线程授权）。
                    | "worker_permission_prompt"
            )
        })
}

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
        // 穷尽 match 强制的分支（`Provider` 是闭包枚举）。Qoder 的 endpoint 不由这里读：
        // 它没有可配的 endpoint 字段，走 `compat::qoder` 自己的身份常量。本文件与它互不引用。
        Provider::Qoder => None,
    };

    from_settings
        .or_else(|| {
            base_url_env_names(provider)
                .iter()
                .find_map(|name| env::var(name).ok())
        })
        .and_then(|value| normalize_base_url(&value))
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

/// 从 hook payload 中提取「用户手输」的提问。
///
/// 只认 `UserPromptSubmit`。`UserPromptExpansion` 紧随其后触发、同样携带 `prompt`，
/// 但那是斜杠命令展开后的模板文本，不是用户输入 —— 放行它会把用户真正的提问覆盖掉。
/// 其余事件本就不带 `prompt`，返回 None 后由内核继承上一次的值。
fn user_prompt(event_name: &str, payload: &Value) -> Option<String> {
    if !is_user_prompt_submit(event_name) {
        return None;
    }
    string_field(payload, &["prompt", "user_prompt", "prompt_text"])
        .and_then(|value| normalize_prompt(&value))
}

/// 构建期由 `build.rs` 从仓库根 `.env` 注入的**本平台**规则表（逗号分隔，`name` / `name:drop` /
/// `name:extract`）。hook 是独立短命进程、cwd 为被监测 agent 的项目目录，运行时读不到仓库里的
/// `.env`，所以列表在构建期注入 —— 改 `.env` 需重新构建才生效。未注入 / 值为空时用下面的默认表。
const TAG_RULES_FROM_ENV: Option<&str> = option_env!("UFO_TAG_RULES_CLAUDE");

/// 没配变量时的内置默认：实测会混进 Claude 提问的元信息标签，一律 `drop`（连内文一起删）。
///
/// Claude 的注入形态是「标签 + 内容都是噪声」，与「正文被包进标签、标签外才是噪声」那种
/// （Qoder `/goal`）方向相反 —— 所以本平台默认表里没有任何 `extract`，规则表也各配各的键。
const DEFAULT_TAG_RULES: &str = "ide_opened_file,system-reminder,local-command,bash_input,bash_output,cli-message,command-message,command-name,command-args,allowed-prompts,env,permission_mode";

/// 归一化提问内容：先按本平台的规则表清掉注入标签，再交给共用的裁剪（去空白 + 超长截断）。
fn normalize_prompt(raw: &str) -> Option<String> {
    let rules = tag_rules();
    if rules.is_empty() {
        return truncate_prompt(raw);
    }
    truncate_prompt(&apply_tag_rules(raw, &rules).unwrap_or_default())
}

/// 本适配器的规则表（扫描机制在 [`super::apply_tag_rules`]，这里只出策略）。
fn tag_rules() -> Vec<TagRule> {
    build_tag_rules(TAG_RULES_FROM_ENV, DEFAULT_TAG_RULES)
}


#[cfg(test)]
mod tests {
    use super::*;

    use crate::modules::agent_status::status::model::{AgentEventKind, SessionPhase};
    use crate::modules::agent_status::status::test_support::TempDir;
    use crate::modules::agent_status::status::StatusRegistry;

    // 只在测试里用到的共享项（EventEnvelope 只在测试里构造事件；长度上限只被断言引用），
    // 故不提到 claude.rs 顶层 —— 否则非测试构建会 unused。
    // TagAction 只在测试里断言默认表用得到，非测试构建不引用它。
    use super::super::{
        EventEnvelope, TagAction, MAX_AGENT_ID_LEN, MAX_BASE_URL_LEN, MAX_PROMPT_CHARS,
        MAX_TRANSCRIPT_PATH_LEN,
    };

    /// 用一份真实形态的 payload 跑一遍适配器 + 内核，断言最终灯态。
    fn light_for_payload(payload: Value, received_at_ms: i64) -> (SessionPhase, bool) {
        let adapted = ClaudeAdapter
            .adapt(&payload, &AdaptContext { cwd: None })
            .expect("claude payload adapts")
            .expect("claude 每个事件都归一出结果");
        let event = adapted.into_event(EventEnvelope {
            provider: Provider::Claude,
            event_id: "e2e".to_string(),
            // 贯穿 id 由 hook 生成（测试里给一个定值即可）：它不参与任何裁决。
            trace_id: "trace-e2e".to_string(),
            received_at_ms,
            pid: None,
        });
        let mut registry = StatusRegistry::new();
        registry.apply_event(event, 30_000);
        let snapshot = registry.snapshot(received_at_ms);
        let session = &snapshot.sessions[0];
        (session.phase, session.is_flashing)
    }

    // ── 适配器端到端：payload → kind / 灯态 ──

    #[test]
    fn stop_payload_drives_flashing_waiting() {
        let payload = serde_json::json!({"session_id": "s1", "hook_event_name": "Stop"});
        let (phase, flashing) = light_for_payload(payload, 100);
        assert_eq!(phase, SessionPhase::Waiting);
        assert!(flashing);
    }

    #[test]
    fn session_end_payload_drives_completed() {
        // SessionEnd.reason（clear）不是失败 outcome，不得被当成红灯。
        let payload = serde_json::json!({
            "session_id": "s1",
            "hook_event_name": "SessionEnd",
            "reason": "clear",
        });
        let (phase, flashing) = light_for_payload(payload, 100);
        assert_eq!(phase, SessionPhase::Completed);
        assert!(flashing);
    }

    #[test]
    fn error_code_wins_over_event_name() {
        // 事件名是执行中，但带 error_code：错误字段优先，落红。
        let payload = serde_json::json!({
            "session_id": "s1",
            "hook_event_name": "PreToolUse",
            "error_code": "overloaded",
        });
        let (phase, _) = light_for_payload(payload, 100);
        assert_eq!(phase, SessionPhase::Error);
    }

    #[test]
    fn notification_permission_prompt_drives_plain_waiting() {
        let payload = serde_json::json!({
            "session_id": "s1",
            "hook_event_name": "Notification",
            "notification_type": "permission_prompt",
        });
        let (phase, flashing) = light_for_payload(payload, 100);
        assert_eq!(phase, SessionPhase::Waiting);
        assert!(!flashing);
    }

    #[test]
    fn adapt_preserves_raw_event_and_diagnostic_fields() {
        let payload = serde_json::json!({
            "session_id": "s1",
            "hook_event_name": "UserPromptSubmit",
            "prompt": "帮我看看这个 bug",
            "agent_id": "agent-abc",
            "transcript_path": "/tmp/sess.jsonl",
        });
        let adapted = ClaudeAdapter
            .adapt(&payload, &AdaptContext { cwd: None })
            .expect("adapts")
            .expect("claude 每个事件都归一出结果");
        assert_eq!(adapted.kind, AgentEventKind::Running);
        assert_eq!(adapted.event, "UserPromptSubmit");
        assert_eq!(adapted.prompt.as_deref(), Some("帮我看看这个 bug"));
        assert_eq!(adapted.agent_id.as_deref(), Some("agent-abc"));
        assert_eq!(adapted.transcript_path.as_deref(), Some("/tmp/sess.jsonl"));
    }

    #[test]
    fn adapt_missing_session_id_is_an_error() {
        let payload = serde_json::json!({"hook_event_name": "Stop"});
        assert!(ClaudeAdapter
            .adapt(&payload, &AdaptContext { cwd: None })
            .is_err());
    }

    // ── 事件名表：Claude 事件名 → 种类 ──

    #[test]
    fn stop_maps_to_turn_finished() {
        assert_eq!(classify("Stop", None), AgentEventKind::TurnFinished);
    }

    #[test]
    fn permission_request_and_elicitation_mean_waiting_for_user() {
        assert_eq!(classify("PermissionRequest", None), AgentEventKind::WaitingForUser);
        assert_eq!(classify("Elicitation", None), AgentEventKind::WaitingForUser);
        assert_eq!(classify("elicit", None), AgentEventKind::WaitingForUser);
    }

    #[test]
    fn waiting_notification_types_map_to_waiting_for_user() {
        for notification_type in [
            "idle_prompt",
            "permission_prompt",
            "elicitation_dialog",
            "agent_needs_input",
            "elicitation_url_dialog",
            "worker_permission_prompt",
        ] {
            assert_eq!(
                classify("Notification", Some(notification_type)),
                AgentEventKind::WaitingForUser,
                "{notification_type}"
            );
        }
    }

    #[test]
    fn non_waiting_or_missing_notification_type_maps_to_running() {
        assert_eq!(
            classify("Notification", Some("auth_success")),
            AgentEventKind::Running
        );
        assert_eq!(classify("Notification", None), AgentEventKind::Running);
    }

    #[test]
    fn permission_denied_and_task_completed_stay_running() {
        assert_eq!(classify("PermissionDenied", None), AgentEventKind::Running);
        assert_eq!(classify("TaskCompleted", None), AgentEventKind::Running);
    }

    #[test]
    fn session_end_maps_to_session_finished() {
        assert_eq!(classify("SessionEnd", None), AgentEventKind::SessionFinished);
        assert_eq!(classify("session_end", None), AgentEventKind::SessionFinished);
    }

    #[test]
    fn stop_failure_maps_to_failed() {
        assert_eq!(classify("StopFailure", None), AgentEventKind::Failed);
    }

    #[test]
    fn unrecognized_event_name_falls_back_to_unknown() {
        assert_eq!(classify("SomethingWeird", None), AgentEventKind::Unknown);
    }

    // ── base_url / settings ──

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
            read_settings_base_url(&[user.clone(), project.clone()], CLAUDE_BASE_URL_KEY)
                .as_deref(),
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
        // 超长仍然丢弃：整体额度只有 hook stdin 的 4 MiB，单个字段再自己收一档，
        // 避免异常环境变量把一个字段撑成整份事件。
        let long = format!("http://example.com/{}", "a".repeat(MAX_BASE_URL_LEN));
        assert_eq!(normalize_base_url(&long), None);
    }

    // ── prompt 提取 / 元标签剔除 ──

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

    /// 逐条平移自本文件旧实现的边界用例：被测函数搬到了共用引擎里，断言值一个都没改 ——
    /// Claude 侧「删标签」的行为必须与迁移前完全一致。
    fn drop_tags(raw: &str, names: &[&str]) -> String {
        let rules: Vec<TagRule> = names.iter().map(|name| TagRule::drop(name)).collect();
        apply_tag_rules(raw, &rules).unwrap_or_default()
    }

    #[test]
    fn strip_removes_tag_and_its_wrapped_content() {
        let raw = "<ide_opened_file>The user opened the file hook.rs</ide_opened_file>\n请帮我排序";
        assert_eq!(drop_tags(raw, &["ide_opened_file", "system-reminder"]), "\n请帮我排序");
    }

    #[test]
    fn strip_handles_multiple_and_interleaved_tags() {
        let raw = "A<system-reminder>x</system-reminder>B<env>y</env>C<ide_opened_file>z</ide_opened_file>D";
        assert_eq!(drop_tags(raw, &["ide_opened_file", "system-reminder", "env"]), "ABCD");
    }

    #[test]
    fn strip_leaves_unlisted_tags_intact() {
        // 代码里常见的 <T>、<div> 不在列表中，绝不能被吞掉。
        let raw = "泛型 <T> 和 <div class=\"x\"> 原样保留";
        assert_eq!(drop_tags(raw, &["env"]), raw);
    }

    #[test]
    fn strip_requires_boundary_so_prefix_does_not_over_match() {
        // 列表里有 env，但 <environment> 不是 env 标签：前缀不能误命中。
        let raw = "看 <environment> 这段";
        assert_eq!(drop_tags(raw, &["env"]), raw);
    }

    #[test]
    fn strip_handles_self_closing_and_unclosed_open_tag() {
        // 自闭合：整段标记吃掉，其后的文字保留。
        assert_eq!(drop_tags("<tag/>keep", &["tag"]), "keep");
        // 带属性的起始标记。
        assert_eq!(drop_tags("<tag a=1>keep", &["tag"]), "keep");
        // 未闭合（prompt 被截断）：只吃起始标记，不吞正文。
        assert_eq!(drop_tags("<tag>keep me", &["tag"]), "keep me");
    }

    #[test]
    fn strip_is_safe_on_multibyte_content() {
        // `<` 后紧跟多字节字符时不能 panic（按字节切片需落在字符边界）。
        let raw = "中文 <漢> 更多 <env>drop</env> tail";
        assert_eq!(drop_tags(raw, &["env"]), "中文 <漢> 更多  tail");
    }

    #[test]
    fn default_rules_are_all_drop() {
        // 本平台的注入形态只有「删标签连同内文」：默认表里出现 extract 就会把正常提问
        // 判成「没有正文」（那是别的平台的模板形态），所以这条要钉住。
        assert!(tag_rules().iter().all(|rule| rule.action == TagAction::Drop));
        assert!(tag_rules().iter().any(|rule| rule.name == "system-reminder"));
    }

    #[test]
    fn normalize_prompt_drops_meta_tags() {
        // 端到端：混进 prompt 的元标签连同内容一起被剔除，只剩用户真正输入。
        let raw = "<ide_opened_file>file.rs</ide_opened_file> 按时间排序 agent";
        let result = normalize_prompt(raw).expect("prompt kept");
        assert_eq!(result, "按时间排序 agent");
    }

    #[test]
    fn normalize_prompt_returns_none_when_only_tags_remain() {
        // 整段都是被剔除的内容时视为「无有效提问」，交给内核继承上一次的值。
        let raw = "<system-reminder>nothing here</system-reminder>";
        assert_eq!(normalize_prompt(raw), None);
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
        // 空 / 空白同样视为未提供，交给内核继承上一次的值
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

    // ── transcript / agent_id ──

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

    #[test]
    fn agent_id_reads_and_trims() {
        let payload: Value = serde_json::json!({"agent_id": "  ac8ff3b71756050d1  "});
        assert_eq!(agent_id(&payload).as_deref(), Some("ac8ff3b71756050d1"));
        // 驼峰别名同样认
        let camel: Value = serde_json::json!({"agentId": "agent-abc"});
        assert_eq!(agent_id(&camel).as_deref(), Some("agent-abc"));
    }

    #[test]
    fn agent_id_is_none_for_main_thread_events() {
        // 主线程事件不带 agent_id（实测为字段缺省，不是空串）。
        assert_eq!(agent_id(&serde_json::json!({})), None);
        assert_eq!(agent_id(&serde_json::json!({"agent_id": null})), None);
        assert_eq!(agent_id(&serde_json::json!({"agent_id": ""})), None);
        assert_eq!(agent_id(&serde_json::json!({"agent_id": "   "})), None);
    }

    #[test]
    fn agent_id_rejects_oversized_value() {
        // 超长标识丢弃 → 视为主线程事件（map 失效也是唯一可能的语义）。
        let huge = "a".repeat(MAX_AGENT_ID_LEN + 1);
        assert_eq!(agent_id(&serde_json::json!({ "agent_id": huge })), None);
    }
}
