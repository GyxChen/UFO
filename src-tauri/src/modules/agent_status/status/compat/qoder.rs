//! Qoder（CLI 与 IDE）平台适配器。
//!
//! ## 为什么走完整的 hook 链路（不像 Codex 那样另建事件源）
//!
//! Qoder 的 hook 覆盖是**完备**的：生命周期四类（SessionStart / Stop / StopFailure /
//! SessionEnd）加授权类（PermissionRequest / Notification）都在，实测一个会话从启动到退出
//! 每步都有事件落进 inbox —— 正是规范里「事件能覆盖生命周期就走路径 A」的判据。
//! 而且它的 payload 与 Claude Code 同构（`session_id` / `hook_event_name` / `transcript_path`
//! / `cwd` / `prompt` / `agent_id` / `notification_type` 全部同名），所以接入面只剩一张事件名表。
//!
//! **CLI 与 IDE 是同一个接入面**：两者读同一份配置（`~/.qoder/settings.json`、
//! `<项目>/.qoder/settings.json`、`<项目>/.qoder/settings.local.json`），一份 hook 配置
//! 同时点亮两端，不需要第二个事件源。
//!
//! ## 与 `claude.rs` 零引用
//!
//! Qoder 的平台知识全在本文件：字段别名、事件名表、等待类 `notification_type` 清单、
//! 提问提取与裁剪规则。`claude.rs` 不引用这里，这里也不引用它 —— 两个平台的灯互不影响，
//! 改 Qoder 不会碰到 Claude（反之亦然）。
//!
//! 只借用 `compat/mod.rs` 里**与平台无关、且 Claude / Codex 同样在用**的通用工具：多别名取值、
//! 事件名归一化、未知事件兜底、错误短路判定、`agent_id` / `transcript_path` 提取、裁剪额度。
//! 那是兼容层的契约本身，不构成两个平台之间的耦合。
//!
//! ## 三处实测差异（与 Claude）
//!
//! 1. **工具失败不是会话失败**：`PostToolUseFailure` 带 `error`（普通工具报错的正文，如路径不存在）。
//!    通用规则「有 error 字段即判失败」会让每次工具报错点亮红灯，而 agent 只是重试一次。
//!    因此本适配器对这一个事件**不读错误字段**，灯保持绿；真正的本轮出错仍由 `StopFailure` 转红。
//! 2. **拿不到真实 endpoint**：Qoder 的 `settings.json` 没有 `env` 块，模型目录是加密的，
//!    BYOK 也不落明文配置。实测模型流量走 `api*.qoder.sh`，但那个 host 由服务端按区域下发
//!    （`center.qoder.sh/api/*/service/region/endpoints`），版本与区域都会换，不适合当档案键。
//!    故 endpoint 用一个稳定身份常量，见 [`QODER_ENDPOINT`]。
//! 3. **transcript 的 token 数值恒为 0**：记录结构与 Claude 同构（`type` / `message.usage` 的
//!    四个 token 键名都在，`<session-id>/subagents/*.jsonl` 布局也一样），但网关不回填数值
//!    （只有 `credits` / `context_usage_ratio` 非 0）。所以复用 `status::tokens::TokenTracker` 是安全的：
//!    解析出 0，前端 `tokenCount > 0` 自然不渲染那一行；哪天网关开始回填，这里就自动出数。

use serde_json::Value;

use super::{
    agent_id, fallback_kind, is_user_prompt_submit, normalize_base_url, normalize_event_name,
    number_field, payload_error, string_field, transcript_path, truncate_prompt, AdaptContext,
    AdaptedEvent, HookAdapter,
};
use crate::modules::agent_status::status::model::AgentEventKind;

/// Qoder 会话的 endpoint **身份常量**，不是一个可请求的地址。
///
/// 为什么要有它：信息卡拓展（流程脚本）按 endpoint 身份建档案（`endpoint_profiles`），
/// endpoint 为空就等于 Qoder 会话**永远配不了拓展字段**。给一个稳定常量后就绪条件成立
/// （`displayEndpoint` 有值 → 登记档案 → 脚本按档案调度），代价是 tooltip 的 Endpoint 行
/// 显示的是这个身份常量本身。
/// ⚠️ 它不可请求、不代表任何用户配置，也不随模型 / 区域变化 —— 换模型不该多出一条档案。
pub(crate) const QODER_ENDPOINT: &str = "https://qoder.cloud";

/// Qoder 适配器单例；`compat::adapter_for(Provider::Qoder)` 直接返回它。
pub(crate) struct QoderAdapter;

impl HookAdapter for QoderAdapter {
    /// 把一次 Qoder hook 的原始 JSON 负载归一化成内核事件。
    fn adapt(
        &self,
        payload: &Value,
        _ctx: &AdaptContext<'_>,
    ) -> Result<Option<AdaptedEvent>, String> {
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
        let normalized = normalize_event_name(&event);

        // ⚠️ 工具失败（`PostToolUseFailure`）的 `error` 是「这次工具没跑成」，不是「本轮出错」：
        // agent 拿到错误文本会自行修正重试，会话没坏。若按通用规则短路成 Failed，用户每敲错
        // 一次路径、每收到一次非零退出码，悬浮窗就闪一下红灯。故这一个事件不读错误字段。
        let tool_failure = normalized == "post_tool_use_failure";
        let error_code = if tool_failure {
            None
        } else {
            string_field(payload, &["error_code", "error", "failure_reason"])
        };
        let outcome = if tool_failure {
            None
        } else {
            string_field(payload, &["outcome", "reason", "status", "stop_reason"])
        };
        let notification_type = string_field(payload, &["notification_type", "notificationType"]);

        let prompt = user_prompt(&event, payload);

        // 错误字段优先于事件名；否则查 Qoder 自己的事件名表。
        let kind = if payload_error(error_code.as_deref(), outcome.as_deref()) {
            AgentEventKind::Failed
        } else {
            classify(&normalized, notification_type.as_deref())
        };

        // 事件名表覆盖 Qoder 配置的全部事件，不返回「忽略」（`Ok(None)`）：
        // 与 Claude 一样，每个事件对灯都是信号。
        Ok(Some(AdaptedEvent {
            session_id,
            kind,
            event,
            agent_id: agent_id(payload),
            error_code,
            outcome,
            notification_type,
            // 身份常量，恒不依赖 payload / 环境 / 配置文件（见 [`QODER_ENDPOINT`]）。
            base_url: normalize_base_url(QODER_ENDPOINT),
            prompt,
            waiting_reason: waiting_reason(&normalized, payload),
            transcript_path: transcript_path(payload),
            sequence: number_field(payload, &["sequence", "seq"]),
            occurred_at_ms: number_field(payload, &["occurred_at_ms", "timestamp_ms"])
                .and_then(|value| i64::try_from(value).ok()),
        }))
    }
}

/// 把 Qoder 的事件名（已归一化成 snake_case）翻译成内核事件种类。
///
/// 表里的名字来自 Qoder 实际会触发的 hook 事件（`~/.qoder/settings.json` 可配的那 12 个，
/// 其中大部分已在真实会话里实测到）。灯色语义与 `reference/doc/agent-status-hooks.md` 一致：
/// 绿常亮 = 执行中；黄 = 等人操作；绿闪落黄 = Stop；绿闪移除 = SessionEnd；红 = StopFailure。
fn classify(normalized_event: &str, notification_type: Option<&str>) -> AgentEventKind {
    match normalized_event {
        // ── 正在执行（绿常亮） ──
        // 工具失败也在这一档：见 `adapt` 里 `tool_failure` 的注释。
        "session_start" | "setup" | "user_prompt_submit" | "pre_tool_use" | "post_tool_use"
        | "post_tool_use_failure" | "permission_denied" | "pre_compact" | "post_compact"
        | "instructions_loaded" | "config_change" | "cwd_changed" | "file_changed"
        | "worktree_create" | "worktree_remove" | "elicitation_result" => AgentEventKind::Running,

        // ── 子代理生命周期 ──
        // 子代理与主会话共用 `session_id`（实测），只有 `agent_id` 分得开；
        // 内核据此维护活跃集合，避免后台子代理把「等你授权」的黄灯顶回绿。
        "subagent_start" => AgentEventKind::SubagentStarted,
        "subagent_stop" => AgentEventKind::SubagentStopped,

        // ── 等待用户操作 ──
        // Stop：一轮回复跑完、等下一条提问 → 内核先绿闪，保留期后落黄常亮。
        "stop" => AgentEventKind::TurnFinished,
        // PermissionRequest / Elicitation：弹框等人选或输入 → 黄灯常亮、不闪。
        "permission_request" | "elicitation" => AgentEventKind::WaitingForUser,
        // Notification：只有「等人」的类型进黄（`is_waiting_notification`），其它保持执行中。
        "notification" => {
            if is_waiting_notification(notification_type) {
                AgentEventKind::WaitingForUser
            } else {
                AgentEventKind::Running
            }
        }

        // ── 对话结束 ──
        // SessionEnd 的 `reason`（实测为 `other` / `clear` 等）不是失败类取值，
        // 不会被 `payload_error` 当成红灯吃掉。
        "session_end" => AgentEventKind::SessionFinished,

        // ── 本轮 API / 模型出错 ──
        "stop_failure" => AgentEventKind::Failed,

        // ── 表未覆盖的名字：靠共享兜底猜，猜不出就是 Unknown（不转灯） ──
        _ => fallback_kind(normalized_event),
    }
}

/// 该 `Notification` 是否在等用户操作。
///
/// 实测值 `permission_prompt`（弹审批 / 选择框）。其余类型（纯提示、已完成回执）按执行中处理。
/// ⚠️ 漏列在等的取值会退化成绿灯 —— 与 Claude 侧同一类坑。
fn is_waiting_notification(notification_type: Option<&str>) -> bool {
    notification_type
        .map(|value| value.to_ascii_lowercase())
        .is_some_and(|value| {
            matches!(
                value.as_str(),
                "permission_prompt" | "idle_prompt" | "elicitation_dialog"
            )
        })
}

/// 「正在等你操作」的原因原文，长度上限（字符）。
///
/// tooltip 只留一行位置，说清「要我批准什么」就够，不需要完整 payload。
const MAX_WAITING_REASON_CHARS: usize = 160;

/// 取「等你操作」的理由。
///
/// 两个来源，按事件类型分流：
/// - `Notification`：Qoder 直接把理由写在顶层 `message`（实测如
///   「Tool AskUserQuestion requires confirmation」）；
/// - `PermissionRequest`：按 Codex 同构的写法取 `tool_input.description`，退到 `tool_input.command`。
///
/// ⚠️ 只走 `waiting_reason`，**不能**塞进 `prompt` —— 那是用户的提问，覆盖它会把 tooltip
/// 的提问行换成系统文案。
fn waiting_reason(normalized_event: &str, payload: &Value) -> Option<String> {
    let raw = match normalized_event {
        "notification" if is_waiting_notification(
            string_field(payload, &["notification_type", "notificationType"]).as_deref(),
        ) =>
        {
            string_field(payload, &["message", "detail", "text"])
        }
        "permission_request" | "elicitation" => payload
            .get("tool_input")
            .and_then(|input| string_at(input, "description"))
            .or_else(|| {
                payload
                    .get("tool_input")
                    .and_then(|input| string_at(input, "command"))
            })
            .or_else(|| string_field(payload, &["message"])),
        _ => None,
    }?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(MAX_WAITING_REASON_CHARS).collect())
}

/// 按名字读一个字符串字段（只在 `tool_input` 这类嵌套对象上用）。
fn string_at(value: &Value, name: &str) -> Option<String> {
    value
        .get(name)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

/// 从 Qoder payload 中提取「用户手输」的提问（**只认 `UserPromptSubmit`**）。
///
/// Qoder 没有 Claude 的 `UserPromptExpansion`（斜杠命令展开的模板文本），所以不存在「展开文本
/// 覆盖用户输入」的问题；但「只在这一个事件采」的纪律要保住 —— 否则一次带 `prompt` 的工具
/// 调用就会把提问改掉。
///
/// **不剔除元标签**：实测 687 条真实 `UserPromptSubmit` payload，`prompt` 全是用户原文、
/// 一个标签都没有 —— Qoder 的注入内容走的是别的通道，不混进这个字段。所以这里只需要
/// 共用的 [`truncate_prompt`]（去空白 + 超长截断）。哪天 Qoder 开始往提问里塞标签，
/// 再按 Claude 那边的做法给本文件补一份剔除逻辑，不与 `claude.rs` 共用。
fn user_prompt(event_name: &str, payload: &Value) -> Option<String> {
    if !is_user_prompt_submit(event_name) {
        return None;
    }
    string_field(payload, &["prompt", "user_prompt", "prompt_text"])
        .and_then(|value| truncate_prompt(&value))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 用一份 payload 跑适配器，拿回归一化结果。
    fn adapt(event_name: &str, extra: Value) -> AdaptedEvent {
        let mut payload = serde_json::json!({"session_id": "s1", "hook_event_name": event_name});
        let map = payload.as_object_mut().expect("object");
        if let Some(extra) = extra.as_object() {
            for (key, value) in extra {
                map.insert(key.clone(), value.clone());
            }
        }
        QoderAdapter
            .adapt(&payload, &AdaptContext { cwd: None })
            .expect("不报错")
            .expect("Qoder 每个事件都归一出结果")
    }

    /// `~/.qoder/settings.json` 里能配的全部 hook 事件 → 期望的内核事件种类。
    ///
    /// 锁住「表 ⊇ Qoder 事件清单」：漏一项就退化成 `fallback_kind` 的猜测，
    /// 表现是该点灯的没点、该红的没红，且没有任何报错。
    const QODER_EVENT_KINDS: &[(&str, AgentEventKind)] = &[
        ("SessionStart", AgentEventKind::Running),
        ("UserPromptSubmit", AgentEventKind::Running),
        ("PreToolUse", AgentEventKind::Running),
        ("PostToolUse", AgentEventKind::Running),
        // 工具报错只算「还在干活」：见 `tool_failure_reports_no_error`。
        ("PostToolUseFailure", AgentEventKind::Running),
        ("PermissionRequest", AgentEventKind::WaitingForUser),
        ("Notification", AgentEventKind::Running),
        ("SubagentStart", AgentEventKind::SubagentStarted),
        ("SubagentStop", AgentEventKind::SubagentStopped),
        ("Stop", AgentEventKind::TurnFinished),
        ("StopFailure", AgentEventKind::Failed),
        ("SessionEnd", AgentEventKind::SessionFinished),
    ];

    #[test]
    fn every_configurable_event_name_is_classified_explicitly() {
        for (name, expected) in QODER_EVENT_KINDS {
            assert_eq!(&adapt(name, serde_json::json!({})).kind, expected, "{name}");
        }
    }

    #[test]
    fn tool_failure_reports_no_error_and_stays_running() {
        // 真实形态：PostToolUseFailure 带 `error`（工具报错正文）。它不得把灯点红。
        let adapted = adapt(
            "PostToolUseFailure",
            serde_json::json!({"error": "Path does not exist: E:\\x\\y.rs", "error_type": "other"}),
        );
        assert_eq!(adapted.kind, AgentEventKind::Running);
        assert_eq!(adapted.error_code, None);
        assert_eq!(adapted.outcome, None);
    }

    #[test]
    fn stop_failure_still_drives_failed() {
        // 真正的本轮出错：事件名 + 错误字段都要能落到红灯。
        assert_eq!(
            adapt("StopFailure", serde_json::json!({})).kind,
            AgentEventKind::Failed
        );
        assert_eq!(
            adapt("StopFailure", serde_json::json!({"error_type": "rate_limit", "error": "429"}))
                .kind,
            AgentEventKind::Failed
        );
    }

    #[test]
    fn waiting_notification_carries_the_reason_text() {
        // 实测形态：Notification 的 `message` 就是「在等你批准什么」。
        let adapted = adapt(
            "Notification",
            serde_json::json!({
                "notification_type": "permission_prompt",
                "message": "Tool AskUserQuestion requires confirmation",
            }),
        );
        assert_eq!(adapted.kind, AgentEventKind::WaitingForUser);
        assert_eq!(
            adapted.waiting_reason.as_deref(),
            Some("Tool AskUserQuestion requires confirmation")
        );
        // ⚠️ 理由绝不能进 prompt：那会覆盖用户真正的提问。
        assert_eq!(adapted.prompt, None);
    }

    #[test]
    fn non_waiting_notification_stays_running_without_a_reason() {
        let adapted = adapt(
            "Notification",
            serde_json::json!({"notification_type": "auth_success", "message": "登录成功"}),
        );
        assert_eq!(adapted.kind, AgentEventKind::Running);
        assert_eq!(adapted.waiting_reason, None);
    }

    #[test]
    fn permission_request_takes_the_tool_input_description_as_reason() {
        let adapted = adapt(
            "PermissionRequest",
            serde_json::json!({
                "tool_name": "Bash",
                "tool_input": {"command": "curl https://example.com", "description": "允许访问官网？"},
            }),
        );
        assert_eq!(adapted.kind, AgentEventKind::WaitingForUser);
        assert_eq!(adapted.waiting_reason.as_deref(), Some("允许访问官网？"));
    }

    #[test]
    fn session_end_reason_is_never_read_as_a_failure() {
        // `reason` 走 outcome 别名，但它的取值（实测 `other`）不在失败类清单里。
        for reason in ["clear", "resume", "logout", "prompt_input_exit", "other"] {
            let adapted = adapt("SessionEnd", serde_json::json!({"reason": reason}));
            assert_eq!(adapted.kind, AgentEventKind::SessionFinished, "{reason}");
        }
    }

    #[test]
    fn subagent_events_keep_their_agent_id() {
        // 实测：子代理的 PreToolUse / SubagentStart / SubagentStop 都带 `agent_id`，
        // 且与主会话共用 `session_id` —— 分开两者的只有这个字段。
        let started =
            adapt("SubagentStart", serde_json::json!({"agent_id": "aExplore-1", "agent_type": "Explore"}));
        assert_eq!(started.kind, AgentEventKind::SubagentStarted);
        assert_eq!(started.agent_id.as_deref(), Some("aExplore-1"));

        let stopped = adapt("SubagentStop", serde_json::json!({"agent_id": "aExplore-1"}));
        assert_eq!(stopped.kind, AgentEventKind::SubagentStopped);
        assert_eq!(stopped.agent_id.as_deref(), Some("aExplore-1"));

        // 主线程事件不带 agent_id。
        assert_eq!(adapt("PreToolUse", serde_json::json!({})).agent_id, None);
    }

    #[test]
    fn prompt_only_from_user_prompt_submit() {
        assert_eq!(
            adapt("UserPromptSubmit", serde_json::json!({"prompt": "qoder 接入"}))
                .prompt
                .as_deref(),
            Some("qoder 接入")
        );
        // 其它事件即使带 prompt 也不认（由内核继承上一次的值）。
        assert_eq!(adapt("PreToolUse", serde_json::json!({"prompt": "噪音"})).prompt, None);
    }

    #[test]
    fn prompt_is_taken_verbatim_and_trimmed() {
        // 实测 Qoder 的 `prompt` 就是用户原文（不含元标签）：只做 trim，不改内容。
        let adapted = adapt("UserPromptSubmit", serde_json::json!({"prompt": "  接入 qoder  
"}));
        assert_eq!(adapted.prompt.as_deref(), Some("接入 qoder"));
        // 尖括号内容原样保留（用户真在写代码时不该被吞掉）。
        let angle = adapt("UserPromptSubmit", serde_json::json!({"prompt": "泛型 <T> 怎么写"}));
        assert_eq!(angle.prompt.as_deref(), Some("泛型 <T> 怎么写"));
    }

    #[test]
    fn endpoint_is_the_identity_constant() {
        // 哨兵与 cwd / 环境无关，且不含任何用户配置或凭据。
        assert_eq!(
            adapt("Stop", serde_json::json!({})).base_url.as_deref(),
            Some(QODER_ENDPOINT)
        );
    }

    #[test]
    fn transcript_path_passes_through() {
        let path = r"C:\Users\me\.qoder\projects\E--x\96288d43.jsonl";
        let adapted = adapt("UserPromptSubmit", serde_json::json!({"transcript_path": path}));
        assert_eq!(adapted.transcript_path.as_deref(), Some(path));
    }

    #[test]
    fn unrecognized_event_name_does_not_panic_and_keeps_light() {
        // 认不出的名字（未来版本可能新增）走共享兜底，不能 panic、也不报错。
        let adapted = adapt("SomethingWeird", serde_json::json!({}));
        assert_eq!(adapted.kind, AgentEventKind::Unknown);
        // 带 start 字样的未知事件按执行中兜底。
        assert_eq!(
            adapt("FutureThingStart", serde_json::json!({})).kind,
            AgentEventKind::Running
        );
    }

    #[test]
    fn missing_session_id_is_an_error() {
        let payload = serde_json::json!({"hook_event_name": "Stop"});
        assert!(QoderAdapter
            .adapt(&payload, &AdaptContext { cwd: None })
            .is_err());
    }
}
