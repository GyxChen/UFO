//! 转灯日志的**词汇表**：阶段、理由、条目结构，以及内核交给日志层的裁决结果。
//!
//! 本文件是全链路「有哪些说法」的唯一来源 —— 前端要把每个 `LogReason` 翻译成人话
//! （见 `src/modules/agent-status/lib/status-log.ts`），并有一条 `?raw` 读源测试
//! 断言两边同源。加一个丢弃点必须同时加一个 reason，否则那处仍是黑洞。
//!
//! ⚠️ 本文件保持**不依赖 tauri**：hook 短命进程也要写日志（它那一侧的静默丢弃恰是
//! 最大的黑洞），而 hook 进程里没有 Tauri 运行时。

use serde::{Deserialize, Serialize};

use super::super::model::{
    now_ms, session_key, AgentEventKind, Provider, SessionPhase,
};

/// 谁写的这一行：排查时用来区分「hook 进程看到的」与「主进程看到的」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogWriter {
    /// `--status-hook` 短命进程。
    Hook,
    /// 常驻 monitor 线程（采集循环、内核、清理、巡检器）。
    Monitor,
    /// 主线程上的命令处理（清空、导出、配置变更）。
    Ui,
}

/// 信号走到了链路的哪一段。**粒度对齐代码里的判定处**，不按其后的灯态归类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogStage {
    /// hook 进程收到一次调用（含各种早退）。
    HookAttempt,
    /// 兼容层裁决：归一成哪种语义，或表明「这条我不关心」。
    CompatVerdict,
    /// hook 把规范事件原子写进 inbox。
    InboxWrite,
    /// monitor 读了一批 inbox（含跳过与清扫）。
    InboxBatch,
    /// 批内某个文件解析失败被隔离（或干脆消失）。
    InboxQuarantine,
    /// 一条规范事件进入内核后的裁决（采纳 / 拒绝及理由）。
    KernelVerdict,
    /// 灯色真的变了。
    LightTransition,
    /// 事件被内核采纳，但灯色没变（子代理记账、Unknown 继承计时等）。
    LightUnchanged,
    /// 带 `agent_id` 的事件只维护了后台集合，灯与计时原样保留。
    SubagentAliveOnly,
    /// 会话被清理（超时 / 进程已死 / 手动）。
    SessionReaped,
    /// 别人的空闲黄灯被同 provider 的新一轮清掉。
    IdleLanternCleared,
    /// Codex rollout 一轮巡检的聚合账（normal 档每会话一条）。
    RolloutScan,
    /// rollout 里某条记录被丢弃及原因。
    RolloutDropped,
    /// rollout 文件首次登记时的播种（刻意不产出事件，否则一启动就复读历史）。
    RolloutSeeded,
    /// 「文件在长 = 有活动」推出的绿灯，或 30 秒保活。
    RolloutActivity,
    /// hook 事件把新灯态回灌给巡检器的去抖基线（不回灌就会出现「批准后卡黄」）。
    BaselineReflow,
    /// endpoint 反查上游的结果。
    UpstreamResolved,
    /// 事件源健康度变化（inbox 有坏文件 = degraded；坏文件清干净 = recovered）。
    SourceHealth,
    /// 流程脚本一次执行的结果。
    FlowRun,
    /// 灯变触发的推送信号与其归宿。
    PushSignal,
    /// 配置热更新后的快照（retention / 超时 / 解析器 / 日志自身）。
    ConfigApplied,
    /// 日志系统自己的开关、轮转与写入失败。
    LogMaintenance,
}

/// 一条裁决**为什么**是这个结果。必须是具名枚举：自由文本没法聚合、也没法解释。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogReason {
    // ── hook 进程（`status/hook.rs`）────────────────────────────────
    /// `--provider` 拼错或未登记。
    UnknownProvider,
    /// stdin 读不出来。
    StdinUnreadable,
    /// payload 超过单次读取上限。
    InputTooLarge,
    /// payload 不是合法 JSON。
    PayloadNotJson,
    /// 该 provider 没有兼容层适配器。
    NoHookAdapter,
    /// 适配器判这条 payload 非法（缺 session id 等）。
    AdapterRejected,
    /// 适配器表明「该事件我不关心」，于是既不落盘也不报错 —— 此前完全无痕。
    AdapterIgnoredEvent,
    /// inbox 目录建不出来。
    InboxDirUnwritable,
    /// 临时文件建不出来。
    TempCreateFailed,
    /// 临时文件写入失败。
    TempWriteFailed,
    /// rename 发布失败（事件等于没送到）。
    PublishRenameFailed,
    /// hook 正常完成，事件已落盘。
    HookCompleted,

    // ── inbox 消费（`status/mod.rs`）────────────────────────────────
    /// 扩展名不是 `.json`，本轮跳过。
    NotJsonExtension,
    /// hook 崩溃遗留的 `.tmp`（写一半就没了），扫描时清掉。
    StaleTempSwept,
    /// 反序列化失败，移进隔离目录。
    ParseFailed,
    /// 隔离目录 rename 也失败，只能删 —— **文件凭空消失**，必须留痕。
    QuarantineRenameFailedFileLost,
    /// inbox 目录读不出来（被删、权限、正被占用）。
    InboxUnreadable,

    // ── 内核（`status/kernel.rs`）───────────────────────────────────
    /// `schema_version` 与本二进制不符。
    SchemaMismatch,
    /// 事件 id 为空，无法去重。
    EmptyEventId,
    /// 会话 id 为空，无法定位灯。
    EmptySessionId,
    /// 命中有界去重窗口：同一 `event_id` 重复投递。
    DuplicateInSeenWindow,
    /// 序号不比该执行者的水位新：乱序迟到的事件。
    StaleSequence,
    /// 会话已完成且仍在绿闪保留窗内。
    CompletedWithinRetention,
    /// 子代理事件落在已结束 / 已出错的会话上：只记账，不驱灯色。
    SubagentOnFinishedSession,
    /// `Unknown` 事件且状态未变：继承旧计时，不重置闪烁。
    UnknownKindInheritsTiming,
    /// 事件被采纳，但推导出的灯态与原态相同。
    PhaseReducedSame,
    /// 灯态发生了变化。
    PhaseChanged,
    /// 绿闪落定（retention 到期，从闪绿收成常黄）。
    FlashSettled,
    /// 用了跨重启存档的回填值（`started_at` / `prompt` / `base_url` / token 地板）。
    SeedApplied,
    /// 采纳且没用到回填，灯态也未变（普通存活事件刷新 `last_seen`）。
    RefreshOnly,

    // ── 清理（`kernel::cleanup`）────────────────────────────────────
    /// 完成灯保留期已过。
    RetentionElapsed,
    /// Running 超时仍无新事件。
    RunningTimeout,
    /// Waiting 兜底超时（30 分钟）。
    WaitingTimeout,
    /// Error 灯超时（60 秒）。
    ErrorTimeout,
    /// 上报的 pid 已经不在（进程被杀，hook 不会再发收尾事件）。
    ProcessDead,
    /// 同 provider 的新一轮把它的空闲黄灯清掉了（「我的灯凭空消失」头号元凶）。
    ClearedByNewTurnOfSameProvider,
    /// 用户手动移除。
    ManuallyDismissed,

    // ── Codex rollout 巡检（`status/codex_rollout.rs`）───────────────
    /// 子串预筛没命中：该行的职责是推进「文件在长」，不是生命周期。
    CaresPrefilterSkip,
    /// 命中了预筛但分类不出语义（新增的记录类型还没登记）。
    UnclassifiedLine,
    /// 同一批里连续同灯态，压成一条。
    SameKindMergedInBatch,
    /// 与上一轮的灯态基线相同 → 丢弃。**「批准后卡黄」就出在这里**，见 `BaselineReflow`。
    SameAsLastRoundBaseline,
    /// 该 rollout 文件上一轮已经登记过。
    AlreadyRegistered,
    /// 文件名解析不出 session id。
    SessionIdUnparsable,
    /// 首次登记只播种（读 cwd 与累计 token），刻意不产出事件。
    SeededWithoutEvent,
    /// 会话目录不存在（Codex 还没跑过，或 `CODEX_HOME` 指错）。
    SessionsDirMissing,
    /// 归档导致读不到文件，判会话结束。
    RolloutFileVanished,
    /// 「文件在长」推出绿灯。
    ActivityImpliesRunning,
    /// 长时间只写命令输出，30 秒补一条保活绿。
    KeepaliveRunning,
    /// verbose 档逐行记账已达本轮上限，之后只报聚合计数。
    BatchCountersOnly,

    // ── 旁路与下游 ─────────────────────────────────────────────────
    /// 代理解析器反查命中上游。
    ResolverHit,
    /// 解析器没有该地址对应的记录（没开代理、或端口变了）。
    ResolverMiss,
    /// 解析器读自己的配置库失败。
    ResolverError,
    /// 事件源降级：inbox 里有解析失败的文件。
    SourceDegraded,
    /// 事件源恢复：坏文件已被清走。
    SourceRecovered,
    /// 脚本执行成功。
    FlowOk,
    /// 脚本执行失败（语法、返回形状、HTTP）。
    FlowError,
    /// 脚本连续失败进入指数退避，本轮不发。
    FlowBackoff,
    /// 推送总开关或服务开关关着。
    PushDisabled,
    /// 该服务一个主题都没有。
    PushNoTopics,
    /// 灯变已排进投递队列。
    PushQueued,
    /// 投递成功。
    PushDelivered,
    /// 投递失败，进入退避。
    PushFailed,

    // ── 配置与日志自身 ─────────────────────────────────────────────
    /// 组件配置热更新已应用到运行时。
    ConfigReloaded,
    /// 日志写入器开启（含档位）。
    LogOpened,
    /// 日志按体积或天数换了文件 / 删了旧文件。
    LogRotated,
    /// 旧文件正被占用，本轮放弃回收。
    LogRotationSkippedFileBusy,
    /// 写日志本身失败（磁盘、权限）。灯态照旧，只是这条线索断了。
    LogWriteFailed,
    /// 日志被关闭了。
    SinkDisabled,
}

/// 一条日志。字段全部可选（除时间、写者、阶段），序列化为**一行 JSON**。
///
/// ⚠️ `detail` 是唯一的自由字段，纪律：**只放白名单标量**（事件名、文件名 basename、
/// 计数、时长、阶段名、长度）。提问原文一类的用户文本只在 verbose 档、且必须经
/// [`excerpt`] 截断后写入；任何情况下不得写入凭据（`secrets.get` 的值、`auth.json`、
/// `settings_config`）与 endpoint 的 query 段。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// 事件时刻（Unix 毫秒）。UI 按本地时区显示；分文件按 UTC，见 [`super::paths`]。
    pub ts_ms: i64,
    /// 写这一行的是哪个进程 / 线程。
    pub writer: LogWriter,
    /// 链路环节。
    pub stage: LogStage,
    /// 该环节为什么给出这个结果；纯记录性的条目（如一次刷新）可以为空。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<LogReason>,
    /// **贯穿 id**：一条信号从 hook 到内核共用它，UI 据此摊开完整链路。
    ///
    /// ⚠️ 刻意**不是** `event_id`：那是 payload 可控字符串，只校验非空与长度，
    /// 拿它拼文件名会把 `..` / `\` / `:` 带进 Windows 路径。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    /// 事件去重 id（原样带上，便于和 inbox 内容对上）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// 会话 key（`provider:session_id`）：UI 按会话过滤走它。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// 执行者：主线程为空串，子代理是它的 `agent_id`。序号水位按执行者分表，
    /// 「谁的事件被谁压住」必须能看出来。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    /// 兼容层归一出的事件语义。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// 灯态：变化前。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase_from: Option<String>,
    /// 灯态：变化后，或被拒绝时「维持原样」的那个态。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase_to: Option<String>,
    /// 事件自带的序号（多数平台不带，带了才有乱序判定）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,
    /// 内核当时为该执行者记的序号水位：`sequence <= watermark` 即被丢弃。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watermark: Option<u64>,
    /// 内核 revision：与广播出去的快照对得上，能确认「这一条到底进没进那次刷新」。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
    /// 白名单标量细节，见类型文档上的纪律。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

impl LogEntry {
    /// 新建一条 `now` 时刻的条目。
    pub fn new(writer: LogWriter, stage: LogStage) -> Self {
        Self {
            ts_ms: now_ms(),
            writer,
            stage,
            reason: None,
            trace_id: None,
            event_id: None,
            provider: None,
            session_id: None,
            key: None,
            actor: None,
            kind: None,
            phase_from: None,
            phase_to: None,
            sequence: None,
            watermark: None,
            revision: None,
            detail: None,
        }
    }

    /// 新建一条带理由的条目 —— 绝大多数裁决都该走这个入口，理由不许留空。
    pub fn decided(writer: LogWriter, stage: LogStage, reason: LogReason) -> Self {
        let mut entry = Self::new(writer, stage);
        entry.reason = Some(reason);
        entry
    }

    /// 补上会话坐标（provider + session_id，并顺手算出 key）。
    pub fn with_session(mut self, provider: Provider, session_id: &str) -> Self {
        self.provider = Some(provider.as_str().to_string());
        self.session_id = Some(session_id.to_string());
        self.key = Some(session_key(provider, session_id));
        self
    }

    pub fn with_trace(mut self, trace_id: &str) -> Self {
        self.trace_id = Some(trace_id.to_string());
        self
    }

    /// 旧版 hook 事件没有 `trace_id` 时的兜底关联键。
    pub fn with_event_id(mut self, event_id: &str) -> Self {
        self.event_id = Some(event_id.to_string());
        self
    }

    /// 只补会话 key（跨会话的账：清理、别人的黄灯被摘 —— 那些事没有「本会话」可言）。
    pub fn with_key(mut self, key: &str) -> Self {
        self.key = Some(key.to_string());
        self
    }

    pub fn with_sequence(mut self, sequence: Option<u64>) -> Self {
        self.sequence = sequence;
        self
    }

    /// 内核当时为该执行者记的序号水位。
    pub fn with_watermark(mut self, watermark: Option<u64>) -> Self {
        self.watermark = watermark;
        self
    }

    /// 内核 revision：与广播出去的快照对得上。
    pub fn with_revision(mut self, revision: u64) -> Self {
        self.revision = Some(revision);
        self
    }

    pub fn with_actor(mut self, actor: &str) -> Self {
        self.actor = Some(actor.to_string());
        self
    }

    pub fn with_reason(mut self, reason: LogReason) -> Self {
        self.reason = Some(reason);
        self
    }

    /// 只补 provider —— 会话 id 还不知道时的降级路径。
    ///
    /// 为什么会有这种半截坐标：Codex 的适配器在**取 session_id 之前**就因为「这事件我不关心」
    /// 返回了 None（`compat/codex.rs`），此时能记的只有 provider + 原始事件名。
    pub fn with_provider(mut self, provider: Provider) -> Self {
        self.provider = Some(provider.as_str().to_string());
        self
    }

    /// 补上兼容层归一出的事件语义。
    pub fn with_kind(mut self, kind: AgentEventKind) -> Self {
        self.kind = Some(wire_name(&kind));
        self
    }

    /// 补上灯态的 before→after（两端都可以是 None：新会话没有 before，从没建出过灯的会话没有 after）。
    pub fn with_phases(
        mut self,
        from: Option<SessionPhase>,
        to: Option<SessionPhase>,
    ) -> Self {
        self.phase_from = from.map(|phase| wire_name(&phase));
        self.phase_to = to.map(|phase| wire_name(&phase));
        self
    }

    pub fn with_detail(mut self, detail: serde_json::Value) -> Self {
        self.detail = Some(compact(detail));
        self
    }

    /// 序列化成**一行**（不带换行符）。序列化失败退化成一条极小的自描述行，
    /// 绝不 panic —— release 是 `panic = "abort"`。
    pub fn line(&self) -> String {
        match serde_json::to_string(self) {
            Ok(line) => line,
            Err(_) => format!(
                "{{\"ts_ms\":{},\"stage\":\"log_maintenance\",\"reason\":\"log_write_failed\"}}",
                self.ts_ms
            ),
        }
    }
}

/// 去掉值为 `null` 的键。
///
/// 埋点处经常「这个字段此刻还不知道」（`Option` 取到 None），留着 null 会让每行白胖一圈
/// —— 日志是按天累积、并且每 500ms 就要读一遍的东西，体积就是读取延迟。
fn compact(detail: serde_json::Value) -> serde_json::Value {
    match detail {
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .filter(|(_, value)| !value.is_null())
                .collect(),
        ),
        other => other,
    }
}

/// 内核处理一条事件的**裁决**：调用方据此写日志，也据此知道灯有没有变。
///
/// 取代原先的 `bool` —— 单靠 `false` 无法区分「该拒」与「灯本来就不该动」，
/// 而那正是「灯不动」查不出原因的根因。
#[derive(Debug, Clone)]
pub struct EventVerdict {
    /// 灯态或计时是否发生变化（原 `bool` 返回值，语义不变）。
    pub changed: bool,
    /// 归入哪个环节。
    pub stage: LogStage,
    /// 为什么是这个结果。
    pub reason: LogReason,
    /// 处理前的灯态（新会话为 None）。
    pub phase_from: Option<SessionPhase>,
    /// 处理后的灯态（被拒绝时 = 维持原样）。
    pub phase_to: Option<SessionPhase>,
    /// 该事件属于哪个执行者。
    pub actor: String,
    /// 内核此刻的 revision。
    pub revision: u64,
    /// 该执行者当时的序号水位：与事件自带的 `sequence` 并排看才知道「差多少」。
    pub watermark: Option<u64>,
    /// 本次是否用了跨重启存档的回填值（`started_at` / prompt / base_url / token 地板）。
    pub seed_applied: bool,
    /// 被本轮「同 provider 新一轮」清掉的别人的空闲黄灯：`(key, 摘掉时是什么灯)`。
    ///
    /// 单独列出来是因为它**不是本会话的事件造成的** —— 用户看到的现象是
    /// 「我另一盏灯凭空没了」，只有记在因果链上才查得出来。
    pub idle_cleared: Vec<(String, SessionPhase)>,
}

impl EventVerdict {
    /// 采纳且灯态真的变了。
    pub fn transitioned(
        phase_from: Option<SessionPhase>,
        phase_to: Option<SessionPhase>,
        actor: String,
        revision: u64,
    ) -> Self {
        Self {
            changed: true,
            stage: LogStage::LightTransition,
            reason: LogReason::PhaseChanged,
            phase_from,
            phase_to,
            actor,
            revision,
            watermark: None,
            seed_applied: false,
            idle_cleared: Vec::new(),
        }
    }

    /// 被拒：`changed` 恒为 false，`phase_to` 是维持原样的那个态。
    pub fn rejected(
        reason: LogReason,
        phase_to: Option<SessionPhase>,
        actor: String,
        revision: u64,
    ) -> Self {
        Self {
            changed: false,
            stage: LogStage::KernelVerdict,
            reason,
            phase_from: phase_to,
            phase_to,
            actor,
            revision,
            watermark: None,
            seed_applied: false,
            idle_cleared: Vec::new(),
        }
    }

    /// 采纳但灯没动（子代理只记账、状态归约相同、纯刷新存活时间）。
    pub fn unchanged(
        stage: LogStage,
        reason: LogReason,
        phase: Option<SessionPhase>,
        actor: String,
        revision: u64,
    ) -> Self {
        Self {
            changed: true,
            stage,
            reason,
            phase_from: phase,
            phase_to: phase,
            actor,
            revision,
            watermark: None,
            seed_applied: false,
            idle_cleared: Vec::new(),
        }
    }

    /// 补上序号水位（乱序判定被拒时最有信息量的一个数）。
    pub fn at_watermark(mut self, watermark: Option<u64>) -> Self {
        self.watermark = watermark;
        self
    }

    /// 标记「本次用到了存档回填」。
    pub fn seeded(mut self) -> Self {
        self.seed_applied = true;
        self
    }

    /// 带上被自己顺手清掉的别人的空闲黄灯。
    pub fn with_idle_cleared(mut self, cleared: Vec<(String, SessionPhase)>) -> Self {
        self.idle_cleared = cleared;
        self
    }

    /// 换理由（同一采纳路径下要把 `PhaseChanged` 改成 `SeedApplied` 一类的情形）。
    pub fn with_reason(mut self, stage: LogStage, reason: LogReason) -> Self {
        self.stage = stage;
        self.reason = reason;
        self
    }
}

/// 一轮清理的账：谁被摘了灯、为什么；谁的绿闪落定了；谁的黄灯被别人清掉了。
#[derive(Debug, Clone, Default)]
pub struct CleanupReport {
    /// 本轮是否有任何会话状态变化（原 `bool` 返回值）。
    pub changed: bool,
    /// 被摘灯的会话：`(key, 理由, 摘掉时是什么灯)`。
    pub reaped: Vec<(String, LogReason, SessionPhase)>,
    /// 绿闪落定、收成常黄的会话 key。
    pub flash_settled: Vec<String>,
    /// 被同 provider 的新一轮清掉的空闲黄灯 key。
    pub idle_cleared: Vec<String>,
}

/// verbose 档的用户文本：按**字符**截断（不能按字节切，中文会切成非法 UTF-8）。
pub fn excerpt(text: &str) -> String {
    const MAX_CHARS: usize = 160;
    let count = text.chars().count();
    if count <= MAX_CHARS {
        return text.to_string();
    }
    let kept: String = text.chars().take(MAX_CHARS).collect();
    format!("{kept}…(+{}字符未记)", count - MAX_CHARS)
}

/// 「只记长度」的 detail：默认档用它替代 [`excerpt`]，提问原文因此永不出现在 normal 档。
pub fn length_of(field: &str, text: Option<&str>) -> serde_json::Value {
    serde_json::json!({ field: text.map(|value| value.chars().count()) })
}

/// 取枚举的线协议字符串（与 `rename_all = "snake_case"` 同一口径）。
///
/// 刻意走 serde 而不是手抄一份 match：埋点每天在写 `kind` / `phase` 的字符串，
/// 手抄的那份 match 会在下一次加枚举成员时静默漏掉它。
pub fn wire_name<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_omits_absent_fields_on_the_wire() {
        // 行数直接决定磁盘占用与 tail 成本：未填的字段必须整键消失，不能留一堆 null。
        let line = LogEntry::new(LogWriter::Hook, LogStage::HookAttempt)
            .with_reason(LogReason::UnknownProvider)
            .line();
        assert!(line.starts_with('{') && !line.contains('\n'));
        assert!(line.contains("\"reason\":\"unknown_provider\""));
        assert!(!line.contains("prompt"), "detail 没填就不该出现任何键");
        assert!(!line.contains("detail"));
        assert!(!line.contains("key"));
    }

    #[test]
    fn with_session_fills_all_three_coordinates() {
        let entry = LogEntry::new(LogWriter::Monitor, LogStage::LightTransition)
            .with_session(Provider::Claude, "s-1");
        assert_eq!(entry.provider.as_deref(), Some("claude"));
        assert_eq!(entry.session_id.as_deref(), Some("s-1"));
        assert_eq!(entry.key.as_deref(), Some("claude:s-1"));
    }

    #[test]
    fn wire_names_are_snake_case_and_are_part_of_the_protocol() {
        // 枚举名就是线协议：前端的过滤参数、导出文件、guide 里的黑洞表格都写这个串。
        // 改任何一个名字都会让已落盘的日志与前端解释表错位，所以把几个代表性的钉死。
        assert_eq!(
            serde_json::to_string(&LogReason::SameAsLastRoundBaseline).unwrap(),
            "\"same_as_last_round_baseline\""
        );
        assert_eq!(
            serde_json::to_string(&LogReason::ClearedByNewTurnOfSameProvider).unwrap(),
            "\"cleared_by_new_turn_of_same_provider\""
        );
        assert_eq!(
            serde_json::to_string(&LogStage::IdleLanternCleared).unwrap(),
            "\"idle_lantern_cleared\""
        );
        assert_eq!(
            serde_json::to_string(&LogStage::BaselineReflow).unwrap(),
            "\"baseline_reflow\""
        );
        assert_eq!(
            serde_json::from_str::<LogReason>("\"adapter_ignored_event\"").unwrap(),
            LogReason::AdapterIgnoredEvent
        );
        assert_eq!(
            serde_json::from_str::<LogWriter>("\"hook\"").unwrap(),
            LogWriter::Hook
        );
    }

    #[test]
    fn excerpt_truncates_by_chars_not_bytes() {
        // 中文按字节切会劈开多字节序列（得到一个 panic 或不合法 UTF-8 串）。
        let long = "这是一段很长的中文提问内容".repeat(30);
        let cut = excerpt(&long);
        assert!(cut.chars().count() <= 161 + 12);
        assert!(cut.contains("未记"));
        assert!(std::str::from_utf8(cut.as_bytes()).is_ok());
        assert_eq!(excerpt("短的"), "短的");
    }

    #[test]
    fn length_of_records_only_a_char_count() {
        // 「修一下 bug」= 3 个汉字 + 1 空格 + 3 字母 = 7 个**字符**（不是字节）。
        let detail = length_of("prompt", Some("修一下 bug"));
        assert_eq!(detail["prompt"], serde_json::json!(7));
        assert_eq!(length_of("prompt", None)["prompt"], serde_json::Value::Null);
    }

    #[test]
    fn verdict_kinds_carry_changed_flag() {
        let applied = EventVerdict::transitioned(
            Some(SessionPhase::Waiting),
            Some(SessionPhase::Running),
            String::new(),
            7,
        );
        assert!(applied.changed);
        assert_eq!(applied.stage, LogStage::LightTransition);
        let rejected =
            EventVerdict::rejected(LogReason::StaleSequence, Some(SessionPhase::Running), String::new(), 7);
        assert!(!rejected.changed);
        assert_eq!(rejected.reason, LogReason::StaleSequence);
        // 采纳但灯没动：changed 为真（内核确实改了记账），但 from == to。
        let quiet = EventVerdict::unchanged(
            LogStage::LightUnchanged,
            LogReason::PhaseReducedSame,
            Some(SessionPhase::Running),
            "agent-1".to_string(),
            8,
        );
        assert!(quiet.changed);
        assert_eq!(quiet.phase_from, quiet.phase_to);
    }

    #[test]
    fn line_is_always_one_parseable_row() {
        // JSONL 的硬约束：一行一条、且能被 `from_str` 读回。tailer 按 `\n` 切行，
        // 只要有一行里混进裸换行（或整行不是合法 JSON），后面所有记录都会错位。
        let mut entry = LogEntry::new(LogWriter::Monitor, LogStage::LogMaintenance);
        entry = entry.with_detail(serde_json::json!({
            "nested": {"a": [1, 2, null]},
            "带换行\n与引号\"\"的键": "值里有 \t 和 中文",
        }));
        let line = entry.line();
        assert!(!line.contains('\n'), "换行必须被转义");
        let back: LogEntry = serde_json::from_str(&line).expect("一行必须能整体解析");
        assert_eq!(back.stage, LogStage::LogMaintenance);
    }
}
