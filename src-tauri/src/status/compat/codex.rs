//! Codex 平台适配器。
//!
//! Codex（CLI / 桌面版）的 hook 机制与 Claude Code 同构：把命令配一次
//! （`~/.codex/hooks.json` 或 `config.toml` 的 `[hooks]`，用 `/hooks` 信任后生效），
//! 之后由 Codex 在生命周期事件发生时调用，stdin 传入 JSON。
//!
//! 事件清单（实测自本机 codex.exe v0.155.0-alpha.2.6 的 `HOOK_EVENT_NAMES`）：
//! `PreToolUse` / `PermissionRequest` / `PostToolUse` / `PreCompact` / `PostCompact` /
//! `SessionStart` / `SessionEnd` / `UserPromptSubmit` / `SubagentStart` / `SubagentStop` /
//! `Stop` / `Interrupt`。
//!
//! 与 Claude 的差异（本文件必须自己处理的部分）：
//! - **没有 `Notification` 事件**，也就没有 `notification_type` 分流。
//! - `Interrupt`（用户 Esc / Ctrl-C 取消当前轮）是 Claude 没有的独立事件。
//! - endpoint 写在 `~/.codex/config.toml`（TOML）里，不是 JSON 配置文件。

use std::{collections::BTreeMap, env, fs, path::{Path, PathBuf}};

use serde::Deserialize;
use serde_json::Value;

use super::{
    agent_id, base_url_env_names, fallback_kind, home_dir, is_user_prompt_submit,
    normalize_base_url, normalize_event_name, payload_error, string_field, transcript_path,
    truncate_prompt, AdaptContext, AdaptedEvent, HookAdapter,
};
use crate::status::model::{AgentEvent, AgentEventKind, Provider, SCHEMA_VERSION};

/// Codex 适配器单例；`compat::adapter_for(Provider::Codex)` 直接返回它。
pub(crate) struct CodexAdapter;

impl HookAdapter for CodexAdapter {
    /// 把一次 Codex hook 的原始 JSON 负载归一化成内核事件。
    fn adapt(&self, payload: &Value, ctx: &AdaptContext<'_>) -> Result<AdaptedEvent, String> {
        let session_id = string_field(payload, &["session_id", "sessionId", "thread_id"])
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

        // Codex 没有 error_code 字段；`reason` 只出现在 SessionEnd（当前恒为 "other"）。
        // 仍按共用规则过一遍：将来 Codex 补上失败类字段时无需改这里。
        let error_code = string_field(payload, &["error_code", "error"]);
        let outcome = string_field(payload, &["reason", "outcome", "status"]);

        let kind = if payload_error(error_code.as_deref(), outcome.as_deref()) {
            AgentEventKind::Failed
        } else {
            classify(&event)
        };

        Ok(AdaptedEvent {
            session_id,
            kind,
            event: event.clone(),
            agent_id: agent_id(payload),
            error_code,
            outcome,
            // Codex 没有 Notification 事件，该字段恒为空。
            notification_type: None,
            base_url: codex_base_url(ctx.cwd),
            prompt: user_prompt(&event, payload),
            transcript_path: transcript_path(payload),
            // Codex 的 hook payload 没有全局序号（rollout 里的 ordinal 不在 payload 中），
            // 乱序保护对它不生效 —— 事件本身带 turn_id 但并非单调递增的会话序号。
            sequence: None,
            occurred_at_ms: None,
        })
    }
}

/// 把 Codex 的事件名归一化成内核事件种类。
///
/// 灯色语义（与 CLAUDE.md 的指示灯约定一致）：
/// 绿常亮 = 执行中；黄 = 等人操作；绿闪落黄 = 一轮正常结束；绿闪移除 = 会话结束。
fn classify(event_name: &str) -> AgentEventKind {
    let normalized = normalize_event_name(event_name);
    match normalized.as_str() {
        // ── 正在执行（绿常亮） ──
        // 工具调用 / 压缩 / 子代理 / 会话启动 / 用户提交提问都表示「这条链路在跑」。
        // ⚠️ 子代理事件（SubagentStart/SubagentStop，以及子代理内部触发的事件）带 agent_id，
        // 由内核对**已存活**会话拦成「只刷存活时间」，因此不会把黄灯顶回绿。
        "session_start" | "user_prompt_submit" | "pre_tool_use" | "post_tool_use"
        | "pre_compact" | "post_compact" | "subagent_start" | "subagent_stop" => {
            AgentEventKind::Running
        }

        // ── 等用户操作（黄灯，不闪） ──
        // PermissionRequest：工具调用要你批准（如 shell 提权 / 网络授权）
        // Interrupt：你按 Esc / Ctrl-C 取消了当前轮 —— 本轮到此为止、等你下一条指令。
        //   刻意不落红灯：本项目的红灯语义是「接口/不可抗力错误」，而这是用户主动取消。
        "permission_request" | "interrupt" => AgentEventKind::WaitingForUser,

        // ── 一轮正常结束（先绿闪，retention 后落黄；对话未结束） ──
        "stop" => AgentEventKind::TurnFinished,

        // ── 对话结束（绿闪后移除） ──
        "session_end" => AgentEventKind::SessionFinished,

        _ => fallback_kind(&normalized),
    }
}

/// 从 hook payload 中提取用户提交的提问（只认 `UserPromptSubmit`）。
///
/// Codex 的该事件带 `prompt` 字段，与 Claude 同名字段，因此采集逻辑共用。
/// 与 Claude 的区别是**不做元标签剔除**：那些 `<system-reminder>` 之类是 Claude Code
/// 自己注入的，Codex 不注入，也就没有可剥的东西 —— 只做去空白与超长截断。
fn user_prompt(event_name: &str, payload: &Value) -> Option<String> {
    if !is_user_prompt_submit(event_name) {
        return None;
    }
    string_field(payload, &["prompt", "user_prompt", "prompt_text"])
        .and_then(|value| truncate_prompt(&value))
}

/// Codex rollout 里「一轮以错误结束」这一事实。
///
/// 它**不是 hook 事件**：Codex 的 12 个 hook 事件里没有错误类（Claude 有 `StopFailure`），
/// 且实测「重试耗尽」这种 error 结束连 `Stop` 都不触发 —— 于是红灯在 Codex 上原本不可达。
/// 这个结构承载从 rollout 里补读到的那份事实，由 `status::codex_rollout` 增量巡检产出。
pub(crate) struct RolloutTurnError {
    /// 去重键：同一轮只上报一次。
    pub turn_id: String,
    /// 错误正文（`error.message`），落进快照的 `error_code` 供排障。
    pub message: String,
}

/// 错误正文长度上限（字符）：够看清是什么错，又不至于撑大每轮广播的快照。
const MAX_ERROR_MESSAGE_CHARS: usize = 300;

/// 从一行 rollout 记录里识别「本轮以错误结束」。
///
/// 只认 `event_msg` / `task_complete` **带非空 `error`**：
/// - 中间的重试失败不会写成 `task_complete`，所以不会把「重试后成功」误报成错误；
/// - 正常完成的那条 `task_complete` 不带 `error` 字段。
pub(crate) fn rollout_turn_error(line: &[u8]) -> Option<RolloutTurnError> {
    if line.is_empty() {
        return None;
    }
    let record: Value = serde_json::from_slice(line).ok()?;
    if record.get("type").and_then(Value::as_str) != Some("event_msg") {
        return None;
    }
    let payload = record.get("payload")?;
    if payload.get("type").and_then(Value::as_str) != Some("task_complete") {
        return None;
    }
    let error = payload.get("error").filter(|error| !error.is_null())?;

    // 去重键优先 turn_id；个别记录没带时退化成文件内的 ordinal（同样唯一），都没有才放弃。
    let turn_id = payload
        .get("turn_id")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            record
                .get("ordinal")
                .map(|ordinal| format!("ordinal-{ordinal}"))
        })?;
    // 正文优先取 error.message；形态变了就退化成整块 JSON，总比丢掉这条错误强。
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| error.to_string());

    Some(RolloutTurnError {
        turn_id,
        message: message.chars().take(MAX_ERROR_MESSAGE_CHARS).collect(),
    })
}

impl RolloutTurnError {
    /// 归一化成内核事件（红灯）。
    ///
    /// `event_id` 由轮 id 派生、**稳定不变**：内核按 `provider:session:event_id` 去重，
    /// 万一同一条记录被重读（rollout 被截断重扫）也不会把红灯重复点亮。
    pub(crate) fn into_event(&self, session_id: &str, received_at_ms: i64) -> AgentEvent {
        AgentEvent {
            schema_version: SCHEMA_VERSION,
            event_id: format!("rollout-error-{}", self.turn_id),
            provider: Provider::Codex,
            session_id: session_id.to_string(),
            kind: AgentEventKind::Failed,
            // 诊断字段：标出这条不是 hook 事件，而是从 rollout 补出来的。
            event: "task_complete(error)".to_string(),
            sequence: None,
            occurred_at_ms: None,
            received_at_ms,
            pid: None,
            error_code: Some(self.message.clone()),
            outcome: None,
            notification_type: None,
            base_url: None,
            prompt: None,
            transcript_path: None,
            agent_id: None,
        }
    }
}

/// 读取 Codex 当前会话的 API endpoint。
///
/// 取值顺序：**配置文件优先，环境变量兜底**（与 Claude 适配器同一口径）。
/// - 配置文件：`$CODEX_HOME/config.toml`（默认 `~/.codex/config.toml`）→
///   `<cwd>/.codex/config.toml` 覆盖前者。取顶层 `model_provider` 指向的
///   `[model_providers.<name>].base_url`。
/// - 环境变量：`OPENAI_BASE_URL` / `OPENAI_API_BASE`。
///
/// 只读 `base_url` 一个 key：同文件里的 `auth.json` / api key 一律不读取、不缓存、不上报。
fn codex_base_url(cwd: Option<&Path>) -> Option<String> {
    read_config_base_url(&codex_config_paths(home_dir().as_deref(), cwd))
        .or_else(|| {
            base_url_env_names(Provider::Codex)
                .iter()
                .find_map(|name| env::var(name).ok())
        })
        .and_then(|value| normalize_base_url(&value))
}

/// Codex 配置文件路径，按优先级**从低到高**排列（后者覆盖前者）。
///
/// `CODEX_HOME` 环境变量可整体搬走 Codex 的配置目录（与 Codex 自身一致）；
/// 未设置时回落 `~/.codex`。cwd 缺失时只读用户级配置。
fn codex_config_paths(home: Option<&Path>, cwd: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let codex_home = env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| home.map(|home| home.join(".codex")));
    if let Some(codex_home) = codex_home {
        paths.push(codex_home.join("config.toml"));
    }
    if let Some(cwd) = cwd {
        paths.push(cwd.join(".codex").join("config.toml"));
    }
    paths
}

/// 依次读取配置文件，后面的覆盖前面的；返回最后一个命中的 base_url。
fn read_config_base_url(paths: &[PathBuf]) -> Option<String> {
    let mut found = None;
    for path in paths {
        if let Some(value) = base_url_from_config(path) {
            found = Some(value);
        }
    }
    found
}

/// `config.toml` 里我们关心的两个键（其余字段一律忽略，不做全量解析）。
///
/// `basic-toml` 只支持 serde 反序列化、没有通用 Value，所以按最小 schema 声明：
/// 这样既拿得到需要的字段，又不会因为 Codex 新增配置项而解析失败。
#[derive(Deserialize)]
struct CodexConfig {
    /// 当前启用的 provider 名（如 `custom`）；缺省时 Codex 用内置 `openai`。
    #[serde(default)]
    model_provider: Option<String>,
    /// 自定义 provider 表：`[model_providers.<name>]`。
    #[serde(default)]
    model_providers: BTreeMap<String, CodexProviderConfig>,
}

/// `[model_providers.<name>]` 里我们只读 `base_url`。
///
/// ⚠️ 同表内通常还有 `requires_openai_auth` 等字段；api key 在 `~/.codex/auth.json` 里，
/// 本模块**不读**它，也不读这里除 `base_url` 外的任何键。
#[derive(Deserialize)]
struct CodexProviderConfig {
    /// 该 provider 的 API 地址。
    #[serde(default)]
    base_url: Option<String>,
}

/// 从单个 `config.toml` 里取「当前 model_provider 的 base_url」。
///
/// 文件缺失 / TOML 非法 / 结构不符一律返回 None（静默跳过）：hook 必须快速返回，
/// 不能因为一个损坏的配置让状态上报失败。`model_provider` 缺省时 Codex 用的是内置
/// `openai`，而内置 provider 不写在 `model_providers` 表里 → 自然回落环境变量。
fn base_url_from_config(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    let config: CodexConfig = basic_toml::from_str(&raw).ok()?;
    let provider = config.model_provider.as_deref().unwrap_or("openai");
    config.model_providers.get(provider)?.base_url.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::status::model::{AgentEventKind, SessionPhase};
    use crate::status::test_support::TempDir;
    use crate::status::StatusRegistry;

    // EventEnvelope 只在测试里构造事件用，故不提到 codex.rs 顶层（否则非测试构建会 unused）。
    use super::super::EventEnvelope;

    /// 用一份真实形态的 payload 跑一遍适配器 + 内核，断言最终灯态。
    fn light_for_payload(payload: Value, received_at_ms: i64) -> (SessionPhase, bool) {
        let adapted = CodexAdapter
            .adapt(&payload, &AdaptContext { cwd: None })
            .expect("codex payload adapts");
        let event = adapted.into_event(EventEnvelope {
            provider: Provider::Codex,
            event_id: "e2e".to_string(),
            received_at_ms,
            pid: None,
        });
        let mut registry = StatusRegistry::new();
        registry.apply_event(event, 30_000);
        let snapshot = registry.snapshot(received_at_ms);
        let session = &snapshot.sessions[0];
        (session.phase, session.is_flashing)
    }

    // ── 事件名表：Codex 事件名 → 种类 ──

    #[test]
    fn running_events_map_to_running() {
        for name in [
            "SessionStart",
            "UserPromptSubmit",
            "PreToolUse",
            "PostToolUse",
            "PreCompact",
            "PostCompact",
            "SubagentStart",
            "SubagentStop",
        ] {
            assert_eq!(classify(name), AgentEventKind::Running, "{name}");
        }
    }

    #[test]
    fn permission_request_means_waiting_for_user() {
        assert_eq!(classify("PermissionRequest"), AgentEventKind::WaitingForUser);
    }

    #[test]
    fn interrupt_means_waiting_for_user_without_flash() {
        // 用户 Esc / Ctrl-C 取消本轮：黄灯（等你下一条），刻意不落红灯。
        assert_eq!(classify("Interrupt"), AgentEventKind::WaitingForUser);
    }

    #[test]
    fn stop_means_turn_finished() {
        assert_eq!(classify("Stop"), AgentEventKind::TurnFinished);
    }

    #[test]
    fn session_end_means_session_finished() {
        assert_eq!(classify("SessionEnd"), AgentEventKind::SessionFinished);
    }

    #[test]
    fn unrecognized_event_name_falls_back_to_unknown() {
        assert_eq!(classify("SomethingWeird"), AgentEventKind::Unknown);
    }

    #[test]
    fn classify_tolerates_snake_case_variants() {
        // hook_event_name 的写法可能带分隔符差异，归一化后再查表。
        assert_eq!(classify("pre_tool_use"), AgentEventKind::Running);
        assert_eq!(classify("session-end"), AgentEventKind::SessionFinished);
    }

    // ── 端到端：payload → 灯态 ──

    #[test]
    fn stop_payload_drives_flashing_waiting() {
        let payload = serde_json::json!({
            "session_id": "thr_1",
            "hook_event_name": "Stop",
            "turn_id": "turn_1",
        });
        let (phase, flashing) = light_for_payload(payload, 100);
        assert_eq!(phase, SessionPhase::Waiting);
        assert!(flashing);
    }

    #[test]
    fn interrupt_payload_drives_plain_waiting() {
        let payload = serde_json::json!({
            "session_id": "thr_1",
            "hook_event_name": "Interrupt",
            "turn_id": "turn_1",
        });
        let (phase, flashing) = light_for_payload(payload, 100);
        assert_eq!(phase, SessionPhase::Waiting);
        assert!(!flashing, "取消本轮不该闪绿（那是「正常完成」的信号）");
    }

    #[test]
    fn permission_request_payload_drives_plain_waiting() {
        let payload = serde_json::json!({
            "session_id": "thr_1",
            "hook_event_name": "PermissionRequest",
            "tool_name": "Bash",
            "tool_input": {"command": "rm -rf build"},
        });
        let (phase, flashing) = light_for_payload(payload, 100);
        assert_eq!(phase, SessionPhase::Waiting);
        assert!(!flashing);
    }

    #[test]
    fn session_end_payload_drives_completed() {
        let payload = serde_json::json!({
            "session_id": "thr_1",
            "hook_event_name": "SessionEnd",
            "reason": "other",
        });
        let (phase, flashing) = light_for_payload(payload, 100);
        assert_eq!(phase, SessionPhase::Completed);
        assert!(flashing);
    }

    #[test]
    fn pre_tool_use_payload_drives_running() {
        let payload = serde_json::json!({
            "session_id": "thr_1",
            "hook_event_name": "PreToolUse",
            "tool_name": "apply_patch",
        });
        let (phase, _) = light_for_payload(payload, 100);
        assert_eq!(phase, SessionPhase::Running);
    }

    /// 子代理事件与主会话共用 session_id：内核靠 agent_id 拦住它，黄灯不被顶回绿。
    #[test]
    fn subagent_event_does_not_override_waiting_light() {
        let mut registry = StatusRegistry::new();
        let waiting = CodexAdapter
            .adapt(
                &serde_json::json!({
                    "session_id": "thr_1",
                    "hook_event_name": "PermissionRequest",
                }),
                &AdaptContext { cwd: None },
            )
            .expect("adapts")
            .into_event(EventEnvelope {
                provider: Provider::Codex,
                event_id: "1".to_string(),
                received_at_ms: 100,
                pid: None,
            });
        registry.apply_event(waiting, 30_000);

        let subagent = CodexAdapter
            .adapt(
                &serde_json::json!({
                    "session_id": "thr_1",
                    "hook_event_name": "PreToolUse",
                    "agent_id": "child-thread-1",
                    "agent_type": "explorer",
                }),
                &AdaptContext { cwd: None },
            )
            .expect("adapts")
            .into_event(EventEnvelope {
                provider: Provider::Codex,
                event_id: "2".to_string(),
                received_at_ms: 200,
                pid: None,
            });
        assert_eq!(subagent.agent_id.as_deref(), Some("child-thread-1"));
        registry.apply_event(subagent, 30_000);

        let session = &registry.snapshot(200).sessions[0];
        assert_eq!(session.phase, SessionPhase::Waiting, "子代理事件不得顶回绿灯");
    }

    // ── 字段提取 ──

    #[test]
    fn adapt_captures_prompt_transcript_and_agent_id() {
        let payload = serde_json::json!({
            "session_id": "thr_1",
            "hook_event_name": "UserPromptSubmit",
            "prompt": "  修一下这个 bug  ",
            "transcript_path": "/tmp/rollout.jsonl",
            "agent_id": "child-1",
            "agent_type": "explorer",
        });
        let adapted = CodexAdapter
            .adapt(&payload, &AdaptContext { cwd: None })
            .expect("adapts");
        assert_eq!(adapted.kind, AgentEventKind::Running);
        assert_eq!(adapted.event, "UserPromptSubmit");
        assert_eq!(adapted.prompt.as_deref(), Some("修一下这个 bug"));
        assert_eq!(adapted.transcript_path.as_deref(), Some("/tmp/rollout.jsonl"));
        assert_eq!(adapted.agent_id.as_deref(), Some("child-1"));
        // Codex 没有 Notification 事件，该字段恒为空。
        assert_eq!(adapted.notification_type, None);
    }

    #[test]
    fn prompt_is_only_taken_from_user_prompt_submit() {
        let payload = serde_json::json!({"prompt": "不该被采集"});
        assert_eq!(user_prompt("PreToolUse", &payload), None);
        assert_eq!(user_prompt("SessionStart", &payload), None);
        assert!(user_prompt("UserPromptSubmit", &payload).is_some());
    }

    #[test]
    fn null_transcript_path_is_none() {
        // Codex 的 /side 分叉会话 transcript_path 为 null。
        let payload = serde_json::json!({
            "session_id": "thr_1",
            "hook_event_name": "SessionStart",
            "transcript_path": null,
        });
        let adapted = CodexAdapter
            .adapt(&payload, &AdaptContext { cwd: None })
            .expect("adapts");
        assert_eq!(adapted.transcript_path, None);
    }

    #[test]
    fn adapt_missing_session_id_is_an_error() {
        let payload = serde_json::json!({"hook_event_name": "Stop"});
        assert!(CodexAdapter
            .adapt(&payload, &AdaptContext { cwd: None })
            .is_err());
    }

    // ── endpoint：config.toml 解析 ──

    #[test]
    fn reads_base_url_of_the_active_model_provider() {
        let temp = TempDir::new("codex-config");
        temp.write(
            ".codex/config.toml",
            r#"
model_provider = "custom"
disable_response_storage = true

[model_providers.custom]
name = "HiKaRi"
base_url = "http://127.0.0.1:15721/v1"
wire_api = "responses"
"#,
        );
        // 项目级配置覆盖用户级；测试只给 cwd，因此结果与用户真实 ~/.codex 无关。
        assert_eq!(
            codex_base_url(Some(temp.path())).as_deref(),
            Some("http://127.0.0.1:15721/v1")
        );
    }

    #[test]
    fn ignores_other_providers_and_missing_keys() {
        let temp = TempDir::new("codex-config-other");
        temp.write(
            ".codex/config.toml",
            r#"
model_provider = "custom"

[model_providers.custom]
base_url = "http://only-this/v1"

[model_providers.backup]
base_url = "http://not-this/v1"
"#,
        );
        assert_eq!(
            codex_config_base_url(&temp).as_deref(),
            Some("http://only-this/v1")
        );
    }

    /// 只解析该临时目录下的 `.codex/config.toml`。
    ///
    /// 刻意不走 `codex_config_paths`：那条路会读 `CODEX_HOME` / 用户真实的 `~/.codex`，
    /// 测试结果就不只取决于本用例的输入了。
    fn codex_config_base_url(temp: &TempDir) -> Option<String> {
        let path = temp.path().join(".codex").join("config.toml");
        read_config_base_url(&[path]).and_then(|value| normalize_base_url(&value))
    }

    #[test]
    fn unknown_model_provider_yields_nothing() {
        let temp = TempDir::new("codex-config-unknown");
        // 指向未在 model_providers 里定义的内置 provider → 不产出 base_url（回落环境变量）。
        temp.write(
            ".codex/config.toml",
            r#"
model_provider = "openai"
"#,
        );
        assert_eq!(codex_config_base_url(&temp), None);
    }

    #[test]
    fn broken_config_is_skipped_not_fatal() {
        let temp = TempDir::new("codex-config-broken");
        temp.write(".codex/config.toml", "this is not = = toml");
        assert_eq!(codex_config_base_url(&temp), None);
    }

    #[test]
    fn later_config_file_overrides_earlier_one() {
        let temp = TempDir::new("codex-config-order");
        temp.write(
            "user/config.toml",
            "model_provider = \"custom\"\n[model_providers.custom]\nbase_url = \"http://user\"\n",
        );
        temp.write(
            "proj/.codex/config.toml",
            "model_provider = \"custom\"\n[model_providers.custom]\nbase_url = \"http://project\"\n",
        );
        let paths = vec![
            temp.path().join("user").join("config.toml"),
            temp.path().join("proj").join(".codex").join("config.toml"),
        ];
        assert_eq!(
            read_config_base_url(&paths).as_deref(),
            Some("http://project")
        );
    }

    #[test]
    fn config_paths_prefer_project_scoped_file_last() {
        let paths = codex_config_paths(Some(Path::new("/home/u")), Some(Path::new("/proj")));
        assert_eq!(paths.len(), 2);
        assert!(paths[1].starts_with("/proj"));
        // 没有 home / cwd 时不报错，只是少读几个文件。
        assert!(codex_config_paths(None, None).is_empty());
    }
}
