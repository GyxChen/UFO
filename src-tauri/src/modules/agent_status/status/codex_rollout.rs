//! Codex 的事件源：读 rollout（Codex 自己写的会话日志），不依赖 hook。
//!
//! 为什么不用 hook、rollout 里有什么，见 `compat::codex` 的模块文档。这里讲机制：
//!
//! 1. **发现**：扫 `$CODEX_HOME/sessions/<年>/<月>/<日>/`（回溯最近两天 —— 跨零点仍在跑的
//!    会话落在前一天目录），登记没见过的 `*.jsonl`。会话 id 直接从文件名末尾的 uuid 取。
//! 2. **播种**：注册时把该文件**已有内容**读一遍，只为拿到会话身份（`cwd` → endpoint）、
//!    累计用量、以及「当前灯态基线」—— **不为这段历史产出事件**（否则 UFO 一启动就会把
//!    当天所有历史会话复读成一串灯色动画）。
//! 3. **增量**：之后每轮只读新增字节。文件在长本身就等于「有活动」→ 绿；命中生命周期记录
//!    （`task_complete` / `turn_aborted` / …）才按 `compat::codex` 的映射改灯态。
//! 4. **去抖**：每轮里**连续的**同灯态压成一条（一条指令会连写十几条记录），但同一批里灯态
//!    真的变化多次时会**按文件里的先后顺序各产出一条** —— 只留最后一条会让内核停在
//!    「等你操作」上，把「恢复执行 → 一轮结束」的完成信号整条吞掉。
//!    长时间保持「绿」则每 30 秒补一条保活事件，免得被内核的「执行中超时」清掉。
//! 5. **结束**：归档会把 rollout **移走**到 `archived_sessions/`，连续两轮不见即判会话结束。
//!
//! ⚠️ 只读、增量、按会话独立；agent 全停时文件不再增长 → 零解析、零事件。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::compat::codex;
use super::lines::LineCursor;
use super::model::{
    now_ms, session_key, AgentEvent, AgentEventKind, Provider, TokenTotals, SCHEMA_VERSION,
};

/// 发现新会话的节流间隔：目录扫描不必每轮（500ms）都做。
const DISCOVERY_INTERVAL: Duration = Duration::from_secs(3);
/// 扫描会话目录时回溯的天数。
const KEEP_DAYS: usize = 2;
/// 「绿」的保活间隔：同一灯态不重复产出事件，但长任务期间要定期刷新存活时间。
const RUNNING_KEEPALIVE: i64 = 30_000;
/// 文件连续多少轮读不到就判定会话结束（归档 = 文件被移走）。
const MISSING_POLLS_BEFORE_END: u32 = 2;

/// 一轮巡检的产出。
#[derive(Default)]
pub(crate) struct RolloutUpdates {
    /// 要喂给内核的事件（已按灯态去重）。
    pub events: Vec<AgentEvent>,
    /// 各会话的最新累计用量：`(session_key, totals)`。
    pub tokens: Vec<(String, TokenTotals)>,
}

/// 一轮巡检里观察到的一次灯态变化。
///
/// 批内连续同灯态会被压成一条（一条指令连写十几条记录），但灯态**真的变了**的那些
/// 变化会各占一条，按文件里的先后顺序排列 —— 内核据此才能看到「恢复执行 → 一轮结束」
/// 这样的序列，而不是只看到最后那个状态。
struct Observed {
    /// 该变化对应的灯态。
    kind: AgentEventKind,
    /// 记录名，写进事件的 `event` 字段供诊断。
    name: &'static str,
    /// turn 级记录的轮 id（去重键）；非 turn 级为 None。
    turn_id: Option<String>,
    /// `task_complete` 的错误正文。
    error_code: Option<String>,
}

/// 单个 rollout 文件的读取状态。
struct FileCursor {
    /// 增量行读取（与 Claude 的 transcript 共用同一套游标语义）。
    lines: LineCursor,
    /// 本文件见过的最大累计用量（Codex 的 `total_token_usage` 是累计值，只能取 max）。
    tokens: Option<TokenTotals>,
    /// 「文件不存在」的连续轮次计数。
    missing_polls: u32,
}

/// 被巡检的一个会话。
struct WatchedSession {
    /// 会话 id（内核 key 的另一半）。
    session_id: String,
    /// 内核 key：`codex:<session_id>`。
    key: String,
    /// 从 `session_meta.cwd` 解析出的 endpoint（解析一次后复用）。
    base_url: Option<String>,
    /// 文件 → 游标。
    files: HashMap<PathBuf, FileCursor>,
    /// 最近一次产出的灯态：只在变化时产出事件。
    last_kind: Option<AgentEventKind>,
    /// 正在等待用户回答的 `request_user_input` 调用 id。
    ///
    /// 等回答期间文件仍可能追加 reasoning / token_count 等普通活动；只要这个 id 还在，
    /// 就不能用「文件在长 = 绿灯」兜底，必须等配对的 `function_call_output` 回来。
    pending_user_input_call_id: Option<String>,
    /// 最近一次产出事件的时间（毫秒）。
    last_emit_ms: i64,
    /// 活动类事件的序号：保证 `event_id` 唯一（内核按 `provider:session:event_id` 去重）。
    activity_seq: u64,
    /// 各文件累计值之和。
    tokens: TokenTotals,
    /// **最近一次见过的提问**（含播种阶段读到的）。
    ///
    /// 单独存一份而不是只在事件里带：提问记录（`UserMessage`）与生命周期记录常常落在同一轮
    /// 巡检里，若只取「本轮最后一条记录」的字段，提问就会被后到的 `task_started` 顶掉。
    /// 而且 UFO 中途启动时提问已经写在文件里 → 播种能读到，但要靠这里才带得进事件。
    prompt: Option<String>,
    /// 最后一次**推给内核**的用量：与 `tokens` 分开记，才能让「播种读到的累计值」
    /// 在注册后的第一轮补推一次（否则 tooltip 的 token 行要等到下一次请求才出现）。
    reported_tokens: TokenTotals,
}

/// Codex rollout 巡检器。
#[derive(Default)]
pub(crate) struct CodexRolloutWatcher {
    sessions: HashMap<String, WatchedSession>,
    /// 已登记的文件 → 所属会话 key，避免重复播种。
    registered: HashMap<PathBuf, String>,
    /// 下次允许扫描会话目录的时间。
    next_discovery_ms: i64,
}

impl CodexRolloutWatcher {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// 每轮调用：按节流发现新会话，并增量巡检已登记的会话。
    pub(crate) fn poll(&mut self) -> RolloutUpdates {
        self.poll_at(now_ms())
    }

    /// 带时间注入的巡检入口（测试用它推进「保活」与「发现节流」）。
    fn poll_at(&mut self, now: i64) -> RolloutUpdates {
        if now >= self.next_discovery_ms {
            self.next_discovery_ms = now.saturating_add(DISCOVERY_INTERVAL.as_millis() as i64);
            self.discover(now);
        }
        self.scan(now)
    }

    /// 扫描会话目录，登记还没见过的 rollout 文件。
    fn discover(&mut self, now: i64) {
        let Some(root) = codex::codex_sessions_dir() else {
            return;
        };
        for path in recent_rollouts(&root, KEEP_DAYS) {
            if self.registered.contains_key(&path) {
                continue;
            }
            let Some(session_id) = session_id_from_path(&path) else {
                continue;
            };
            let key = session_key(Provider::Codex, &session_id);
            let session = self
                .sessions
                .entry(key.clone())
                .or_insert_with(|| WatchedSession {
                    session_id: session_id.clone(),
                    key: key.clone(),
                    base_url: None,
                    files: HashMap::new(),
                    last_kind: None,
                    pending_user_input_call_id: None,
                    last_emit_ms: now,
                    activity_seq: 0,
                    tokens: TokenTotals::default(),
                    prompt: None,
                    reported_tokens: TokenTotals::default(),
                });
            session.files.insert(
                path.clone(),
                FileCursor {
                    lines: LineCursor::new(),
                    tokens: None,
                    missing_polls: 0,
                },
            );
            self.registered.insert(path.clone(), key);
            // 播种：把已有内容读一遍，只为拿身份/用量/基线，**不产出事件**。
            self.seed(&path);
        }
    }

    /// 读一遍文件已有内容，填充会话身份、用量与灯态基线（不产出事件）。
    fn seed(&mut self, path: &Path) {
        let Some(key) = self.registered.get(path).cloned() else {
            return;
        };
        let Some(session) = self.sessions.get_mut(&key) else {
            return;
        };
        let Some(cursor) = session.files.get_mut(path) else {
            return;
        };
        let mut identity: Option<codex::CodexSessionMeta> = None;
        let mut totals: Option<TokenTotals> = None;
        let mut seeded_prompt: Option<String> = None;
        let mut pending_user_input_call_id = session.pending_user_input_call_id.clone();
        cursor.lines.read_appended(path, |line| {
            let waiting_output = pending_user_input_call_id
                .as_deref()
                .is_some_and(|call_id| codex::is_function_call_output_for(line, call_id));
            if !codex::cares(line) && !waiting_output {
                return;
            }
            let Some(record) = codex::classify_rollout_line(line) else {
                return;
            };
            if record.name == "request_user_input" {
                pending_user_input_call_id = record.call_id.clone();
            }
            if record.name == "user_input_output"
                && pending_user_input_call_id.as_deref() == record.call_id.as_deref()
            {
                pending_user_input_call_id = None;
            }
            if let Some(meta) = record.session {
                identity = Some(meta);
            }
            if let Some(record_totals) = record.tokens {
                totals = Some(match totals {
                    Some(previous) => max_totals(previous, record_totals),
                    None => record_totals,
                });
            }
            if record.prompt.is_some() {
                seeded_prompt = record.prompt;
            }
        });
        cursor.tokens = totals;
        session.pending_user_input_call_id = pending_user_input_call_id;
        session.prompt = seeded_prompt.or_else(|| session.prompt.clone());
        // ⚠️ 刻意**不**用历史定灯态基线：`session_meta` 本身就映射成「绿」，若拿它当基线，
        // 会话里第一条新记录（`task_started` → 绿）会被当成重复而吃掉，新会话的灯永远不出来。
        // 播种只负责身份与用量，灯态交给「注册之后的第一条新记录」。
        if let Some(meta) = identity {
            session.base_url = codex::codex_base_url(meta.cwd.as_deref().map(Path::new));
        }
        session.tokens = sum_tokens(&session.files);
    }

    /// 增量巡检所有已登记会话，产出事件与用量。
    fn scan(&mut self, now: i64) -> RolloutUpdates {
        let mut updates = RolloutUpdates::default();
        for session in self.sessions.values_mut() {
            // 本轮观察到的**灯态变化序列**：批内连续同灯态压成一条（一条指令会连写十几条
            // 记录），但**不再只留最后一条** —— 同一批里真的出现了「恢复执行 → 结束」时
            // 必须按顺序发出去，否则内核停在「等你操作」上，完成信号被整条吞掉。
            let mut observed: Vec<Observed> = Vec::new();
            // 本轮是否读到过生命周期记录（决定「只有活动」的兜底要不要生效）。
            let mut saw_lifecycle = false;
            // 本轮的提问：**独立于 `observed`**，因为它常与生命周期记录同轮出现（见字段注释）。
            let mut prompt: Option<String> = None;
            // 本文件的等待状态：闭包里还要借用 `session.files`，先用局部变量承载，扫完再写回。
            let mut pending_user_input_call_id = session.pending_user_input_call_id.clone();
            let mut grew = false;
            let mut vanished: Vec<PathBuf> = Vec::new();

            for (path, cursor) in session.files.iter_mut() {
                // 归档会把 rollout 移走 —— 连续两轮读不到即判会话结束。
                if !path.is_file() {
                    cursor.missing_polls = cursor.missing_polls.saturating_add(1);
                    if cursor.missing_polls >= MISSING_POLLS_BEFORE_END {
                        vanished.push(path.clone());
                    }
                    continue;
                }
                cursor.missing_polls = 0;

                let mut identity: Option<codex::CodexSessionMeta> = None;
                cursor.lines.read_appended(path, |line| {
                    // 文件在长 = 有活动（大记录不必解析，见 compat::codex::cares）。
                    grew = true;
                    let waiting_output = pending_user_input_call_id
                        .as_deref()
                        .is_some_and(|call_id| codex::is_function_call_output_for(line, call_id));
                    if !codex::cares(line) && !waiting_output {
                        return;
                    }
                    let Some(record) = codex::classify_rollout_line(line) else {
                        return;
                    };
                    if record.name == "request_user_input" {
                        pending_user_input_call_id = record.call_id.clone();
                    }
                    if record.name == "user_input_output"
                        && pending_user_input_call_id.as_deref() == record.call_id.as_deref()
                    {
                        pending_user_input_call_id = None;
                    }
                    if let Some(meta) = record.session {
                        identity = Some(meta);
                    }
                    if let Some(totals) = record.tokens {
                        cursor.tokens = Some(match cursor.tokens {
                            Some(previous) => max_totals(previous, totals),
                            None => totals,
                        });
                    }
                    if record.prompt.is_some() {
                        prompt = record.prompt.clone();
                    }
                    if let Some(kind) = record.kind {
                        saw_lifecycle = true;
                        // 连续同灯态压成一条；灯态真的变了才追加。
                        if observed.last().map(|entry| entry.kind) != Some(kind) {
                            observed.push(Observed {
                                kind,
                                name: record.name,
                                turn_id: record.turn_id,
                                error_code: record.error_code,
                            });
                        }
                    }
                });
                if let Some(meta) = identity {
                    session.base_url = codex::codex_base_url(meta.cwd.as_deref().map(Path::new));
                }
            }
            session.pending_user_input_call_id = pending_user_input_call_id;

            if !vanished.is_empty() {
                for path in &vanished {
                    session.files.remove(path);
                    self.registered.remove(path);
                }
                let event_id = format!("rollout-session-end-{}", now);
                updates.events.push(session.event(
                    AgentEventKind::SessionFinished,
                    "rollout_missing",
                    event_id,
                    None,
                    None,
                    now,
                ));
                session.last_kind = Some(AgentEventKind::SessionFinished);
                session.last_emit_ms = now;
                continue;
            }

            session.tokens = sum_tokens(&session.files);

            // 灯态：**首条与上一轮基线相同**的丢掉（不是新变化），其后的每一条都是本轮
            // 新观测到的变化。长时间保持「绿」则按间隔补一条保活事件。
            let keepalive_due = now.saturating_sub(session.last_emit_ms) >= RUNNING_KEEPALIVE;
            let mut emitting: Vec<Observed> = Vec::new();
            for entry in observed {
                if emitting.is_empty() && Some(entry.kind) == session.last_kind {
                    continue;
                }
                emitting.push(entry);
            }
            // 没有新的灯态变化时兜两件事：
            // - 只有活动、没有生命周期记录（`!saw_lifecycle`）→ 文件在长就是「绿」；
            // - 灯本来就是绿的且到点 → 补一条保活，刷新内核那边的存活时间。
            //   长命令期间 Codex 一条记录都不写，不续命会被内核的「执行中超时」清掉；
            //   即便本批有生命周期记录，只要它没构成新灯态（基线已相同）也要续这条命。
            if emitting.is_empty() {
                if session.pending_user_input_call_id.is_none()
                    && grew
                    && !saw_lifecycle
                    && session.last_kind != Some(AgentEventKind::Running)
                {
                    emitting.push(Observed {
                        kind: AgentEventKind::Running,
                        name: "activity",
                        turn_id: None,
                        error_code: None,
                    });
                } else if keepalive_due && session.last_kind == Some(AgentEventKind::Running) {
                    emitting.push(Observed {
                        kind: AgentEventKind::Running,
                        name: "keepalive",
                        turn_id: None,
                        error_code: None,
                    });
                }
            }

            // 提问取「本轮见过的」或「此前记住的」：提问只在每轮开头写一次，
            // 而 tooltip 任何时候悬停都要能显示最近一次提问。
            if let Some(fresh) = prompt {
                session.prompt = Some(fresh);
            }

            let produced_event = !emitting.is_empty();
            for entry in emitting {
                session.activity_seq = session.activity_seq.saturating_add(1);
                // turn 级记录用语义 id：文件被截断重扫时，内核那层去重仍能兜住。
                let event_id = match entry.turn_id {
                    Some(turn_id) => format!("rollout-{}-{turn_id}", entry.name),
                    None => format!("rollout-{}-{}", entry.name, session.activity_seq),
                };
                updates.events.push(session.event(
                    entry.kind,
                    entry.name,
                    event_id,
                    entry.error_code,
                    session.prompt.clone(),
                    now,
                ));
                session.last_kind = Some(entry.kind);
                session.last_emit_ms = now;
            }

            // 用量：变化时推，**正在产出事件时也推一次** —— 内核里灯得先存在，用量才登记得上
            // （否则「先推用量、后建灯」的那次会被 `set_tokens` 丢掉）。未变化的数值
            // `set_tokens` 会幂等返回 false，不会白刷 revision。
            if (session.tokens != session.reported_tokens || produced_event)
                && session.tokens != TokenTotals::default()
            {
                session.reported_tokens = session.tokens;
                updates.tokens.push((session.key.clone(), session.tokens));
            }
        }
        updates
    }

    /// 外部来源（hook 的 `PermissionRequest`）改了这盏灯 —— 把去抖基线同步过来。
    ///
    /// ⚠️ **不调它就会出现「批准后灯卡黄」**：hook 把灯抬成 WaitingForUser 走的是另一条链路
    /// （inbox → 内核），巡检器完全不知情，去抖仍拿自己上一次产出的 Running 当基线。于是
    /// 用户批准、rollout 恢复写入时，本该转绿的活动被当成「灯态没变」整个吃掉，灯会一直卡在
    /// 黄色，直到 `RUNNING_KEEPALIVE`（30 秒）的保活事件才回来。
    ///
    /// 巡检器还没发现这个会话时（UFO 刚启动、rollout 尚未扫到）无处可同步，直接忽略 ——
    /// 那一刻退化成原行为，不会更糟。
    pub(crate) fn note_light(&mut self, key: &str, kind: AgentEventKind) {
        let Some(session) = self.sessions.get_mut(key) else {
            return;
        };
        // 基线已经是这个灯态：没什么可同步的（也不该白刷保活计时）。
        if session.last_kind == Some(kind) {
            return;
        }
        session.last_kind = Some(kind);
        session.last_emit_ms = now_ms();
    }

    /// 丢弃已不存在的会话（由 monitor 在清理后调用），连同其文件登记一并释放。
    pub(crate) fn retain<F>(&mut self, mut keep: F)
    where
        F: FnMut(&str) -> bool,
    {
        let dropped: Vec<String> = self
            .sessions
            .keys()
            .filter(|key| !keep(key))
            .cloned()
            .collect();
        for key in dropped {
            self.sessions.remove(&key);
            self.registered.retain(|_, owner| owner != &key);
        }
    }
}

/// 各文件累计值之和。
fn sum_tokens(files: &HashMap<PathBuf, FileCursor>) -> TokenTotals {
    files
        .values()
        .filter_map(|cursor| cursor.tokens)
        .fold(TokenTotals::default(), add_totals)
}

impl WatchedSession {
    /// 组装一条内核事件。
    ///
    /// 只填 rollout 能给的字段：会话 id、灯态、错误正文/提问（诊断），以及解析出的 endpoint。
    /// `pid` / `sequence` / `transcript_path` 留空 —— rollout 里没有它们。
    fn event(
        &self,
        kind: AgentEventKind,
        name: &str,
        event_id: String,
        error_code: Option<String>,
        prompt: Option<String>,
        received_at_ms: i64,
    ) -> AgentEvent {
        AgentEvent {
            schema_version: SCHEMA_VERSION,
            event_id,
            provider: Provider::Codex,
            session_id: self.session_id.clone(),
            kind,
            event: name.to_string(),
            sequence: None,
            occurred_at_ms: None,
            received_at_ms,
            pid: None,
            error_code,
            outcome: None,
            notification_type: None,
            base_url: self.base_url.clone(),
            prompt,
            transcript_path: None,
            // rollout 侧的灯态都不是「等人操作」（那条只能由 PermissionRequest 补），故恒为空。
            waiting_reason: None,
            agent_id: None,
        }
    }
}

/// 从 rollout 文件名里取会话 id（末尾 5 段即 uuid）。
///
/// 例：`rollout-2026-09-18T00-23-47-01a0b02e-6b09-71d1-bc6b-4d75b962dd35.jsonl`
/// → `01a0b02e-6b09-71d1-bc6b-4d75b962dd35`（与 `session_meta.id` 同一个值）。
fn session_id_from_path(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let groups: Vec<&str> = stem.split('-').collect();
    if groups.len() < 5 {
        return None;
    }
    Some(groups[groups.len() - 5..].join("-"))
}

/// 列出最近若干天目录下的 rollout 文件。
///
/// 目录名是 `<年>/<月>/<日>`，**字典序即时间序**，因此只需取最后几个目录 ——
/// 不必知道「今天」是几号（也就用不着本地时区/日期库）。
fn recent_rollouts(root: &Path, keep_days: usize) -> Vec<PathBuf> {
    let mut days: Vec<PathBuf> = Vec::new();
    for year in last_sorted_entries(root, 2) {
        for month in last_sorted_entries(&year, 2) {
            days.extend(last_sorted_entries(&month, keep_days));
        }
    }
    let mut files = Vec::new();
    for day in days {
        let Ok(entries) = std::fs::read_dir(&day) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) == Some("jsonl") {
                files.push(path);
            }
        }
    }
    files
}

/// 取目录下按名字排序后的最后 `count` 个子目录。
fn last_sorted_entries(dir: &Path, count: usize) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    if dirs.len() > count {
        dirs.split_off(dirs.len() - count)
    } else {
        dirs
    }
}

/// 逐字段相加（同一会话多个 rollout 各自的累计值合并）。
fn add_totals(left: TokenTotals, right: TokenTotals) -> TokenTotals {
    TokenTotals {
        input_tokens: left.input_tokens + right.input_tokens,
        cache_creation_input_tokens: left.cache_creation_input_tokens
            + right.cache_creation_input_tokens,
        cache_read_input_tokens: left.cache_read_input_tokens + right.cache_read_input_tokens,
        output_tokens: left.output_tokens + right.output_tokens,
    }
}

/// 逐字段取最大（累计值只增不减）。
fn max_totals(left: TokenTotals, right: TokenTotals) -> TokenTotals {
    TokenTotals {
        input_tokens: left.input_tokens.max(right.input_tokens),
        cache_creation_input_tokens: left
            .cache_creation_input_tokens
            .max(right.cache_creation_input_tokens),
        cache_read_input_tokens: left
            .cache_read_input_tokens
            .max(right.cache_read_input_tokens),
        output_tokens: left.output_tokens.max(right.output_tokens),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::compat::codex::CodexHomeGuard;
    use crate::modules::agent_status::status::test_support::TempDir;

    /// 造一条 rollout 记录行。
    fn record(value: serde_json::Value) -> String {
        format!("{value}\n")
    }

    fn session_meta(id: &str, cwd: Option<&str>) -> String {
        record(serde_json::json!({
            "timestamp": "2026-09-18T00:00:00.000Z",
            "type": "session_meta",
            "payload": {"session_id": id, "id": id, "cwd": cwd, "originator": "Codex Desktop"},
        }))
    }

    fn task_started(turn: &str) -> String {
        record(serde_json::json!({"type": "event_msg", "payload": {"type": "task_started", "turn_id": turn}}))
    }

    fn task_complete(turn: &str, error: Option<&str>) -> String {
        let payload = match error {
            Some(message) => {
                serde_json::json!({"type": "task_complete", "turn_id": turn, "error": {"message": message}})
            }
            None => serde_json::json!({"type": "task_complete", "turn_id": turn}),
        };
        record(serde_json::json!({"type": "event_msg", "payload": payload}))
    }

    fn turn_aborted(turn: &str) -> String {
        record(serde_json::json!({"type": "event_msg", "payload": {"type": "turn_aborted", "turn_id": turn, "reason": "interrupted"}}))
    }

    fn request_user_input(call_id: &str) -> String {
        record(serde_json::json!({
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "name": "request_user_input",
                "call_id": call_id,
                "arguments": "{\"questions\":[]}",
            },
        }))
    }

    fn function_call_output(call_id: &str) -> String {
        record(serde_json::json!({
            "type": "response_item",
            "payload": {
                "type": "function_call_output",
                "call_id": call_id,
                "output": "{\"answers\":[]}",
            },
        }))
    }

    fn token_count(input: u64, cached: u64, output: u64) -> String {
        record(serde_json::json!({"type": "event_msg", "payload": {"type": "token_count", "info": {"total_token_usage": {
            "input_tokens": input, "cached_input_tokens": cached, "cache_write_input_tokens": 0,
            "output_tokens": output, "reasoning_output_tokens": 9, "total_tokens": input + output}}}}))
    }

    const SESSION: &str = "01a0b02e-6b09-71d1-bc6b-4d75b962dd35";

    /// 造一个临时 CODEX_HOME，返回 (根目录, 今天那天的目录, 临时目录)。
    fn codex_home() -> (PathBuf, PathBuf, TempDir) {
        let temp = TempDir::new("codex-rollout");
        let day = temp.path().join("sessions").join("2026").join("09").join("18");
        std::fs::create_dir_all(&day).expect("create day dir");
        (temp.path().to_path_buf(), day, temp)
    }

    fn rollout_path(day: &Path) -> PathBuf {
        day.join(format!("rollout-2026-09-18T00-23-47-{SESSION}.jsonl"))
    }

    /// 建好文件、跑一轮发现 + 播种。
    ///
    /// ⚠️ 守卫必须**跟着返回值交回调用方**并绑定到具名变量：`CODEX_HOME` 是进程级变量，
    /// 若在这里就 Drop，测试后半段的巡检会去扫真实的 `~/.codex`（或被别的用例牵着走），
    /// 于是出现「偶发多出事件」这种并行才复现的鬼结果。
    fn watched(contents: &str) -> (CodexRolloutWatcher, PathBuf, TempDir, CodexHomeGuard) {
        let (root, day, temp) = codex_home();
        let path = rollout_path(&day);
        std::fs::write(&path, contents).expect("write rollout");
        let home = CodexHomeGuard::new(&root);
        let mut watcher = CodexRolloutWatcher::new();
        watcher.poll_at(1_000);
        (watcher, path, temp, home)
    }

    fn append(path: &Path, contents: &str) {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open for append");
        file.write_all(contents.as_bytes()).expect("append");
    }

    #[test]
    fn seed_does_not_replay_history_as_events() {
        // 历史内容（含一次中断）只用来定基线，不能产出一串灯色动画。
        let contents = format!(
            "{}{}{}",
            session_meta(SESSION, None),
            task_started("t1"),
            turn_aborted("t1")
        );
        let (mut watcher, _path, _temp, _home) = watched(&contents);
        let updates = watcher.poll_at(2_000);
        assert!(updates.events.is_empty(), "历史不该产出事件");
        assert!(updates.tokens.is_empty());
    }

    #[test]
    fn appended_turn_start_drives_a_running_event() {
        let (mut watcher, path, _temp, _home) = watched(&session_meta(SESSION, None));
        append(&path, &task_started("t1"));
        let updates = watcher.poll_at(2_000);
        assert_eq!(updates.events.len(), 1);
        let event = &updates.events[0];
        assert_eq!(event.provider, Provider::Codex);
        assert_eq!(event.session_id, SESSION);
        assert_eq!(event.kind, AgentEventKind::Running);
        assert_eq!(event.event, "task_started");
    }

    #[test]
    fn repeated_running_records_emit_a_single_event() {
        let (mut watcher, path, _temp, _home) = watched(&session_meta(SESSION, None));
        // 一条指令会连写十几条记录：同灯态只产出一次，避免刷爆 revision。
        append(&path, &task_started("t1"));
        append(
            &path,
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"function_call\"}}\n",
        );
        append(
            &path,
            &record(serde_json::json!({"type": "event_msg", "payload": {"type": "agent_message", "message": "hi"}})),
        );
        assert_eq!(watcher.poll_at(2_000).events.len(), 1);

        // 再追加纯活动记录：灯态没变，不产出。
        append(
            &path,
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"reasoning\"}}\n",
        );
        assert!(watcher.poll_at(2_500).events.is_empty());
    }

    #[test]
    fn turn_lifecycle_drives_the_expected_kinds() {
        let (mut watcher, path, _temp, _home) = watched(&session_meta(SESSION, None));
        append(&path, &task_started("t1"));
        assert_eq!(watcher.poll_at(2_000).events[0].kind, AgentEventKind::Running);

        // 正常跑完 → TurnFinished（内核绿闪落黄）。
        append(&path, &task_complete("t1", None));
        assert_eq!(
            watcher.poll_at(2_500).events[0].kind,
            AgentEventKind::TurnFinished
        );

        // 一轮内「开始 → 被中断」落在同一次巡检里：两个**真的不同**的灯态各产出一条，
        // 按文件里的先后顺序。同步丢掉中间态会让内核以为「这一轮压根没跑起来」。
        append(&path, &task_started("t2"));
        append(&path, &turn_aborted("t2"));
        let updates = watcher.poll_at(3_000);
        assert_eq!(updates.events.len(), 2);
        assert_eq!(updates.events[0].kind, AgentEventKind::Running);
        assert_eq!(updates.events[0].event, "task_started");
        assert_eq!(updates.events[1].kind, AgentEventKind::WaitingForUser);
        assert_eq!(updates.events[1].event, "turn_aborted");
    }

    #[test]
    fn request_user_input_keeps_waiting_until_matching_output() {
        let (mut watcher, path, _temp, _home) = watched(&session_meta(SESSION, None));
        append(&path, &task_started("t1"));
        assert_eq!(watcher.poll_at(2_000).events[0].kind, AgentEventKind::Running);

        append(&path, &request_user_input("call-ask-1"));
        let waiting = watcher.poll_at(2_500);
        assert_eq!(waiting.events.len(), 1);
        assert_eq!(waiting.events[0].kind, AgentEventKind::WaitingForUser);
        assert_eq!(waiting.events[0].event, "request_user_input");
        assert_eq!(
            watcher.sessions[&session_key(Provider::Codex, SESSION)]
                .pending_user_input_call_id
                .as_deref(),
            Some("call-ask-1")
        );

        // 输出正文里提到等待中的 call id，不代表这就是它的 output；必须精确匹配 call_id 字段。
        append(
            &path,
            &record(serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "function_call_output",
                    "call_id": "call-other",
                    "output": "mentions call-ask-1 in the body",
                },
            })),
        );
        assert!(watcher.poll_at(2_650).events.is_empty());
        assert_eq!(
            watcher.sessions[&session_key(Provider::Codex, SESSION)]
                .pending_user_input_call_id
                .as_deref(),
            Some("call-ask-1")
        );

        // 等待回答期间会继续写 reasoning / token_count 等普通活动，不能兜底刷绿。
        append(&path, &token_count(100, 0, 10));
        assert!(watcher.poll_at(2_700).events.is_empty());

        // 其它 tool call 的输出不能解除这次等待。
        append(&path, &function_call_output("call-other"));
        assert!(watcher.poll_at(2_800).events.is_empty());
        assert_eq!(
            watcher.sessions[&session_key(Provider::Codex, SESSION)]
                .pending_user_input_call_id
                .as_deref(),
            Some("call-ask-1")
        );

        // 同一个 call_id 的输出才代表用户回答已回来，继续执行。
        append(&path, &function_call_output("call-ask-1"));
        let resumed = watcher.poll_at(2_900);
        assert_eq!(resumed.events.len(), 1);
        assert_eq!(resumed.events[0].kind, AgentEventKind::Running);
        assert_eq!(resumed.events[0].event, "user_input_output");
        assert_eq!(
            watcher.sessions[&session_key(Provider::Codex, SESSION)]
                .pending_user_input_call_id,
            None
        );
    }

    #[test]
    fn request_user_input_and_output_in_one_poll_keep_order() {
        let (mut watcher, path, _temp, _home) = watched(&session_meta(SESSION, None));
        append(&path, &task_started("t1"));
        assert_eq!(watcher.poll_at(2_000).events[0].kind, AgentEventKind::Running);

        append(&path, &request_user_input("call-ask-1"));
        append(&path, &function_call_output("call-ask-1"));
        let updates = watcher.poll_at(2_500);
        let kinds: Vec<_> = updates
            .events
            .iter()
            .map(|event| (event.kind, event.event.as_str()))
            .collect();
        assert_eq!(
            kinds,
            vec![
                (AgentEventKind::WaitingForUser, "request_user_input"),
                (AgentEventKind::Running, "user_input_output"),
            ]
        );
    }

    /// 中断后用户接着发了下一条、且「恢复执行 + 一轮跑完」落在同一次巡检里：
    /// 必须**按顺序**看到 Running → TurnFinished。
    ///
    /// 早先只留最后一条（TurnFinished），而内核在「等你操作」态下不认主线程 Stop
    /// （那是 Claude 阻塞等待的保护），完成信号就被整条吞掉 —— 灯一直卡在黄灯上。
    #[test]
    fn turn_aborted_then_a_completed_turn_reports_running_before_finished() {
        let (mut watcher, path, _temp, _home) = watched(&session_meta(SESSION, None));
        append(&path, &task_started("t1"));
        append(&path, &turn_aborted("t1"));
        assert_eq!(watcher.poll_at(2_000).events.len(), 2);

        append(&path, &task_started("t2"));
        append(&path, &task_complete("t2", None));
        let updates = watcher.poll_at(2_500);
        assert_eq!(updates.events.len(), 2, "恢复执行与完成各算一次变化");
        assert_eq!(updates.events[0].kind, AgentEventKind::Running);
        assert_eq!(updates.events[1].kind, AgentEventKind::TurnFinished);
    }

    #[test]
    fn interrupt_after_a_gap_keeps_the_previous_state_until_it_changes() {
        // 中断发生在「上一次状态已经是 Waiting」之后（例如连着中断两次）：不重复产出。
        let (mut watcher, path, _temp, _home) = watched(&session_meta(SESSION, None));
        append(&path, &task_started("t1"));
        append(&path, &turn_aborted("t1"));
        let first = watcher.poll_at(2_000);
        assert_eq!(first.events.len(), 2);
        assert_eq!(
            first.events.last().map(|event| event.kind),
            Some(AgentEventKind::WaitingForUser)
        );

        append(&path, &turn_aborted("t2"));
        assert!(watcher.poll_at(2_500).events.is_empty(), "同灯态不重复");
    }

    #[test]
    fn failed_turn_carries_the_error_message() {
        let (mut watcher, path, _temp, _home) = watched(&session_meta(SESSION, None));
        append(
            &path,
            &task_complete("t1", Some("exceeded retry limit, last status: 429")),
        );
        let updates = watcher.poll_at(2_000);
        assert_eq!(updates.events[0].kind, AgentEventKind::Failed);
        assert!(updates.events[0]
            .error_code
            .as_deref()
            .is_some_and(|code| code.contains("exceeded retry limit")));
    }

    #[test]
    fn turn_level_event_ids_are_stable_across_rescans() {
        // turn 级事件用语义 id：同一轮被重复读到（文件被截断重扫）时 id 不变，
        // 内核那层 `provider:session:event_id` 去重仍能兜住。
        let (mut watcher, path, _temp, _home) = watched(&session_meta(SESSION, None));
        append(&path, &task_complete("t1", Some("429")));
        let first = watcher.poll_at(2_000);
        assert_eq!(first.events.len(), 1);
        let id = first.events[0].event_id.clone();

        std::fs::write(
            &path,
            format!("{}{}", session_meta(SESSION, None), task_complete("t1", Some("429"))),
        )
        .unwrap();
        for event in watcher.poll_at(3_000).events {
            if event.event == "task_complete" {
                assert_eq!(event.event_id, id, "同一轮的 id 必须稳定");
            }
        }
    }

    #[test]
    fn tokens_come_from_the_same_read() {
        let (mut watcher, path, _temp, _home) = watched(&session_meta(SESSION, None));
        append(&path, &token_count(3000, 2900, 50));
        let updates = watcher.poll_at(2_000);
        assert_eq!(updates.tokens.len(), 1);
        let (key, totals) = &updates.tokens[0];
        assert_eq!(key, &format!("codex:{SESSION}"));
        // input 已含缓存，要减出来。
        assert_eq!(totals.input_tokens, 100);
        assert_eq!(totals.cache_read_input_tokens, 2900);
        assert_eq!(totals.output_tokens, 50);
    }

    #[test]
    fn session_meta_supplies_the_endpoint() {
        // cwd 指向临时项目 → 项目级 config.toml 决定 endpoint。
        let (root, day, temp) = codex_home();
        let project = temp.path().join("proj");
        std::fs::create_dir_all(project.join(".codex")).unwrap();
        std::fs::write(
            project.join(".codex").join("config.toml"),
            "model_provider = \"custom\"\n[model_providers.custom]\nbase_url = \"https://api.deepseek.com\"\n",
        )
        .unwrap();
        let path = rollout_path(&day);
        std::fs::write(&path, session_meta(SESSION, project.to_str())).unwrap();

        let _home = CodexHomeGuard::new(&root);
        let mut watcher = CodexRolloutWatcher::new();
        watcher.poll_at(1_000);
        append(&path, &task_started("t1"));
        let updates = watcher.poll_at(2_000);
        assert_eq!(
            updates.events[0].base_url.as_deref(),
            Some("https://api.deepseek.com")
        );
    }

    #[test]
    fn vanished_rollout_ends_the_session() {
        // 归档 = 文件被移走（`archived_sessions/`）：判会话结束。
        let (mut watcher, path, _temp, _home) = watched(&session_meta(SESSION, None));
        append(&path, &task_started("t1"));
        assert_eq!(watcher.poll_at(2_000).events.len(), 1);

        std::fs::remove_file(&path).unwrap();
        // 连续两轮读不到才判结束（避免瞬时读失败误判）。
        assert!(watcher.poll_at(2_500).events.is_empty());
        let updates = watcher.poll_at(3_000);
        assert_eq!(updates.events.len(), 1);
        assert_eq!(updates.events[0].kind, AgentEventKind::SessionFinished);
    }

    #[test]
    fn long_running_sessions_get_keepalive_events() {
        // 长任务期间没有生命周期记录：按间隔补一条绿，免得被「执行中超时」清掉。
        let (mut watcher, path, _temp, _home) = watched(&session_meta(SESSION, None));
        append(&path, &task_started("t1"));
        assert_eq!(watcher.poll_at(1_000).events.len(), 1);

        append(
            &path,
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"function_call\"}}\n",
        );
        assert!(watcher.poll_at(5_000).events.is_empty(), "保活间隔内不重复");
        let updates = watcher.poll_at(1_000 + RUNNING_KEEPALIVE + 1);
        assert_eq!(updates.events.len(), 1);
        assert_eq!(updates.events[0].kind, AgentEventKind::Running);
    }

    #[test]
    fn note_light_makes_the_green_return_right_after_an_approval() {
        // 现场回放：跑着跑着 Codex 弹审批框（hook 把灯抬成黄），用户批准后命令开跑。
        let (mut watcher, path, _temp, _home) = watched(&session_meta(SESSION, None));
        append(&path, &task_started("t1"));
        assert_eq!(watcher.poll_at(2_000).events[0].kind, AgentEventKind::Running);

        // hook 的 PermissionRequest（走 inbox，不经过巡检器）→ 黄灯。
        watcher.note_light(&session_key(Provider::Codex, SESSION), AgentEventKind::WaitingForUser);

        // 批准后 rollout 恢复写入，且写的是**纯活动**记录（命令输出之类，不含生命周期记录）。
        // 契约：这一轮就必须转绿，而不是等 30 秒保活。
        append(
            &path,
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"function_call_output\"}}\n",
        );
        let updates = watcher.poll_at(2_500);
        assert_eq!(updates.events.len(), 1, "批准后的第一条活动要立刻产出");
        assert_eq!(updates.events[0].kind, AgentEventKind::Running);
        assert_eq!(updates.events[0].event, "activity");
    }

    #[test]
    fn note_light_for_an_unknown_session_is_ignored() {
        // UFO 刚启动、rollout 还没被扫到时会走到这条：无处同步，只能忽略（不得凭空造会话）。
        let (mut watcher, _path, _temp, _home) = watched(&session_meta(SESSION, None));
        watcher.note_light("codex:never-seen", AgentEventKind::WaitingForUser);
        assert_eq!(watcher.sessions.len(), 1);
        assert!(watcher.sessions.values().all(|session| session.last_kind.is_none()));
    }

    #[test]
    fn sessions_in_older_days_are_still_watched() {
        // 跨零点仍在跑的会话落在前一天目录：回溯两天必须能发现它。
        let temp = TempDir::new("codex-rollout-yesterday");
        let root = temp.path().to_path_buf();
        let yesterday = temp
            .path()
            .join("sessions")
            .join("2026")
            .join("09")
            .join("17");
        std::fs::create_dir_all(&yesterday).unwrap();
        let path = yesterday.join(format!("rollout-2026-09-17T19-08-39-{SESSION}.jsonl"));
        std::fs::write(&path, session_meta(SESSION, None)).unwrap();

        let _home = CodexHomeGuard::new(&root);
        let mut watcher = CodexRolloutWatcher::new();
        watcher.poll_at(1_000);
        append(&path, &task_started("t1"));
        assert_eq!(watcher.poll_at(2_000).events.len(), 1);
    }

    #[test]
    fn retain_drops_sessions_that_are_gone() {
        let (mut watcher, _path, _temp, _home) = watched(&session_meta(SESSION, None));
        assert_eq!(watcher.sessions.len(), 1);
        watcher.retain(|_| false);
        assert!(watcher.sessions.is_empty());
        assert!(watcher.registered.is_empty());
    }
}

#[cfg(test)]
mod tooltip_contract_tests {
    use super::*;
    use crate::modules::agent_status::status::compat::codex::CodexHomeGuard;
    use crate::modules::agent_status::status::kernel::StatusRegistry;
    use crate::modules::agent_status::status::SessionPhase;
    use crate::modules::agent_status::status::test_support::TempDir;

    /// 把一轮巡检的产出喂给内核（与 monitor 的顺序一致：**先事件后用量**）。
    fn feed(watcher: &mut CodexRolloutWatcher, registry: &mut StatusRegistry, at: i64) {
        let updates = watcher.poll_at(at);
        for event in updates.events {
            registry.apply_event_with_seed(event, 30_000, None);
        }
        for (key, totals) in updates.tokens {
            registry.set_tokens(&key, totals);
        }
    }

    /// 造一个 rollout 目录并写入初始内容，返回 (路径, 守卫, 临时目录)。
    fn rollout(contents: &str) -> (PathBuf, CodexHomeGuard, TempDir) {
        let temp = TempDir::new("codex-tooltip");
        let root = temp.path().to_path_buf();
        let day = root.join("sessions").join("2026").join("09").join("18");
        std::fs::create_dir_all(&day).unwrap();
        let path = day.join("rollout-2026-09-18T00-23-47-01a0b02e-6b09-71d1-bc6b-4d75b962dd35.jsonl");
        std::fs::write(&path, contents).unwrap();
        let guard = CodexHomeGuard::new(&root);
        (path, guard, temp)
    }

    fn append(path: &Path, value: serde_json::Value) {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        writeln!(file, "{value}").unwrap();
    }

    /// `session_meta` 行。
    fn meta() -> serde_json::Value {
        serde_json::json!({"type": "session_meta", "payload": {
            "session_id": "01a0b02e-6b09-71d1-bc6b-4d75b962dd35",
            "id": "01a0b02e-6b09-71d1-bc6b-4d75b962dd35"}})
    }

    /// **桌面端**的提问记录：`item_completed` 里包一个 `UserMessage` item（实测形态）。
    fn user_message(text: &str, turn: &str) -> serde_json::Value {
        serde_json::json!({"type": "event_msg", "payload": {
            "type": "item_completed", "turn_id": turn,
            "item": {"type": "UserMessage", "id": "u1",
                     "content": [{"type": "text", "text": text, "text_elements": []}]}}})
    }

    fn token_count(input: u64, cached: u64, output: u64) -> serde_json::Value {
        serde_json::json!({"type": "event_msg", "payload": {"type": "token_count", "info": {
            "total_token_usage": {"input_tokens": input, "cached_input_tokens": cached,
                "cache_write_input_tokens": 0, "output_tokens": output,
                "reasoning_output_tokens": 9, "total_tokens": input + output}}}})
    }

    /// tooltip 契约：一轮跑完后，快照里必须有**提问**与**用量**。
    ///
    /// 提问的坑：Codex **桌面端不写** `event_msg/user_message`，只写 `item_completed` 里的
    /// `UserMessage` item —— 少认这一种，tooltip 的提问行就永远是空的。
    #[test]
    fn desktop_turn_puts_prompt_and_tokens_into_the_snapshot() {
        let (path, _guard, _temp) = rollout(&format!("{}
", meta()));
        let mut watcher = CodexRolloutWatcher::new();
        let mut registry = StatusRegistry::new();
        feed(&mut watcher, &mut registry, 1_000);

        append(&path, serde_json::json!({"type": "event_msg", "payload": {"type": "task_started", "turn_id": "t1"}}));
        append(&path, user_message("你是什么模型
", "t1"));
        append(&path, token_count(3000, 2900, 50));
        feed(&mut watcher, &mut registry, 2_000);
        append(&path, serde_json::json!({"type": "event_msg", "payload": {"type": "turn_aborted", "turn_id": "t1", "reason": "interrupted"}}));
        feed(&mut watcher, &mut registry, 2_500);

        let snapshot = registry.snapshot(3_000);
        let session = &snapshot.sessions[0];
        assert_eq!(session.prompt.as_deref(), Some("你是什么模型"));
        let totals = session.tokens.expect("用量要进快照");
        assert_eq!(totals.input_tokens, 100, "缓存要从 input 里减出去");
        assert_eq!(totals.cache_read_input_tokens, 2900);
        assert_eq!(totals.output_tokens, 50);
    }

    /// UFO **中途启动**：提问与用量早已写在文件里（播种读到），灯却还没建。
    ///
    /// 契约：注册之后只要有活动，事件必须带上播种读到的提问，用量也必须补推一次 ——
    /// 否则 tooltip 会一直空着，直到用户再发一条提问、再跑一次请求才恢复。
    #[test]
    fn seeded_session_still_reports_prompt_and_tokens() {
        let seed = format!(
            "{}
{}
{}
",
            meta(),
            user_message("帮我看看这个 bug", "t0"),
            token_count(23729, 11072, 6864)
        );
        let (path, _guard, _temp) = rollout(&seed);
        let mut watcher = CodexRolloutWatcher::new();
        let mut registry = StatusRegistry::new();
        feed(&mut watcher, &mut registry, 1_000);
        // 播种阶段不建灯（历史不产出事件），但状态已经记住了。
        assert!(registry.snapshot(1_000).sessions.is_empty());

        // 用户接着发了下一条：这一轮的事件必须带上「上次的提问」+ 累计用量。
        append(&path, serde_json::json!({"type": "event_msg", "payload": {"type": "task_started", "turn_id": "t1"}}));
        feed(&mut watcher, &mut registry, 2_000);

        let snapshot = registry.snapshot(2_000);
        let session = &snapshot.sessions[0];
        assert_eq!(session.prompt.as_deref(), Some("帮我看看这个 bug"));
        let totals = session.tokens.expect("播种读到的用量要补推");
        assert_eq!(totals.input_tokens + totals.cache_read_input_tokens + totals.output_tokens, 30593);
    }

    /// Codex 快速轮次：先中断（黄），下一轮「恢复 + 跑完」落在**同一次巡检**里。
    ///
    /// 契约：灯要真的落回「等 prompt」（绿闪后黄常亮），而不是卡在「等你操作」上 ——
    /// 早先那一批被压成最后一条 TurnFinished，而内核在「等你操作」态下不认主线程 Stop
    /// （那是 Claude 阻塞等待的保护），完成信号就被整条吞掉。
    #[test]
    fn aborted_turn_then_a_completed_turn_settles_the_light() {
        let (path, _guard, _temp) = rollout(&format!("{}\n", meta()));
        let mut watcher = CodexRolloutWatcher::new();
        let mut registry = StatusRegistry::new();
        feed(&mut watcher, &mut registry, 1_000);

        append(&path, serde_json::json!({"type": "event_msg", "payload": {"type": "task_started", "turn_id": "t1"}}));
        append(&path, serde_json::json!({"type": "event_msg", "payload": {"type": "turn_aborted", "turn_id": "t1", "reason": "interrupted"}}));
        feed(&mut watcher, &mut registry, 2_000);
        assert_eq!(
            registry.snapshot(2_000).sessions[0].phase,
            SessionPhase::Waiting,
            "中断 → 黄灯不闪"
        );

        append(&path, serde_json::json!({"type": "event_msg", "payload": {"type": "task_started", "turn_id": "t2"}}));
        append(&path, serde_json::json!({"type": "event_msg", "payload": {"type": "task_complete", "turn_id": "t2"}}));
        feed(&mut watcher, &mut registry, 2_500);

        let session = &registry.snapshot(2_500).sessions[0];
        assert_eq!(session.phase, SessionPhase::Waiting, "一轮跑完应落回等 prompt");
        assert!(session.is_flashing, "正常结束先绿闪");
        assert_eq!(session.remove_at_ms, Some(32_500), "retention 30s 后落黄常亮");
        assert_eq!(session.waiting_reason, None);
    }

    /// 中断后用户接着发了下一条、且「恢复执行 + 一轮跑完」落在同一次巡检里：
    /// 必须**按顺序**看到 Running → TurnFinished。
    #[test]
    fn rollback_ordering_is_preserved_for_the_kernel() {
        let (path, _guard, _temp) = rollout(&format!("{}\n", meta()));
        let mut watcher = CodexRolloutWatcher::new();
        feed(&mut watcher, &mut StatusRegistry::new(), 1_000);

        append(&path, serde_json::json!({"type": "event_msg", "payload": {"type": "task_started", "turn_id": "t1"}}));
        append(&path, serde_json::json!({"type": "event_msg", "payload": {"type": "turn_aborted", "turn_id": "t1", "reason": "interrupted"}}));
        let updates = watcher.poll_at(2_000);
        let kinds: Vec<_> = updates.events.iter().map(|event| event.kind).collect();
        assert_eq!(
            kinds,
            vec![AgentEventKind::Running, AgentEventKind::WaitingForUser]
        );
    }
}
