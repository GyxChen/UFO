mod hook;
mod model;
mod reducer;

use std::{
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

    loop {
        let mut changed = consume_inbox(&app, &inbox, &quarantine, &state);
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
