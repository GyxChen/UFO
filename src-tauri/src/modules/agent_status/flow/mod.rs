//! 信息卡拓展「流程脚本」的运行侧：状态、调度、worker 线程、供前端调用的命令。
//!
//! ## 数据流
//! 主窗口为某 endpoint 存一份 [`config::FlowConfig`]（脚本 + 间隔 + 密钥名清单）→ 落
//! [`super::endpoint_profiles`] 的 extensions → status monitor 每轮调 [`FlowState::tick`]，对
//! **当前存活会话涉及的**、且启用且到点的 endpoint 入队一个 [`FlowJob`] → 专用 worker 线程串行跑
//! QuickJS（[`runner::run_flow`]）→ 结果写回 [`FlowState`] 并按**会话 key** 广播 `flow-results` →
//! tooltip 只做 `map[session.key]` 查表渲染。
//!
//! ## 为什么只在有存活会话时跑
//! 拓展字段只在悬停某会话灯时看得到；agent 全停时再轮询就是纯出网骚扰。口径是
//! 「还在 `StatusSnapshot.sessions` 里」，不看 phase —— 黄/绿/闪烁灯都可能被悬停，都需要数据。
//!
//! ## 锁与 I/O 的次序（沿用 status monitor 的惯例）
//! tick 与命令都只在锁内做纯内存查表，QuickJS 执行、emit 一律在锁外。worker 不回头查 registry /
//! profiles 锁（会话摘要在入队时就拷进 job），避免把最坏 15s 的脚本阻塞在 monitor 热路径上。
//!
//! ## panic-free（release 是 panic=abort，worker 一次 panic 带走整个 App）
//! 见 `runner.rs` / `api.rs` 顶部约定；本文件不 unwrap、不索引，失败一律变 `Err(String)` 或跳过。

mod api;
mod config;
mod runner;

use std::{
    collections::{HashMap, HashSet},
    sync::{
        mpsc::{self, Sender},
        Arc, Mutex,
    },
};

use serde::Serialize;
use serde_json::Value as JsonValue;
use tauri::{AppHandle, Emitter, Runtime, State};

use crate::modules::agent_status::endpoint_profiles::{
    EndpointProfileList, EndpointProfileState, ENDPOINT_PROFILES_EVENT,
};
use crate::modules::agent_status::proxy::{endpoint_identity, is_loopback_endpoint};
use crate::modules::agent_status::status::{now_ms, SessionSnapshot, StatusSnapshot};

pub use api::{HttpFetch, HttpRequest, KeyringSecrets, SecretSource, UreqFetch};
// HttpResponse 只被推送 / 流程的测试假件用来构造响应，生产路径不引用 → 只在测试构建下导出。
#[cfg(test)]
pub use api::HttpResponse;
pub use config::{validate_secret_name, FlowConfig};
pub use runner::{FlowField as RunnerField, RunOutcome as RunnerOutcome, SessionContext};

/// 后端广播拓展字段结果的事件名（payload 为 [`FlowSnapshot`]）。
pub const FLOW_RESULTS_EVENT: &str = "flow-results";

/// 失败退避封顶（秒）：脚本连续报错时下次运行的最小间隔，防止挂了还每秒打网。
const MAX_BACKOFF_SEC: u64 = 600;

/// 前端消费的单个拓展字段（已清洗，`color` 为 `green|yellow|red|muted` 或 null）。
#[derive(Debug, Clone, Serialize)]
pub struct FlowField {
    /// 展示标签。
    pub label: String,
    /// 展示值。
    pub value: String,
    /// 语义色；非法值已清洗为 None。
    pub color: Option<String>,
}

impl FlowField {
    fn from_runner(field: RunnerField) -> Self {
        Self {
            label: field.label,
            value: field.value,
            color: field.color,
        }
    }
}

/// 一个 endpoint 最近一次运行的完整产物（供主窗口读、tooltip 只取 fields）。
#[derive(Debug, Clone, Serialize)]
pub struct FlowOutcome {
    /// 清洗后的字段（成功时有值；失败时为空）。
    pub fields: Vec<FlowField>,
    /// 上次失败的简短原因；成功时 None。
    pub error: Option<String>,
    /// 本次结果的生成时刻（毫秒）。
    pub updated_at_ms: i64,
    /// 上次运行是否成功（决定 UI 是否显红、以及退避）。
    pub ok: bool,
}

/// 一次运行的产物中「与新鲜度有关」的部分：字段 + 生成时刻 + 下次刷新的间隔。
///
/// 后两项只为 tooltip 标注「更新于 HH:MM:SS · 每 Ns 刷新」，让用户能判断屏上的余额/用量
/// 是刚跑的还是几十分钟前的旧值（脚本挂了时 tooltip 直接不显字段，更需要这个锚点）。
#[derive(Debug, Clone, Serialize)]
pub struct FlowSessionResult {
    /// 清洗后的拓展字段。
    pub fields: Vec<FlowField>,
    /// 本批字段的生成时刻（毫秒）。
    pub updated_at_ms: i64,
    /// 该 endpoint 配置的轮询间隔（秒）；退避只会把它拉长，显示值取配置原值。
    pub interval_sec: u64,
}

/// 广播给前端的快照：按**会话 key** 索引该会话所配 endpoint 的字段与新鲜度。
///
/// 用会话 key 而非 endpoint id 做键，是为了让 tooltip 零归一逻辑（它手里只有会话 key）；
/// endpoint 归一口径（`proxy::endpoint_identity`）只在后端算一次。
#[derive(Debug, Clone, Serialize)]
pub struct FlowSnapshot {
    /// 每次内容变化递增，供前端丢弃乱序事件。
    pub revision: u64,
    /// 会话 key → 该会话 endpoint 的拓展字段结果（无字段/未启用的会话不进表）。
    pub sessions: HashMap<String, FlowSessionResult>,
}

/// 一次入队的运行任务：所有依赖都在 tick 时拷好，worker 不再回查任何锁。
struct FlowJob {
    /// endpoint 去重键（`host:port`）。
    endpoint_id: String,
    /// endpoint 原始展示地址。
    endpoint_url: String,
    /// 脚本源码。
    script: String,
    /// 单次超时（毫秒，已 clamp）。
    timeout_ms: u64,
    /// 该 endpoint 配置的轮询间隔（秒）：透传给结果，供 tooltip 标注「每 Ns 刷新」。
    interval_sec: u64,
    /// 脚本指纹：worker 成功回来后按它写 last_result，tick 据此判断脚本是否被改过。
    script_hash: u64,
    /// 上次成功结果（`ctx.lastResult`），已在 tick 侧按 hash 过滤。
    last_result: Option<JsonValue>,
    /// 一个代表会话的只读上下文（同 endpoint 多会话时任取其一）。
    session: Option<SessionContext>,
}

/// endpoint 维度的运行态（结果、退避计数、刷新间隔）。
#[derive(Debug, Clone)]
struct EndpointState {
    /// 最近一次运行产物（None = 从没跑过）。
    outcome: Option<FlowOutcome>,
    /// 连续失败次数：算退避用（`interval * 2^n` 封顶 [`MAX_BACKOFF_SEC`]）。
    fail_streak: u32,
    /// 该 endpoint 配置的轮询间隔（秒）：tick 到点时按当轮配置写入，供广播带出。
    /// （没跑过的 endpoint 不会有结果，因此这个值只在有 outcome 时被读到。）
    interval_sec: u64,
}

/// FlowState 的内部可变状态。
struct FlowInner {
    /// endpoint id → 运行态。
    endpoints: HashMap<String, EndpointState>,
    /// 正在跑 / 已入队的 endpoint（去抖，避免同 endpoint 并发或排队重复）。
    running: HashSet<String>,
    /// endpoint id → (脚本指纹, 上次成功结果)，供 `ctx.lastResult`。
    last_result: HashMap<String, (u64, JsonValue)>,
    /// 进程内 KV 缓存（跨运行共享，按 endpoint 分组）。
    cache: Arc<api::CacheStore>,
    /// 广播序号。
    revision: u64,
    /// 会话 key → 当前映射到的 endpoint id（供结果回填时算 sessions 映射）。
    session_endpoints: HashMap<String, String>,
}

/// 跨窗口共享 + worker 线程持有的流程状态。
#[derive(Clone)]
pub struct FlowState {
    inner: Arc<Mutex<FlowInner>>,
    tx: Sender<FlowJob>,
    secrets: Arc<dyn SecretSource>,
    http: Arc<dyn HttpFetch>,
}

impl FlowState {
    /// 建状态并返回（尚未 spawn worker；调用方拿到 AppHandle 后 [`spawn_worker`]）。
    pub fn new(http: Arc<dyn HttpFetch>, secrets: Arc<dyn SecretSource>) -> (Self, ReceiverHolder) {
        let (tx, rx) = mpsc::channel::<FlowJob>();
        let state = Self {
            inner: Arc::new(Mutex::new(FlowInner {
                endpoints: HashMap::new(),
                running: HashSet::new(),
                last_result: HashMap::new(),
                cache: Arc::new(api::CacheStore::new()),
                revision: 0,
                session_endpoints: HashMap::new(),
            })),
            tx,
            secrets,
            http,
        };
        (state, ReceiverHolder(rx))
    }

    /// 读某 endpoint 的运行产物（命令 `get_flow_results` 用）。
    pub fn outcome(&self, endpoint_id: &str) -> Option<FlowOutcome> {
        self.inner
            .lock()
            .ok()?
            .endpoints
            .get(endpoint_id)?
            .outcome
            .clone()
    }

    /// 读某个会话当前的脚本结果（推送正文用）。
    ///
    /// 只查这一个 key —— ⚠️ 别改用 [`snapshot()`]：那会整表 clone 含脚本正文，
    /// 而调用方每轮都要跑一次（与 tick 同频），纯浪费。
    pub fn session_fields(&self, session_key: &str) -> Option<Vec<FlowField>> {
        let inner = self.inner.lock().ok()?;
        let endpoint_id = inner.session_endpoints.get(session_key)?;
        let state = inner.endpoints.get(endpoint_id)?;
        // 与 build_snapshot 同一口径：只有「成功且有字段」才算有结果。
        let outcome = state.outcome.as_ref().filter(|outcome| outcome.ok)?;
        (!outcome.fields.is_empty()).then(|| outcome.fields.clone())
    }

    /// 取共享的 KV 缓存句柄（worker 建 FlowHost 时用，跨运行复用）。
    fn cache_handle(&self) -> Arc<api::CacheStore> {
        match self.inner.lock() {
            Ok(inner) => Arc::clone(&inner.cache),
            Err(_) => Arc::new(api::CacheStore::new()),
        }
    }

    /// 当前配置下的会话→字段映射快照（不触发运行，只读现有结果）。
    pub fn snapshot(&self) -> FlowSnapshot {
        let inner = match self.inner.lock() {
            Ok(inner) => inner,
            Err(_) => {
                return FlowSnapshot {
                    revision: 0,
                    sessions: HashMap::new(),
                }
            }
        };
        Self::build_snapshot(&inner)
    }

    /// 从内部状态算广播快照（锁内调用；只读 endpoints + session_endpoints）。
    ///
    /// 字段之外还带该 endpoint 的 `updated_at_ms` 与配置的 `interval_sec`：tooltip 据此标出
    /// 「更新于 · 每 Ns 刷新」，让用户能分辨屏上是刚跑的值还是旧值。
    fn build_snapshot(inner: &FlowInner) -> FlowSnapshot {
        let mut sessions: HashMap<String, FlowSessionResult> = HashMap::new();
        for (session_key, endpoint_id) in &inner.session_endpoints {
            let Some(state) = inner.endpoints.get(endpoint_id) else {
                continue;
            };
            // 只广播成功且有字段的运行：失败/空结果不进表（tooltip 也就不占位）。
            let outcome = state.outcome.as_ref().filter(|outcome| outcome.ok);
            let Some(outcome) = outcome else { continue };
            if outcome.fields.is_empty() {
                continue;
            }
            sessions.insert(
                session_key.clone(),
                FlowSessionResult {
                    fields: outcome.fields.clone(),
                    updated_at_ms: outcome.updated_at_ms,
                    interval_sec: state.interval_sec,
                },
            );
        }
        FlowSnapshot {
            revision: inner.revision,
            sessions,
        }
    }

    /// 调度入口：由 status monitor 每轮（约 500ms）调用一次。
    ///
    /// 返回「是否需要广播 flow-results」—— 有 endpoint 从跑完/报错变成新结果时才 true，
    /// 避免每轮空发事件。QuickJS 执行与 emit 都在本函数之外（worker 线程）发生。
    pub fn tick(&self, snapshot: &StatusSnapshot, profiles: &EndpointProfileState, now: i64) -> bool {
        // 1) 存活会话 → endpoint：算出 session key、归一 endpoint id（upstream 优先、跳 loopback）。
        let mut per_endpoint: HashMap<String, (String, Vec<String>, Option<SessionContext>)> =
            HashMap::new();
        let mut seen_sessions: HashSet<String> = HashSet::new();
        for session in &snapshot.sessions {
            let endpoint_raw = session
                .upstream_url
                .as_deref()
                .or(session.base_url.as_deref())
                .unwrap_or("");
            let Some(endpoint_id) = endpoint_identity(endpoint_raw) else {
                continue;
            };
            if is_loopback_endpoint(&endpoint_id) {
                continue;
            }
            seen_sessions.insert(session.key.clone());
            let entry = per_endpoint
                .entry(endpoint_id.clone())
                .or_insert_with(|| (endpoint_raw.to_string(), Vec::new(), session_context(session)));
            entry.1.push(session.key.clone());
        }

        // 2) 记下会话→endpoint 映射（供结果回填算快照）；清掉本轮不再存活的会话。
        //    映射变化（会话增删）本身不广播字段，等 worker 回来后统一广播。
        if let Ok(mut inner) = self.inner.lock() {
            inner.session_endpoints.retain(|key, _| seen_sessions.contains(key));
            for (endpoint_id, (_, keys, _)) in &per_endpoint {
                for key in keys {
                    if inner
                        .session_endpoints
                        .get(key)
                        .map(|current| current != endpoint_id)
                        .unwrap_or(true)
                    {
                        inner.session_endpoints.insert(key.clone(), endpoint_id.clone());
                    }
                }
            }
        }

        // 3) 逐 endpoint 判断是否到点、是否已启用、是否已在跑 → 入队。
        let mut enqueued = false;
        for (endpoint_id, (endpoint_url, _keys, session)) in per_endpoint {
            // 读配置：profiles.with_profile 只 clone 该档案的 extensions，不整表 clone。
            let Some(config) = profiles
                .with_profile(&endpoint_id, |extensions| FlowConfig::load_from(extensions))
                .flatten()
            else {
                continue;
            };
            if !config.enabled || config.script.trim().is_empty() {
                continue;
            }
            // 是否到点：结合退避（失败后拉长间隔，封顶 MAX_BACKOFF_SEC）。
            let due = {
                let inner = match self.inner.lock() {
                    Ok(inner) => inner,
                    Err(_) => continue,
                };
                if inner.running.contains(&endpoint_id) {
                    false
                } else {
                    let (last_run, backoff_sec) = match inner.endpoints.get(&endpoint_id) {
                        Some(state) => match state.outcome.as_ref() {
                            Some(outcome) => (
                                Some(outcome.updated_at_ms),
                                backoff_seconds(config.interval_sec, state.fail_streak),
                            ),
                            None => (None, config.interval_sec),
                        },
                        None => (None, config.interval_sec),
                    };
                    match last_run {
                        Some(at) => now.saturating_sub(at) >= (backoff_sec as i64 * 1000),
                        None => true,
                    }
                }
            };
            if !due {
                continue;
            }

            // 取上次结果：脚本没改（hash 一致）才继承 lastResult。
            let last_result = self
                .inner
                .lock()
                .ok()
                .and_then(|inner| inner.last_result.get(&endpoint_id).cloned())
                .filter(|(hash, _)| *hash == config.script_hash)
                .map(|(_, value)| value);

            let job = FlowJob {
                endpoint_id: endpoint_id.clone(),
                endpoint_url: endpoint_url.clone(),
                script: config.script.clone(),
                timeout_ms: config.timeout_ms,
                interval_sec: config.interval_sec,
                script_hash: config.script_hash,
                last_result,
                session,
            };
            if let Ok(mut inner) = self.inner.lock() {
                inner.running.insert(endpoint_id.clone());
            }
            if self.tx.send(job).is_err() {
                // worker 已死（不该发生）：撤回去抖标记，别再狂试。
                if let Ok(mut inner) = self.inner.lock() {
                    inner.running.remove(&endpoint_id);
                }
                eprintln!("flow worker channel closed");
            } else {
                enqueued = true;
            }
        }

        // 入队本身不广播（结果还没回来）；结果由 worker 线程跑完后 emit。返回是否有入队，仅供测试观察。
        enqueued
    }

    /// worker 跑完一个 job 后回写结果 + 去抖。返回最新快照供锁外 emit。
    fn apply_result(&self, job: &FlowJob, result: Result<RunnerOutcome, String>) -> FlowSnapshot {
        let inner_arc = Arc::clone(&self.inner);
        let Ok(mut inner) = inner_arc.lock() else {
            return FlowSnapshot {
                revision: 0,
                sessions: HashMap::new(),
            };
        };
        inner.running.remove(&job.endpoint_id);

        let (outcome, fail_streak) = match result {
            Ok(outcome) if !outcome.timed_out => {
                let fields: Vec<FlowField> =
                    outcome.fields.into_iter().map(FlowField::from_runner).collect();
                // 成功：记下 lastResult（带脚本指纹），退避清零。
                if let Some(raw) = &outcome.raw_result {
                    inner
                        .last_result
                        .insert(job.endpoint_id.clone(), (job.script_hash, raw.clone()));
                }
                (
                    FlowOutcome {
                        fields,
                        error: None,
                        updated_at_ms: now_ms(),
                        ok: true,
                    },
                    0u32,
                )
            }
            Ok(outcome) => {
                let reason = if outcome.timed_out {
                    "脚本执行超时".to_string()
                } else {
                    "脚本无有效返回".to_string()
                };
                (
                    FlowOutcome {
                        fields: Vec::new(),
                        error: Some(reason),
                        updated_at_ms: now_ms(),
                        ok: false,
                    },
                    self_fail_streak(&inner, &job.endpoint_id),
                )
            }
            Err(message) => (
                FlowOutcome {
                    fields: Vec::new(),
                    error: Some(message),
                    updated_at_ms: now_ms(),
                    ok: false,
                },
                self_fail_streak(&inner, &job.endpoint_id),
            ),
        };

        let state = inner.endpoints.entry(job.endpoint_id.clone()).or_insert_with(|| {
            EndpointState {
                outcome: None,
                fail_streak: 0,
                interval_sec: job.interval_sec,
            }
        });
        state.fail_streak = fail_streak;
        state.outcome = Some(outcome);
        // 刷新间隔随每次回写更新：用户改了「间隔」但没改脚本时（不触发 invalidate），标注也要跟上。
        state.interval_sec = job.interval_sec;
        // 每次回写都推进 revision：广播快照带它，前端据此丢弃乱序。
        inner.revision = inner.revision.saturating_add(1);
        Self::build_snapshot(&inner)
    }

    /// 改脚本 / 删档案后清掉某 endpoint 的运行态与缓存（保存新配置、删除档案时调）。
    pub fn invalidate(&self, endpoint_id: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.endpoints.remove(endpoint_id);
            inner.last_result.remove(endpoint_id);
            inner.session_endpoints.retain(|_, id| id != endpoint_id);
            inner.cache.clear_scope(endpoint_id);
            inner.revision = inner.revision.saturating_add(1);
        }
    }

    /// 删除档案时的级联清理：按档案里记的密钥名逐个删凭据，再清运行态与缓存。
    ///
    /// ⚠️ 必须在档案行被删**之前**调用（名字清单存在 extensions 里，删了就无从得知）。
    /// keyring 删除失败只 warn、不阻断（否则凭据服务不可用会让档案永远删不掉）。
    pub fn cleanup_for_profile(&self, profiles: &EndpointProfileState, endpoint_id: &str) {
        let names = secret_names_for(profiles, endpoint_id);
        cleanup_secrets_for(self.secrets.as_ref(), endpoint_id, &names);
        self.invalidate(endpoint_id);
    }
}

/// 当前 endpoint 的连续失败次数（读旧值 +1 的基数；不含本次）。
fn self_fail_streak(inner: &FlowInner, endpoint_id: &str) -> u32 {
    inner
        .endpoints
        .get(endpoint_id)
        .map(|state| state.fail_streak.saturating_add(1))
        .unwrap_or(1)
}

/// 由间隔与失败次数算实际退避秒数：`interval * 2^n`，封顶 [`MAX_BACKOFF_SEC`]。
fn backoff_seconds(interval_sec: u64, fail_streak: u32) -> u64 {
    if fail_streak == 0 {
        return interval_sec;
    }
    let factor = 1u64 << fail_streak.min(5);
    interval_sec.saturating_mul(factor).min(MAX_BACKOFF_SEC)
}

/// 从会话快照取一份脚本可见的只读上下文（同 endpoint 多会话时任一即可）。
fn session_context(session: &SessionSnapshot) -> Option<SessionContext> {
    let tokens = session
        .tokens
        .map(|t| t.input_tokens + t.cache_creation_input_tokens + t.cache_read_input_tokens + t.output_tokens)
        .unwrap_or(0);
    Some(SessionContext {
        provider: session.provider.as_str().to_string(),
        phase: phase_str(session).to_string(),
        prompt: session.prompt.clone(),
        tokens_total: tokens,
    })
}

/// 会话阶段的字符串写法（与前端 `phase` 字段同一口径：serde snake_case）。
fn phase_str(session: &SessionSnapshot) -> &'static str {
    use crate::modules::agent_status::status::SessionPhase;
    match session.phase {
        SessionPhase::Running => "running",
        SessionPhase::Waiting => "waiting",
        SessionPhase::Error => "error",
        SessionPhase::Unknown => "unknown",
        SessionPhase::Completed => "completed",
    }
}

/// mpsc Receiver 的薄封装：只在 spawn_worker 里被取走一次，避免把 Receiver 塞进 Clone 的 FlowState。
pub struct ReceiverHolder(mpsc::Receiver<FlowJob>);

impl ReceiverHolder {
    /// 非阻塞取一个 job（测试用，验证 tick 是否真的入队）。
    #[cfg(test)]
    fn try_recv(&self) -> Result<FlowJob, mpsc::TryRecvError> {
        self.0.try_recv()
    }
}

/// 起专用 worker 线程串行消费队列（单 endpoint 一次 HTTP 最长 15s，串行天然限流）。
///
/// 每 job 一个全新 QuickJS Runtime/Context，不跨线程持有 JS 对象。跑完回写结果并**在锁外**广播。
pub fn spawn_worker<R: Runtime + 'static>(
    app: AppHandle<R>,
    state: FlowState,
    rx: ReceiverHolder,
) {
    let ReceiverHolder(rx) = rx;
    std::thread::spawn(move || {
        for job in rx {
            let host = runner::FlowHost {
                http: Arc::clone(&state.http),
                secrets: Arc::clone(&state.secrets),
                cache: state.cache_handle(),
                secrets_service: KeyringSecrets::service_for(&job.endpoint_id),
                endpoint_id: job.endpoint_id.clone(),
                endpoint_url: job.endpoint_url.clone(),
                session: job.session.clone(),
                last_result: job.last_result.clone(),
            };
            let result = runner::run_flow(host, &job.script, job.timeout_ms);
            let snapshot = state.apply_result(&job, result);
            // emit 在 apply_result 的锁释放之后（apply_result 返回时 guard 已 drop）。
            let _ = app.emit(FLOW_RESULTS_EVENT, &snapshot);
        }
    });
}

// —— 命令 ——

/// 读某 endpoint 的运行产物（三窗口都可调；tooltip/主窗都据它显示「上次失败」等）。
#[tauri::command]
pub fn get_flow_results(
    state: State<'_, FlowState>,
    endpoint_id: String,
) -> Option<FlowOutcome> {
    state.outcome(endpoint_id.trim())
}

/// 读当前「会话 key → 字段」映射（tooltip 初次挂载时拉一次，之后靠事件）。
#[tauri::command]
pub fn get_flow_snapshot(state: State<'_, FlowState>) -> FlowSnapshot {
    state.snapshot()
}

/// 保存某 endpoint 的流程配置（仅主窗口）。写 extensions + 广播档案 + 失效旧结果。
#[tauri::command]
pub fn save_endpoint_flow(
    app: AppHandle,
    profiles: State<'_, EndpointProfileState>,
    flow: State<'_, FlowState>,
    endpoint_id: String,
    config: FlowConfig,
) -> Result<EndpointProfileList, String> {
    let id = endpoint_id.trim().to_string();
    if id.is_empty() {
        return Err("endpoint id 为空".to_string());
    }
    // ⚠️ 客户端只负责 enabled/interval/timeout/script；secret_names 与 script_hash 由服务端权威维护：
    // 前端漏传/误传 secret_names 会把凭据记账清空，导致删档案时孤儿凭据清不掉。指纹由服务端按脚本重算。
    let mut config = config.sanitized();
    let carried_names = profiles
        .with_profile(&id, |extensions| {
            FlowConfig::load_from(extensions)
                .map(|existing| existing.secret_names)
                .unwrap_or_default()
        })
        .unwrap_or_default();
    config.secret_names = carried_names;
    config.rehash_script();
    let list = profiles.update_extensions(&id, |extensions| {
        config.write_into(extensions);
    })?;
    // 改脚本 → 旧的增量缓存与 lastResult 语义失效；按新配置重算。
    flow.invalidate(&id);
    let _ = app.emit(ENDPOINT_PROFILES_EVENT, &list);
    // 立刻广播一次（此时该 endpoint 的字段已被 invalidate 清掉）：
    // 否则 tooltip 手里缓存的仍是旧脚本的字段，要等下次轮询才刷新 —— 与「改完脚本不显示旧结果」的语义冲突。
    let _ = app.emit(FLOW_RESULTS_EVENT, flow.snapshot());
    Ok(list)
}

/// 「运行测试」同步产物：字段 + console 日志 + 错误（不进队列、不动持久 lastResult）。
#[derive(Serialize)]
pub struct FlowTestResult {
    /// 清洗后的字段。
    pub fields: Vec<FlowField>,
    /// console 缓冲（等宽回显）。
    pub logs: Vec<String>,
    /// 失败原因；成功时 None。
    pub error: Option<String>,
}

/// 跑一次脚本供预览（仅主窗口）。
///
/// 不走 worker 队列 —— 否则一个卡住的测试会让调度停摆最长 15s。日志在此回显（正式调度路径只写
/// 内存缓冲、不广播，防脚本 `console.log(secrets.get('K'))` 泄漏）。
///
/// ⚠️ 声明为 `async` 并把 QuickJS + HTTP 丢进 `spawn_blocking`：Tauri 的非 async 命令在**主线程**
/// 执行，一次测试最坏阻塞 15s 会冻结整个窗口事件循环（拖拽、其它 IPC 全排队）。先同步取出需要的
/// owned 数据（`State` 引用不能进 `'static` 闭包），再到 blocking 池里跑。
/// 带引用入参的 async 命令**必须返回 Result**（tauri 宏要求），故包一层 Ok。
#[tauri::command]
pub async fn run_flow_test(
    profiles: State<'_, EndpointProfileState>,
    flow: State<'_, FlowState>,
    endpoint_id: String,
    script: String,
    timeout_ms: Option<u64>,
) -> Result<FlowTestResult, String> {
    let id = endpoint_id.trim().to_string();
    // 展示原串在档案里；测试上下文拿不到档案时退化成 id（脚本不消费 endpoint_url，无影响）。
    let endpoint_url = profiles
        .profile(&id)
        .map(|profile| profile.url)
        .unwrap_or_else(|| id.clone());
    let http = Arc::clone(&flow.http);
    let secrets = Arc::clone(&flow.secrets);
    let timeout = timeout_ms.unwrap_or(config::DEFAULT_SCRIPT_TIMEOUT_MS);
    let script_for_run = script.clone();

    let endpoint_for_run = id.clone();
    let run = tauri::async_runtime::spawn_blocking(move || {
        let host = runner::FlowHost {
            http,
            secrets,
            cache: Arc::new(api::CacheStore::new()),
            secrets_service: KeyringSecrets::service_for(&endpoint_for_run),
            endpoint_id: endpoint_for_run,
            endpoint_url,
            session: None,
            last_result: None,
        };
        finish_test(runner::run_flow(host, &script_for_run, timeout))
    });

    // blocking 池不会 panic（run_flow 全 Err 返回），但 join 失败时兜底成错误结果，绝不 expect。
    Ok(match run.await {
        Ok(result) => result,
        Err(_) => FlowTestResult {
            fields: Vec::new(),
            logs: Vec::new(),
            error: Some("测试执行异常".to_string()),
        },
    })
}

/// 把一次沙盒运行结果整理成 UI 预览产物（拆出来便于复用与阅读）。
fn finish_test(result: Result<RunnerOutcome, String>) -> FlowTestResult {
    match result {
        Ok(outcome) => FlowTestResult {
            fields: outcome.fields.into_iter().map(FlowField::from_runner).collect(),
            logs: outcome.logs,
            error: if outcome.timed_out {
                Some("脚本执行超时".to_string())
            } else {
                None
            },
        },
        Err(message) => FlowTestResult {
            fields: Vec::new(),
            logs: Vec::new(),
            error: Some(message),
        },
    }
}

/// 写一个密钥到系统凭据管理器并记账名字（仅主窗口）。值不落盘、不进档案。
#[tauri::command]
pub fn set_flow_secret(
    app: AppHandle,
    profiles: State<'_, EndpointProfileState>,
    flow: State<'_, FlowState>,
    endpoint_id: String,
    name: String,
    value: String,
) -> Result<(), String> {
    let id = endpoint_id.trim().to_string();
    if id.is_empty() {
        return Err("endpoint id 为空".to_string());
    }
    let name = validate_secret_name(&name)?;
    let service = KeyringSecrets::service_for(&id);
    flow.secrets.set(&service, &name, &value)?;
    // 把名字记进档案 extensions（供级联删除）。值绝不进这里。
    let list = profiles.update_extensions(&id, |extensions| {
        let mut config = FlowConfig::load_from(extensions).unwrap_or_default();
        config.remember_secret(&name);
        config.write_into(extensions);
    })?;
    let _ = app.emit(ENDPOINT_PROFILES_EVENT, &list);
    Ok(())
}

/// 删一个密钥 + 去记账（仅主窗口）。
#[tauri::command]
pub fn delete_flow_secret(
    app: AppHandle,
    profiles: State<'_, EndpointProfileState>,
    flow: State<'_, FlowState>,
    endpoint_id: String,
    name: String,
) -> Result<(), String> {
    let id = endpoint_id.trim().to_string();
    let name = validate_secret_name(&name)?;
    let service = KeyringSecrets::service_for(&id);
    // keyring 删除失败（如条目已不在）不阻塞去记账；这里 Err 只在真正不可用时冒泡。
    let delete_result = flow.secrets.delete(&service, &name);
    let list = profiles.update_extensions(&id, |extensions| {
        if let Some(mut config) = FlowConfig::load_from(extensions) {
            config.forget_secret(&name);
            config.write_into(extensions);
        }
    })?;
    let _ = app.emit(ENDPOINT_PROFILES_EVENT, &list);
    delete_result
}

/// 供删除档案时级联清理凭据（按档案里记的名字逐个删）。由 endpoint_profiles 侧调用。
///
/// keyring 删除失败只 warn、不阻断删除流程（否则凭据服务不可用会让档案永远删不掉）。
pub fn cleanup_secrets_for(secrets: &dyn SecretSource, endpoint_id: &str, names: &[String]) {
    let service = KeyringSecrets::service_for(endpoint_id);
    for name in names {
        if let Err(error) = secrets.delete(&service, name) {
            eprintln!("failed to delete flow secret {name} for {endpoint_id}: {error}");
        }
    }
}

/// 从档案 extensions 里取密钥名清单（删除档案前读，用于级联清理）。
pub fn secret_names_for(profiles: &EndpointProfileState, endpoint_id: &str) -> Vec<String> {
    profiles
        .with_profile(endpoint_id, |extensions| {
            FlowConfig::load_from(extensions)
                .map(|config| config.secret_names)
                .unwrap_or_default()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::agent_status::status::test_support::TempDir;
    use crate::modules::agent_status::status::{
        Provider, SessionPhase, SessionSnapshot, StatusCounts, StatusSource,
    };

    /// 构造一条最小存活会话快照（flow 侧只读 key/provider/phase/base_url/upstream_url/prompt/tokens）。
    fn session(key_id: &str, base_url: Option<&str>) -> SessionSnapshot {
        SessionSnapshot {
            key: format!("claude:{key_id}"),
            provider: Provider::Claude,
            session_id: key_id.to_string(),
            phase: SessionPhase::Running,
            source: StatusSource::Hook,
            confidence: 100,
            started_at_ms: 1,
            last_seen_at_ms: 2,
            completed_at_ms: None,
            remove_at_ms: None,
            is_flashing: false,
            pid: None,
            error_code: None,
            base_url: base_url.map(|value| value.to_string()),
            upstream_url: None,
            prompt: None,
            waiting_reason: None,
            tokens: None,
            last_sequence: None,
            termination_confirmed: false,
        }
    }

    fn snapshot_of(sessions: Vec<SessionSnapshot>) -> StatusSnapshot {
        StatusSnapshot {
            schema_version: 1,
            revision: 1,
            generated_at_ms: 1000,
            sessions,
            counts: StatusCounts::default(),
            source_health: "hook".to_string(),
        }
    }

    /// 建一个登记了 `api.example.com:443` 并配好启用流程的档案状态。
    /// TempDir 需保活至断言结束，用 leaked 路径避免其 Drop 删掉配置文件。
    fn profiles_with_flow(tag: &str, interval_sec: u64) -> (TempDir, EndpointProfileState) {
        let dir = TempDir::new(tag);
        let profiles = EndpointProfileState::load(dir.path().join("endpoint-profiles.json"));
        profiles.record_all(&["https://api.example.com".to_string()], 1);
        let mut config = FlowConfig::default();
        config.enabled = true;
        config.interval_sec = interval_sec;
        config.script = "return [{label:'a',value:1}]".to_string();
        config.rehash_script();
        profiles
            .update_extensions("api.example.com:443", |extensions| {
                config.write_into(extensions);
            })
            .expect("配置应写入");
        (dir, profiles)
    }

    #[test]
    fn tick_enqueues_once_per_endpoint_not_per_session() {
        // 两会话同 endpoint：只入队 1 个 job，但两个会话 key 都进 session→endpoint 映射。
        let (state, rx) = FlowState::new(Arc::new(UreqFetch), Arc::new(KeyringSecrets));
        let (_dir, profiles) = profiles_with_flow("tick_dedupe", 60);
        let snapshot = snapshot_of(vec![
            session("s1", Some("https://api.example.com")),
            session("s2", Some("https://api.example.com")),
        ]);
        assert!(state.tick(&snapshot, &profiles, 10_000));
        assert!(rx.try_recv().is_ok(), "应有 1 个 job 入队");
        assert!(rx.try_recv().is_err(), "同 endpoint 不应入队第 2 个");
        let inner = state.inner.lock().unwrap();
        assert_eq!(inner.session_endpoints.len(), 2);
    }

    #[test]
    fn tick_skips_when_already_running() {
        // 第一次结果未回（running 仍置位）→ 第二次 tick 不再入队（去抖）。
        let (state, rx) = FlowState::new(Arc::new(UreqFetch), Arc::new(KeyringSecrets));
        let (_dir, profiles) = profiles_with_flow("tick_running", 60);
        let snapshot = snapshot_of(vec![session("s1", Some("https://api.example.com"))]);
        assert!(state.tick(&snapshot, &profiles, 10_000));
        assert!(rx.try_recv().is_ok());
        assert!(!state.tick(&snapshot, &profiles, 10_500));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn tick_skips_before_interval_elapses() {
        // 结果已回但间隔未到 → 不入队；跨过间隔 → 再入队。
        // ⚠️ updated_at_ms 由 apply_result 内部取真实 now_ms()，所以 tick 的 now 也必须用真实时钟，
        // 否则「10_000 这样的纪元毫秒」与真实时刻相减会得到负数，判定逻辑整个失真。
        let (state, rx) = FlowState::new(Arc::new(UreqFetch), Arc::new(KeyringSecrets));
        let (_dir, profiles) = profiles_with_flow("tick_interval", 60);
        let snapshot = snapshot_of(vec![session("s1", Some("https://api.example.com"))]);
        // 先入队一个 job 并回填一个成功结果，模拟 worker 完成。
        state.tick(&snapshot, &profiles, now_ms());
        let job = rx.try_recv().expect("首个 tick 应入队");
        state.apply_result(
            &job,
            Ok(RunnerOutcome {
                fields: Vec::new(),
                logs: Vec::new(),
                timed_out: false,
                raw_result: Some(JsonValue::Array(Vec::new())),
            }),
        );
        // 刚跑完（远不足 60s）：不再入队。
        assert!(!state.tick(&snapshot, &profiles, now_ms() + 1_000));
        assert!(rx.try_recv().is_err());
        // 超过 60s：再次入队。
        assert!(state.tick(&snapshot, &profiles, now_ms() + 61_000));
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn tick_drops_mapping_for_vanished_sessions() {
        // 会话从快照消失（灯灭）→ 映射被清，且不再为该 endpoint 入队。
        let (state, _rx) = FlowState::new(Arc::new(UreqFetch), Arc::new(KeyringSecrets));
        let (_dir, profiles) = profiles_with_flow("tick_vanish", 60);
        state.tick(&snapshot_of(vec![session("s1", Some("https://api.example.com"))]), &profiles, 1);
        assert!(state.inner.lock().unwrap().session_endpoints.contains_key("claude:s1"));
        // 换一份没有会话的快照再 tick。
        state.tick(&snapshot_of(Vec::new()), &profiles, 100_000);
        assert!(!state.inner.lock().unwrap().session_endpoints.contains_key("claude:s1"));
    }

    #[test]
    fn tick_ignores_loopback_and_missing_config() {
        let (state, rx) = FlowState::new(Arc::new(UreqFetch), Arc::new(KeyringSecrets));
        let (_dir, profiles) = profiles_with_flow("tick_skip", 60);
        // loopback（本地代理）地址：不登记也不调度。
        state.tick(
            &snapshot_of(vec![session("s1", Some("http://127.0.0.1:15721"))]),
            &profiles,
            1,
        );
        // 有 endpoint 但没配流程的档案（另一个 host）：跳过。
        state.tick(
            &snapshot_of(vec![session("s2", Some("https://other.example.com"))]),
            &profiles,
            1,
        );
        assert!(rx.try_recv().is_err(), "loopback 与无配置都不应入队");
    }

    #[test]
    fn backoff_caps_at_max() {
        assert_eq!(backoff_seconds(60, 0), 60);
        assert_eq!(backoff_seconds(60, 1), 120);
        assert_eq!(backoff_seconds(60, 2), 240);
        // 高失败次数封顶。
        assert_eq!(backoff_seconds(60, 20), MAX_BACKOFF_SEC);
        // 小间隔 × 大因子也封顶。
        assert_eq!(backoff_seconds(3600, 3), MAX_BACKOFF_SEC);
    }

    #[test]
    fn snapshot_builds_session_map_from_ok_outcomes() {
        // 手工塞一个成功结果 + 会话映射，验证按会话 key 出字段、失败/空的不进表。
        let (state, _rx) = FlowState::new(Arc::new(UreqFetch), Arc::new(KeyringSecrets));
        {
            let mut inner = state.inner.lock().unwrap();
            inner.endpoints.insert(
                "api.example.com:443".to_string(),
                EndpointState {
                    outcome: Some(FlowOutcome {
                        fields: vec![FlowField {
                            label: "余额".to_string(),
                            value: "10".to_string(),
                            color: Some("green".to_string()),
                        }],
                        error: None,
                        updated_at_ms: 1,
                        ok: true,
                    }),
                    fail_streak: 0,
                    interval_sec: 60,
                },
            );
            inner.endpoints.insert(
                "err.example.com:443".to_string(),
                EndpointState {
                    outcome: Some(FlowOutcome {
                        fields: Vec::new(),
                        error: Some("boom".to_string()),
                        updated_at_ms: 1,
                        ok: false,
                    }),
                    fail_streak: 1,
                    interval_sec: 60,
                },
            );
            inner
                .session_endpoints
                .insert("claude:s1".to_string(), "api.example.com:443".to_string());
            inner
                .session_endpoints
                .insert("claude:s2".to_string(), "err.example.com:443".to_string());
        }
        let snapshot = state.snapshot();
        assert!(snapshot.sessions.contains_key("claude:s1"));
        assert!(!snapshot.sessions.contains_key("claude:s2"), "失败结果不进表");
        // 成功项要带新鲜度信息（tooltip 用「更新于 / 每 Ns 刷新」标注）。
        let result = snapshot.sessions.get("claude:s1").expect("s1 应有结果");
        assert_eq!(result.updated_at_ms, 1);
        assert_eq!(result.interval_sec, 60);
        assert_eq!(result.fields.len(), 1);
    }

    #[test]
    fn invalidate_clears_state_and_cache() {
        let (state, _rx) = FlowState::new(Arc::new(UreqFetch), Arc::new(KeyringSecrets));
        {
            let mut inner = state.inner.lock().unwrap();
            inner.endpoints.insert(
                "api.example.com:443".to_string(),
                EndpointState {
                    outcome: None,
                    fail_streak: 3,
                    interval_sec: 60,
                },
            );
            inner.last_result.insert(
                "api.example.com:443".to_string(),
                (1, JsonValue::Null),
            );
            inner
                .session_endpoints
                .insert("claude:s1".to_string(), "api.example.com:443".to_string());
        }
        state.invalidate("api.example.com:443");
        let inner = state.inner.lock().unwrap();
        assert!(!inner.endpoints.contains_key("api.example.com:443"));
        assert!(!inner.last_result.contains_key("api.example.com:443"));
        assert!(!inner.session_endpoints.contains_key("claude:s1"));
    }

    #[test]
    fn apply_result_records_success_and_advances_revision() {
        let (state, _rx) = FlowState::new(Arc::new(UreqFetch), Arc::new(KeyringSecrets));
        let job = FlowJob {
            endpoint_id: "api.example.com:443".to_string(),
            endpoint_url: "https://api.example.com".to_string(),
            script: "return []".to_string(),
            timeout_ms: 2000,
            interval_sec: 60,
            script_hash: 7,
            last_result: None,
            session: None,
        };
        {
            let mut inner = state.inner.lock().unwrap();
            inner
                .session_endpoints
                .insert("claude:s1".to_string(), job.endpoint_id.clone());
            inner.running.insert(job.endpoint_id.clone());
        }
        let outcome = RunnerOutcome {
            fields: vec![RunnerField {
                label: "k".to_string(),
                value: "v".to_string(),
                color: None,
            }],
            logs: Vec::new(),
            timed_out: false,
            raw_result: Some(JsonValue::Array(vec![])),
        };
        let snapshot = state.apply_result(&job, Ok(outcome));
        assert_eq!(snapshot.revision, 1);
        assert!(snapshot.sessions.contains_key("claude:s1"));
        // running 去抖标记被清。
        assert!(!state.inner.lock().unwrap().running.contains(&job.endpoint_id));
        // lastResult 按指纹记下，供下次继承。
        assert!(state.inner.lock().unwrap().last_result.contains_key(&job.endpoint_id));
    }

    #[test]
    fn apply_result_failure_increments_streak_and_keeps_error() {
        let (state, _rx) = FlowState::new(Arc::new(UreqFetch), Arc::new(KeyringSecrets));
        let job = FlowJob {
            endpoint_id: "x.example.com:443".to_string(),
            endpoint_url: "https://x.example.com".to_string(),
            script: "throw 1".to_string(),
            timeout_ms: 2000,
            interval_sec: 60,
            script_hash: 1,
            last_result: None,
            session: None,
        };
        let snapshot = state.apply_result(&job, Err("boom".to_string()));
        assert!(snapshot.sessions.is_empty(), "失败不产字段");
        let inner = state.inner.lock().unwrap();
        let endpoint = inner.endpoints.get(&job.endpoint_id).unwrap();
        assert_eq!(endpoint.fail_streak, 1);
        assert!(!endpoint.outcome.as_ref().unwrap().ok);
    }
}
