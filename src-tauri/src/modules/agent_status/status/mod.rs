mod codex_rollout;
mod compat;
mod hook;
mod kernel;
mod lines;
mod model;
#[cfg(test)]
pub(crate) mod test_support;
mod tokens;

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use tauri::{AppHandle, Emitter, Manager, Runtime, State};

pub use hook::try_run_status_hook;
// 供 proxy 子模块复用主目录探测（定位 ~/.cc-switch）；实现已随兼容层迁到 compat。
pub(crate) use compat::home_dir;
// 供 proxy 的 cc-switch 解析器复用：Codex 上游写在 config TOML 里（cc-switch 把它内嵌在
// providers.settings_config.config），必须与适配器用同一套解析规则。
pub(crate) use compat::codex::base_url_from_config_text;
// flow 模块用 App 标识派生凭据 service 名，与 hook 定位 status 目录同一来源。
pub(crate) use hook::APP_IDENTIFIER;
// model 子模块本身私有，但这些类型是跨模块协议（session_store 存档回填、前端快照），
// 统一从这里 re-export，避免 session_store 直连 `status::model`。
pub use model::{
    now_ms, session_key, AgentEvent, Provider, SessionSnapshot, StatusSnapshot, StoredSeed,
    TokenTotals,
};
// StatusCounts 仅 flow 测试构造 StatusSnapshot 时需要，生产路径不引用 → 只在测试构建下导出。
#[cfg(test)]
pub(crate) use model::StatusCounts;
// SessionPhase 供 flow 侧把阶段映射成脚本可见的字符串，非测试构建也需要；
// StatusSource 仅 session_store 测试构造 SessionSnapshot 时需要 → 只在测试构建下导出。
pub use kernel::StatusRegistry;
pub(crate) use model::SessionPhase;
#[cfg(test)]
pub(crate) use model::StatusSource;

use codex_rollout::CodexRolloutWatcher;
use tokens::TokenTracker;

use crate::modules::agent_status::endpoint_profiles::{
    EndpointProfileState, ENDPOINT_PROFILES_EVENT,
};
use crate::modules::agent_status::flow::FlowState;
use crate::modules::agent_status::proxy::{ProxyResolverInfo, ProxyState, NONE_RESOLVER};
use crate::modules::agent_status::session_store::{SessionStoreState, SESSION_RECORDS_EVENT};

#[derive(Clone)]
pub struct StatusState {
    pub registry: Arc<Mutex<StatusRegistry>>,
    pub status_dir: PathBuf,
    pub retention_ms: Arc<Mutex<u64>>,
    /// Running 无新 hook 时的超时（毫秒），与 Error 的 60s 分离，可配置。
    pub running_timeout_ms: Arc<Mutex<u64>>,
    /// 代理解析器状态：把会话的本地代理地址反查成上游 endpoint。
    pub proxy: ProxyState,
    /// endpoint 档案登记表：发现非本地 endpoint 时落盘，供主窗口配置 tooltip 拓展字段。
    pub profiles: EndpointProfileState,
    /// 会话存档（SQLite）：跨重启续档的读写句柄，与命令侧共享同一份。
    pub store: SessionStoreState,
    /// 信息卡拓展的流程脚本运行态：monitor 每轮调它做调度（只对存活会话涉及的 endpoint 跑脚本）。
    pub flow: FlowState,
}

impl StatusState {
    pub fn new(
        status_dir: PathBuf,
        retention_ms: u64,
        running_timeout_ms: u64,
        proxy: ProxyState,
        profiles: EndpointProfileState,
        store: SessionStoreState,
        flow: FlowState,
    ) -> Self {
        Self {
            registry: Arc::new(Mutex::new(StatusRegistry::new())),
            status_dir,
            retention_ms: Arc::new(Mutex::new(retention_ms)),
            running_timeout_ms: Arc::new(Mutex::new(running_timeout_ms)),
            proxy,
            profiles,
            store,
            flow,
        }
    }

    /// 为快照里每个会话填充 `upstream_url`（按当前选中的解析器反查）。
    ///
    /// 生成快照的热路径上调用；解析器内部有 TTL 缓存，未启用解析时直接返回、零开销。
    /// 只填 `base_url` 存在、且解析命中上游、且上游与原值不同的会话。
    fn attach_upstreams(&self, snapshot: &mut StatusSnapshot) {
        if self.proxy.selected() == NONE_RESOLVER {
            return;
        }
        for session in &mut snapshot.sessions {
            if let Some(base) = session.base_url.as_deref() {
                session.upstream_url = self.proxy.resolve_upstream(session.provider, base);
            }
        }
    }

    /// 从一份快照取出「有效 endpoint」列表：`upstream_url ?? base_url`（与前端 `displayEndpoint` 同口径）。
    ///
    /// 入参必须是 `snapshot_with_upstream()` 的结果而不是直接读 registry —— registry 里的会话
    /// `upstream_url` 恒为 None（它由解析器现算），直接读会漏掉上游、只剩本地代理地址。
    /// loopback 过滤在登记表侧做，这里只负责取原始串。
    fn endpoints_from(snapshot: &StatusSnapshot) -> Vec<String> {
        snapshot
            .sessions
            .iter()
            .filter_map(|session| {
                session
                    .upstream_url
                    .clone()
                    .or_else(|| session.base_url.clone())
            })
            .collect()
    }

    /// 取带上游信息的快照（供 command 与 emit 共用）。
    fn snapshot_with_upstream(&self) -> StatusSnapshot {
        let mut snapshot = self
            .registry
            .lock()
            .expect("status registry lock poisoned")
            .snapshot(now_ms());
        self.attach_upstreams(&mut snapshot);
        snapshot
    }

    /// 当前所有会话「key → 解析出的 upstream」的稳定签名，供 monitor 监测上游变化。
    ///
    /// 只在启用解析器时调用；解析器有 TTL 缓存，重复调用不会频繁读库。
    /// 注意：先在锁内拷出解析所需的 `(key, provider, base_url)` 随即释放 registry 锁，
    /// 再到锁外调 `resolve_upstream`（可能触发读 SQLite）—— 否则会把 `apply_event`
    /// 阻塞在最坏 200ms 的库读上。
    fn upstream_signature(&self) -> Vec<(String, Option<String>)> {
        // 锁内：只取三元组快照，不做任何 I/O。
        let inputs: Vec<(String, Provider, String)> = {
            let registry = self.registry.lock().expect("status registry lock poisoned");
            registry
                .snapshot(now_ms())
                .sessions
                .into_iter()
                .filter_map(|session| {
                    let base = session.base_url?;
                    Some((session.key, session.provider, base))
                })
                .collect()
        };
        // 锁外：逐会话反查上游（解析器自带 TTL 缓存）。
        let mut sig: Vec<_> = inputs
            .into_iter()
            .map(|(key, provider, base)| {
                let upstream = self.proxy.resolve_upstream(provider, &base);
                (key, upstream)
            })
            .collect();
        sig.sort();
        sig
    }
}

#[tauri::command]
pub fn get_status_snapshot(state: State<'_, StatusState>) -> StatusSnapshot {
    state.snapshot_with_upstream()
}

/// Manually removes one visible status light and broadcasts the new snapshot.
#[tauri::command]
pub fn dismiss_status_session(
    app: AppHandle,
    state: State<'_, StatusState>,
    key: String,
) -> Result<StatusSnapshot, String> {
    let removed = state
        .registry
        .lock()
        .map_err(|_| "status registry lock poisoned".to_string())?
        .dismiss_session(key.trim());
    if !removed {
        return Err("status session not found".to_string());
    }

    let snapshot = state.snapshot_with_upstream();
    let _ = app.emit("agent-status", &snapshot);
    Ok(snapshot)
}

/// Toggles tooltip hit testing so its close button can receive clicks while visible.
#[tauri::command]
pub fn set_tooltip_interactive(app: AppHandle, interactive: bool) -> Result<(), String> {
    let window = app
        .get_webview_window("tooltip")
        .ok_or_else(|| "tooltip window unavailable".to_string())?;
    window
        .set_ignore_cursor_events(!interactive)
        .map_err(|error| error.to_string())
}

/// 供主窗口「代理解析器」下拉列出的选项（含 `none`）。
///
/// 选项来自注册表，接入新代理工具后前端无需改动即会自动出现。
#[tauri::command]
pub fn get_proxy_resolvers(state: State<'_, StatusState>) -> Vec<ProxyResolverInfo> {
    state.proxy.options()
}

pub fn spawn_monitor<R: Runtime + 'static>(app: AppHandle<R>, state: StatusState) {
    thread::spawn(move || monitor_loop(app, state));
}

fn monitor_loop<R: Runtime + 'static>(app: AppHandle<R>, state: StatusState) {
    let inbox = state.status_dir.join("inbox");
    let quarantine = state.status_dir.join("quarantine");
    let _ = fs::create_dir_all(&inbox);
    let mut next_cleanup = now_ms();
    // token 统计器由 monitor 线程独占，无需加锁。
    let mut tokens = TokenTracker::new();
    // Codex 的事件源：读它自己写的 rollout（hook 实测不可靠），同样由 monitor 独占。
    let mut codex_rollout = CodexRolloutWatcher::new();
    // 上一轮快照的「上游签名」：会话 key → 解析出的 upstream_url。
    // 用于在 cc-switch 里手动切了 provider（无新 hook 事件）时，也能把新上游推到前端。
    let mut last_upstream_sig: Vec<(String, Option<String>)> = Vec::new();

    loop {
        let mut changed = false;

        // Codex：事件与用量都来自 rollout（hook 实测不可靠，见 compat::codex 的模块文档）。
        // 发现新会话、增量读、去重都在巡检器内部，这里只把产出喂给内核。
        //
        // ⚠️ 顺序：**rollout 在前、inbox 在后**。hook 事件代表的是更晚发生的事实
        // （弹审批框之前 rollout 已经写过 function_call 记录），放后手它才能在同一轮里压过
        // 由「文件在长」推出的绿；反过来会让刚抬起的黄灯被同一批旧字节顶回绿。
        let updates = codex_rollout.poll();
        for event in updates.events {
            changed |= apply_status_event(
                &app,
                &state,
                &mut tokens,
                &mut codex_rollout,
                EventSource::Rollout,
                event,
            );
        }
        for (key, totals) in updates.tokens {
            // 事件先应用（会话得先存在），用量才登记得上；已消失的会话会被 set_tokens 忽略。
            if let Ok(mut registry) = state.registry.lock() {
                changed |= registry.set_tokens(&key, totals);
            }
        }

        changed |= consume_inbox(
            &app,
            &inbox,
            &quarantine,
            &state,
            &mut tokens,
            &mut codex_rollout,
        );

        // 增量读取各会话的 transcript；只有数值真的变了才推快照。
        for (key, totals) in tokens.refresh_all() {
            if let Ok(mut registry) = state.registry.lock() {
                changed |= registry.set_tokens(&key, totals);
            }
        }

        // 本轮只构建一次带上游信息的快照，endpoint 采集与会话存档刷盘共用同一份数据。
        // ⚠️ snapshot_with_upstream 返回 owned 数据、返回时 registry 锁已释放，
        // 因此随后 record_flush 做 SQLite I/O 不会把 apply_event 卡在锁上。
        let now = now_ms();
        let snapshot = state.snapshot_with_upstream();

        // 采集会话的有效 endpoint（上游优先）登记进「信息卡拓展」档案表。
        // 内部只在出现未见过的 id 时才写盘，因此每轮调用足够廉价；
        // 有新增时单独广播事件，让主窗口左侧列表即时刷新（不依赖快照事件）。
        let endpoints = StatusState::endpoints_from(&snapshot);
        if state.profiles.record_all(&endpoints, now) {
            let _ = app.emit(
                ENDPOINT_PROFILES_EVENT,
                state.profiles.snapshot(),
            );
        }

        // 会话存档（SQLite）：内部只写「内容真的变了」的行，空闲时零 I/O，所以每轮都调得起。
        // 顺序要紧 —— 刷盘必须在下面的 cleanup 之前：会话被清理前必须留档一次。
        // 有新行时单独广播，让开着「会话记录」页的窗口即时长出来。
        let flush = state.store.record_flush(&snapshot.sessions);
        if flush.should_broadcast() {
            let _ = app.emit(
                SESSION_RECORDS_EVENT,
                state.store.list(),
            );
        }

        // 信息卡拓展：为「当前存活会话涉及的 endpoint」调度流程脚本（tick 只入队，跑在 worker 线程）。
        // 复用本轮已带上游信息的 snapshot；内部对同 endpoint 去抖 + 按间隔退避，空闲（无会话）时零入队。
        state.flow.tick(&snapshot, &state.profiles, now);

        if now >= next_cleanup {
            let running_timeout = state
                .running_timeout_ms
                .lock()
                .map(|value| *value)
                .unwrap_or(60_000)
                .min(i64::MAX as u64) as i64;
            let cleanup_changed = state
                .registry
                .lock()
                .map(|mut registry| registry.cleanup(now, running_timeout, process_alive))
                .unwrap_or(false);
            changed |= cleanup_changed;
            if cleanup_changed {
                // 会话被移除后同步丢弃其 token 记录与 rollout 巡检状态，避免无限增长。
                if let Ok(registry) = state.registry.lock() {
                    let live: HashSet<String> = registry.live_keys();
                    tokens.retain(|key| live.contains(key));
                    codex_rollout.retain(|key| live.contains(key));
                }
            }
            next_cleanup = now.saturating_add(10_000);
        }

        // 启用了解析器时，额外监测上游是否变化（TTL 到期后会重新读 cc-switch）。
        // 未启用解析时 upstream_signature 恒空，不产生任何额外开销。
        if state.proxy.selected() != NONE_RESOLVER {
            let sig = state.upstream_signature();
            if sig != last_upstream_sig {
                last_upstream_sig = sig;
                changed = true;
            }
        }

        if changed {
            emit_snapshot(&app, &state);
        }
        thread::sleep(Duration::from_millis(500));
    }
}

/// 一条事件的**来源**：决定它要不要把灯态回灌给 Codex 巡检器的去抖基线。
///
/// 巡检器自己产出的事件不需要回灌（它在产出前已经写好基线），因此只有 hook 那一侧要同步。
#[derive(Clone, Copy, PartialEq, Eq)]
enum EventSource {
    /// inbox 里的规范事件：各平台 hook 落的盘。
    Hook,
    /// Codex rollout 巡检器合成的事件。
    Rollout,
}

fn consume_inbox<R: Runtime>(
    app: &AppHandle<R>,
    inbox: &Path,
    quarantine: &Path,
    state: &StatusState,
    tokens: &mut TokenTracker,
    codex: &mut CodexRolloutWatcher,
) -> bool {
    let Ok(entries) = fs::read_dir(inbox) else {
        return false;
    };
    let mut changed = false;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }

        let parsed = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<AgentEvent>(&bytes).ok());
        match parsed {
            Some(event) => {
                let _ = fs::remove_file(&path);
                changed |= apply_status_event(app, state, tokens, codex, EventSource::Hook, event);
            }
            None => {
                let _ = fs::create_dir_all(quarantine);
                if let Some(name) = path.file_name() {
                    if fs::rename(&path, quarantine.join(name)).is_err() {
                        let _ = fs::remove_file(&path);
                    }
                }
                if let Ok(mut registry) = state.registry.lock() {
                    registry.mark_source_degraded();
                }
                changed = true;
            }
        }
    }
    changed
}

/// 把一条**规范事件**送进内核 —— 所有事件源的唯一入口。
///
/// 事件源目前有两个：hook（inbox 里的 JSON，兼容层已归一化）与 Codex rollout 补漏
/// （`codex_rollout.rs` 合成）。两者都产出 `AgentEvent`，因此后续处理完全共用：
/// 广播给前端调试、登记 transcript、取存档回填、进内核。
fn apply_status_event<R: Runtime>(
    app: &AppHandle<R>,
    state: &StatusState,
    tokens: &mut TokenTracker,
    codex: &mut CodexRolloutWatcher,
    source: EventSource,
    event: AgentEvent,
) -> bool {
    let _ = app.emit("status-hook-event", &event);
    // 先记下 key 与会话 id：`event` 随后会被 `apply_event_with_seed` 移走。
    let key = session_key(event.provider, &event.session_id);
    // Codex 的 hook 事件（当前只有 `PermissionRequest`）改的是巡检器**之外**的那盏灯：
    // 把新灯态同步回去抖基线，否则巡检器认不出「黄 → 绿」这次变化 —— 用户批准后 rollout
    // 恢复写入的那条绿活动会被当成重复吃掉，灯要等 30 秒保活才回绿（见 `note_light`）。
    if source == EventSource::Hook && event.provider == Provider::Codex {
        codex.note_light(&key, event.kind);
    }
    // 只登记路径：真正的解析在 monitor 线程里增量做，
    // hook 进程每次事件都要快速返回，不能去读 MB 级的 transcript。
    // （Codex 的 transcript 不由这里登记 —— 它的会话由 rollout 巡检器自己发现，见 codex_rollout.rs。）
    // ⚠️ 只给 Claude 登记：Codex 的 transcript 是 rollout，由 `codex_rollout` 自己发现并读，
    // 塞进 Claude 的 TokenTracker 只会白解析一遍（格式还不对）。Codex 的 hook 事件也带
    // transcript_path（PermissionRequest 实测带），所以这里必须按 provider 分流。
    if event.provider == Provider::Claude {
        if let Some(transcript) = event.transcript_path.as_deref() {
            tokens.observe(&key, Path::new(transcript));
        }
    }
    // 回填摘要在 **registry 锁之外**取：内部只查一张内存 HashMap，不做 I/O。
    let seed = state.store.seed_for(&key);
    let retention = state
        .retention_ms
        .lock()
        .map(|value| *value)
        .unwrap_or(30_000);
    state
        .registry
        .lock()
        .map(|mut registry| registry.apply_event_with_seed(event, retention, seed.as_ref()))
        .unwrap_or(false)
}

fn emit_snapshot<R: Runtime>(app: &AppHandle<R>, state: &StatusState) {
    // 走带上游信息的快照，与 get_status_snapshot 同一口径。
    let _ = app.emit("agent-status", state.snapshot_with_upstream());
}

fn process_alive(session: &SessionSnapshot) -> Option<bool> {
    let pid = session.pid?;
    #[cfg(target_os = "windows")]
    {
        let filter = format!("PID eq {pid}");
        let output = Command::new("tasklist")
            .args(["/FI", &filter, "/FO", "CSV", "/NH"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let output = String::from_utf8_lossy(&output.stdout);
        return Some(!output.contains("INFO:") && output.contains(&pid.to_string()));
    }

    #[cfg(not(target_os = "windows"))]
    {
        let status = Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .ok()?;
        Some(status.success())
    }
}
