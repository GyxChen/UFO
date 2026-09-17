//! Hook 外壳：只做与平台无关的收发包，平台知识全在 `super::compat`。
//!
//! 本文件对应 `ufo --status-hook` 的短命进程路径：读 stdin 的 hook payload →
//! 交给按 `--provider` 选出的兼容适配器归一化成内核事件 → 原子写进状态目录 inbox。
//! 它不解析平台字段、不读平台配置，也必须快进快出。

use std::{
    env,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

use super::compat;
use super::model::{now_ms, Provider};

/// 单次 hook payload 的读取上限，防止异常输入拖慢短命进程。
///
/// 放到 4 MiB 是因为 **Codex 的工具事件会把 `tool_input` 原样带上**：`apply_patch` 的补丁、
/// 长命令、MCP 调用的参数都能轻松超过 64 KiB，卡在 64 KiB 会把有效生命周期事件整个丢掉
/// （表现为灯卡住不动）。读的是管道、只解析一次，这点量对短命进程无压力；
/// 真正落盘的 inbox 事件只带提取后的字段，仍然是 KB 级。
const MAX_HOOK_INPUT: u64 = 4 * 1024 * 1024;
/// App 标识（与 `tauri.conf.json` 的 `identifier` 保持一致）：既用于定位 AppData 下的
/// status 目录，也被 flow 模块用作凭据 service 前缀 —— 统一从这里导出，避免两处各写一份。
pub(crate) const APP_IDENTIFIER: &str = "com.tauri-app.my";

pub fn try_run_status_hook(args: &[String]) -> Option<i32> {
    if !args.iter().any(|arg| arg == "--status-hook") {
        return None;
    }

    // Debug: log that hook was invoked
    let debug_path = default_status_dir().join("hook-debug.log");
    let _ = fs::create_dir_all(debug_path.parent().unwrap_or(std::path::Path::new(".")));
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

    process_payload(provider, &status_dir, &payload)
}

/// 把一次 hook 的 payload 走完「兼容层归一化 → 补信封 → 落盘」整条链路。
///
/// 从 `run_status_hook` 拆出来单独成函数：stdin 喂不进单测，但这条核心链路必须能在
/// 没有真实 hook 进程的情况下验证（见下方 `process_payload_writes_canonical_event`）。
fn process_payload(provider: Provider, status_dir: &Path, payload: &Value) -> Result<(), String> {
    // ── 兼容层：按 provider 取适配器，把平台 payload 归一化成内核事件 ──
    let adapter = compat::adapter_for(provider)
        .ok_or_else(|| format!("no hook adapter for {provider:?}"))?;
    // 项目级配置以会话的 cwd 为基准；payload 未带 cwd 时适配器只读用户级配置。
    let cwd = compat::string_field(payload, &["cwd"]).map(PathBuf::from);
    let adapted = adapter.adapt(
        payload,
        &compat::AdaptContext { cwd: cwd.as_deref() },
    )?;

    // ── 信封字段（与平台无关）：事件去重 id、进程 id、接收时刻 ──
    // 事件名 / 字段别名是通用约定，平台特有字段一律由适配器处理。
    let event_id = compat::string_field(payload, &["event_id", "eventId", "id"])
        .filter(|value| !value.is_empty() && value.len() <= 256)
        .unwrap_or_else(unique_event_id);
    let pid = compat::string_field(payload, &["pid", "process_id"])
        .and_then(|value| value.parse::<u32>().ok())
        .or_else(|| env::var("UFO_AGENT_PID").ok()?.parse::<u32>().ok());
    let event = adapted.into_event(compat::EventEnvelope {
        provider,
        event_id,
        received_at_ms: now_ms(),
        pid,
    });

    // 原子写盘：先写临时文件再 rename，避免 monitor 读到半截 JSON。
    let inbox = status_dir.join("inbox");
    fs::create_dir_all(&inbox).map_err(|error| format!("cannot create status inbox: {error}"))?;
    let final_path = inbox.join(format!("{}.json", unique_event_id()));
    let temporary_path = inbox.join(format!(".{}.tmp", unique_event_id()));
    let encoded = serde_json::to_vec(&event).map_err(|error| error.to_string())?;
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

    use crate::status::model::{AgentEvent, AgentEventKind};
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

    /// hook 边缘链路：一份 Claude payload 落盘后，inbox 里的规范事件带正确的 kind。
    ///
    /// 这是「平台 payload → 兼容层 → 内核事件」整条缝的端到端验证（不含 stdin/args，
    /// 那两处由 `argument_value_*` 与 `try_run_status_hook_returns_none_without_flag` 覆盖）。
    #[test]
    fn process_payload_writes_canonical_event_with_kind() {
        let temp = TempDir::new("hook-shell");
        let payload = serde_json::json!({"session_id": "s1", "hook_event_name": "Stop"});
        process_payload(Provider::Claude, temp.path(), &payload).expect("hook writes event");

        let inbox = temp.path().join("inbox");
        let mut entries: Vec<_> = fs::read_dir(&inbox)
            .expect("inbox exists")
            .map(|entry| entry.expect("readable").path())
            .collect();
        assert_eq!(entries.len(), 1, "一次 hook 恰好落一个事件文件");
        let event: AgentEvent =
            serde_json::from_slice(&fs::read(&entries.pop().unwrap()).unwrap()).unwrap();

        assert_eq!(event.provider, Provider::Claude);
        assert_eq!(event.session_id, "s1");
        // 兼容层把 Claude 的 Stop 归一成 TurnFinished；内核据此绿闪落黄。
        assert_eq!(event.kind, AgentEventKind::TurnFinished);
        // 原始事件名作为诊断字段原样保留。
        assert_eq!(event.event, "Stop");
    }

    /// 同一条链路走 Codex：证明 `--provider codex` 能选到适配器（注册表接线）。
    #[test]
    fn process_payload_writes_canonical_event_for_codex() {
        let temp = TempDir::new("hook-shell-codex");
        let payload = serde_json::json!({
            "session_id": "thr_1",
            "hook_event_name": "Interrupt",
            "turn_id": "turn_1",
        });
        process_payload(Provider::Codex, temp.path(), &payload).expect("hook writes event");

        let inbox = temp.path().join("inbox");
        let path = fs::read_dir(&inbox)
            .expect("inbox exists")
            .next()
            .expect("one event")
            .expect("readable")
            .path();
        let event: AgentEvent =
            serde_json::from_slice(&fs::read(path).unwrap()).unwrap();

        assert_eq!(event.provider, Provider::Codex);
        // 用户 Esc 取消本轮 → 黄灯（不闪），见 compat::codex::classify。
        assert_eq!(event.kind, AgentEventKind::WaitingForUser);
        assert_eq!(event.event, "Interrupt");
    }
}
