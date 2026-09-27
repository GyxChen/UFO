mod codex_rollout;
mod compat;
mod hook;
mod kernel;
mod lines;
mod log;
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
    now_ms, session_key, AgentEvent, AgentEventKind, Provider, SessionSnapshot, StatusSnapshot,
    StoredSeed, TokenTotals,
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

use codex_rollout::{BaselineEffect, CodexRolloutWatcher};
use tokens::TokenTracker;

// 转灯日志：`mod log` 私有，外部（`settings.rs` 转发配置、组合根注册命令）都通过这里点名。
// 全部收在 `pub(crate)` —— 本 crate 不对外导出 API，放开成 `pub` 只会让私有类型撞上
// 「public interface 里出现 crate-private 类型」的限制。
pub(crate) use log::{
    clear_status_log, export_status_log, get_status_log, set_status_log_view, CleanupReport,
    EventVerdict, ExportedLog, LogConfig, LogEntry, LogReason, LogStage, LogState, LogWriter,
    LogLevel, StatusLogEntry, StatusLogPage, StatusLogQuery, wire_name, STATUS_LOG_EVENT,
};
use serde_json::json;
// 内核裁决化的两个返回值类型（`EventVerdict` / `CleanupReport`）由 kernel 直接 use，不必再导出。

use crate::modules::agent_status::endpoint_profiles::{
    EndpointProfileState, ENDPOINT_PROFILES_EVENT,
};
use crate::modules::agent_status::flow::FlowState;
use crate::modules::agent_status::push::PushState;
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
    /// 消息推送运行态：灯态真的变了才记信号，monitor 每轮调它做投递调度。
    pub push: PushState,
    /// 转灯日志运行态：全链路裁决的落盘与读取。**只写不读灯态**，埋点绝不回灌内核。
    pub log: LogState,
}

impl StatusState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        status_dir: PathBuf,
        retention_ms: u64,
        running_timeout_ms: u64,
        proxy: ProxyState,
        profiles: EndpointProfileState,
        store: SessionStoreState,
        flow: FlowState,
        push: PushState,
        log: LogState,
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
            push,
            log,
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
    // 手动摘灯也进日志：否则回看时会话「无缘由消失」，分不清是用户点的还是超时清的。
    state.log.report_at(
        LogLevel::Minimal,
        LogEntry::decided(
            LogWriter::Ui,
            LogStage::SessionReaped,
            LogReason::ManuallyDismissed,
        )
        .with_key(key.trim()),
    );

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
        // 巡检账先落日志（每会话一行聚合）：几类丢弃里只有「与上轮基线相同」和
        // 「分类不出记录」值得单独成行，其余留在这一行的计数里。
        for scan in &updates.scans {
            let mut entry = LogEntry::new(LogWriter::Monitor, LogStage::RolloutScan)
                .with_key(&scan.key)
                .with_detail(json!({
                    "lines": scan.lines,
                    "prefiltered": scan.prefiltered,
                    "unclassified": scan.unclassified,
                    "merged_in_batch": scan.merged,
                    "baseline_dropped": scan.baseline_dropped,
                    "emitted": scan.emitted,
                    "fallback": scan.fallback,
                }));
            // 「文件在长就推绿」与「30 秒保活」是两条**没有生命周期记录**的绿，
            // 用户看到的「Codex 明明在跑却没写记录」全靠这两行解释。
            entry = match scan.fallback {
                Some("keepalive") => entry.with_reason(LogReason::KeepaliveRunning),
                Some("activity") => entry.with_reason(LogReason::ActivityImpliesRunning),
                _ => entry,
            };
            state.log.report_at(LogLevel::Normal, entry);
            if scan.baseline_dropped > 0 {
                state.log.report(
                    LogEntry::decided(
                        LogWriter::Monitor,
                        LogStage::RolloutDropped,
                        LogReason::SameAsLastRoundBaseline,
                    )
                    .with_key(&scan.key)
                    .with_detail(json!({ "dropped": scan.baseline_dropped })),
                );
            }
            if scan.unclassified > 0 {
                state.log.report(
                    LogEntry::decided(
                        LogWriter::Monitor,
                        LogStage::RolloutDropped,
                        LogReason::UnclassifiedLine,
                    )
                    .with_key(&scan.key)
                    .with_detail(json!({ "lines": scan.unclassified })),
                );
            }
        }
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

        // 消息推送：把本轮攒下的灯态信号换成投递任务（同样只入队，发在 worker 线程）。
        // 复用同一份带上游信息的快照；总开关关着 / 没有存活会话时它自己会早退，零出网。
        state.push.tick(&snapshot, &state.flow, now);

        if now >= next_cleanup {
            let running_timeout = state
                .running_timeout_ms
                .lock()
                .map(|value| *value)
                .unwrap_or(60_000)
                .min(i64::MAX as u64) as i64;
            let report = state
                .registry
                .lock()
                .map(|mut registry| registry.cleanup_report(now, running_timeout, process_alive))
                .unwrap_or_default();
            changed |= report.changed;
            // 摘灯的理由与绿闪落定逐条落日志（见 `report_cleanup`）。
            report_cleanup(&state.log, &report);
            if report.changed {
                // 会话被移除后同步丢弃其 token 记录与 rollout 巡检状态，避免无限增长。
                if let Ok(registry) = state.registry.lock() {
                    let live: HashSet<String> = registry.live_keys();
                    tokens.retain(|key| live.contains(key));
                    codex_rollout.retain(|key| live.contains(key));
                }
            }
            // 日志按天 / 按量回收（与清理同频；`sweep` 内部还有一道 5 秒限流）。
            let removed = state.log.sweep();
            if removed > 0 {
                state.log.report(
                    LogEntry::decided(
                        LogWriter::Monitor,
                        LogStage::LogMaintenance,
                        LogReason::LogRotated,
                    )
                    .with_detail(json!({ "removed_files": removed })),
                );
            }
            next_cleanup = now.saturating_add(10_000);
        }

        // 启用了解析器时，额外监测上游是否变化（TTL 到期后会重新读 cc-switch）。
        // 未启用解析时 upstream_signature 恒空，不产生任何额外开销。
        if state.proxy.selected() != NONE_RESOLVER {
            let sig = state.upstream_signature();
            if sig != last_upstream_sig {
                // 反查结论变了才留一行（解析器有 5 秒 TTL，逐轮写就是噪声）：
                // tooltip 上显示的到底是本地代理地址还是真上游，取决于这一步。
                // ⚠️ 只记「有没有解析出上游」，**不记地址本身** —— endpoint 的 query 段
                // 可能带凭据，而日志是要给人整屏翻、甚至导出发出去的。
                for (key, upstream) in sig.iter().filter(|(key, _)| {
                    !last_upstream_sig
                        .iter()
                        .any(|(previous, _)| previous == key)
                }) {
                    state.log.report(
                        LogEntry::decided(
                            LogWriter::Monitor,
                            LogStage::UpstreamResolved,
                            if upstream.is_some() {
                                LogReason::ResolverHit
                            } else {
                                LogReason::ResolverMiss
                            },
                        )
                        .with_key(key)
                        .with_detail(json!({ "resolver": state.proxy.selected() })),
                    );
                }
                last_upstream_sig = sig;
                changed = true;
            }
        }

        if changed {
            emit_snapshot(&app, &state);
        }
        // 转灯日志：把本轮新写入的行读进内存环形缓冲（**含 hook 写的那些**，这是唯一真源）。
        // 广播要压在最后一步：本轮自己写的行也得进这一批，否则面板看到的顺序会晚一轮。
        let fresh = state.log.drain();
        if !fresh.is_empty() && state.log.viewer_attached() {
            // 每轮一批而不是逐条 emit：一轮可能落几十条，逐条会把 IPC 打成噪声。
            let _ = app.emit(STATUS_LOG_EVENT, state.log.page_of(&fresh));
        }
        thread::sleep(Duration::from_millis(500));
    }
}

/// 把一轮清理的账落成日志：谁被摘了灯、为什么；谁的绿闪落定了。
///
/// 单独抽出来是因为这两种变化都是「用户没动任何东西、灯却变了」的场景，
/// 属于最需要留下因果链的那一类。档位按 `Minimal`：最低档也要看得见。
fn report_cleanup(log: &LogState, report: &CleanupReport) {
    for (key, reason, phase) in &report.reaped {
        log.report_at(
            LogLevel::Minimal,
            LogEntry::decided(LogWriter::Monitor, LogStage::SessionReaped, *reason)
                .with_key(key)
                .with_phases(Some(*phase), None),
        );
    }
    for key in &report.flash_settled {
        // 绿闪落定的前后**阶段相同**（都是 Waiting），变的是 `is_flashing` ——
        // 也就是颜色从「闪绿」收成「常黄」，这正是用户会注意到的一刻。
        log.report_at(
            LogLevel::Minimal,
            LogEntry::decided(
                LogWriter::Monitor,
                LogStage::LightTransition,
                LogReason::FlashSettled,
            )
            .with_key(key)
            .with_phases(Some(SessionPhase::Waiting), Some(SessionPhase::Waiting)),
        );
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

impl EventSource {
    /// 写进日志 detail 的稳定名字（前端按它区分「hook 到的」与「巡检器补的」）。
    fn as_str(&self) -> &'static str {
        match self {
            EventSource::Hook => "hook",
            EventSource::Rollout => "rollout",
        }
    }
}

/// 一条信号的坐标：事件会被内核**吃掉**（`apply_event_verdict` 按值收走），
/// 所以要在调用之前把日志需要的字段抄下来。
struct SignalCoords {
    /// 贯穿 id（hook 生成的那个，不是 payload 可控的 `event_id`）。
    trace_id: Option<String>,
    event_id: String,
    provider: Provider,
    session_id: String,
    kind: AgentEventKind,
    sequence: Option<u64>,
}

impl SignalCoords {
    /// 从事件抄一份坐标。
    fn of(event: &AgentEvent) -> Self {
        Self {
            trace_id: event.trace_id.clone(),
            event_id: event.event_id.clone(),
            provider: event.provider,
            session_id: event.session_id.clone(),
            kind: event.kind,
            sequence: event.sequence,
        }
    }

    /// 起一条属于该信号的日志行（坐标一次填齐）。
    fn entry(&self, stage: LogStage, reason: LogReason) -> LogEntry {
        let entry = LogEntry::decided(LogWriter::Monitor, stage, reason)
            .with_session(self.provider, &self.session_id)
            .with_kind(self.kind);
        let entry = match &self.trace_id {
            Some(trace) => entry.with_trace(trace),
            // 旧版 hook 二进制写的事件没有 trace_id：只能靠 event_id 关联。
            None => entry.with_event_id(&self.event_id),
        };
        entry.with_sequence(self.sequence)
    }
}

/// 把一条内核裁决落成日志。**必须在 registry 锁外调用**（与取回填同一套规矩：
/// 埋点里做一次磁盘写，不能把整盏灯的锁按住）。
///
/// 档位：灯真的变了 = `Minimal`（最低档也要有这条），其余裁决 = `Normal`。
/// 被顺手清掉的「别人的空闲黄灯」按 `Minimal` 单独成行 —— 它是用户眼里最费解的现象。
fn report_verdict(
    log: &LogState,
    source: EventSource,
    coords: &SignalCoords,
    verdict: &EventVerdict,
) {
    let needed = if verdict.stage == LogStage::LightTransition {
        LogLevel::Minimal
    } else {
        LogLevel::Normal
    };
    log.report_at(
        needed,
        coords
            .entry(verdict.stage, verdict.reason)
            .with_phases(verdict.phase_from, verdict.phase_to)
            .with_revision(verdict.revision)
            .with_watermark(verdict.watermark)
            .with_detail(json!({
                "source": source.as_str(),
                // 事件是主线程的还是某个子代理的：空串 = 主线程，写出来才看得懂。
                "actor": verdict.actor,
                "seed_applied": verdict.seed_applied,
            })),
    );
    for (key, phase) in &verdict.idle_cleared {
        log.report_at(
            LogLevel::Minimal,
            LogEntry::decided(
                LogWriter::Monitor,
                LogStage::IdleLanternCleared,
                LogReason::ClearedByNewTurnOfSameProvider,
            )
            .with_key(key)
            .with_phases(Some(*phase), None)
            .with_revision(verdict.revision)
            .with_detail(json!({ "cleared_by": coords.session_id })),
        );
    }
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
        // 目录本身读不出来（被删、权限、正被占用）：原先这里静默 `return false`，
        // 表现就是「事件全都不来了」而没有任何解释。
        state.log.report(
            LogEntry::decided(
                LogWriter::Monitor,
                LogStage::InboxBatch,
                LogReason::InboxUnreadable,
            )
            .with_detail(json!({ "inbox": inbox.to_string_lossy() })),
        );
        return false;
    };
    // 先把一批整体读出来，按**接收顺序**排好再逐条消费 —— `read_dir` 的顺序由文件系统
    // 决定，与写入顺序无关。批内不排序会让迟到的「子代理收尾」先于更早的「开工」生效，
    // 把一个仍在后台跑的执行者从活跃集合里抹掉。
    let mut batch: Vec<(PathBuf, Option<AgentEvent>)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if extension != "json" {
            // 认不出的文件分两类，都不放过：
            // - `.tmp` 是 hook 写一半就崩了留下的残骸，以前**永久滞留**在 inbox 里没人扫；
            // - 其它扩展名可能是用户手放的东西，只报一行、不动它。
            let reason = if extension == "tmp" {
                if fs::remove_file(&path).is_ok() {
                    LogReason::StaleTempSwept
                } else {
                    LogReason::NotJsonExtension
                }
            } else {
                LogReason::NotJsonExtension
            };
            state.log.report(
                LogEntry::decided(LogWriter::Monitor, LogStage::InboxBatch, reason)
                    .with_detail(json!({ "file": file_name_of(&path) })),
            );
            continue;
        }
        let parsed = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<AgentEvent>(&bytes).ok());
        batch.push((path, parsed));
    }

    let mut changed = false;
    let mut broken = 0_usize;
    for (path, parsed) in order_inbox_batch(batch) {
        match parsed {
            Some(event) => {
                let _ = fs::remove_file(&path);
                changed |= apply_status_event(app, state, tokens, codex, EventSource::Hook, event);
            }
            None => {
                // ⚠️ 只记文件名与判定，**不记内容**：坏文件很可能是一条写了一半的提问，
                // 那正是「不能落盘」的那类文本。
                let mut reason = LogReason::ParseFailed;
                let _ = fs::create_dir_all(quarantine);
                if let Some(name) = path.file_name() {
                    if fs::rename(&path, quarantine.join(name)).is_err() {
                        // 隔离都失败 → 只能删：这条信号的内容就此**永远查不到**，必须留痕。
                        let _ = fs::remove_file(&path);
                        reason = LogReason::QuarantineRenameFailedFileLost;
                    }
                }
                broken += 1;
                state.log.report(
                    LogEntry::decided(LogWriter::Monitor, LogStage::InboxQuarantine, reason)
                        .with_detail(json!({ "file": file_name_of(&path) })),
                );
                changed = true;
            }
        }
    }
    // 事件源健康度：本轮有坏文件就降级，坏文件清干净就恢复。
    // 只有**翻转**才写行 —— 否则每 500ms 重复一行同样的话，把有用的行淹掉。
    if let Ok(mut registry) = state.registry.lock() {
        if broken > 0 {
            if registry.mark_source_degraded() {
                state.log.report(LogEntry::decided(
                    LogWriter::Monitor,
                    LogStage::SourceHealth,
                    LogReason::SourceDegraded,
                ));
            }
        } else if registry.mark_source_healthy() {
            // 这条补的是既存缺口：原先 `source_health` 只降不升，
            // 隔离过一次坏文件就让主窗永久显示「数据源异常」。
            state.log.report(LogEntry::decided(
                LogWriter::Monitor,
                LogStage::SourceHealth,
                LogReason::SourceRecovered,
            ));
        }
    }
    changed
}

/// 文件名的展示串：日志与隔离目录都只带 basename，不带完整路径。
///
/// 刻意不带路径 —— inbox 的父目录里有用户名，日志是要给人翻、甚至导出发出去的。
fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "<无文件名>".to_string())
}

/// 把一批 inbox 事件排成消费顺序：**接收时刻**升序，同一毫秒按事件 id 兜底。
///
/// ⚠️ 排序只用接收时间，**不用** `occurred_at_ms`：那是平台自报的时间，可能缺失、
/// 也可能与接收时间不是同一把尺子（跨机器 / 时钟偏移），拿它排序会把迟到的事件插到前面。
/// 收件箱文件名是 `<pid>-<纳秒>`，字典序也不代表时间序，所以靠文件名同样不行。
/// 认不出的文件（解析失败）排在最后，按路径兜底，跟着走隔离流程。
///
/// **局限**：平台事件多数不带 `sequence`，因此「同一执行者的先后」只能靠这里的接收顺序
/// （加内核按 `agent_id` 分开的序号水位兜住带序号的那部分）。没有可靠源序号时，
/// 批与批之间的乱序无法被识破 —— 这是链路上已知的边界，不假装已经 exactly-once。
fn order_inbox_batch(
    mut batch: Vec<(PathBuf, Option<AgentEvent>)>,
) -> Vec<(PathBuf, Option<AgentEvent>)> {
    batch.sort_by(|(left_path, left), (right_path, right)| {
        let left_key = left
            .as_ref()
            .map(|event| (event.received_at_ms, event.event_id.clone()));
        let right_key = right
            .as_ref()
            .map(|event| (event.received_at_ms, event.event_id.clone()));
        match (left_key, right_key) {
            (Some(left), Some(right)) => left.cmp(&right),
            // 可解析的排在不可解析的前面：坏文件不参与顺序。
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => left_path.cmp(right_path),
        }
    });
    batch
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
    // 先记下 key 与会话 id：`event` 随后会被 `apply_event_verdict` 移走。
    let key = session_key(event.provider, &event.session_id);
    // 日志要的坐标同样在这里抄一份（事件是移动的，之后拿不到）。
    let coords = SignalCoords::of(&event);
    // Codex 的 hook 事件（当前只有 `PermissionRequest`）改的是巡检器**之外**的那盏灯：
    // 把新灯态同步回去抖基线，否则巡检器认不出「黄 → 绿」这次变化 —— 用户批准后 rollout
    // 恢复写入的那条绿活动会被当成重复吃掉，灯要等 30 秒保活才回绿（见 `note_light`）。
    if source == EventSource::Hook && event.provider == Provider::Codex {
        // 回灌效果留一行：「灯卡在黄最多 30 秒」这类现象的因就在这一步做没做。
        match codex.note_light(&key, event.kind) {
            BaselineEffect::Reflowed { from, to } => {
                state.log.report(
                    coords
                        .entry(LogStage::BaselineReflow, LogReason::RefreshOnly)
                        .with_detail(json!({
                            "baseline_from": from.map(|kind| wire_name(&kind)),
                            "baseline_to": wire_name(&to),
                        })),
                );
            }
            // 巡检器还没扫到这个会话（UFO 中途启动）：那一刻没有基线可同步，
            // 不会比不做更糟 —— 但值得留一行，否则「hook 明明到了、灯却没反应」无从解释。
            BaselineEffect::Untracked => {
                state.log.report(
                    coords
                        .entry(LogStage::BaselineReflow, LogReason::SeededWithoutEvent)
                        .with_detail(json!({ "note": "巡检器尚未发现该会话" })),
                );
            }
            // 基线本来就是这个灯态：不值得占一行。
            BaselineEffect::Unchanged => {}
        }
    }
    // 只登记路径：真正的解析在 monitor 线程里增量做，
    // hook 进程每次事件都要快速返回，不能去读 MB 级的 transcript。
    // ⚠️ 只登记「与 Claude 同构」的 transcript：Codex 的会话是 rollout，由 `codex_rollout`
    // 自己发现并读，塞进 Claude 的 TokenTracker 只会白解析一遍（格式还不对）。Codex 的 hook
    // 事件也带 transcript_path（PermissionRequest 实测带），所以这里必须按 provider 分流。
    // Qoder 与 Claude 同构（同一个 `message.usage` 结构、同一套 subagents 目录），故一并登记。
    if event.provider.uses_claude_transcript() {
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
    // 事件会被 `apply_event_verdict` 移走，所以先记下它的语义（推送要用）。
    let kind = event.kind;
    let verdict = match state.registry.lock() {
        Ok(mut registry) => registry.apply_event_verdict(event, retention, seed.as_ref()),
        // 锁被弄污意味着别处已经 panic 过（release 是 panic = abort，真到这一步进程将不再存在），
        // 这里只为编译期完备：按「没变」处理，不写日志行。
        Err(_) => return false,
    };
    // ⚠️ 埋点在锁**外**：裁决已经在内存里算好，落盘不占着 registry 锁。
    report_verdict(&state.log, source, &coords, &verdict);
    // 只有**灯真的变了**才给推送记一条信号：手机与悬浮窗因此不可能给出两个结论
    // （推送层绝不自己判定灯态，见 `push` 模块文档）。
    if verdict.changed {
        state.push.note_change(&key, kind);
    }
    verdict.changed
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::agent_status::status::model::{AgentEventKind, SCHEMA_VERSION};

    fn event(id: &str, received_at_ms: i64) -> AgentEvent {
        AgentEvent {
            schema_version: SCHEMA_VERSION,
            event_id: id.to_string(),
            // 测试里的事件没有真实 hook 来源，贯穿 id 留空（日志侧另有测试覆盖）。
            trace_id: None,
            provider: Provider::Claude,
            session_id: "s1".to_string(),
            kind: AgentEventKind::Running,
            event: String::new(),
            sequence: None,
            occurred_at_ms: None,
            received_at_ms,
            pid: None,
            error_code: None,
            outcome: None,
            notification_type: None,
            base_url: None,
            prompt: None,
            transcript_path: None,
            waiting_reason: None,
            agent_id: None,
        }
    }

    /// 一批事件按**接收时刻**排序，与 `read_dir` 给出的顺序无关。
    #[test]
    fn inbox_batch_is_ordered_by_receive_time() {
        let batch = vec![
            (PathBuf::from("c.json"), Some(event("c", 300))),
            (PathBuf::from("a.json"), Some(event("a", 100))),
            (PathBuf::from("b.json"), Some(event("b", 200))),
        ];
        let ordered: Vec<String> = order_inbox_batch(batch)
            .into_iter()
            .filter_map(|(_, parsed)| parsed.map(|event| event.event_id))
            .collect();
        assert_eq!(ordered, vec!["a", "b", "c"]);
    }

    /// 同一毫秒按事件 id 兜底，保证顺序确定（不能用文件名：它是 `<pid>-<纳秒>`）。
    #[test]
    fn inbox_batch_ties_break_on_event_id() {
        let batch = vec![
            (PathBuf::from("999-9.json"), Some(event("z", 100))),
            (PathBuf::from("1-1.json"), Some(event("a", 100))),
        ];
        let ordered: Vec<String> = order_inbox_batch(batch)
            .into_iter()
            .filter_map(|(_, parsed)| parsed.map(|event| event.event_id))
            .collect();
        assert_eq!(ordered, vec!["a", "z"]);
    }

    /// 认不出的文件排在最后（它不参与顺序），跟着走隔离流程。
    #[test]
    fn unparsable_inbox_files_are_ordered_last() {
        let batch = vec![
            (PathBuf::from("broken.json"), None),
            (PathBuf::from("b.json"), Some(event("b", 200))),
        ];
        let ordered = order_inbox_batch(batch);
        assert!(ordered[0].1.is_some(), "可解析的排前面");
        assert!(ordered[1].1.is_none());
    }
}
