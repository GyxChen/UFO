mod hook;
mod model;
mod reducer;
#[cfg(test)]
mod test_support;
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
pub use model::{now_ms, AgentEvent, SessionSnapshot, StatusSnapshot};
pub use reducer::StatusRegistry;

use model::session_key;
use tokens::TokenTracker;

#[derive(Clone)]
pub struct StatusState {
    pub registry: Arc<Mutex<StatusRegistry>>,
    pub status_dir: PathBuf,
    pub retention_ms: Arc<Mutex<u64>>,
    /// Running 无新 hook 时的超时（毫秒），与 Error 的 60s 分离，可配置。
    pub running_timeout_ms: Arc<Mutex<u64>>,
}

impl StatusState {
    pub fn new(status_dir: PathBuf, retention_ms: u64, running_timeout_ms: u64) -> Self {
        Self {
            registry: Arc::new(Mutex::new(StatusRegistry::new())),
            status_dir,
            retention_ms: Arc::new(Mutex::new(retention_ms)),
            running_timeout_ms: Arc::new(Mutex::new(running_timeout_ms)),
        }
    }
}

#[tauri::command]
pub fn get_status_snapshot(state: State<'_, StatusState>) -> StatusSnapshot {
    state
        .registry
        .lock()
        .expect("status registry lock poisoned")
        .snapshot(now_ms())
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
    let snapshot = state
        .registry
        .lock()
        .map(|registry| registry.snapshot(now_ms()));
    if let Ok(snapshot) = snapshot {
        let _ = app.emit("agent-status", snapshot);
    }
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
