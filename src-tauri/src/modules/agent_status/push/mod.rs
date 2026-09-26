//! 消息推送：把灯态变化投递到 ntfy / Bark / 将来的服务。
//!
//! ## 分工
//! - **本文件 = 调度层**：读配置、判开关、拼文案、排队、跑投递、发广播。
//! - [`providers`] 只负责「一种服务怎么发」。一个服务只有一个可配字段（服务器地址），
//!   收件方固定是「名字 + 主题」两列 —— 没有通用字段表，前端也就没有按 spec 渲染的表单。
//! - 触发信号来自内核：`status::apply_status_event` 在**灯真的变了**时调 [`PushState::note_change`]，
//!   于是手机上的结论和悬浮窗上的灯必然一致。⚠️ 推送层不得自己判定灯态。
//!
//! ## 三条不变量
//! 1. **网络只跑在专用 worker 线程**：一次投递最长 10s，绝不能拖住 500ms 的状态循环。
//! 2. **agent 全停 = 零出网**：只对快照里的存活会话产生任务（与流程脚本共享这条不变量）。
//! 3. **连续失败要退避**：`BACKOFF_BASE_SEC × 2^n` 封顶 600s。一个坏掉的主题不该每秒打一次网，
//!    所以退避不算「用户功能」，它是出网护栏，精简时也不动。
//!
//! ## ⚠️ 主题不是密钥
//! 主题串（Bark 的设备密钥也算一个主题）**明文**存在 `module_settings['agent-status'].push` 里。
//! 这是用户明确要的口径 —— 要在面板里看得见、能命名，所以不进系统凭据管理器。
//! 与「提问明文落会话存档」同一条已知代价（见 `reference/doc/agent-status-push-guide.md` §12）。
//!
//! ## 与流程脚本的地址策略差异
//! `flow` 挡掉 loopback / 私网，因为那里发的是**用户自写脚本**拼出的任意地址；
//! 这里的地址是用户在设置页**自己显式填**的固定字段，自建 ntfy 常在局域网，
//! 所以 push 只校 scheme 与形状（见 [`providers::normalize_server`]）。
//!
//! ## panic 红线
//! release 是 `panic = "abort"`：本模块不 `unwrap` / 不 `expect` / 不裸索引，
//! 锁中毒一律降级成「这一轮不发」。

pub mod config;
pub mod message;
pub mod providers;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

use config::{backoff_seconds, ChannelConfig, PushConfig, MAX_CONTENT_CHARS};
use message::{PushContent, PushEvent, PushMessage};
use providers::{bark::BarkProvider, ntfy::NtfyProvider, PushProvider, SendRequest, ServerSpec};

use crate::modules::agent_status::flow::{FlowField, FlowState, HttpFetch, UreqFetch};
use crate::modules::agent_status::status::{now_ms, AgentEventKind, SessionSnapshot, StatusSnapshot};

/// 广播推送面板数据的事件名（payload 为 [`PushSnapshot`]）。
pub const PUSH_STATUS_EVENT: &str = "push-status";
/// 连续失败退避的基数（秒）：第一次失败后至少等这么久再试。
const BACKOFF_BASE_SEC: u64 = 60;
/// 测试消息的正文：走的是同一条 `send`，但内容固定（测试要回答的是「这条地址通不通」）。
const TEST_BODY: &str = "这是一条来自 UFO 的测试推送";

/// 一个待投递任务：文案在入队时就拼好（那时还拿得到会话快照），worker 只管发。
#[derive(Debug, Clone)]
pub struct PushJob {
    /// 主题行 id（退避按这一行计）。
    pub channel_id: String,
    /// 所属 provider id。
    pub provider_id: String,
    /// 已拼好的文案。
    pub message: PushMessage,
}

/// 一个主题的测试结果：面板据此逐行报「哪一条成了、哪一条为什么没成」。
#[derive(Debug, Clone, Serialize)]
pub struct PushTestOutcome {
    /// 主题名字。
    pub name: String,
    /// 是否送达。
    pub ok: bool,
    /// HTTP 状态码；没发出去时为 0。
    pub status: u16,
    /// 失败原因（可行动的一句话）；成功为 None。
    pub error: Option<String>,
}

/// 面板用的主题视图（一行 = 一个收件地址）。
#[derive(Debug, Clone, Serialize)]
pub struct ChannelView {
    /// 稳定行 id。
    pub id: String,
    /// 所属 provider。
    pub provider: String,
    /// 用户起的名字（列表左侧）。
    pub name: String,
    /// 主题串（列表右侧，明文展示）。
    pub topic: String,
}

/// 面板用的 provider 视图：开关 + 服务器地址 + 那一栏的文案。
#[derive(Debug, Clone, Serialize)]
pub struct ProviderView {
    /// provider id。
    pub id: String,
    /// 展示名。
    pub label: String,
    /// 该服务是否启用（两层开关的第二层）。
    pub enabled: bool,
    /// 服务器地址原样值；空串表示用 [`ServerSpec::default`]。
    pub server: String,
    /// 地址栏的标签 / 默认值 / 说明。
    pub spec: ServerSpec,
}

/// 一次广播 / 一次 `get_push_status` 的完整载荷。
#[derive(Debug, Clone, Serialize)]
pub struct PushSnapshot {
    /// 推送配置（含主题串本身 —— 它不是密钥，见模块文档）。
    pub config: PushConfig,
    /// 已注册的服务（顺序 = UI 顺序）。
    pub providers: Vec<ProviderView>,
    /// 主题清单（顺序即 UI 顺序）。
    pub channels: Vec<ChannelView>,
}

/// 一个主题的失败计数（退避用）。
#[derive(Debug, Default, Clone, Copy)]
struct FailState {
    /// 连续失败次数。
    streak: u32,
    /// 下次允许尝试的时刻（epoch 毫秒）。
    next_at_ms: i64,
}

/// 调度层的全部可变状态。
#[derive(Default)]
struct Inner {
    /// 当前生效的推送配置。
    config: PushConfig,
    /// 本轮攒下的信号：会话 key → 最新事件语义（同一轮多次触发只留最后一个）。
    signals: HashMap<String, PushEvent>,
    /// 每主题的退避状态。
    fail: HashMap<String, FailState>,
}

/// 跨窗口共享、worker 线程也持有的一份推送状态。
#[derive(Clone)]
pub struct PushState {
    inner: Arc<Mutex<Inner>>,
    tx: mpsc::Sender<PushJob>,
    http: Arc<dyn HttpFetch>,
    providers: Arc<Vec<Arc<dyn PushProvider>>>,
}

/// 已注册的全部推送服务。**加一个 provider 就在这里多写一行**（规范 §5）。
pub fn build_providers() -> Vec<Arc<dyn PushProvider>> {
    vec![Arc::new(NtfyProvider), Arc::new(BarkProvider)]
}

impl PushState {
    /// 建状态并返回接收端句柄（调用方拿到 `AppHandle` 后再 [`spawn_worker`]）。
    pub fn new(http: Arc<dyn HttpFetch>) -> (Self, ReceiverHolder) {
        let (tx, rx) = mpsc::channel::<PushJob>();
        let state = Self {
            inner: Arc::new(Mutex::new(Inner::default())),
            tx,
            http,
            providers: Arc::new(build_providers()),
        };
        (state, ReceiverHolder(rx))
    }

    /// 生产环境用的依赖装配：ureq 发请求。
    pub fn with_production_backends() -> (Self, ReceiverHolder) {
        Self::new(Arc::new(UreqFetch))
    }

    /// 按 id 找 provider。
    fn provider(&self, id: &str) -> Option<Arc<dyn PushProvider>> {
        self.providers.iter().find(|item| item.id() == id).cloned()
    }

    /// 配置落盘后由设置钩子调用（改完即时生效，无需重启）。
    ///
    /// 顺手按存活主题裁剪退避账：删掉的行不该继续在内存里占位。
    pub fn configure(&self, config: PushConfig) {
        let mut guard = match self.inner.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        guard.fail.retain(|id, _| {
            config
                .channels
                .iter()
                .any(|channel| &channel.id == id)
        });
        guard.config = config;
    }

    /// 灯态真的变了 → 记一条待推送信号。
    ///
    /// 由 `status::apply_status_event` 调用（那里已确定「这一条事件让灯变了」），
    /// 因此不需要推送层再去比对上一轮快照。子代理与 Running 一类不产生信号。
    pub fn note_change(&self, session_key: &str, kind: AgentEventKind) {
        let Some(event) = PushEvent::from_kind(kind) else {
            return;
        };
        if let Ok(mut guard) = self.inner.lock() {
            if !guard.config.enabled {
                return;
            }
            guard.signals.insert(session_key.to_string(), event);
        }
    }

    /// 每轮调度：把信号换成任务入队。
    ///
    /// 依次过三道闸：总开关与事件勾选 → 服务开关 → 退避。
    /// 存活会话为空时**直接返回**，这是「agent 全停 = 零出网」的落点。
    pub fn tick(&self, snapshot: &StatusSnapshot, flow: &FlowState, now_ms_value: i64) {
        // 先取走本轮信号：即使后面全部被拦掉，也不该留到下一轮重复判定。
        let signals = match self.inner.lock() {
            Ok(mut guard) => std::mem::take(&mut guard.signals),
            Err(_) => return,
        };
        if signals.is_empty() {
            return;
        }
        let config = match self.inner.lock() {
            Ok(guard) => guard.config.clone(),
            Err(_) => return,
        };
        if !config.enabled {
            return;
        }

        // 可投的主题：服务已启用 + provider 已注册（主题这一层不再有独立开关）。
        let targets: Vec<&ChannelConfig> = config
            .channels
            .iter()
            .filter(|channel| {
                config
                    .providers
                    .get(&channel.provider)
                    .is_some_and(|provider| provider.enabled)
                    && self.provider(&channel.provider).is_some()
            })
            .collect();

        for (session_key, event) in signals {
            if !config.wants(event) {
                continue;
            }
            let Some(session) = snapshot
                .sessions
                .iter()
                .find(|item| item.key == session_key)
            else {
                continue;
            };
            // 只有勾了「脚本内容」才去读 flow 结果 —— 没勾就不该为它加一次锁。
            let script = config
                .content
                .contains(&PushContent::Script)
                .then(|| flow.session_fields(&session.key))
                .flatten();
            let message = build_message(&config, session, event, script.as_deref());
            for channel in &targets {
                if self.on_backoff(&channel.id, now_ms_value) {
                    continue;
                }
                self.enqueue(PushJob {
                    channel_id: channel.id.clone(),
                    provider_id: channel.provider.clone(),
                    message: message.clone(),
                });
            }
        }
    }

    /// 跑一个任务、回写退避，返回新的面板快照（worker 在**锁外**广播它）。
    pub fn handle_job(&self, job: &PushJob) -> PushSnapshot {
        let outcome = self.deliver(job);
        self.note_outcome(&job.channel_id, outcome.is_ok());
        self.snapshot()
    }

    /// 解析主题配置 → 交给 provider 发送。任何一步坏了都变成 `Err(原因)`。
    fn deliver(&self, job: &PushJob) -> Result<u16, String> {
        let (provider, server, topic) = self.resolve(&job.channel_id, &job.provider_id)?;
        self.send_with(&provider, &server, &topic, &job.message)
    }

    /// 从当前配置里取出 (provider, 服务器地址, 主题串)。
    fn resolve(
        &self,
        channel_id: &str,
        provider_id: &str,
    ) -> Result<(Arc<dyn PushProvider>, String, String), String> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| "推送状态暂不可用".to_string())?;
        let channel = guard
            .config
            .channels
            .iter()
            .find(|item| item.id == channel_id)
            .ok_or_else(|| "该主题已被删除".to_string())?;
        let provider = self
            .provider(provider_id)
            .ok_or_else(|| "该推送服务未注册".to_string())?;
        let server = guard
            .config
            .providers
            .get(provider_id)
            .map(|item| item.server.clone())
            .unwrap_or_default();
        Ok((provider, server, channel.topic.clone()))
    }

    /// 真正发一条：拼 [`SendRequest`] 并调 provider。投递与测试共用这一条路径。
    fn send_with(
        &self,
        provider: &Arc<dyn PushProvider>,
        server: &str,
        topic: &str,
        message: &PushMessage,
    ) -> Result<u16, String> {
        provider.send(&SendRequest { server, topic, message }, self.http.as_ref())
    }

    /// 该主题是否还在退避窗口内。
    fn on_backoff(&self, channel_id: &str, now_ms_value: i64) -> bool {
        self.inner
            .lock()
            .map(|guard| {
                guard
                    .fail
                    .get(channel_id)
                    .is_some_and(|state| state.next_at_ms > now_ms_value)
            })
            .unwrap_or(false)
    }

    /// 回写一次投递结果：成功清零，失败按 `base × 2^n` 推后下次尝试时刻。
    fn note_outcome(&self, channel_id: &str, ok: bool) {
        if let Ok(mut guard) = self.inner.lock() {
            let entry = guard.fail.entry(channel_id.to_string()).or_default();
            if ok {
                *entry = FailState::default();
            } else {
                entry.streak = entry.streak.saturating_add(1);
                let wait = backoff_seconds(BACKOFF_BASE_SEC, entry.streak);
                entry.next_at_ms = now_ms() + (wait as i64) * 1000;
            }
        }
    }

    /// 入队（worker 没起来 / 队列断了就静默丢掉：推送是尽力而为，不该反过来弄挂主流程）。
    fn enqueue(&self, job: PushJob) {
        let _ = self.tx.send(job);
    }

    /// 组一份面板快照。
    pub fn snapshot(&self) -> PushSnapshot {
        let config = match self.inner.lock() {
            Ok(guard) => guard.config.clone(),
            Err(_) => PushConfig::default(),
        };
        let providers = self
            .providers
            .iter()
            .map(|provider| {
                let service = config.providers.get(provider.id());
                ProviderView {
                    id: provider.id().to_string(),
                    label: provider.label().to_string(),
                    enabled: service.is_some_and(|item| item.enabled),
                    server: service.map(|item| item.server.clone()).unwrap_or_default(),
                    spec: provider.server(),
                }
            })
            .collect();
        let channels = config
            .channels
            .iter()
            .map(|channel| ChannelView {
                id: channel.id.clone(),
                provider: channel.provider.clone(),
                name: channel.name.clone(),
                topic: channel.topic.clone(),
            })
            .collect();
        PushSnapshot { config, providers, channels }
    }

    /// 「发一条测试」：给该服务下**每个主题**各发一条，串行、不进队列、不参与退避。
    ///
    /// 一次点完就能看出哪几行是坏的，所以逐主题报成败，而不是「遇到第一个错就返回」。
    /// ⚠️ 刻意**不看**服务与总开关：测试要回答的是「这条地址发得出去吗」，与当前是否启用无关。
    pub fn test_provider(&self, provider_id: &str) -> Result<Vec<PushTestOutcome>, String> {
        let provider = self
            .provider(provider_id)
            .ok_or_else(|| "该推送服务未注册".to_string())?;
        let (server, topics) = {
            let guard = self
                .inner
                .lock()
                .map_err(|_| "推送状态暂不可用".to_string())?;
            let server = guard
                .config
                .providers
                .get(provider_id)
                .map(|item| item.server.clone())
                .unwrap_or_default();
            let topics: Vec<(String, String)> = guard
                .config
                .channels
                .iter()
                .filter(|channel| channel.provider == provider_id)
                .map(|channel| (channel.name.clone(), channel.topic.clone()))
                .collect();
            (server, topics)
        };
        if topics.is_empty() {
            return Err("该服务还没有主题，先加一行".to_string());
        }
        let message = PushMessage {
            title: PushEvent::TurnFinished.title(),
            body: TEST_BODY.to_string(),
        };
        let outcomes = topics
            .into_iter()
            .map(|(name, topic)| match self.send_with(&provider, &server, &topic, &message) {
                Ok(status) => PushTestOutcome { name, ok: true, status, error: None },
                Err(error) => PushTestOutcome { name, ok: false, status: 0, error: Some(error) },
            })
            .collect();
        Ok(outcomes)
    }
}

/// 组一条正文。
///
/// **固定发**的只有 agent 状态：`{平台} {事件}`（如「claude 在等你」）——
/// 一条不含状态的推送没有意义。其余四项按 `config.content` 勾选追加，**默认一个都不带**：
/// endpoint / 提问 / 脚本 / 会话 id 都可能敏感，而 ntfy 的主题是公开可订阅的（规范 §3）。
fn build_message(
    config: &PushConfig,
    session: &SessionSnapshot,
    event: PushEvent,
    script: Option<&[FlowField]>,
) -> PushMessage {
    let mut lines = vec![format!("{} {}", session.provider.as_str(), event.label())];
    for item in &config.content {
        // 每项都按 `标签: 值` 追加；取不到值就整行不出现（不写「提问: 无」那种噪音）。
        let value = match item {
            PushContent::Endpoint => session
                .upstream_url
                .as_deref()
                .or(session.base_url.as_deref())
                .map(|url| clip_text(url, 160)),
            PushContent::Prompt => session
                .prompt
                .as_deref()
                .map(|prompt| clip_text(prompt, MAX_CONTENT_CHARS)),
            PushContent::Script => script.map(|fields| {
                clip_text(
                    &fields
                        .iter()
                        .map(|field| format!("{}={}", field.label, field.value))
                        .collect::<Vec<_>>()
                        .join("、"),
                    MAX_CONTENT_CHARS,
                )
            }),
            PushContent::SessionId => Some(clip_text(&session.key, 160)),
        };
        if let Some(value) = value.filter(|value| !value.is_empty()) {
            lines.push(format!("{}: {value}", item.label()));
        }
    }
    PushMessage {
        title: event.title(),
        body: lines.join("\n"),
    }
}

/// 按**字符**裁剪（按字节切会把中文截成半个字），并把换行压成空格 —— 一行一项，正文更好读。
fn clip_text(value: &str, max_chars: usize) -> String {
    let flat: String = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max_chars {
        return flat;
    }
    let mut clipped: String = flat.chars().take(max_chars).collect();
    clipped.push('…');
    clipped
}

/// `mpsc::Receiver` 的薄封装：只在 [`spawn_worker`] 里被取走一次。
pub struct ReceiverHolder(mpsc::Receiver<PushJob>);

impl ReceiverHolder {
    /// 非阻塞取一个任务（测试用：验证 tick 是否真的入队）。
    #[cfg(test)]
    fn try_recv(&self) -> Result<PushJob, mpsc::TryRecvError> {
        self.0.try_recv()
    }
}

/// 起专用 worker 线程串行消费队列。
///
/// 串行是刻意的：一次投递最长 10s，并发只会让同一个主题被连打；
/// 广播一律在锁外做，避免持锁期间被前端拖住。
pub fn spawn_worker<R: Runtime + 'static>(app: AppHandle<R>, state: PushState, rx: ReceiverHolder) {
    let ReceiverHolder(rx) = rx;
    std::thread::spawn(move || {
        for job in rx {
            let snapshot = state.handle_job(&job);
            let _ = app.emit(PUSH_STATUS_EVENT, &snapshot);
        }
    });
}

// ── 命令 ──

/// 读推送面板数据（服务列表 + 主题清单 + 配置）。
#[tauri::command]
pub fn get_push_status(state: State<'_, PushState>) -> Result<PushSnapshot, String> {
    Ok(state.snapshot())
}

/// 给某个服务下的每个主题各发一条测试。
///
/// ⚠️ 必须是 `async` + `spawn_blocking`：非 async 命令跑在主线程，
/// 几个主题串起来最坏 10s × N，会冻结整个窗口的事件循环。
#[tauri::command]
pub async fn test_push_provider(
    app: AppHandle,
    provider_id: String,
) -> Result<Vec<PushTestOutcome>, String> {
    // 取一份 owned 的克隆再离开块：`try_state` 的 guard 借用了本地 `app`，
    // 直接把 guard 留给 await 之后的话 async 命令的 future 活不过函数体。
    let state = {
        let guard = app
            .try_state::<PushState>()
            .ok_or_else(|| "推送状态还没就绪".to_string())?;
        guard.inner().clone()
    };
    tauri::async_runtime::spawn_blocking(move || state.test_provider(&provider_id))
        .await
        .map_err(|error| format!("测试任务没跑起来：{error}"))?
}

/// 测试专用假件（`cfg(test)`，不进生产构建）。
#[cfg(test)]
pub mod testing {
    use super::*;
    use crate::modules::agent_status::flow::{HttpRequest, HttpResponse};

    /// 记录请求并可预设返回码的假 HTTP。
    #[derive(Default)]
    pub struct FakeFetch {
        /// 记下的全部请求。
        pub calls: Mutex<Vec<HttpRequest>>,
        /// 固定返回码。
        pub status: u16,
        /// 固定响应体。
        pub body: String,
    }

    impl FakeFetch {
        /// 200 的默认假件。
        pub fn ok() -> Self {
            Self { calls: Mutex::new(Vec::new()), status: 200, body: String::new() }
        }

        /// 指定失败码与响应体。
        pub fn failing(status: u16, body: &str) -> Self {
            Self { calls: Mutex::new(Vec::new()), status, body: body.to_string() }
        }

        /// 最近一次请求。
        pub fn last(&self) -> Recorded {
            let guard = match self.calls.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            let request = guard.last().expect("测试里应当发过请求");
            Recorded {
                method: request.method.clone(),
                url: request.url.clone(),
                headers: request.headers.clone(),
                body: request.body.clone(),
            }
        }

        /// 已记录的请求条数（统一测试要断言「每个主题各一条」）。
        pub fn len(&self) -> usize {
            let guard = match self.calls.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            guard.len()
        }

        /// 是否一次都没发过。
        pub fn is_empty(&self) -> bool {
            self.len() == 0
        }

        /// 全部请求的 URL（按发起顺序）。
        pub fn urls(&self) -> Vec<String> {
            let guard = match self.calls.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            guard.iter().map(|item| item.url.clone()).collect()
        }
    }

    /// 一次请求的可断言快照（只拷断言要用的字段）。
    pub struct Recorded {
        /// 方法。
        pub method: String,
        /// 完整 URL。
        pub url: String,
        /// 请求头。
        pub headers: Vec<(String, String)>,
        /// 请求体。
        pub body: Option<String>,
    }

    impl HttpFetch for FakeFetch {
        fn fetch(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
            if let Ok(mut guard) = self.calls.lock() {
                guard.push(HttpRequest {
                    method: request.method.clone(),
                    url: request.url.clone(),
                    headers: request.headers.clone(),
                    body: request.body.clone(),
                    timeout_ms: request.timeout_ms,
                });
            }
            Ok(HttpResponse {
                status: self.status,
                body: self.body.clone(),
                headers: Vec::new(),
            })
        }
    }
}
