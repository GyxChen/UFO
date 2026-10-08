//! Hook 外壳：只做与平台无关的收发包，平台知识全在 `super::compat`。
//!
//! 本文件对应 `ufo --status-hook` 的短命进程路径：读 stdin 的 hook payload →
//! 交给按 `--provider` 选出的兼容适配器归一化成内核事件 → 原子写进状态目录 inbox。
//! 它不解析平台字段、不读平台配置，也必须快进快出。
//!
//! # 日志：这里是全链路最大的黑洞
//!
//! 适配器返回 `Ok(None)`（「这事件我不关心」）原先**完全无痕**，Codex 的 11 个非
//! `PermissionRequest` 事件都从这里消失 —— 灯不动时没有任何地方能看出「事件到了、被丢了」。
//! 现在每次 hook 调用**恰好写一行**：要么交付凭证，要么裁决理由。
//!
//! 代价必须算清（这是 agent 的调用路径）：一次 `read_dir` + 一次 `open` + 一次
//! `write_all`，约 0.1–0.3ms；比原先的 `hook-debug.log`（每次进程 open 三回）还少。
//! 日志失败**永不**改变退出码，也永不改变落盘行为。

use std::{
    env,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{json, Value};

use super::compat;
use super::log::{LogConfig, LogEntry, LogReason, LogStage, LogLevel, LogWriter, Sink};
use super::model::{now_ms, Provider};
use crate::floating::module_config::MODULE_CONFIG_FILE;
use crate::modules::agent_status::settings::MODULE_ID;

/// 单次 hook payload 的读取上限，防止异常输入拖慢短命进程。
///
/// 放到 4 MiB 是因为 **Codex 的工具事件会把 `tool_input` 原样带上**：`apply_patch` 的补丁、
/// 长命令、MCP 调用的参数都能轻松超过 64 KiB，卡在 64 KiB 会把有效生命周期事件整个丢掉
/// （表现为灯卡住不动）。读的是管道、只解析一次，这点量对短命进程无压力；
/// 真正落盘的 inbox 事件只带提取后的字段，仍然是 KB 级。
const MAX_HOOK_INPUT: u64 = 4 * 1024 * 1024;
/// App 标识（与 `tauri.conf.json` 的 `identifier` 保持一致）：既用于定位 AppData 下的
/// status 目录，也被 flow 模块用作凭据 service 前缀 —— 统一从这里导出，避免两处各写一份。
pub(crate) const APP_IDENTIFIER: &str = "com.ufo.app";
/// 改名前的旧 App 标识：只被 `crate::migrate` 的一次性搬迁用到（搬完不再有任何读取点）。
pub(crate) const LEGACY_APP_IDENTIFIER: &str = "com.tauri-app.my";

pub fn try_run_status_hook(args: &[String]) -> Option<i32> {
    if !args.iter().any(|arg| arg == "--status-hook") {
        return None;
    }
    Some(run_status_hook(args))
}

/// 跑一次 hook，返回进程退出码（0 = 事件已交付或按规则静默忽略）。
fn run_status_hook(args: &[String]) -> i32 {
    // ⚠️ 目录**必须先解析**：日志要与 inbox 同目录，用户用 `--status-dir` 挪走状态目录时，
    // 那些信号只能在那儿找。原先 `hook-debug.log` 硬写默认目录、inbox 跟着参数走，
    // 两边分家导致「日志说有事件、目录里却没有」这种排查黑洞。
    let status_dir = resolve_status_dir(args);
    let config = hook_log_config();
    // ⚠️ 开关必须在这儿兑现：关着就换一个什么都不做的写入器，连 `create_dir_all` 都不发生。
    // 「关掉日志」要省下的是 hook 这一次磁盘往返本身，只丢内容而留着 open/write 等于没关。
    let sink = if config.hook_writes() {
        Sink::for_status_dir(&status_dir, config.max_file_bytes)
    } else {
        Sink::disabled()
    };
    match hook_attempt(args, &status_dir, &sink, &config) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("UFO status hook failed: {error}");
            1
        }
    }
}

/// 参数与 stdin → 归一化 → 落盘，全程的失败短路都在这里（并由 [`hook_log_config`] 决定记哪一行）。
fn hook_attempt(
    args: &[String],
    status_dir: &Path,
    sink: &Sink,
    config: &LogConfig,
) -> Result<(), String> {
    // 报错文案刻意不列举已接入平台（`claude|codex|…`）：那份清单每接一个平台就会失真一次，
    // 而把用户实际写的值原样回显出来，对排障更管用（拼错 vs 没登记，一眼分得开）。
    let requested = argument_value(args, "--provider").or_else(|| argument_value(args, "--status-hook"));
    let Some(provider) = requested.as_deref().and_then(Provider::parse) else {
        sink.report(
            entry(LogStage::HookAttempt, LogReason::UnknownProvider)
                .with_detail(json!({ "requested_provider": requested })),
        );
        return Err(format!(
            "unknown --provider {:?}",
            requested.unwrap_or_else(|| "<missing>".to_string())
        ));
    };

    let mut input = Vec::new();
    let mut stdin = io::stdin().take(MAX_HOOK_INPUT + 1);
    if let Err(error) = stdin.read_to_end(&mut input) {
        sink.report(
            entry(LogStage::HookAttempt, LogReason::StdinUnreadable)
                .with_provider(provider)
                .with_detail(json!({ "error": error.to_string() })),
        );
        return Err(format!("cannot read stdin: {error}"));
    }
    if input.len() as u64 > MAX_HOOK_INPUT {
        sink.report(
            entry(LogStage::HookAttempt, LogReason::InputTooLarge)
                .with_provider(provider)
                .with_detail(json!({ "bytes": input.len(), "limit": MAX_HOOK_INPUT })),
        );
        return Err("hook input is too large".to_string());
    }
    let payload: Value = match serde_json::from_slice(&input) {
        Ok(payload) => payload,
        Err(error) => {
            // 只带字节数与错误文本：payload 本身可能有几 MB，也不该整段进日志。
            sink.report(
                entry(LogStage::HookAttempt, LogReason::PayloadNotJson)
                    .with_provider(provider)
                    .with_detail(json!({ "bytes": input.len(), "error": error.to_string() })),
            );
            return Err(format!("invalid hook JSON: {error}"));
        }
    };

    process_payload(provider, status_dir, &payload, sink, config)
}

/// 把一次 hook 的 payload 走完「兼容层归一化 → 补信封 → 落盘」整条链路。
///
/// 从 `run_status_hook` 拆出来单独成函数：stdin 喂不进单测，但这条核心链路必须能在
/// 没有真实 hook 进程的情况下验证（见 `process_payload_writes_canonical_event`）。
fn process_payload(
    provider: Provider,
    status_dir: &Path,
    payload: &Value,
    sink: &Sink,
    config: &LogConfig,
) -> Result<(), String> {
    // 贯穿 id：**只在这里生成一次**，同时用作 inbox 文件名、事件里的 `trace_id` 与日志行的关联键。
    // ⚠️ 刻意不用 payload 里的 id（见 `model::AgentEvent::trace_id` 的说明：那是外部可控串）。
    let trace_id = unique_event_id();

    // ── 兼容层：按 provider 取适配器，把平台 payload 归一化成内核事件 ──
    let Some(adapter) = compat::adapter_for(provider) else {
        sink.report(
            entry(LogStage::CompatVerdict, LogReason::NoHookAdapter)
                .with_trace(&trace_id)
                .with_provider(provider),
        );
        return Err(format!("no hook adapter for {provider:?}"));
    };
    // 项目级配置以会话的 cwd 为基准；payload 未带 cwd 时适配器只读用户级配置。
    let cwd = compat::string_field(payload, &["cwd"]).map(PathBuf::from);
    let adapted = match adapter.adapt(payload, &compat::AdaptContext { cwd: cwd.as_deref() }) {
        Ok(Some(adapted)) => adapted,
        // ★ 静默忽略：适配器表明「这条事件我不关心」（如 Codex 只要 PermissionRequest）。
        //   不落盘、不算失败，但**必须留一行** —— 否则「事件到底来没来过」永远问不出答案。
        Ok(None) => {
            let base = entry(LogStage::CompatVerdict, LogReason::AdapterIgnoredEvent)
                .with_trace(&trace_id);
            let entry = match peek_session_id(payload) {
                Some(session_id) => base.with_session(provider, &session_id),
                // 适配器在取 session id 之前就返回了 None：能记的只有 provider + 原始事件名。
                None => base.with_provider(provider),
            };
            sink.report(entry.with_detail(json!({ "event": peek_event_name(payload) })));
            return Ok(());
        }
        Err(error) => {
            // 报错文本是我们自己写的（缺哪个字段、超长多少），对排障正是最有用的那部分。
            sink.report(
                entry(LogStage::CompatVerdict, LogReason::AdapterRejected)
                    .with_trace(&trace_id)
                    .with_provider(provider)
                    .with_detail(json!({ "event": peek_event_name(payload), "error": error })),
            );
            return Err(error);
        }
    };

    // ── 信封字段（与平台无关）：事件去重 id、贯穿 id、进程 id、接收时刻 ──
    // 事件名 / 字段别名是通用约定，平台特有字段一律由适配器处理。
    let event_id = compat::string_field(payload, &["event_id", "eventId", "id"])
        .filter(|value| !value.is_empty() && value.len() <= 256)
        .unwrap_or_else(|| trace_id.clone());
    let pid = compat::string_field(payload, &["pid", "process_id"])
        .and_then(|value| value.parse::<u32>().ok())
        .or_else(|| env::var("UFO_AGENT_PID").ok()?.parse::<u32>().ok());
    let verbose = config.covers(LogLevel::Verbose);
    let session_id = adapted.session_id.clone();
    let kind = adapted.kind;
    let raw_event = adapted.event.clone();
    let agent_id = adapted.agent_id.clone();
    let sequence = adapted.sequence;
    let prompt = adapted.prompt.clone();
    let waiting_reason = adapted.waiting_reason.clone();
    let event = adapted.into_event(compat::EventEnvelope {
        provider,
        event_id,
        trace_id: trace_id.clone(),
        received_at_ms: now_ms(),
        pid,
    });

    // ── 原子写盘：先写临时文件再 rename，避免 monitor 读到半截 JSON ──
    let inbox = status_dir.join("inbox");
    if let Err(error) = fs::create_dir_all(&inbox) {
        sink.report(
            entry(LogStage::InboxWrite, LogReason::InboxDirUnwritable)
                .with_trace(&trace_id)
                .with_session(provider, &session_id)
                .with_kind(kind)
                .with_detail(json!({ "error": error.to_string() })),
        );
        return Err(format!("cannot create status inbox: {error}"));
    }
    let final_path = inbox.join(format!("{trace_id}.json"));
    let temporary_path = inbox.join(format!(".{trace_id}.tmp"));
    let encoded = match serde_json::to_vec(&event) {
        Ok(encoded) => encoded,
        Err(error) => {
            sink.report(
                entry(LogStage::InboxWrite, LogReason::TempWriteFailed)
                    .with_trace(&trace_id)
                    .with_session(provider, &session_id)
                    .with_kind(kind)
                    .with_detail(json!({ "stage": "encode", "error": error.to_string() })),
            );
            return Err(error.to_string());
        }
    };
    let written = File::create(&temporary_path)
        .and_then(|mut file| {
            file.write_all(&encoded)?;
            // 这里**要** sync：monitor 下一次读目录就可能看到这个文件，rename 之前的
            // 半截内容会让它把一条有效事件判成解析失败（并永久移进隔离目录）。
            file.sync_all()
        })
        .and_then(|_| fs::rename(&temporary_path, &final_path));
    if let Err(error) = written {
        let reason = if temporary_path.exists() {
            // 临时文件还在 → 写这一步坏了；已经没了 → rename 失败（等于事件没送到）。
            LogReason::TempWriteFailed
        } else {
            LogReason::PublishRenameFailed
        };
        sink.report(
            entry(LogStage::InboxWrite, reason)
                .with_trace(&trace_id)
                .with_session(provider, &session_id)
                .with_kind(kind)
                .with_detail(json!({ "error": error.to_string() })),
        );
        // 原两条错误文案分开，用户/日志看到的仍是各自那一句。
        return Err(if reason == LogReason::TempWriteFailed {
            format!("cannot write status event: {error}")
        } else {
            format!("cannot publish status event: {error}")
        });
    }

    // ── 交付凭证：一行说明「这条信号确实变成了 inbox 里的一个文件」 ──
    sink.report(
        entry(LogStage::InboxWrite, LogReason::HookCompleted)
            .with_trace(&trace_id)
            .with_session(provider, &session_id)
            .with_kind(kind)
            .with_detail(json!({
                "event": raw_event,
                "agent_id": agent_id,
                "sequence": sequence,
                "file": final_path.file_name().map(|name| name.to_string_lossy().to_string()),
                // ⚠️ 用户文本只在 verbose 档出现，默认档只有长度（见 log/mod.rs 的红线二）。
                "prompt": verbose.then(|| prompt.as_deref()).flatten().map(excerpt_or_len),
                "prompt_len": (!verbose).then(|| prompt.as_ref().map(|p| p.chars().count())),
                "waiting_reason": verbose.then(|| waiting_reason.as_deref()).flatten().map(excerpt_or_len),
            })),
    );
    Ok(())
}

/// hook 侧的条目起手式（写者恒为 Hook，阶段与理由必填）。
fn entry(stage: LogStage, reason: LogReason) -> LogEntry {
    LogEntry::decided(LogWriter::Hook, stage, reason)
}

/// 用户文本进日志的唯一姿势：截断，且**永远不带超长原文**。
fn excerpt_or_len(text: &str) -> Value {
    Value::String(super::log::excerpt(text))
}

/// **只为日志** peek 原始事件名。
///
/// ⚠️ 这与各适配器那份归一表**无关、也不许共用**：那张表决定灯色，平台之间刻意各写一份
/// （共用会让改一个平台静默带偏另一个的灯，见 `reference/doc/guide/agent-status-platform-onboarding-guide.md`）。
/// 这里取不到只是日志少一个字段，不参与任何裁决。
fn peek_event_name(payload: &Value) -> Option<String> {
    compat::string_field(payload, &["hook_event_name", "event", "event_name", "event_type", "type"])
}

/// 同上，peek 会话 id：适配器可能在建出事件之前就放弃，日志里能带上就带。
fn peek_session_id(payload: &Value) -> Option<String> {
    compat::string_field(payload, &["session_id", "sessionId", "conversation_id"])
        .filter(|value| !value.is_empty() && value.len() <= 256)
}

/// `--status-dir=` / `UFO_STATUS_DIR` / 默认目录，三档优先级。
fn resolve_status_dir(args: &[String]) -> PathBuf {
    argument_value(args, "--status-dir")
        .map(PathBuf::from)
        .or_else(|| env::var_os("UFO_STATUS_DIR").map(PathBuf::from))
        .unwrap_or_else(default_status_dir)
}

/// hook 侧的日志闸门：读组件配置文件里 `module_settings['agent-status'].log`。
///
/// 为什么要读：`hook_enabled` 关掉就得**真的**不写，否则这个开关是骗人的。
/// 复用 [`LogConfig::from_block`] 而不是自己解析一遍，两处口径因此不可能分家。
///
/// ⚠️ 两个刻意的选择：
/// - 配置永远在 AppData 根下（全局唯一），**不跟** `--status-dir`；日志目录才跟。
/// - 读不到 / 解析不了时**放行**（fail-open）：关掉日志应当是用户的明确决定，
///   而不是「配置文件恰好被占用」的后果 —— 这套东西的存在意义正是抓偶发反常。
fn hook_log_config() -> LogConfig {
    let path = default_status_dir()
        .parent()
        .map(|root| root.join(MODULE_CONFIG_FILE));
    let Some(path) = path else {
        return LogConfig::default();
    };
    let Ok(bytes) = fs::read(&path) else {
        return LogConfig::default();
    };
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return LogConfig::default();
    };
    LogConfig::from_block(
        value
            .get("module_settings")
            .and_then(|settings| settings.get(MODULE_ID)),
    )
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

    base.unwrap_or_else(|| std::path::Path::new(".").to_path_buf())
        .join(APP_IDENTIFIER)
        .join("status")
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::modules::agent_status::status::log::LogEntry;
    use crate::modules::agent_status::status::model::{AgentEvent, AgentEventKind};
    use crate::modules::agent_status::status::test_support::TempDir;

    fn s(value: &str) -> String {
        value.to_string()
    }

    /// 走一遍真实链路：临时目录当状态目录，日志与 inbox 自然同源。
    fn run(provider: Provider, temp: &TempDir, payload: &Value) -> Result<(), String> {
        let config = LogConfig::default();
        let sink = Sink::for_status_dir(temp.path(), config.max_file_bytes);
        process_payload(provider, temp.path(), payload, &sink, &config)
    }

    /// 把这次 hook 写出的日志行读回来（应当**恰好一行**）。
    fn lines(temp: &TempDir) -> Vec<LogEntry> {
        let dir = super::super::log::log_dir(temp.path());
        let Ok(entries) = fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let Ok(raw) = fs::read_to_string(entry.path()) else {
                continue;
            };
            out.extend(raw.lines().filter_map(|line| serde_json::from_str::<LogEntry>(line).ok()));
        }
        out
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
    fn status_dir_prefers_the_argument() {
        // 日志目录必须跟着它走：否则「事件在 A 目录、日志在 B 目录」这种排查黑洞会重演。
        let args = vec![s("--status-dir=/tmp/elsewhere"), s("--provider=claude")];
        assert_eq!(resolve_status_dir(&args), PathBuf::from("/tmp/elsewhere"));
    }

    #[test]
    fn unique_event_ids_are_unique() {
        let id1 = unique_event_id();
        let id2 = unique_event_id();
        assert_ne!(id1, id2);
    }

    /// hook 边缘链路：一份 Claude payload 落盘后，inbox 里的规范事件带正确的 kind，
    /// 并且**留下一行交付凭证**（文件名 == `trace_id` == 日志里的关联键）。
    #[test]
    fn process_payload_writes_canonical_event_with_kind() {
        let temp = TempDir::new("hook-shell");
        let payload = serde_json::json!({"session_id": "s1", "hook_event_name": "Stop"});
        run(Provider::Claude, &temp, &payload).expect("hook writes event");

        let inbox = temp.path().join("inbox");
        let mut entries: Vec<_> = fs::read_dir(&inbox)
            .expect("inbox exists")
            .map(|entry| entry.expect("readable").path())
            .collect();
        assert_eq!(entries.len(), 1, "一次 hook 恰好落一个事件文件");
        let path = entries.pop().unwrap();
        let event: AgentEvent =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();

        assert_eq!(event.provider, Provider::Claude);
        assert_eq!(event.session_id, "s1");
        // 兼容层把 Claude 的 Stop 归一成 TurnFinished；内核据此绿闪落黄。
        assert_eq!(event.kind, AgentEventKind::TurnFinished);
        // 原始事件名作为诊断字段原样保留。
        assert_eq!(event.event, "Stop");
        // 文件名就是贯穿 id：三方（文件名 / 事件 / 日志行）必须能对上。
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        assert_eq!(event.trace_id.as_deref(), Some(stem.as_str()));

        let logged = lines(&temp);
        assert_eq!(logged.len(), 1, "一次 hook 只留一行");
        assert_eq!(logged[0].stage, LogStage::InboxWrite);
        assert_eq!(logged[0].reason, Some(LogReason::HookCompleted));
        assert_eq!(logged[0].trace_id.as_deref(), Some(stem.as_str()));
        assert_eq!(logged[0].key.as_deref(), Some("claude:s1"));
        assert_eq!(logged[0].kind.as_deref(), Some("turn_finished"));
    }

    /// ⚠️ **本轮硬验收**：关掉转灯日志**不许动到采集链路**。
    ///
    /// 提问原文 / `base_url` / 事件本身照常落 inbox —— 「会话记录」与「记录详情」里那些字段是
    /// 加日志之前就有的独立功能（见 `reference/doc/rule/agent-status-session-data.md` §3）。日志开关只管新那本 transition
    /// log，一旦它能连带关掉存档，用户看到的就是一次静默的功能回归。
    /// 同时「关 = 零落盘」也要真：连日志目录都不许建出来。
    #[test]
    fn a_disabled_log_still_publishes_the_event_and_touches_nothing_else() {
        let temp = TempDir::new("hook-shell-log-off");
        let payload = serde_json::json!({
            "session_id": "s9",
            "hook_event_name": "UserPromptSubmit",
            "prompt": "把 rollout 巡检的去抖基线接上 note_light",
        });
        // 默认档 = 关（设置页那个 switch 没打开过）。
        let config = LogConfig::default();
        assert!(!config.hook_writes(), "默认必须是关，否则这条测试没有意义");

        let sink = Sink::disabled();
        process_payload(Provider::Claude, temp.path(), &payload, &sink, &config)
            .expect("关掉日志不影响 hook 落盘");

        let inbox = temp.path().join("inbox");
        let files: Vec<_> = fs::read_dir(&inbox)
            .expect("inbox 照常建出来")
            .map(|entry| entry.expect("readable").path())
            .collect();
        assert_eq!(files.len(), 1, "该落的事件一个不少");
        let event: AgentEvent = serde_json::from_slice(&fs::read(&files[0]).unwrap()).unwrap();
        assert_eq!(event.kind, AgentEventKind::Running);
        assert_eq!(
            event.prompt.as_deref(),
            Some("把 rollout 巡检的去抖基线接上 note_light"),
            "提问必须原样进事件 —— 它是「记录详情 · 最近提问」的数据源"
        );
        assert_eq!(event.session_id, "s9");

        assert!(lines(&temp).is_empty(), "关了一行都不写");
        assert!(
            !super::super::log::log_dir(temp.path()).exists(),
            "关了就连日志目录都不该建出来"
        );
    }

    /// Codex 的 hook 口子很窄：**只有 `PermissionRequest` 落盘，其余留一行「被忽略」**。
    ///
    /// 其余事件由 rollout（`status::codex_rollout`）负责；这里多落一个盘，就多一处
    /// 与 rollout 抢同一盏灯的地方。见 `compat::codex` 的模块文档。
    #[test]
    fn codex_hook_writes_only_permission_request() {
        let temp = TempDir::new("hook-shell-codex");

        // 其余事件：不落盘、不报错（退出码仍是 0），但**必须留下一行被忽略的裁决**。
        let other = serde_json::json!({"session_id": "thr_1", "hook_event_name": "PreToolUse"});
        run(Provider::Codex, &temp, &other).expect("静默跳过不算失败");
        assert!(fs::read_dir(temp.path().join("inbox")).is_err(), "不该写出事件");
        let ignored = lines(&temp);
        assert_eq!(ignored.len(), 1);
        assert_eq!(ignored[0].stage, LogStage::CompatVerdict);
        assert_eq!(ignored[0].reason, Some(LogReason::AdapterIgnoredEvent));
        assert_eq!(ignored[0].key.as_deref(), Some("codex:thr_1"));
        assert_eq!(
            ignored[0].detail.as_ref().and_then(|d| d.get("event")).and_then(Value::as_str),
            Some("PreToolUse"),
            "被丢掉的是哪个事件必须写在行里"
        );

        // PermissionRequest：落盘一条「等人授权」。
        let request = serde_json::json!({
            "session_id": "thr_1",
            "hook_event_name": "PermissionRequest",
            "tool_name": "Bash",
            "tool_input": {"command": "curl https://example.com", "description": "是否允许我访问官网"},
        });
        run(Provider::Codex, &temp, &request).expect("落盘");

        let inbox = temp.path().join("inbox");
        let path = fs::read_dir(&inbox)
            .expect("inbox exists")
            .next()
            .expect("one event")
            .expect("readable")
            .path();
        let event: AgentEvent = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(event.provider, Provider::Codex);
        assert_eq!(event.kind, AgentEventKind::WaitingForUser);
        assert_eq!(event.waiting_reason.as_deref(), Some("是否允许我访问官网"));
    }

    #[test]
    fn missing_session_id_is_a_rejected_verdict_not_a_silent_drop() {
        let temp = TempDir::new("hook-shell-no-session");
        let payload = serde_json::json!({"hook_event_name": "Stop"});
        let error = run(Provider::Claude, &temp, &payload).expect_err("缺 session id 要报错");
        assert!(error.contains("session"), "文案要点名缺什么：{error}");
        let logged = lines(&temp);
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].reason, Some(LogReason::AdapterRejected));
        assert_eq!(logged[0].stage, LogStage::CompatVerdict);
        assert!(logged[0].trace_id.is_some(), "被拒的信号也要能顺着 id 找回上下文");
    }

    #[test]
    fn default_level_never_records_the_prompt_text() {
        // 敏感度底线：默认档只记长度。提问原文只有 verbose 档才有（见 log/mod.rs 红线二）。
        // 这条断言查的是**整个文件**，不是某个字段 —— 防的是以后加埋点时顺手把原文带出来。
        let temp = TempDir::new("hook-shell-privacy");
        let payload = serde_json::json!({
            "session_id": "s9",
            "hook_event_name": "UserPromptSubmit",
            "prompt": "这是包含密钥的提问 sk-SECRET-VALUE",
        });
        run(Provider::Claude, &temp, &payload).expect("落盘");
        let logged = lines(&temp);
        let detail = logged[0].detail.clone().unwrap_or(Value::Null);
        assert_eq!(detail.get("prompt").and_then(Value::as_str), None, "默认档不该出现提问正文");
        assert_eq!(detail.get("prompt_len").and_then(Value::as_u64), Some(25));
        let raw: String = read_log_dir(&temp);
        assert!(!raw.contains("sk-SECRET-VALUE"), "日志文件里不得出现提问原文");
    }

    #[test]
    fn verbose_level_records_a_truncated_excerpt() {
        let temp = TempDir::new("hook-shell-verbose");
        let mut config = LogConfig::default();
        config.level = LogLevel::Verbose;
        let sink = Sink::for_status_dir(temp.path(), config.max_file_bytes);
        let payload = serde_json::json!({
            "session_id": "s9",
            "hook_event_name": "UserPromptSubmit",
            "prompt": "abcdefg",
        });
        process_payload(Provider::Claude, temp.path(), &payload, &sink, &config).expect("落盘");
        let logged = lines(&temp);
        let detail = logged[0].detail.clone().unwrap_or(Value::Null);
        assert_eq!(detail.get("prompt").and_then(Value::as_str), Some("abcdefg"));
        // 超长部分必须被截断，不能整段落盘（否则一次粘贴几 MB 的提问就把日志撑爆）。
        let long = "x".repeat(4_000);
        let payload = serde_json::json!({"session_id": "s9", "hook_event_name": "UserPromptSubmit", "prompt": long});
        process_payload(Provider::Claude, temp.path(), &payload, &sink, &config).expect("落盘");
        let logged = lines(&temp);
        let detail = logged[logged.len() - 1].detail.clone().unwrap_or(Value::Null);
        let recorded = detail.get("prompt").and_then(Value::as_str).unwrap_or_default();
        assert!(recorded.chars().count() < 300, "摘录必须被夹住：{recorded}");
    }

    #[test]
    fn a_payload_id_cannot_escape_the_inbox_directory() {
        // 回归防线：`event_id` 是 payload 可控串（旧写法只校验非空与长度），
        // 所以它**只能当数据**存进事件里，文件名一律用我们自己生成的 `trace_id`。
        let temp = TempDir::new("hook-shell-path");
        let payload = serde_json::json!({
            "session_id": "s1",
            "event_id": "../../escape.json",
            "hook_event_name": "Stop",
        });
        run(Provider::Claude, &temp, &payload).expect("落盘");
        let inbox = temp.path().join("inbox");
        let names: Vec<_> = fs::read_dir(&inbox)
            .expect("inbox exists")
            .map(|entry| entry.expect("readable").file_name())
            .collect();
        assert_eq!(names.len(), 1);
        assert!(
            !names.iter().any(|name| name.to_string_lossy().contains("escape")),
            "payload 给的 id 不得出现在文件名里：{names:?}"
        );
        let event: AgentEvent =
            serde_json::from_slice(&fs::read(inbox.join(&names[0])).expect("read event")).unwrap();
        assert_eq!(event.event_id, "../../escape.json", "id 原样保留，但只作为数据");
    }

    /// 把日志目录里所有文件的正文拼起来（几条断言都要查「全文有没有出现某个串」）。
    fn read_log_dir(temp: &TempDir) -> String {
        let mut raw = String::new();
        let Ok(entries) = fs::read_dir(super::super::log::log_dir(temp.path())) else {
            return raw;
        };
        for entry in entries.flatten() {
            if let Ok(text) = fs::read_to_string(entry.path()) {
                raw.push_str(&text);
            }
        }
        raw
    }
}
