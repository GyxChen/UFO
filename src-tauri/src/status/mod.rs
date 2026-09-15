mod hook;
mod model;
mod reducer;
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

use tauri::{AppHandle, Emitter, Runtime, State};

pub use hook::try_run_status_hook;
// 供 proxy 子模块复用主目录探测（定位 ~/.cc-switch）。
pub(crate) use hook::home_dir;
pub use model::{now_ms, AgentEvent, Provider, SessionSnapshot, StatusSnapshot};
pub use reducer::StatusRegistry;

use model::session_key;
use tokens::TokenTracker;

use crate::proxy::ProxyState;

#[derive(Clone)]
pub struct StatusState {
    pub registry: Arc<Mutex<StatusRegistry>>,
    pub status_dir: PathBuf,
    pub retention_ms: Arc<Mutex<u64>>,
    /// Running 无新 hook 时的超时（毫秒），与 Error 的 60s 分离，可配置。
    pub running_timeout_ms: Arc<Mutex<u64>>,
    /// 代理解析器状态：把会话的本地代理地址反查成上游 endpoint。
    pub proxy: ProxyState,
}

impl StatusState {
    pub fn new(
        status_dir: PathBuf,
        retention_ms: u64,
        running_timeout_ms: u64,
        proxy: ProxyState,
    ) -> Self {
        Self {
            registry: Arc::new(Mutex::new(StatusRegistry::new())),
            status_dir,
            retention_ms: Arc::new(Mutex::new(retention_ms)),
            running_timeout_ms: Arc::new(Mutex::new(running_timeout_ms)),
            proxy,
        }
    }

    /// 为快照里每个会话填充 `upstream_url`（按当前选中的解析器反查）。
    ///
    /// 生成快照的热路径上调用；解析器内部有 TTL 缓存，未启用解析时直接返回、零开销。
    /// 只填 `base_url` 存在、且解析命中上游、且上游与原值不同的会话。
    fn attach_upstreams(&self, snapshot: &mut StatusSnapshot) {
        if self.proxy.selected() == crate::proxy::NONE_RESOLVER {
            return;
        }
        for session in &mut snapshot.sessions {
            if let Some(base) = session.base_url.as_deref() {
                session.upstream_url = self.proxy.resolve_upstream(session.provider, base);
            }
        }
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

/// 供主窗口「代理解析器」下拉列出的选项（含 `none`）。
///
/// 选项来自注册表，接入新代理工具后前端无需改动即会自动出现。
#[tauri::command]
pub fn get_proxy_resolvers(state: State<'_, StatusState>) -> Vec<crate::proxy::ProxyResolverInfo> {
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
    // 上一轮快照的「上游签名」：会话 key → 解析出的 upstream_url。
    // 用于在 cc-switch 里手动切了 provider（无新 hook 事件）时，也能把新上游推到前端。
    let mut last_upstream_sig: Vec<(String, Option<String>)> = Vec::new();

    loop {
        let mut changed = consume_inbox(&app, &inbox, &quarantine, &state, &mut tokens);

        // 增量读取各会话的 transcript；只有数值真的变了才推快照。
        for (key, totals) in tokens.refresh_all() {
            if let Ok(mut registry) = state.registry.lock() {
                changed |= registry.set_tokens(&key, totals);
            }
        }

        let now = now_ms();
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
                // 会话被移除后同步丢弃其 token 记录，避免统计器无限增长。
                if let Ok(registry) = state.registry.lock() {
                    let live: HashSet<String> = registry.live_keys();
                    tokens.retain(|key| live.contains(key));
                }
            }
            next_cleanup = now.saturating_add(10_000);
        }

        // 启用了解析器时，额外监测上游是否变化（TTL 到期后会重新读 cc-switch）。
        // 未启用解析时 upstream_signature 恒空，不产生任何额外开销。
        if state.proxy.selected() != crate::proxy::NONE_RESOLVER {
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

fn consume_inbox<R: Runtime>(
    app: &AppHandle<R>,
    inbox: &Path,
    quarantine: &Path,
    state: &StatusState,
    tokens: &mut TokenTracker,
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
                let _ = app.emit("status-hook-event", &event);
                // 只登记路径：真正的解析在 monitor 线程里增量做，
                // hook 进程每次事件都要快速返回，不能去读 MB 级的 transcript。
                if let Some(transcript) = event.transcript_path.as_deref() {
                    tokens.observe(
                        &session_key(event.provider, &event.session_id),
                        Path::new(transcript),
                    );
                }
                let _ = fs::remove_file(&path);
                let retention = state
                    .retention_ms
                    .lock()
                    .map(|value| *value)
                    .unwrap_or(30_000);
                changed |= state
                    .registry
                    .lock()
                    .map(|mut registry| registry.apply_event(event, retention))
                    .unwrap_or(false);
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
