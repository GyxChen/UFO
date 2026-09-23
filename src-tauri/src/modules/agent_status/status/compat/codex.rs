//! Codex 平台适配器：解析 **rollout**（Codex 自己写的会话日志），不依赖 hook。
//!
//! ## 为什么不用 hook
//!
//! Codex 确实有一套 hook（12 个事件），但实测**不足以支撑状态灯**：
//! - **没有错误类事件**（Claude 有 `StopFailure`）：429/502 这类以 error 结束的轮次不触发任何 hook；
//! - **中断路径也不触发**：从 Codex Desktop 的停止按钮取消一轮（app-server 的 `turn/interrupt`），
//!   `turn_aborted` 已写进 rollout，hook 侧却一次调用都没有；
//! - **会静默失效**：hooks 需要用户在设置页逐条信任（且按内容哈希记账，改一次要重新信任），
//!   实测出现过「一段时间内一次都不触发、且四处日志无任何报错」的情况。
//!
//! rollout 则相反：它是 Codex 自己 resume 用的账本，**turn 级真相全在里面**
//! （`task_started` / `task_complete` / `turn_aborted`），且零配置、永不静默失效。
//!
//! ## 唯一的例外：`PermissionRequest`
//!
//! rollout 里**没有**「正在等你授权」的任何记录（记录类型、item 字段、output 正文都查过），
//! 所以这一个状态只能靠 hook 补 —— 实测 Codex 确实会触发 `PermissionRequest`，且 payload 里
//! 带着 `tool_name` / `tool_input.command` / `tool_input.description`（给用户看的审批理由）。
//!
//! 因此 [`CodexPermissionAdapter`] **刻意极窄**：只认这一个事件，其余一律 `Ok(None)` 静默忽略。
//! 两个来源同时驱动同一盏灯会互相顶（rollout 说绿、hook 说黄），窄口子才不会打架：
//! 等审批期间 rollout 不写任何记录（实测），批准后命令一跑就有新记录 → 自然恢复绿灯。
//!
//! ## 本文件负责什么
//!
//! 把一行 rollout JSON 归一化成内核能消费的东西（见 [`CodexRecord`]）、归一化
//! `PermissionRequest` 这一条 hook，以及解析 Codex 的配置/上游地址。
//! **增量读文件、会话发现、去重、组装事件**在 `status::codex_rollout`。
//!
//! ⚠️ rollout 是 Codex 的内部格式（非公开 API），只按记录类型取需要的字段；
//! 认不出的记录一律**不产出事件**（宁可少动灯，也不猜）。

use std::{collections::BTreeMap, env, fs, path::{Path, PathBuf}};

use serde::Deserialize;
use serde_json::Value;

use super::{base_url_env_names, home_dir, normalize_base_url, string_field};
use crate::modules::agent_status::status::model::{AgentEventKind, Provider, TokenTotals};

/// Codex 的 hook 适配器：**只认 `PermissionRequest`**，其余事件静默忽略。
///
/// 它是 rollout 之外唯一的补充来源，见模块文档「唯一的例外」。一个事件都不要多认 ——
/// 多认一个，就多一处与 rollout 抢同一盏灯的地方。
pub(crate) struct CodexPermissionAdapter;

impl super::HookAdapter for CodexPermissionAdapter {
    fn adapt(
        &self,
        payload: &Value,
        ctx: &super::AdaptContext<'_>,
    ) -> Result<Option<super::AdaptedEvent>, String> {
        let event = string_field(
            payload,
            &["hook_event_name", "event", "event_name", "event_type", "type"],
        )
        .unwrap_or_default();
        // 其余 11 个事件一概不管（包括 PreToolUse 这种 rollout 已经覆盖的）：
        // 返回 Ok(None) 让 hook 外壳静默跳过，既不算失败也不落盘。
        if super::normalize_event_name(&event) != "permission_request" {
            return Ok(None);
        }
        let session_id = string_field(payload, &["session_id", "sessionId", "thread_id"])
            .ok_or_else(|| "hook payload has no session id".to_string())?;

        Ok(Some(super::AdaptedEvent {
            session_id,
            // 等人授权 → 黄灯不闪（与「用户中断」同一语义：都在等你，都不是错误）。
            kind: AgentEventKind::WaitingForUser,
            event,
            agent_id: None,
            error_code: None,
            outcome: None,
            notification_type: None,
            base_url: codex_base_url(ctx.cwd),
            // ⚠️ 审批理由走 `waiting_reason`，**不能**塞进 `prompt` —— 那是用户的提问，
            // 塞进去会把 tooltip 的提问行覆盖掉。
            waiting_reason: approval_reason(payload),
            prompt: None,
            transcript_path: super::transcript_path(payload),
            sequence: None,
            occurred_at_ms: None,
        }))
    }
}

/// 审批理由的长度上限（字符）：tooltip 只有一行位置，够说清「要我批准什么」即可。
const MAX_REASON_CHARS: usize = 160;

/// 从 `PermissionRequest` payload 里取「给用户看的审批理由」。
///
/// 优先 `tool_input.description`（Codex 自己写的自然语言说明，如「是否允许我访问 …」），
/// 退而用 `tool_input.command`（命令原文）。两者都没有就不显示理由。
fn approval_reason(payload: &Value) -> Option<String> {
    let tool_input = payload.get("tool_input");
    let reason = tool_input
        .and_then(|input| string_at(input, "description"))
        .or_else(|| tool_input.and_then(|input| string_at(input, "command")))
        .or_else(|| tool_input.and_then(|input| string_at(input, "cmd")))?;
    let trimmed = reason.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(MAX_REASON_CHARS).collect())
}

/// 会话身份：只有 `session_meta`（文件首行）带。
///
/// 会话 id **不在这里** —— 它由 `codex_rollout` 从文件名末尾的 uuid 直接取
/// （实测与 `session_meta.id` 同一个值），这样会话 key 在读到内容之前就确定了。
pub(crate) struct CodexSessionMeta {
    /// 会话工作目录：用来定位项目级配置、解析 endpoint。
    pub cwd: Option<String>,
}

/// 一行 rollout 归一化后的结果。
///
/// 一次解析同时产出四类信息（一条记录通常只占其中一项）：灯态、会话身份、token 用量、提问。
pub(crate) struct CodexRecord {
    /// 该记录对应的灯态；`None` 表示这条记录不改变灯态（如 `token_count`）。
    pub kind: Option<AgentEventKind>,
    /// 记录名，写进事件的 `event` 字段供诊断。
    pub name: &'static str,
    /// turn 级记录带的轮 id（去重键）；非 turn 级记录为 None。
    pub turn_id: Option<String>,
    /// `task_complete` 的错误正文。
    pub error_code: Option<String>,
    /// `user_message` 的提问文本。
    pub prompt: Option<String>,
    /// `session_meta` 带来的会话身份。
    pub session: Option<CodexSessionMeta>,
    /// `token_count` 带来的会话级累计用量。
    pub tokens: Option<TokenTotals>,
}

/// 字节级预筛：这一行是否**可能**是我们关心的记录。
///
/// 实测 97% 的字节落在不需要解析的大记录上（命令输出、消息正文、工具输出，单条可达 100 KB）。
/// 这里只做一次子串扫描（`str::contains`，两级字节搜索），命中的少数小记录才做完整 JSON 解析。
///
/// 非 UTF-8 或认不出标记的行返回 false —— 由调用方按「文件在长 = 有活动」处理，
/// 因此漏判只会退化成「当作活动」而不会丢状态。
pub(crate) fn cares(line: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(line) else {
        return false;
    };
    const MARKERS: [&str; 7] = [
        "\"session_meta\"",
        "\"task_started\"",
        "\"task_complete\"",
        "\"turn_aborted\"",
        // CLI 写小写；**Desktop 只写 `item_completed` 里的 `UserMessage` item**（实测），
        // 提问正文在 item.content[].text —— 少认这一个标记，桌面端的 tooltip 就没有提问。
        "\"user_message\"",
        "\"UserMessage\"",
        "\"token_count\"",
    ];
    MARKERS.iter().any(|marker| text.contains(marker))
}

/// 把一行 rollout 记录归一化成 [`CodexRecord`]；认不出返回 None。
///
/// 灯态映射（与 CLAUDE.md 的指示灯约定一致）：
/// - `session_meta` / `task_started` / `user_message` → 🟢 Running
/// - `task_complete` 无 error → 🟢 绿闪 → 🟡 Waiting（一轮正常结束，对话未结束）
/// - `task_complete` 带 error → 🔴 Failed
/// - `turn_aborted` → 🟡 Waiting 不闪（用户中断，等你下一条）
pub(crate) fn classify_rollout_line(line: &[u8]) -> Option<CodexRecord> {
    if line.is_empty() {
        return None;
    }
    let record: Value = serde_json::from_slice(line).ok()?;
    let outer = record.get("type").and_then(Value::as_str)?;
    let payload = record.get("payload");

    if outer == "session_meta" {
        let payload = payload?;
        return Some(CodexRecord {
            kind: Some(AgentEventKind::Running),
            name: "session_meta",
            turn_id: None,
            error_code: None,
            prompt: None,
            session: Some(CodexSessionMeta {
                cwd: string_at(payload, "cwd"),
            }),
            tokens: None,
        });
    }

    if outer != "event_msg" {
        return None;
    }
    let payload = payload?;
    let inner = payload.get("type").and_then(Value::as_str)?;

    let simple = |kind: AgentEventKind, name: &'static str| CodexRecord {
        kind: Some(kind),
        name,
        turn_id: None,
        error_code: None,
        prompt: None,
        session: None,
        tokens: None,
    };

    match inner {
        // 桌面端的提问走这里：`item_completed` 里包一个 `UserMessage` item，
        // 正文是 `item.content[].text`（多个 text 段拼起来）。
        "item_completed" => {
            let item = payload.get("item")?;
            if item.get("type").and_then(Value::as_str) != Some("UserMessage") {
                return None;
            }
            let mut record = simple(AgentEventKind::Running, "user_message");
            record.turn_id = string_at(payload, "turn_id");
            record.prompt = item_prompt(item);
            Some(record)
        }
        "task_started" => Some(simple(AgentEventKind::Running, "task_started")),
        // CLI 路径：提问直接在 event_msg 的 `message` 字段里。
        "user_message" => {
            let mut record = simple(AgentEventKind::Running, "user_message");
            record.turn_id = string_at(payload, "turn_id");
            record.prompt = string_at(payload, "message").and_then(|text| super::truncate_prompt(&text));
            Some(record)
        }
        "task_complete" => {
            let error_code = payload
                .get("error")
                .filter(|error| !error.is_null())
                .map(|error| {
                    // 正文优先取 error.message；形态变了就退化成整块 JSON，总比丢掉强。
                    error
                        .get("message")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .unwrap_or_else(|| error.to_string())
                        .chars()
                        .take(MAX_ERROR_MESSAGE_CHARS)
                        .collect::<String>()
                });
            let kind = if error_code.is_some() {
                AgentEventKind::Failed
            } else {
                AgentEventKind::TurnFinished
            };
            let mut record = simple(kind, "task_complete");
            record.turn_id = string_at(payload, "turn_id");
            record.error_code = error_code;
            Some(record)
        }
        // 用户中断（Esc / Desktop 的停止按钮）→ 黄灯不闪：本轮到此为止、等下一条指令。
        // 刻意不落红灯：红灯的语义是「接口/不可抗力错误」，这是用户主动取消。
        "turn_aborted" => {
            let mut record = simple(AgentEventKind::WaitingForUser, "turn_aborted");
            record.turn_id = string_at(payload, "turn_id");
            Some(record)
        }
        // 用量：不改灯态，只出累计值（口径换算见下）。
        "token_count" => {
            let mut record = simple(AgentEventKind::Running, "token_count");
            record.kind = None;
            record.tokens = payload
                .get("info")
                .and_then(|info| info.get("total_token_usage"))
                .map(token_totals);
            Some(record)
        }
        _ => None,
    }
}

/// 错误正文长度上限（字符）：够看清是什么错，又不至于撑大每轮广播的快照。
const MAX_ERROR_MESSAGE_CHARS: usize = 300;

/// 从 `UserMessage` item 里取提问正文。
///
/// 形态：`{"type":"UserMessage","content":[{"type":"text","text":"…"}]}` —— 把各 text 段拼起来，
/// 再走共用的裁剪（去空白 + 超长截断）。
fn item_prompt(item: &Value) -> Option<String> {
    let parts = item.get("content")?.as_array()?;
    let mut text = String::new();
    for part in parts {
        if let Some(segment) = part.get("text").and_then(Value::as_str) {
            text.push_str(segment);
        }
    }
    super::truncate_prompt(&text)
}

/// 取一个字符串字段；缺失 / 非字符串 / 空白都算没有。
fn string_at(value: &Value, name: &str) -> Option<String> {
    value
        .get(name)?
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToString::to_string)
}

/// 把 Codex 的累计用量换算成 [`TokenTotals`]（Anthropic 口径）。
///
/// ⚠️ 两个平台口径**相反**，必须换算，否则前端 `tokenTotal()` 会把缓存算两遍：
/// - OpenAI（Codex）：`input_tokens` 是 prompt **总量**，已含 cached / cache_write
///   （实测 `total_tokens == input + output`，如 23729 + 6864 = 30593）；
/// - Anthropic（Claude）：`input_tokens` **不含**缓存，两者并列相加。
///
/// 另外 `output_tokens` 已包含 `reasoning_output_tokens`（实测 6766 含 4864），不重复相加。
fn token_totals(usage: &Value) -> TokenTotals {
    let cached = u64_at(usage, "cached_input_tokens");
    let cache_write = u64_at(usage, "cache_write_input_tokens");
    TokenTotals {
        input_tokens: u64_at(usage, "input_tokens").saturating_sub(cached + cache_write),
        cache_creation_input_tokens: cache_write,
        cache_read_input_tokens: cached,
        output_tokens: u64_at(usage, "output_tokens"),
    }
}

/// 读取一个非负整数字段；缺失或类型不符按 0 处理。
fn u64_at(value: &Value, name: &str) -> u64 {
    value.get(name).and_then(Value::as_u64).unwrap_or(0)
}

/// Codex 存放会话 rollout 的根目录（`$CODEX_HOME/sessions`，默认 `~/.codex/sessions`）。
///
/// 目录结构是 `<根>/<年>/<月>/<日>/rollout-<本地时间>-<会话 id>.jsonl`。
pub(crate) fn codex_sessions_dir() -> Option<PathBuf> {
    codex_home_dir().map(|home| home.join("sessions"))
}

/// Codex 的配置目录：`$CODEX_HOME` 优先（与 Codex 自身一致），否则 `~/.codex`。
fn codex_home_dir() -> Option<PathBuf> {
    env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".codex")))
}

/// 读取该会话当前使用的 API endpoint。
///
/// 取值顺序：**配置文件优先，环境变量兜底**（与 Claude 同口径）。
/// - 配置文件：`$CODEX_HOME/config.toml`（默认 `~/.codex/config.toml`）→
///   `<cwd>/.codex/config.toml` 覆盖前者。取顶层 `model_provider` 指向的
///   `[model_providers.<name>].base_url`。
/// - 环境变量：`OPENAI_BASE_URL` / `OPENAI_API_BASE`。
///
/// 只读 `base_url` 一个 key：同目录下的 `auth.json` / api key 一律不读取、不缓存、不上报。
pub(crate) fn codex_base_url(cwd: Option<&Path>) -> Option<String> {
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

/// 把 `CODEX_HOME` 指向临时目录的测试守卫（Drop 时还原）。
///
/// `CODEX_HOME` 是**进程级**变量，多个测试并行改会互相打架 —— 守卫持有一把全局锁，
/// 因此整个测试期间（而不只是设置的那一瞬间）都不会有别的用例改它。
/// ⚠️ 必须让守卫活到测试结束（绑定到具名变量，别写 `let _ =` 立即 Drop）。
#[cfg(test)]
pub(crate) struct CodexHomeGuard {
    /// 原值，Drop 时还原。
    previous: Option<std::ffi::OsString>,
    /// 全程持有的全局锁。
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl CodexHomeGuard {
    pub(crate) fn new(dir: &Path) -> Self {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let lock = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = env::var_os("CODEX_HOME");
        env::set_var("CODEX_HOME", dir);
        Self {
            previous,
            _lock: lock,
        }
    }
}

#[cfg(test)]
impl Drop for CodexHomeGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => env::set_var("CODEX_HOME", value),
            None => env::remove_var("CODEX_HOME"),
        }
    }
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

/// 从单个 `config.toml` 里取「当前 model_provider 的 base_url」。
///
/// 文件缺失 / TOML 非法 / 结构不符一律返回 None（静默跳过）：读盘的一方是常驻 monitor
/// 或短命 hook，都不能因为一个损坏的配置出错。
fn base_url_from_config(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    base_url_from_config_text(&raw)
}

/// 从一段 Codex config TOML **文本**里取「当前 model_provider 的 base_url」。
///
/// 抽成独立函数是因为同一份 TOML 有**两种来源**：磁盘上的 `config.toml`（本模块用），
/// 以及 cc-switch 数据库里 `settings_config.config` 字段内嵌的那份（代理解析器用，
/// 见 `proxy::ccswitch`）。两处必须用同一套解析规则，否则「tooltip 显示的 endpoint」
/// 与「代理解析器反查出的上游」会各说各话。
///
/// `model_provider` 缺省时 Codex 用的是内置 `openai`，而内置 provider 不写在
/// `model_providers` 表里 → 返回 None，由调用方回落其它来源。
pub(crate) fn base_url_from_config_text(raw: &str) -> Option<String> {
    let config: CodexConfig = basic_toml::from_str(raw).ok()?;
    let provider = config.model_provider.as_deref().unwrap_or("openai");
    config.model_providers.get(provider)?.base_url.clone()
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

#[cfg(test)]
mod tests {
    use super::*;

    use crate::modules::agent_status::status::model::{AgentEventKind, SessionPhase};
    use crate::modules::agent_status::status::test_support::TempDir;
    use crate::modules::agent_status::status::StatusRegistry;

    use super::super::{AdaptContext, EventEnvelope, HookAdapter};

    /// 造一行 rollout 记录。
    fn line(value: serde_json::Value) -> Vec<u8> {
        value.to_string().into_bytes()
    }

    // ── 灯态映射 ──

    #[test]
    fn session_meta_carries_identity_and_starts_running() {
        let raw = line(serde_json::json!({
            "timestamp": "2026-09-18T00:23:51.761Z",
            "type": "session_meta",
            "payload": {"session_id": "thr-1", "id": "thr-1", "cwd": "E:\\proj", "originator": "Codex Desktop"},
        }));
        let record = classify_rollout_line(&raw).expect("session_meta 认得出");
        assert_eq!(record.kind, Some(AgentEventKind::Running));
        let session = record.session.expect("带会话身份");
        assert_eq!(session.cwd.as_deref(), Some("E:\\proj"));
    }

    #[test]
    fn task_started_and_user_message_are_running() {
        for (inner, name) in [("task_started", "task_started"), ("user_message", "user_message")] {
            let raw = line(serde_json::json!({
                "type": "event_msg",
                "payload": {"type": inner, "turn_id": "t1", "message": "帮我看看"},
            }));
            let record = classify_rollout_line(&raw).expect("认得出");
            assert_eq!(record.kind, Some(AgentEventKind::Running), "{name}");
            assert_eq!(record.name, name);
        }
    }

    #[test]
    fn user_message_carries_the_prompt() {
        let raw = line(serde_json::json!({
            "type": "event_msg",
            "payload": {"type": "user_message", "turn_id": "t1", "message": "  跑一下测试  "},
        }));
        let record = classify_rollout_line(&raw).expect("认得出");
        assert_eq!(record.prompt.as_deref(), Some("跑一下测试"));
    }

    #[test]
    fn task_complete_without_error_finishes_the_turn() {
        let raw = line(serde_json::json!({
            "type": "event_msg",
            "payload": {"type": "task_complete", "turn_id": "t1", "last_agent_message": "done"},
        }));
        let record = classify_rollout_line(&raw).expect("认得出");
        assert_eq!(record.kind, Some(AgentEventKind::TurnFinished));
        assert_eq!(record.error_code, None);
        assert_eq!(record.turn_id.as_deref(), Some("t1"));
    }

    #[test]
    fn task_complete_with_error_fails_the_turn() {
        let raw = line(serde_json::json!({
            "type": "event_msg",
            "payload": {
                "type": "task_complete", "turn_id": "t1",
                "error": {"message": "exceeded retry limit, last status: 429 Too Many Requests",
                          "codex_error_info": {"http_status_code": 429}},
            },
        }));
        let record = classify_rollout_line(&raw).expect("认得出");
        assert_eq!(record.kind, Some(AgentEventKind::Failed));
        assert!(record
            .error_code
            .as_deref()
            .is_some_and(|code| code.contains("exceeded retry limit")));
    }

    #[test]
    fn turn_aborted_waits_for_the_user() {
        let raw = line(serde_json::json!({
            "type": "event_msg",
            "payload": {"type": "turn_aborted", "turn_id": "t1", "reason": "interrupted"},
        }));
        let record = classify_rollout_line(&raw).expect("认得出");
        // 用户中断 → 黄灯不闪（不是红灯：那是接口/不可抗力错误的语义）。
        assert_eq!(record.kind, Some(AgentEventKind::WaitingForUser));
        assert_eq!(record.turn_id.as_deref(), Some("t1"));
    }

    #[test]
    fn token_count_carries_totals_without_moving_the_light() {
        let raw = line(serde_json::json!({
            "type": "event_msg",
            "payload": {"type": "token_count", "info": {"total_token_usage": {
                "input_tokens": 3000, "cached_input_tokens": 2900,
                "cache_write_input_tokens": 20, "output_tokens": 50,
                "reasoning_output_tokens": 999, "total_tokens": 3050}}},
        }));
        let record = classify_rollout_line(&raw).expect("认得出");
        assert_eq!(record.kind, None, "用量不改灯态");
        let totals = record.tokens.expect("带用量");
        assert_eq!(totals.input_tokens, 80, "缓存要减出来");
        assert_eq!(totals.cache_read_input_tokens, 2900);
        assert_eq!(totals.cache_creation_input_tokens, 20);
        assert_eq!(totals.output_tokens, 50);
        // 四项之和 == Codex 自己的 total_tokens（input + output）
        assert_eq!(
            totals.input_tokens
                + totals.cache_creation_input_tokens
                + totals.cache_read_input_tokens
                + totals.output_tokens,
            3050
        );
    }

    #[test]
    fn unrelated_records_are_ignored() {
        // 这些是「文件在长 = 有活动」的记录，不该产出灯态事件（由调用方按活动处理）。
        for payload in [
            serde_json::json!({"type": "agent_message", "message": "hi"}),
            serde_json::json!({"type": "item_completed", "item": {"type": "CommandExecution"}}),
        ] {
            let raw = line(serde_json::json!({"type": "event_msg", "payload": payload}));
            assert!(classify_rollout_line(&raw).is_none(), "{raw:?}");
        }
        let raw = line(serde_json::json!({"type": "response_item", "payload": {"type": "function_call"}}));
        assert!(classify_rollout_line(&raw).is_none());
    }

    #[test]
    fn broken_lines_are_not_an_error() {
        assert!(classify_rollout_line(b"").is_none());
        assert!(classify_rollout_line(b"not json").is_none());
        // 半行（写到一半）也解析不出东西，但绝不能 panic。
        assert!(classify_rollout_line(br#"{"type":"event_msg","payload":{"type":"task_com"#).is_none());
    }

    // ── 预筛（省掉 97% 字节的解析）──

    #[test]
    fn prefilter_accepts_lifecycle_records() {
        for raw in [
            line(serde_json::json!({"type": "session_meta", "payload": {}})),
            line(serde_json::json!({"type": "event_msg", "payload": {"type": "task_complete"}})),
            line(serde_json::json!({"type": "event_msg", "payload": {"type": "turn_aborted"}})),
            line(serde_json::json!({"type": "event_msg", "payload": {"type": "token_count"}})),
        ] {
            assert!(cares(&raw), "{:?}", String::from_utf8_lossy(&raw));
        }
    }

    #[test]
    fn prefilter_rejects_bulky_irrelevant_records() {
        // 一条 100 KB 的命令输出记录：不该被送进 JSON 解析。
        let big = serde_json::json!({
            "type": "event_msg",
            "payload": {"type": "item_completed", "item": {"type": "CommandExecution",
                        "aggregated_output": "x".repeat(100 * 1024)}},
        });
        assert!(!cares(&big.to_string().into_bytes()));
        // 非 UTF-8 行不预筛，交给调用方当活动处理。
        assert!(!cares(&[0xff, 0xfe, 0x00]));
    }

    /// 端到端：真实的 `PermissionRequest` payload → 内核 → 快照的**黄灯 + 理由**。
    ///
    /// payload 形态取自 2026-09-18 的探针实测（用 hook 抓的原文）。
    #[test]
    fn permission_request_drives_a_yellow_light_with_reason() {
        let payload = serde_json::json!({
            "session_id": "01a0b060-f0b3-7b91-a904-b62b609bb1bd",
            "turn_id": "01a0b063-35b6-72f1-9fbc-1b1b7d2ffe4c",
            "transcript_path": null,
            "cwd": "E:/proj",
            "hook_event_name": "PermissionRequest",
            "model": "deepseek-flash",
            "permission_mode": "default",
            "tool_name": "Bash",
            "tool_input": {
                "command": "curl.exe -L https://developers.openai.com/sitemap.xml",
                "description": "是否允许我访问 OpenAI 官方网站，定位 Codex 沙盒配置说明",
            },
        });
        let adapted = CodexPermissionAdapter
            .adapt(&payload, &AdaptContext { cwd: None })
            .expect("不报错")
            .expect("接受 PermissionRequest");
        let event = adapted.into_event(EventEnvelope {
            provider: Provider::Codex,
            event_id: "hook-1".to_string(),
            received_at_ms: 100,
            pid: None,
        });

        let mut registry = StatusRegistry::new();
        registry.apply_event(event, 30_000);
        let session = &registry.snapshot(100).sessions[0];
        // 黄灯**不闪**：等人授权不是「一轮跑完」，不该走绿闪。
        assert_eq!(session.phase, SessionPhase::Waiting);
        assert!(!session.is_flashing);
        assert_eq!(
            session.waiting_reason.as_deref(),
            Some("是否允许我访问 OpenAI 官方网站，定位 Codex 沙盒配置说明")
        );
    }

    /// 理由退化：没有 `description` 时用命令原文（否则 tooltip 只有黄灯、说不清在等什么）。
    #[test]
    fn approval_reason_falls_back_to_the_command() {
        let payload = serde_json::json!({
            "session_id": "s1",
            "hook_event_name": "PermissionRequest",
            "tool_input": {"command": "Get-Content config.toml"},
        });
        let adapted = CodexPermissionAdapter
            .adapt(&payload, &AdaptContext { cwd: None })
            .expect("不报错")
            .expect("接受");
        assert!(adapted
            .waiting_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("Get-Content")));
    }

    // ── endpoint：config.toml 解析 ──

    /// 只解析该临时目录下的 `.codex/config.toml`。
    ///
    /// 刻意不走 `codex_config_paths`：那条路会读 `CODEX_HOME` / 用户真实的 `~/.codex`，
    /// 测试结果就不只取决于本用例的输入了。
    fn config_base_url(temp: &TempDir) -> Option<String> {
        let path = temp.path().join(".codex").join("config.toml");
        read_config_base_url(&[path]).and_then(|value| normalize_base_url(&value))
    }

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
        assert_eq!(
            config_base_url(&temp).as_deref(),
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
        assert_eq!(config_base_url(&temp).as_deref(), Some("http://only-this/v1"));
    }

    #[test]
    fn unknown_model_provider_yields_nothing() {
        let temp = TempDir::new("codex-config-unknown");
        // 指向未在 model_providers 里定义的内置 provider → 不产出 base_url（回落环境变量）。
        temp.write(".codex/config.toml", "model_provider = \"openai\"\n");
        assert_eq!(config_base_url(&temp), None);
    }

    #[test]
    fn broken_config_is_skipped_not_fatal() {
        let temp = TempDir::new("codex-config-broken");
        temp.write(".codex/config.toml", "this is not = = toml");
        assert_eq!(config_base_url(&temp), None);
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
        assert_eq!(read_config_base_url(&paths).as_deref(), Some("http://project"));
    }

    #[test]
    fn config_paths_prefer_project_scoped_file_last() {
        let paths = codex_config_paths(Some(Path::new("/home/u")), Some(Path::new("/proj")));
        // 用户级在前、项目级在后（后者覆盖前者）。断言顺序而不是长度：
        // 其它用例可能把 CODEX_HOME 指向临时目录（多出/少掉一份用户级配置）。
        assert!(paths.len() >= 2);
        assert!(paths.last().is_some_and(|path| path.starts_with("/proj")));
        // 没有 home / cwd 时不报错，只是少读几个文件。
        assert!(codex_config_paths(None, None)
            .iter()
            .all(|path| !path.starts_with("/home/u")));
    }

    /// 端到端：一份真实形态的 rollout 记录 → 灯态（内核只看 kind，这里断言语义映射）。
    #[test]
    fn rollout_records_map_to_expected_phases() {
        let cases: [(serde_json::Value, SessionPhase, bool); 5] = [
            (
                serde_json::json!({"type": "event_msg", "payload": {"type": "task_started"}}),
                SessionPhase::Running,
                false,
            ),
            (
                serde_json::json!({"type": "event_msg", "payload": {"type": "task_complete", "turn_id": "t"}}),
                SessionPhase::Waiting,
                true, // TurnFinished → 绿闪落黄
            ),
            (
                serde_json::json!({"type": "event_msg", "payload": {"type": "turn_aborted", "turn_id": "t"}}),
                SessionPhase::Waiting,
                false,
            ),
            (
                serde_json::json!({"type": "event_msg", "payload": {"type": "task_complete", "turn_id": "t",
                                  "error": {"message": "429"}}}),
                SessionPhase::Error,
                false,
            ),
            (
                serde_json::json!({"type": "session_meta", "payload": {"id": "thr"}}),
                SessionPhase::Running,
                false,
            ),
        ];
        for (value, phase, flashing) in cases {
            let record = classify_rollout_line(&line(value.clone())).expect("认得出");
            // 内核的 kind → 灯态策略表（与 kernel::reduce_state 同一口径）。
            let (got_phase, got_flash) = match record.kind.expect("有灯态") {
                AgentEventKind::Running => (SessionPhase::Running, false),
                AgentEventKind::TurnFinished => (SessionPhase::Waiting, true),
                AgentEventKind::WaitingForUser => (SessionPhase::Waiting, false),
                AgentEventKind::SessionFinished => (SessionPhase::Completed, false),
                // Codex 侧暂无子代理事件；这里仅补齐穷尽匹配，真到了内核会走
                // 后台运行状态机，同样投影为绿常亮。
                AgentEventKind::SubagentStarted | AgentEventKind::SubagentStopped => {
                    (SessionPhase::Running, false)
                }
                AgentEventKind::Failed => (SessionPhase::Error, false),
                AgentEventKind::Unknown => (SessionPhase::Unknown, false),
            };
            assert_eq!(got_phase, phase, "{value:?}");
            assert_eq!(got_flash, flashing, "{value:?}");
        }
    }
}
