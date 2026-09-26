//! 内核层：消费兼容层归一化出的事件，维护状态灯。
//!
//! 这里唯一认的平台语义是 [`AgentEventKind`]（六种）—— 不解析任何平台的原始事件名
//! （`event` / `notification_type` 只当诊断字段原样带着，**不得据此转灯**）。
//! 平台词汇全都活在 `super::compat`，内核保持对平台无感。

use std::collections::{HashMap, HashSet, VecDeque};

use super::model::{
    session_key, AgentEvent, AgentEventKind, SessionPhase, SessionSnapshot, StatusCounts,
    StatusSnapshot, StatusSource, StoredSeed, TokenTotals, SCHEMA_VERSION,
};

/// 去重窗口的容量（事件条数）。
///
/// 窗口刻意**有界**：早期实现用无界 `HashSet` 记所有见过的事件 id，长期运行会一直长。
/// 代价是窗口外的旧事件不再保证 exactly-once —— 但窗口按 FIFO 淘汰最旧的，而事件也基本
/// 按时间到达，重放几百条以前的事件这一情形在实际链路上不会出现。
const SEEN_EVENTS_CAPACITY: usize = 4096;

/// 主线程在序号水位表里的执行者名。
///
/// 子代理用各自的 `agent_id`，主线程用这个空串 —— 两者是**不同的可比较来源**，
/// 不能拿子代理的序号去比主线程的水位（反之亦然）。
const MAIN_ACTOR: &str = "";

/// 一次事件对**计时类字段**的效果（显式三分支，替代原先靠布尔拼装的写法）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Timing {
    /// 是否正在绿闪。
    flashing: bool,
    /// `remove_at_ms`：Completed = 移除时间；Waiting + Stop = 绿闪结束时间。
    remove_at: Option<i64>,
    /// `completed_at_ms`：只有 Completed 态有值。
    completed_at: Option<i64>,
}

pub struct StatusRegistry {
    sessions: HashMap<String, SessionSnapshot>,
    /// 各会话的内核私有状态机状态。
    ///
    /// 它比对外契约 `SessionPhase` 多出「后台子代理运行中」等中间态，
    /// 不参与序列化，也不进 `SessionSnapshot`：对外仍只投影为五种阶段。
    state: HashMap<String, SessionState>,
    /// 各会话当前仍在运行的子代理集合（`agent_id` 全集）。
    ///
    /// 用集合而不是布尔值，是为了支持并行子代理：先结束一个不代表后台已经空闲，
    /// 必须等最后一个活跃子代理结束才退出「后台运行」。
    subagents: HashMap<String, HashSet<String>>,
    /// 各会话累计的 token；由 monitor 的 TokenTracker 写入。
    /// 它不是事件派生字段，所以在 snapshot() 时合并进 SessionSnapshot。
    token_totals: HashMap<String, TokenTotals>,
    /// 各会话的**序号水位**：会话 key → 执行者（主线程或某个 `agent_id`）→ 最大已见序号。
    ///
    /// 每个执行者各有一条独立序列：子代理工具事件的序号与主线程的对不上，
    /// 混在一把尺子上会把合法事件当乱序丢掉。无序号的事件不入表，也**不清空**已有水位。
    sequence_watermarks: HashMap<String, HashMap<String, u64>>,
    /// 已处理事件 id 的近期窗口（去重）。
    seen_events: HashSet<String>,
    /// 上面对应窗口的 FIFO 顺序：超出容量时按插入顺序淘汰最旧的。
    seen_order: VecDeque<String>,
    revision: u64,
    source_health: String,
}

impl Default for StatusRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusRegistry {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            state: HashMap::new(),
            subagents: HashMap::new(),
            token_totals: HashMap::new(),
            sequence_watermarks: HashMap::new(),
            seen_events: HashSet::new(),
            seen_order: VecDeque::new(),
            revision: 0,
            source_health: "healthy".to_string(),
        }
    }

    /// 记下一条已处理的事件 id；窗口内重复命中返回 `false`。
    ///
    /// 重复命中**不刷新**它在窗口里的位置：否则重复事件会把它推回队尾，
    /// 把真正该记住的旧 id 提前挤出去（去重窗口是 FIFO，不是 LRU）。
    fn mark_seen(&mut self, event_key: String) -> bool {
        if self.seen_events.contains(&event_key) {
            return false;
        }
        self.seen_events.insert(event_key.clone());
        self.seen_order.push_back(event_key);
        while self.seen_order.len() > SEEN_EVENTS_CAPACITY {
            if let Some(evicted) = self.seen_order.pop_front() {
                self.seen_events.remove(&evicted);
            }
        }
        true
    }

    /// 某会话的内核私有状态；内存里缺记录时按对外阶段回推（与事件入口同一规则）。
    fn internal_state(&self, session_key: &str, phase: SessionPhase) -> SessionState {
        self.state
            .get(session_key)
            .copied()
            .unwrap_or_else(|| SessionState::from_phase(phase))
    }

    /// 丢掉一个会话的**全部**运行时记录（对外快照 + 私有状态机 + 子代理集合 + token）。
    ///
    /// 只从 `sessions` 里删掉、把这些留着，会让同一个 key 再次出现时读到上一世的
    /// 私有状态（灯态判断依据），因此删除必须是一整套。
    fn forget_session(&mut self, key: &str) {
        self.sessions.remove(key);
        self.state.remove(key);
        self.subagents.remove(key);
        self.token_totals.remove(key);
        self.sequence_watermarks.remove(key);
    }

    /// 收尾子代理集合：空集合直接删记录（不留空壳，长期运行下少一份占用）。
    fn store_subagents(&mut self, key: &str, subagent_set: HashSet<String>) {
        if subagent_set.is_empty() {
            self.subagents.remove(key);
        } else {
            self.subagents.insert(key.to_string(), subagent_set);
        }
    }

    pub fn mark_source_degraded(&mut self) {
        self.source_health = "degraded".to_string();
    }

    /// 写入某会话累计的 token；数值变化时返回 true 并推进 revision（触发前端刷新）。
    ///
    /// 会话已被移除时不登记 —— 避免为已消失的灯留下孤儿记录。
    ///
    /// **逐字段取 max**：跨重启回填进来的历史值要当地板用。`TokenTracker` 对新会话从
    /// offset 0 重扫 transcript，transcript 被截断 / 删除时重扫值会**小于**历史值 ——
    /// 若直接覆盖，用户已经看到的数字会凭空缩小甚至归零。取 max 后：地板保住，正常续写时
    /// 新值更大、仍准确增长。
    pub fn set_tokens(&mut self, key: &str, totals: TokenTotals) -> bool {
        if !self.sessions.contains_key(key) {
            return false;
        }
        let merged = match self.token_totals.get(key) {
            Some(previous) => merge_token_floor(*previous, totals),
            None => totals,
        };
        // 与**合并后**的值比较：否则 transcript 变短时每个 500ms 都会白 bump 一次 revision。
        if self.token_totals.get(key) == Some(&merged) {
            return false;
        }
        self.token_totals.insert(key.to_string(), merged);
        self.revision = self.revision.saturating_add(1);
        true
    }

    /// 当前存活会话的 key，供外部统计器同步清理。
    pub fn live_keys(&self) -> HashSet<String> {
        self.sessions.keys().cloned().collect()
    }

    /// Manually removes one visible session light from the runtime registry.
    pub fn dismiss_session(&mut self, key: &str) -> bool {
        let removed = self.sessions.contains_key(key);
        self.forget_session(key);
        if removed {
            self.revision = self.revision.saturating_add(1);
        }
        removed
    }

    /// 应用一条 hook 事件（无存档回填）。
    ///
    /// 生产路径走 `apply_event_with_seed`（monitor 总会先查一次回填缓存），
    /// 这个无 seed 便捷入口仅供测试构造事件用，因此标 `cfg(test)` 以免在正式构建里成 dead code。
    #[cfg(test)]
    pub fn apply_event(&mut self, event: AgentEvent, retention_ms: u64) -> bool {
        self.apply_event_with_seed(event, retention_ms, None)
    }

    /// 带存档回填的事件应用。
    ///
    /// `seed` 是同一 `provider:session_id` 的跨重启历史摘要（`claude --resume` 继续旧对话等场景）：
    /// **只在内存里没有该会话时生效** —— 存活中的会话一律以内存值为准，回填不得覆盖新鲜数据。
    /// 回填只改字段取值，不改「事件是否造成变化」的判定，因此 `revision` 语义保持不变。
    pub fn apply_event_with_seed(
        &mut self,
        event: AgentEvent,
        retention_ms: u64,
        seed: Option<&StoredSeed>,
    ) -> bool {
        let event_key = format!(
            "{}:{}",
            session_key(event.provider, &event.session_id),
            event.event_id
        );
        if event.schema_version != SCHEMA_VERSION
            || event.event_id.is_empty()
            || event.session_id.is_empty()
            || !self.mark_seen(event_key)
        {
            return false;
        }

        let now = event.received_at_ms;
        let key = session_key(event.provider, &event.session_id);
        // 序号水位按**执行者**分开比：主线程与每个子代理各有一条独立序列。
        let actor = event.agent_id.clone().unwrap_or_else(|| MAIN_ACTOR.to_string());
        if event
            .sequence
            .zip(
                self.sequence_watermarks
                    .get(&key)
                    .and_then(|actors| actors.get(&actor))
                    .copied(),
            )
            .is_some_and(|(incoming, last)| incoming <= last)
        {
            return false;
        }

        // 先把上一版快照的数据整块取出来：后面要多处查询 / 更新 self，
        // 不能在借用 self.sessions 的同时再去碰别的字段。
        let (
            previous_phase,
            previous_started_at,
            previous_pid,
            previous_error_code,
            previous_base_url,
            previous_prompt,
            previous_waiting_reason,
            previous_last_sequence,
            previous_timing,
        ) = match self.sessions.get(&key) {
            Some(previous) => {
                if previous.phase == SessionPhase::Completed
                    && previous
                        .remove_at_ms
                        .is_some_and(|remove_at| now < remove_at)
                {
                    return false;
                }
                (
                    Some(previous.phase),
                    Some(previous.started_at_ms),
                    previous.pid,
                    previous.error_code.clone(),
                    previous.base_url.clone(),
                    previous.prompt.clone(),
                    previous.waiting_reason.clone(),
                    previous.last_sequence,
                    Timing {
                        flashing: previous.is_flashing,
                        remove_at: previous.remove_at_ms,
                        completed_at: previous.completed_at_ms,
                    },
                )
            }
            None => (
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Timing::default(),
            ),
        };

        // 上一版状态机状态：老会话从对外阶段回推，新会话从未知起步。
        let previous_state = self
            .state
            .get(&key)
            .copied()
            .or_else(|| previous_phase.map(SessionState::from_phase))
            .unwrap_or(SessionState::Unknown);

        // ── 子代理事件：驱动「后台运行」语义 ──
        // 子代理与主会话**共用同一个 `session_id`**（实测确认），只靠 `agent_id` 区分。
        // 主线程 `Stop` 落黄后，子代理仍在跑时灯要保持绿色；最后一个子代理结束才落黄。
        // 但「等用户操作」（权限 / 提问）优先级更高：那种情形下子代理事件不得把黄灯顶回绿。
        let mut previous_state = previous_state;
        // 中途启动兜底：内存里从没见过该会话时，一条子代理活动事件也要能建出灯。
        // 起点是**后台运行**（而不是「主线程在跑」）：否则最后一个子代理结束时无处可退，
        // 这个错记的「主线程正在跑」会让灯一直绿下去。
        let bootstrap = previous_phase.is_none()
            && event.agent_id.is_some()
            && event.kind == AgentEventKind::Running;
        if bootstrap {
            previous_state = SessionState::BackgroundRunning;
        }
        let mut subagent_set = self.subagents.remove(&key).unwrap_or_default();
        if let Some(id) = event.agent_id.as_deref() {
            // 生命周期事实**先记账**，不论当前灯态：红灯期间后台的开工 / 收尾同样要记下来，
            // 否则主线程恢复后集合里既没数据、也丢了「后台已经结束」的事实。
            match event.kind {
                AgentEventKind::SubagentStarted | AgentEventKind::Running => {
                    // 已确认结束的会话不再收集（转灯那一刻集合已清过）。
                    // 同名 agent 之后重新 Started/Running 会再次入集合 —— 不永久拉黑，
                    // 较晚的合法恢复必须能重新激活。
                    if previous_state != SessionState::Completed {
                        subagent_set.insert(id.to_string());
                    }
                }
                // 收尾与出错都代表这个执行者已不在跑。
                AgentEventKind::SubagentStopped | AgentEventKind::Failed => {
                    subagent_set.remove(id);
                }
                _ => {}
            }
        }

        // 已结束 / 已失败：**子代理**事件只刷新存活时间，不动任何计时（Completed 的移除
        // 计时、Error 的错误码都必须原样保留），也不驱动灯色。主线程事件照旧往下走 ——
        // 红灯不粘，任何能映射出别的阶段的主线程事件都可以转灯。
        if event.agent_id.is_some()
            && matches!(
                previous_state,
                SessionState::Completed | SessionState::Failed
            )
        {
            if let Some(previous) = self.sessions.get_mut(&key) {
                previous.last_seen_at_ms = now;
            }
            self.store_subagents(&key, subagent_set);
            // 状态未被驱动，但存活时间已推进，需要对外刷新一次。
            self.revision = self.revision.saturating_add(1);
            return true;
        }

        // 状态机归约：把「主线程事件」与「子代理事件」统一成下一次内部状态。
        let (session_state, flash_waiting) = reduce_state(previous_state, &event, &subagent_set);
        // 红灯不粘，但**只有会话确认结束**才清空仍然存活的子代理集合：
        // 主线程出错不代表后台工作已经结束，一并清掉会让恢复后丢掉「仍在运行」的事实。
        if session_state == SessionState::Completed {
            subagent_set.clear();
        }

        // 同样地，外部只有五种阶段：Completed / Failed 不能被投影成后台运行。
        let phase = session_state.to_phase();

        // 主线程**自己**开始新一轮时，清掉同 provider 其它会话的**空闲黄灯**
        // （「完成态保留到下一条指令」的语义）。三个限定缺一不可：
        // - 触发者必须是主线程的运行事件：子代理事件（带 `agent_id`）不构成新的一轮；
        // - 判据看**内部状态**而不是投影片段：主线程 Stop 后仍有子代理在跑时对外也是
        //   Running，那不是「新一轮开始」，不能顺手抹掉别人的审批灯；
        // - 只清「等 prompt」的空闲黄，**不清**「等用户操作」的审批黄。
        if event.agent_id.is_none() && event.kind == AgentEventKind::Running {
            let provider = event.provider;
            let current_session = event.session_id.clone();
            let candidates: Vec<(String, SessionPhase)> = self
                .sessions
                .iter()
                .filter(|(_, session)| {
                    session.provider == provider
                        && session.session_id != current_session
                        && session.phase == SessionPhase::Waiting
                })
                .map(|(session_key, session)| (session_key.clone(), session.phase))
                .collect();
            let idle: Vec<String> = candidates
                .into_iter()
                .filter(|(session_key, phase)| {
                    self.internal_state(session_key, *phase) == SessionState::WaitingForPrompt
                })
                .map(|(session_key, _)| session_key)
                .collect();
            for session_key in idle {
                // 整套删除：私有状态机与子代理记录不能留给同一个 key 的下一世。
                self.forget_session(&session_key);
            }
        }

        // 回填只在「内存里没有该会话」时启用（previous_started_at 为该分支恒 None 的判据，
        // 上面 match 已经查过一次 self.sessions.get(&key)，不再多查一次哈希）。
        let seed = seed.filter(|_| previous_started_at.is_none());

        let started_at_ms = previous_started_at
            // 本次运行没见过这个会话 → 用存档里的创建时刻：--resume 的会话因此保住原灯位
            // （snapshot() 按 started_at_ms 升序排指示灯）。
            .or_else(|| seed.map(|seed| seed.started_at_ms))
            .unwrap_or(now);
        // 计时效果显式三选一：
        // - 事件**没构成转灯信号**（`Unknown` 保持上一状态）→ 原样继承上一版计时，
        //   否则一次无法识别的事件会把 Stop 的绿闪提前掐掉（is_flashing 归 false、
        //   remove_at 归 None，绿灯再也不落黄）；
        // - 其余按「下一状态 + 是否先绿闪的等待」重算：
        //   TurnFinished → Waiting 但先绿闪，retention 后落成黄常亮；
        //   SessionFinished → Completed，绿闪后移除；其它 Waiting 直接黄灯不闪。
        let timing = if session_state == previous_state && event.kind == AgentEventKind::Unknown {
            previous_timing
        } else {
            resolve_timing(phase, flash_waiting, now, retention_ms)
        };

        // 回填：把存档 token 当作**地板**立刻登记，避免 transcript 重扫完成前灯上显示 0。
        // 只在非零时插入 —— snapshot() 用 token_totals.get(key) 覆盖 SessionSnapshot.tokens，
        // 插入一个全零值会把「未统计到」的会话从 None 变成 Some(0)、改变 tooltip 占位表现。
        // 已有残留值时逐字段取 max：会话被 apply_event 内的 retain 移除后，其 token 记录要到
        // 下次 cleanup 才被裁剪，直接覆盖可能把用户见过的更大数字改小。
        // 在 self.sessions.insert 之前取 key（insert 会把 key move 进快照）。
        if let Some(seed) = seed.filter(|seed| seed.tokens != TokenTotals::default()) {
            let floored = match self.token_totals.get(&key) {
                Some(previous) => merge_token_floor(*previous, seed.tokens),
                None => seed.tokens,
            };
            self.token_totals.insert(key.clone(), floored);
        }

        // 收尾前落库状态机与活跃子代理集合：非法 / 去重事件在更早的守卫里已经返回，
        // 走到这里说明事件确实生效，可以安全覆盖运行时镜像。
        self.state.insert(key.clone(), session_state);
        self.store_subagents(&key, subagent_set);
        // 序号水位：**只在本事件确实带了序号时推进** —— 缺省序号的事件不得清掉已有水位
        // （否则一条无序号的事件会让后续的重放 / 乱序事件重新变得可接受）。
        if let Some(sequence) = event.sequence {
            self.sequence_watermarks
                .entry(key.clone())
                .or_default()
                .insert(actor, sequence);
        }

        self.sessions.insert(
            key.clone(),
            SessionSnapshot {
                key,
                provider: event.provider,
                session_id: event.session_id,
                phase,
                source: StatusSource::Hook,
                confidence: 100,
                started_at_ms,
                last_seen_at_ms: now,
                completed_at_ms: timing.completed_at,
                remove_at_ms: timing.remove_at,
                is_flashing: timing.flashing,
                pid: event.pid.or(previous_pid),
                // error_code 只在 Error 阶段保留：转灯后（如 Error → Running）必须清掉，
                // 避免快照出现「绿灯 + 错误码」这种自相矛盾的状态。
                error_code: if phase == SessionPhase::Error {
                    event.error_code.or(previous_error_code)
                } else {
                    None
                },
                // base_url 是会话属性，与灯色无关：新事件没带就继承旧值，
                // 避免后续事件把已探明的 endpoint 抹成 null。
                // 继承阶梯：事件 > 内存前驱 > 存档回填。
                base_url: event
                    .base_url
                    .or(previous_base_url)
                    .or_else(|| seed.and_then(|seed| seed.base_url.clone())),
                // upstream_url 不是事件派生字段，由 status 侧生成快照时按选中的
                // 解析器现算，这里恒置 None（见 StatusState::attach_upstreams）。
                upstream_url: None,
                // 「等你操作」的理由（如 Codex 的审批理由）：与 error_code 同策略 —— 只在
                // 等待期间保留。同一段等待内后续事件没带理由时继承旧的；一旦离开等待态
                // （或被别的灯态接管）立刻清空，免得 tooltip 里挂着过期的理由。
                waiting_reason: if phase == SessionPhase::Waiting {
                    event.waiting_reason.clone().or_else(|| {
                        previous_waiting_reason
                            .filter(|_| previous_phase == Some(SessionPhase::Waiting))
                    })
                } else {
                    None
                },
                // prompt 同理：只有 UserPromptSubmit 携带，同轮内的后续事件继承，
                // 直到下一次提交才被覆盖。第三档回填让 resume 的会话立刻有「最近提问」。
                prompt: event
                    .prompt
                    .or(previous_prompt)
                    .or_else(|| seed.and_then(|seed| seed.prompt.clone())),
                // token 不是事件派生字段，由 snapshot() 从 token_totals 合并进来。
                tokens: None,
                // 对外只暴露**主线程**的序号：子代理的序号属于另一条序列，混进来会
                // 让看到它的地方以为主线程跳号了。缺序号时继承上一版，别把已有的抹掉。
                last_sequence: if event.agent_id.is_none() {
                    event.sequence.or(previous_last_sequence)
                } else {
                    previous_last_sequence
                },
                termination_confirmed: phase == SessionPhase::Completed,
            },
        );
        self.revision = self.revision.saturating_add(1);
        true
    }

    /// 清理过期会话。
    ///
    /// `running_timeout_ms`：Running 无新 hook 的可配置超时（与 Error/Unknown 的 60s 分离）。
    pub fn cleanup<F>(&mut self, now: i64, running_timeout_ms: i64, mut is_alive: F) -> bool
    where
        F: FnMut(&SessionSnapshot) -> Option<bool>,
    {
        let before = self.sessions.len();
        let mut flash_settled = false;
        // 兜底：配置异常时至少保留 60s，避免 Running 被瞬间清掉。
        let running_timeout = running_timeout_ms.max(ERROR_TIMEOUT_MS);

        // Waiting + Stop 绿闪到期：关掉 is_flashing，落成黄常亮（会话保留）。
        for session in self.sessions.values_mut() {
            if session.phase == SessionPhase::Waiting
                && session.is_flashing
                && session
                    .remove_at_ms
                    .is_some_and(|remove_at| now >= remove_at)
            {
                session.is_flashing = false;
                session.remove_at_ms = None;
                flash_settled = true;
            }
        }

        self.sessions.retain(|_, session| {
            match session.phase {
                // Completed: follow retention timer, then remove.
                SessionPhase::Completed => {
                    if let Some(remove_at_ms) = session.remove_at_ms {
                        return now < remove_at_ms;
                    }
                    true
                }
                // Waiting: keep until next run clears it; failsafe timeout.
                SessionPhase::Waiting => {
                    now.saturating_sub(session.last_seen_at_ms) <= WAITING_TIMEOUT_MS
                }
                // Running：可配置的长超时 + 进程存活检查（慢 agent 不会被 60s 误杀）。
                SessionPhase::Running => {
                    if now.saturating_sub(session.last_seen_at_ms) > running_timeout {
                        return false;
                    }
                    match is_alive(session) {
                        Some(false) => false,
                        _ => true,
                    }
                }
                // Error / Unknown：短超时 + 进程存活检查。
                SessionPhase::Error | SessionPhase::Unknown => {
                    if now.saturating_sub(session.last_seen_at_ms) > ERROR_TIMEOUT_MS {
                        return false;
                    }
                    match is_alive(session) {
                        Some(false) => false,
                        _ => true,
                    }
                }
            }
        });

        // 会话消失后同步丢弃它的运行时映射（token / 状态机 / 子代理 / 序号水位）：
        // 会话一旦被清理，对应记录必须一起丢，否则长期运行会越积越多。
        // 直接查 sessions 判存活，不额外复制一份全部 key 的临时集合 —— 这是每 10 秒就跑的路径。
        self.token_totals
            .retain(|key, _| self.sessions.contains_key(key));
        self.state.retain(|key, _| self.sessions.contains_key(key));
        self.subagents.retain(|key, _| self.sessions.contains_key(key));
        self.sequence_watermarks
            .retain(|key, _| self.sessions.contains_key(key));

        let changed = flash_settled || before != self.sessions.len();
        if changed {
            self.revision = self.revision.saturating_add(1);
        }
        changed
    }

    pub fn snapshot(&self, now: i64) -> StatusSnapshot {
        let mut sessions: Vec<_> = self.sessions.values().cloned().collect();
        // token 由 monitor 的统计器单独维护，这里再合并进快照。
        for session in &mut sessions {
            session.tokens = self.token_totals.get(&session.key).copied();
        }
        sessions.sort_by(|left, right| {
            // 按「时间」排序，最早的排在最前面：以会话创建时间 started_at_ms 升序为准。
            // 用创建时间而非 last_seen_at_ms，是为了让指示灯位置稳定 ——
            // 否则任一会话每次活动都会重排，物理格子里的灯会不停跳动。
            // 同一毫秒创建（或 started 相同）时按 key 字母序兜底，保证顺序确定。
            left
                .started_at_ms
                .cmp(&right.started_at_ms)
                .then_with(|| left.key.cmp(&right.key))
        });

        let mut counts = StatusCounts::default();
        for session in &sessions {
            match session.phase {
                SessionPhase::Running => counts.running += 1,
                SessionPhase::Waiting => counts.waiting += 1,
                SessionPhase::Error => counts.error += 1,
                SessionPhase::Unknown => counts.unknown += 1,
                SessionPhase::Completed => counts.completed += 1,
            }
        }

        StatusSnapshot {
            schema_version: SCHEMA_VERSION,
            revision: self.revision,
            generated_at_ms: now,
            sessions,
            counts,
            source_health: self.source_health.clone(),
        }
    }
}

/// 把「地板值」与「新值」逐字段取 max 合并（token 不回退的守卫）。
///
/// `floor` 是已存/已回填的累计，`incoming` 是本轮重扫 transcript 得到的值。
/// transcript 被截断 / 删除时 incoming 会小于 floor，逐字段取 max 保证数字只增不减；
/// 正常续写时 incoming 更大，仍准确增长。
fn merge_token_floor(floor: TokenTotals, incoming: TokenTotals) -> TokenTotals {
    TokenTotals {
        input_tokens: floor.input_tokens.max(incoming.input_tokens),
        cache_creation_input_tokens: floor
            .cache_creation_input_tokens
            .max(incoming.cache_creation_input_tokens),
        cache_read_input_tokens: floor
            .cache_read_input_tokens
            .max(incoming.cache_read_input_tokens),
        output_tokens: floor.output_tokens.max(incoming.output_tokens),
    }
}

/// Error/Unknown 无新事件时的短超时（毫秒）；Running 使用可配置的 running_timeout_ms。
const ERROR_TIMEOUT_MS: i64 = 60 * 1000; // 1 minute

/// 测试未指定 Running 超时时使用的默认值（60 秒）。
///
/// 生产路径的超时由 `module_config` 提供，因此标为 `cfg(test)`，避免正式构建里 dead code。
#[cfg(test)]
const DEFAULT_RUNNING_TIMEOUT_MS: i64 = 60 * 1000;

/// Failsafe timeout (ms) for Waiting sessions — normally they are cleared
/// when the next run starts, but this prevents leaks if no run ever starts.
const WAITING_TIMEOUT_MS: i64 = 30 * 60 * 1000; // 30 minutes

/// 内核私有状态机的状态。
///
/// 它比对外契约 [`SessionPhase`] 多一层：把「绿灯」拆成「主线程正在跑」与
/// 「主线程已停、子代理仍在后台跑」。后者也要绿，但语义不同 —— 子代理全部结束
/// 后要立刻落回黄灯，而不是再走一次 Stop 的绿闪。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionState {
    /// 主线程正在执行（工具调用 / 生成中）。
    Running,
    /// 主线程一轮结束、等下一条 prompt；此时黄灯（Stop 之后先绿闪再落成它）。
    WaitingForPrompt,
    /// 正在等用户操作（权限弹窗 / elicitation）；黄灯且不闪。
    WaitingForUser,
    /// 主线程已停，但仍有子代理在后台跑；对外仍表现为绿常亮。
    BackgroundRunning,
    /// 会话确认结束，绿闪后移除。
    Completed,
    /// 本轮或会话出错，红灯。
    Failed,
    /// 无法识别 / 冷启动，保守状态：不主动转灯。
    Unknown,
}

impl SessionState {
    /// 从对外快照阶段回推内部状态。
    ///
    /// 只有内存里缺少私有状态（例如进程重启后从存档恢复）时才会用到；
    /// 回推不出「后台运行」这一中间态，因此退化为对应的对外语义。
    fn from_phase(phase: SessionPhase) -> Self {
        match phase {
            SessionPhase::Running => Self::Running,
            SessionPhase::Waiting => Self::WaitingForPrompt,
            SessionPhase::Completed => Self::Completed,
            SessionPhase::Error => Self::Failed,
            SessionPhase::Unknown => Self::Unknown,
        }
    }

    /// 投影成对外契约阶段：后台运行也算「正在执行」。
    fn to_phase(self) -> SessionPhase {
        match self {
            Self::Running | Self::BackgroundRunning => SessionPhase::Running,
            Self::WaitingForPrompt | Self::WaitingForUser => SessionPhase::Waiting,
            Self::Completed => SessionPhase::Completed,
            Self::Failed => SessionPhase::Error,
            Self::Unknown => SessionPhase::Unknown,
        }
    }

}

/// 由「下一状态 + 是否先绿闪的等待」算出计时类字段。
///
/// - `Completed`（会话结束）与「`Waiting` + 先绿闪」（主线程一轮结束）都从 `now` 起算
///   `retention_ms` 的绿闪；前者到期后移除，后者到期后只是落成黄常亮（不改 `remove_at`
///   之外的语义 —— `remove_at` 对 Waiting 只表示「绿闪何时结束」）。
/// - `retention_ms = 0`（用户关掉了完成灯保留）时不绿闪；Completed 仍记 `completed_at`。
fn resolve_timing(
    phase: SessionPhase,
    flash_waiting: bool,
    now: i64,
    retention_ms: u64,
) -> Timing {
    let flashing = retention_ms > 0
        && (phase == SessionPhase::Completed || (phase == SessionPhase::Waiting && flash_waiting));
    let deadline = if flashing {
        Some(now.saturating_add(retention_ms.min(i64::MAX as u64) as i64))
    } else {
        None
    };
    Timing {
        flashing,
        remove_at: deadline,
        completed_at: if phase == SessionPhase::Completed {
            Some(now)
        } else {
            None
        },
    }
}

/// 状态机归约：把「上一状态 + 本次事件」映射为「下一状态 + 是否先绿闪的等待」。
///
/// 灯色语义：
/// - Running / BackgroundRunning → 绿常亮（正在执行）
/// - WaitingForPrompt / WaitingForUser → 黄灯（等待用户操作，对话未结束）
///   - 其中主线程 TurnFinished 会额外先绿闪，retention 后落成黄常亮
/// - Failed → 红灯（接口/不可抗力错误）
/// - Completed → 绿闪后移除（对话已完成，非错误结束）
///
/// 关键优先级：**等待用户操作 > 子代理后台运行 > 空闲等待**。
/// 主线程已经弹出权限框 / 提问时，后台子代理事件不得把黄灯顶回绿。
/// 另一条对称的规则：**子代理事件不得代表主线程** —— 子代理出错不等于父会话出错。
fn reduce_state(
    previous: SessionState,
    event: &AgentEvent,
    subagents: &HashSet<String>,
) -> (SessionState, bool) {
    let from_subagent = event.agent_id.is_some();
    let next = match event.kind {
        AgentEventKind::Running => {
            // 子代理的活动事件只在「主线程已停、等 prompt」时把它拉回后台运行；
            // 等待用户操作（黄灯不闪）优先级更高，不能被子代理顶回绿。
            if from_subagent {
                match previous {
                    SessionState::WaitingForPrompt => SessionState::BackgroundRunning,
                    other => other,
                }
            } else {
                SessionState::Running
            }
        }
        AgentEventKind::TurnFinished => {
            // 主线程已经 Stop，但子代理仍在后台跑时不属于「等 prompt」，
            // 否则 Stop 的绿闪结束后会错误落黄。
            match previous {
                SessionState::WaitingForUser => SessionState::WaitingForUser,
                _ if subagents.is_empty() => SessionState::WaitingForPrompt,
                _ => SessionState::BackgroundRunning,
            }
        }
        AgentEventKind::WaitingForUser => SessionState::WaitingForUser,
        AgentEventKind::SessionFinished => SessionState::Completed,
        AgentEventKind::SubagentStarted => match previous {
            // 主线程仍在跑时由主线程状态占优；等待用户操作时黄灯优先。
            SessionState::Running => SessionState::Running,
            SessionState::WaitingForUser => SessionState::WaitingForUser,
            SessionState::Completed => SessionState::Completed,
            SessionState::Failed => SessionState::Failed,
            SessionState::WaitingForPrompt
            | SessionState::BackgroundRunning
            | SessionState::Unknown => SessionState::BackgroundRunning,
        },
        AgentEventKind::SubagentStopped => {
            match previous {
                // 主线程仍在跑 / 正在等用户操作时，子代理结束不改变当前语义。
                SessionState::Running => SessionState::Running,
                SessionState::WaitingForUser => SessionState::WaitingForUser,
                SessionState::Completed => SessionState::Completed,
                SessionState::Failed => SessionState::Failed,
                // 仍有其它并行子代理 → 保持后台运行；
                // 最后一个结束 → 落回等 prompt（黄灯）。
                SessionState::BackgroundRunning => {
                    if subagents.is_empty() {
                        SessionState::WaitingForPrompt
                    } else {
                        SessionState::BackgroundRunning
                    }
                }
                // 没有后台运行的已知状态时，不退化成「等 prompt」。
                SessionState::WaitingForPrompt | SessionState::Unknown => previous,
            }
        }
        AgentEventKind::Failed => {
            // 子代理出错只代表**那个执行者**失败（它的工具报错），不能把父会话的灯打成
            // 红的：子代理的工具失败是常见输入组合（实测 PostToolUseFailure 带 error 字段
            // 且带 agent_id），染红父灯会同时抹掉「后台还有别的子代理在跑」的事实。
            if from_subagent {
                previous
            } else {
                SessionState::Failed
            }
        }
        AgentEventKind::Unknown => previous,
    };
    // 只有主线程「一轮顺利结束且后台没有子代理」才绿闪；
    // 子代理收尾与权限等待都不闪。
    let flash_waiting = !from_subagent
        && event.kind == AgentEventKind::TurnFinished
        && matches!(next, SessionState::WaitingForPrompt);
    (next, flash_waiting)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::agent_status::status::model::Provider;

    fn event(
        id: &str,
        provider: Provider,
        session_id: &str,
        kind: AgentEventKind,
        received: i64,
    ) -> AgentEvent {
        AgentEvent {
            schema_version: SCHEMA_VERSION,
            event_id: id.to_string(),
            provider,
            session_id: session_id.to_string(),
            kind,
            // 内核不读原始事件名：这里填一个与 kind 无关的占位，顺便锁住「不据此转灯」。
            event: String::new(),
            waiting_reason: None,
            sequence: None,
            occurred_at_ms: None,
            received_at_ms: received,
            pid: None,
            error_code: None,
            outcome: None,
            notification_type: None,
            base_url: None,
            prompt: None,
            transcript_path: None,
            agent_id: None,
        }
    }

    /// 构造一条**子代理**事件：与 `event()` 同构，但带 `agent_id`。
    ///
    /// 子代理事件与主会话共用同一个 `session_id`，只靠本字段区分 —— 这正是
    /// `apply_event_with_seed` 里「已存活会话只刷存活时间、不驱灯色」的判据。
    fn subagent_event(
        id: &str,
        provider: Provider,
        session_id: &str,
        kind: AgentEventKind,
        received: i64,
    ) -> AgentEvent {
        let mut event = event(id, provider, session_id, kind, received);
        event.agent_id = Some("agent-abc".to_string());
        event
    }

    #[test]
    fn keeps_multiple_provider_sessions_separate() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(event("1", Provider::Claude, "same", AgentEventKind::Running, 1), 30_000);
        registry.apply_event(event("2", Provider::Codex, "same", AgentEventKind::Running, 2), 30_000);

        let snapshot = registry.snapshot(3);
        assert_eq!(snapshot.sessions.len(), 2);
        // 按 started 升序：Claude 先创建，排在最前。
        assert_eq!(snapshot.sessions[0].key, "claude:same");
        assert_eq!(snapshot.sessions[1].key, "codex:same");
    }

    #[test]
    fn sessions_are_ordered_earliest_created_first() {
        let mut registry = StatusRegistry::new();
        // s1 最早创建，s3 最晚；但 s3 最近才活动（last_seen 最大）。
        registry.apply_event(event("1", Provider::Claude, "s1", AgentEventKind::Running, 100), 30_000);
        registry.apply_event(event("2", Provider::Claude, "s2", AgentEventKind::Running, 200), 30_000);
        registry.apply_event(event("3", Provider::Claude, "s3", AgentEventKind::Running, 300), 30_000);
        // s1 收到新事件：last_seen 最新，但仍应排在最前（按 started 而非 last_seen）。
        registry.apply_event(
            event("4", Provider::Claude, "s1", AgentEventKind::Running, 999),
            30_000,
        );

        let snapshot = registry.snapshot(1000);
        let ids: Vec<_> = snapshot
            .sessions
            .iter()
            .map(|s| s.session_id.as_str())
            .collect();
        assert_eq!(ids, vec!["s1", "s2", "s3"]);
    }


    #[test]
    fn completed_sessions_flash_until_retention_expires() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", AgentEventKind::SessionFinished, 100),
            30_000,
        );

        assert!(!registry.cleanup(30_099, DEFAULT_RUNNING_TIMEOUT_MS, |_| Some(false)));
        assert!(registry.cleanup(30_100, DEFAULT_RUNNING_TIMEOUT_MS, |_| Some(false)));
        assert!(registry.snapshot(30_100).sessions.is_empty());
    }

    #[test]
    fn dead_error_sessions_are_removed_by_liveness_check() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(event("1", Provider::Claude, "one", AgentEventKind::Failed, 100), 30_000);

        assert!(registry.cleanup(200, DEFAULT_RUNNING_TIMEOUT_MS, |_| Some(false)));
        assert!(registry.snapshot(200).sessions.is_empty());
    }

    #[test]
    fn stop_means_waiting_with_green_flash_then_settles_yellow() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(event("1", Provider::Claude, "one", AgentEventKind::TurnFinished, 100), 30_000);
        let session = &registry.snapshot(100).sessions[0];
        // Stop 仍是 Waiting（对话未结束），但先绿闪。
        assert_eq!(session.phase, SessionPhase::Waiting);
        assert!(session.is_flashing);
        assert_eq!(session.remove_at_ms, Some(30_100));

        // retention 未到：仍闪烁，不移除。
        assert!(!registry.cleanup(30_099, DEFAULT_RUNNING_TIMEOUT_MS, |_| Some(true)));
        assert!(registry.snapshot(30_099).sessions[0].is_flashing);

        // retention 到期：关掉闪烁，落成黄常亮，会话保留。
        assert!(registry.cleanup(30_100, DEFAULT_RUNNING_TIMEOUT_MS, |_| Some(true)));
        let settled = &registry.snapshot(30_100).sessions[0];
        assert_eq!(settled.phase, SessionPhase::Waiting);
        assert!(!settled.is_flashing);
        assert_eq!(settled.remove_at_ms, None);
    }

    #[test]
    fn permission_waiting_does_not_flash() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", AgentEventKind::WaitingForUser, 100),
            30_000,
        );
        let session = &registry.snapshot(100).sessions[0];
        assert_eq!(session.phase, SessionPhase::Waiting);
        assert!(!session.is_flashing);
        assert_eq!(session.remove_at_ms, None);
    }

    /// 内核不读 `event` 原始名：一条带平台事件名、但 kind 为 Running 的事件必须照常转绿。
    ///
    /// 这正是分层要锁住的边界 —— 平台词汇（这里故意写 `Stop`）不得左右内核的灯态。
    #[test]
    fn kernel_ignores_raw_event_name() {
        let mut registry = StatusRegistry::new();
        let mut misleading = event("1", Provider::Claude, "one", AgentEventKind::Running, 100);
        misleading.event = "Stop".to_string();
        registry.apply_event(misleading, 30_000);
        assert_eq!(
            registry.snapshot(100).sessions[0].phase,
            SessionPhase::Running
        );
    }

    #[test]
    fn event_ids_are_scoped_to_the_session() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(event("same", Provider::Claude, "one", AgentEventKind::Running, 1), 30_000);
        registry.apply_event(event("same", Provider::Claude, "two", AgentEventKind::Running, 2), 30_000);
        assert_eq!(registry.snapshot(2).sessions.len(), 2);
    }

    #[test]
    fn stale_sequences_and_completed_sessions_are_ignored() {
        let mut registry = StatusRegistry::new();
        let mut first = event("1", Provider::Claude, "one", AgentEventKind::Running, 100);
        first.sequence = Some(2);
        registry.apply_event(first, 30_000);

        let mut stale = event("2", Provider::Claude, "one", AgentEventKind::Failed, 200);
        stale.sequence = Some(1);
        assert!(!registry.apply_event(stale, 30_000));
        assert_eq!(
            registry.snapshot(200).sessions[0].phase,
            SessionPhase::Running
        );

        registry.apply_event(
            event("3", Provider::Claude, "one", AgentEventKind::SessionFinished, 300),
            30_000,
        );
        assert!(!registry.apply_event(event("4", Provider::Claude, "one", AgentEventKind::Running, 400), 30_000));
        assert_eq!(
            registry.snapshot(400).sessions[0].phase,
            SessionPhase::Completed
        );
    }

    #[test]
    fn duplicate_events_do_not_change_revision() {
        let mut registry = StatusRegistry::new();
        let first = event("1", Provider::Claude, "one", AgentEventKind::Running, 100);
        assert!(registry.apply_event(first.clone(), 30_000));
        assert!(!registry.apply_event(first, 30_000));
        assert_eq!(registry.snapshot(100).revision, 1);
    }

    #[test]
    fn wrong_schema_version_is_rejected() {
        let mut registry = StatusRegistry::new();
        let mut bad = event("1", Provider::Claude, "one", AgentEventKind::Running, 100);
        bad.schema_version = 99;
        assert!(!registry.apply_event(bad, 30_000));
        assert!(registry.snapshot(100).sessions.is_empty());
    }

    #[test]
    fn empty_event_id_is_rejected() {
        let mut registry = StatusRegistry::new();
        let bad = event("", Provider::Claude, "one", AgentEventKind::Running, 100);
        assert!(!registry.apply_event(bad, 30_000));
        assert!(registry.snapshot(100).sessions.is_empty());
    }

    #[test]
    fn empty_session_id_is_rejected() {
        let mut registry = StatusRegistry::new();
        let bad = event("1", Provider::Claude, "", AgentEventKind::Running, 100);
        assert!(!registry.apply_event(bad, 30_000));
        assert!(registry.snapshot(100).sessions.is_empty());
    }

    #[test]
    fn custom_retention_affects_remove_at() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", AgentEventKind::SessionFinished, 100),
            60_000,
        );
        let session = &registry.snapshot(100).sessions[0];
        assert_eq!(session.remove_at_ms, Some(60_100));
    }

    #[test]
    fn liveness_check_preserves_completed_green_lights() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", AgentEventKind::SessionFinished, 100),
            30_000,
        );
        // Even if process is dead, completed sessions are kept until remove_at_ms
        assert!(!registry.cleanup(200, DEFAULT_RUNNING_TIMEOUT_MS, |_| Some(false)));
        assert_eq!(registry.snapshot(200).sessions.len(), 1);
        assert_eq!(
            registry.snapshot(200).sessions[0].phase,
            SessionPhase::Completed
        );
    }

    #[test]
    fn no_pid_unknown_liveness_keeps_confirmed_sessions() {
        let mut registry = StatusRegistry::new();
        // Completed sessions have termination_confirmed=true, so they survive unknown liveness
        registry.apply_event(
            event("1", Provider::Claude, "one", AgentEventKind::SessionFinished, 100),
            30_000,
        );
        assert!(!registry.cleanup(200, DEFAULT_RUNNING_TIMEOUT_MS, |_| None));
        assert_eq!(registry.snapshot(200).sessions.len(), 1);
    }

    #[test]
    fn stale_error_sessions_are_removed_after_timeout() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(event("1", Provider::Claude, "one", AgentEventKind::Failed, 100), 30_000);
        // Error sessions are kept while within the timeout window.
        assert!(!registry.cleanup(200, DEFAULT_RUNNING_TIMEOUT_MS, |_| None));
        assert_eq!(registry.snapshot(200).sessions.len(), 1);
        // After 60s without new events, stale error sessions are removed.
        let stale_time = 100 + super::ERROR_TIMEOUT_MS + 1;
        assert!(registry.cleanup(stale_time, DEFAULT_RUNNING_TIMEOUT_MS, |_| None));
        assert!(registry.snapshot(stale_time).sessions.is_empty());
    }

    #[test]
    fn running_sessions_use_configurable_timeout_not_error_60s() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", AgentEventKind::Running, 100),
            30_000,
        );
        // 把 Running 超时拉到 3 分钟：超过 Error 的 60s 仍保留绿灯。
        let running_timeout = 3 * 60 * 1000;
        let after_error_window = 100 + super::ERROR_TIMEOUT_MS + 1;
        assert!(!registry.cleanup(after_error_window, running_timeout, |_| None));
        assert_eq!(registry.snapshot(after_error_window).sessions.len(), 1);
        assert_eq!(
            registry.snapshot(after_error_window).sessions[0].phase,
            SessionPhase::Running
        );

        // 超过配置的 Running 超时：移除。
        let after_running = 100 + running_timeout + 1;
        assert!(registry.cleanup(after_running, running_timeout, |_| None));
        assert!(registry.snapshot(after_running).sessions.is_empty());
    }

    #[test]
    fn non_error_events_clear_error_state() {
        let mut registry = StatusRegistry::new();
        let mut failure = event("1", Provider::Claude, "one", AgentEventKind::Failed, 100);
        failure.error_code = Some("overloaded".to_string());
        registry.apply_event(failure, 30_000);
        assert_eq!(
            registry.snapshot(100).sessions[0].error_code.as_deref(),
            Some("overloaded")
        );

        // 红灯不粘：后续执行事件可以把红灯带回绿灯，并清掉残留的错误码。
        registry.apply_event(
            event("2", Provider::Claude, "one", AgentEventKind::Running, 200),
            30_000,
        );
        let session = &registry.snapshot(200).sessions[0];
        assert_eq!(session.phase, SessionPhase::Running);
        assert_eq!(session.error_code, None);
    }

    #[test]
    fn stop_after_error_turns_light_yellow() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", AgentEventKind::Failed, 100),
            30_000,
        );
        assert_eq!(
            registry.snapshot(100).sessions[0].phase,
            SessionPhase::Error
        );

        // 红灯期间收到 Stop：允许转灯 → Waiting 绿闪，随后落黄。
        registry.apply_event(event("2", Provider::Claude, "one", AgentEventKind::TurnFinished, 200), 30_000);
        let session = &registry.snapshot(200).sessions[0];
        assert_eq!(session.phase, SessionPhase::Waiting);
        assert!(session.is_flashing);
    }

    #[test]
    fn session_end_after_error_completes_and_removes() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", AgentEventKind::Failed, 100),
            30_000,
        );
        // 红灯期间会话结束：允许转灯 → Completed 绿闪，retention 后移除。
        registry.apply_event(
            event("2", Provider::Claude, "one", AgentEventKind::SessionFinished, 200),
            30_000,
        );
        let session = &registry.snapshot(200).sessions[0];
        assert_eq!(session.phase, SessionPhase::Completed);
        assert!(session.is_flashing);
        assert_eq!(session.remove_at_ms, Some(30_200));
    }

    #[test]
    fn unknown_event_does_not_clear_error_state() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(event("1", Provider::Claude, "one", AgentEventKind::Failed, 100), 30_000);
        // 无法识别的事件不构成转灯信号，红灯保持。
        registry.apply_event(
            event("2", Provider::Claude, "one", AgentEventKind::Unknown, 200),
            30_000,
        );
        assert_eq!(
            registry.snapshot(200).sessions[0].phase,
            SessionPhase::Error
        );
    }

    #[test]
    fn user_prompt_submit_recovers_from_error() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", AgentEventKind::Failed, 100),
            30_000,
        );
        assert_eq!(
            registry.snapshot(100).sessions[0].phase,
            SessionPhase::Error
        );

        // StopFailure 后用户再发 prompt，应恢复 Running。
        registry.apply_event(
            event("2", Provider::Claude, "one", AgentEventKind::Running, 200),
            30_000,
        );
        assert_eq!(
            registry.snapshot(200).sessions[0].phase,
            SessionPhase::Running
        );
    }

    #[test]
    fn base_url_is_kept_when_later_events_lack_it() {
        let mut registry = StatusRegistry::new();
        let mut first = event("1", Provider::Claude, "one", AgentEventKind::Running, 100);
        first.base_url = Some("http://127.0.0.1:15721".to_string());
        registry.apply_event(first, 30_000);
        assert_eq!(
            registry.snapshot(100).sessions[0].base_url.as_deref(),
            Some("http://127.0.0.1:15721")
        );

        // 后续事件没带 base_url：应继承，而不是被抹成 null。
        registry.apply_event(
            event("2", Provider::Claude, "one", AgentEventKind::Running, 200),
            30_000,
        );
        assert_eq!(
            registry.snapshot(200).sessions[0].base_url.as_deref(),
            Some("http://127.0.0.1:15721")
        );
    }

    #[test]
    fn base_url_is_updated_when_a_new_value_arrives() {
        let mut registry = StatusRegistry::new();
        let mut first = event("1", Provider::Claude, "one", AgentEventKind::Running, 100);
        first.base_url = Some("http://127.0.0.1:15721".to_string());
        registry.apply_event(first, 30_000);

        let mut second = event("2", Provider::Claude, "one", AgentEventKind::Running, 200);
        second.base_url = Some("https://api.example.com".to_string());
        registry.apply_event(second, 30_000);
        assert_eq!(
            registry.snapshot(200).sessions[0].base_url.as_deref(),
            Some("https://api.example.com")
        );
    }

    #[test]
    fn prompt_is_kept_across_the_turn_and_replaced_on_next_submit() {
        let mut registry = StatusRegistry::new();
        let mut submit = event("1", Provider::Claude, "one", AgentEventKind::Running, 100);
        submit.prompt = Some("第一个问题".to_string());
        registry.apply_event(submit, 30_000);
        assert_eq!(
            registry.snapshot(100).sessions[0].prompt.as_deref(),
            Some("第一个问题")
        );

        // 同一轮的后续事件不带 prompt：应继承，直到下一次提交。
        registry.apply_event(
            event("2", Provider::Claude, "one", AgentEventKind::Running, 200),
            30_000,
        );
        assert_eq!(
            registry.snapshot(200).sessions[0].prompt.as_deref(),
            Some("第一个问题")
        );

        // 下一次提交：覆盖为新提问。
        let mut next = event("3", Provider::Claude, "one", AgentEventKind::Running, 300);
        next.prompt = Some("第二个问题".to_string());
        registry.apply_event(next, 30_000);
        assert_eq!(
            registry.snapshot(300).sessions[0].prompt.as_deref(),
            Some("第二个问题")
        );
    }

    #[test]
    fn source_health_starts_healthy() {
        let registry = StatusRegistry::new();
        let snapshot = registry.snapshot(0);
        assert_eq!(snapshot.source_health, "healthy");
    }

    #[test]
    fn custom_retention_zero_disables_flashing() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(event("1", Provider::Claude, "one", AgentEventKind::SessionFinished, 100), 0);
        let session = &registry.snapshot(100).sessions[0];
        assert!(!session.is_flashing);
    }

    #[test]
    fn concurrent_sessions_same_provider_different_ids() {
        let mut registry = StatusRegistry::new();
        for i in 0..8 {
            registry.apply_event(
                event(
                    &format!("{i}"),
                    Provider::Claude,
                    &format!("session-{i}"),
                    AgentEventKind::Running,
                    i as i64,
                ),
                30_000,
            );
        }
        assert_eq!(registry.snapshot(8).sessions.len(), 8);
        assert_eq!(registry.snapshot(8).counts.running, 8);
    }

    #[test]
    fn waiting_sessions_cleared_when_new_run_starts() {
        let mut registry = StatusRegistry::new();
        // Session 1: agent runs, then stops (Waiting).
        registry.apply_event(
            event("1", Provider::Claude, "sess-1", AgentEventKind::Running, 100),
            30_000,
        );
        registry.apply_event(event("2", Provider::Claude, "sess-1", AgentEventKind::TurnFinished, 200), 30_000);
        assert_eq!(registry.snapshot(200).sessions.len(), 1);
        assert_eq!(
            registry.snapshot(200).sessions[0].phase,
            SessionPhase::Waiting
        );

        // Session 2 starts: old Waiting session should be cleared.
        registry.apply_event(
            event("3", Provider::Claude, "sess-2", AgentEventKind::Running, 300),
            30_000,
        );
        assert_eq!(registry.snapshot(300).sessions.len(), 1);
        assert_eq!(
            registry.snapshot(300).sessions[0].phase,
            SessionPhase::Running
        );
        assert_eq!(registry.snapshot(300).sessions[0].session_id, "sess-2");
    }

    #[test]
    fn waiting_sessions_not_cleared_by_other_provider() {
        let mut registry = StatusRegistry::new();
        // Claude session stops (Waiting).
        registry.apply_event(event("1", Provider::Claude, "c1", AgentEventKind::TurnFinished, 100), 30_000);
        assert_eq!(registry.snapshot(100).sessions.len(), 1);
        // Codex session starts: should NOT clear Claude's Waiting session.
        registry.apply_event(
            event("2", Provider::Codex, "x1", AgentEventKind::Running, 200),
            30_000,
        );
        assert_eq!(registry.snapshot(200).sessions.len(), 2);
    }

    // ── 子代理状态机 ──
    // 子代理事件与主会话共用 session_id，只靠 agent_id 区分。这批用例锁定
    // 「主线程停下后子代理仍算后台运行」以及「等用户操作优先于后台运行」两条契约。

    #[test]
    fn subagent_events_do_not_override_waiting() {
        let mut registry = StatusRegistry::new();
        // 主线程弹出提问/授权框 → 黄灯（AskUserQuestion 实测走 PermissionRequest）。
        registry.apply_event(
            event("1", Provider::Claude, "one", AgentEventKind::WaitingForUser, 100),
            30_000,
        );
        assert_eq!(
            registry.snapshot(100).sessions[0].phase,
            SessionPhase::Waiting
        );

        // 后台子代理仍在跑：它的工具事件与主会话共用 session_id。
        // 等用户作答/授权优先于「后台运行」：此时事件不得把黄灯顶回绿。
        registry.apply_event(
            subagent_event("2", Provider::Claude, "one", AgentEventKind::Running, 200),
            30_000,
        );
        registry.apply_event(
            subagent_event("3", Provider::Claude, "one", AgentEventKind::Running, 300),
            30_000,
        );
        registry.apply_event(
            subagent_event("4", Provider::Claude, "one", AgentEventKind::Running, 400),
            30_000,
        );

        let session = &registry.snapshot(400).sessions[0];
        assert_eq!(session.phase, SessionPhase::Waiting, "子代理事件不得把黄灯顶回绿");
        assert!(!session.is_flashing);
        assert_eq!(session.remove_at_ms, None);

        // 子代理结束也不能覆盖「等用户操作」：仍是黄灯。
        registry.apply_event(
            subagent_event("5", Provider::Claude, "one", AgentEventKind::SubagentStopped, 500),
            30_000,
        );
        assert_eq!(
            registry.snapshot(500).sessions[0].phase,
            SessionPhase::Waiting
        );
    }

    #[test]
    fn subagent_start_does_not_override_waiting_for_user() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", AgentEventKind::WaitingForUser, 100),
            30_000,
        );

        // 等用户操作时即使子代理开始，也必须保持黄灯。
        registry.apply_event(
            subagent_event("2", Provider::Claude, "one", AgentEventKind::SubagentStarted, 200),
            30_000,
        );
        let session = &registry.snapshot(200).sessions[0];
        assert_eq!(session.phase, SessionPhase::Waiting);
        assert!(!session.is_flashing);
        assert_eq!(session.remove_at_ms, None);

        // 即便随后收到主线程 Stop，等待用户操作的优先级仍然更高。
        registry.apply_event(
            event("3", Provider::Claude, "one", AgentEventKind::TurnFinished, 300),
            30_000,
        );
        let session = &registry.snapshot(300).sessions[0];
        assert_eq!(session.phase, SessionPhase::Waiting);
        assert!(!session.is_flashing);
        assert_eq!(session.remove_at_ms, None);
    }

    #[test]
    fn subagent_stop_does_not_override_running_main_thread() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", AgentEventKind::Running, 100),
            30_000,
        );
        registry.apply_event(
            subagent_event("2", Provider::Claude, "one", AgentEventKind::SubagentStarted, 200),
            30_000,
        );

        // 主线程仍在执行时，某个子代理结束不能把灯误落成黄灯。
        registry.apply_event(
            subagent_event("3", Provider::Claude, "one", AgentEventKind::SubagentStopped, 300),
            30_000,
        );
        let session = &registry.snapshot(300).sessions[0];
        assert_eq!(session.phase, SessionPhase::Running);
        assert!(!session.is_flashing);
        assert_eq!(session.remove_at_ms, None);
    }

    #[test]
    fn turn_finished_keeps_green_when_subagent_already_active() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", AgentEventKind::Running, 100),
            30_000,
        );
        registry.apply_event(
            subagent_event("2", Provider::Claude, "one", AgentEventKind::Running, 200),
            30_000,
        );

        // 如果只收到子代理工具事件、主线程后 Stop，Stop 也不能被当成「等 prompt」而落黄。
        registry.apply_event(
            event("3", Provider::Claude, "one", AgentEventKind::TurnFinished, 300),
            30_000,
        );
        let session = &registry.snapshot(300).sessions[0];
        assert_eq!(session.phase, SessionPhase::Running);
        assert!(!session.is_flashing);
        assert_eq!(session.remove_at_ms, None);
    }

    #[test]
    fn subagent_start_does_not_resurrect_failed_session() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", AgentEventKind::Failed, 100),
            30_000,
        );

        // 红灯不能被迟到的子代理生命周期事件顶回绿灯。
        registry.apply_event(
            subagent_event("2", Provider::Claude, "one", AgentEventKind::SubagentStarted, 200),
            30_000,
        );
        let session = &registry.snapshot(200).sessions[0];
        assert_eq!(session.phase, SessionPhase::Error);
        assert_eq!(session.last_seen_at_ms, 200);
    }

    #[test]
    fn stop_then_subagent_start_keeps_the_light_green() {
        let mut registry = StatusRegistry::new();
        // Stop：一轮结束，先绿闪、等待落黄。
        registry.apply_event(event("1", Provider::Claude, "one", AgentEventKind::TurnFinished, 100), 30_000);
        assert_eq!(registry.snapshot(100).sessions[0].remove_at_ms, Some(30_100));

        // 主线程 Stop 后子代理开始：后台仍在干活 → 回到绿常亮，并清掉 Stop 的落黄计时。
        registry.apply_event(
            subagent_event("2", Provider::Claude, "one", AgentEventKind::SubagentStarted, 200),
            30_000,
        );
        let session = &registry.snapshot(200).sessions[0];
        assert_eq!(session.phase, SessionPhase::Running, "子代理在跑时灯应保持绿色");
        assert!(!session.is_flashing);
        assert_eq!(session.remove_at_ms, None, "后台运行期间不该保留落黄计时");
    }

    #[test]
    fn last_subagent_stop_lands_on_waiting_without_flash() {
        let mut registry = StatusRegistry::new();
        // 主线程已 Stop（绿闪等待落黄），两个子代理并行在后台跑。
        registry.apply_event(event("1", Provider::Claude, "one", AgentEventKind::TurnFinished, 100), 30_000);
        let mut first = subagent_event("2", Provider::Claude, "one", AgentEventKind::SubagentStarted, 200);
        first.agent_id = Some("agent-1".to_string());
        registry.apply_event(first, 30_000);
        let mut second = subagent_event("3", Provider::Claude, "one", AgentEventKind::SubagentStarted, 300);
        second.agent_id = Some("agent-2".to_string());
        registry.apply_event(second, 30_000);

        // 只结束一个：仍有后台工作 → 继续绿。
        let mut stop_first = subagent_event("4", Provider::Claude, "one", AgentEventKind::SubagentStopped, 400);
        stop_first.agent_id = Some("agent-1".to_string());
        registry.apply_event(stop_first, 30_000);
        let session = &registry.snapshot(400).sessions[0];
        assert_eq!(session.phase, SessionPhase::Running);
        assert!(!session.is_flashing);
        assert_eq!(session.remove_at_ms, None);

        // 最后一个结束：立即落黄，且不重新绿闪、不留移除计时。
        let mut stop_second = subagent_event("5", Provider::Claude, "one", AgentEventKind::SubagentStopped, 500);
        stop_second.agent_id = Some("agent-2".to_string());
        registry.apply_event(stop_second, 30_000);
        let session = &registry.snapshot(500).sessions[0];
        assert_eq!(session.phase, SessionPhase::Waiting);
        assert!(!session.is_flashing, "最后一个子代理结束不应再绿闪");
        assert_eq!(session.remove_at_ms, None);
    }

    #[test]
    fn subagent_events_preserve_completed_removal_timer() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", AgentEventKind::SessionFinished, 100),
            30_000,
        );
        assert_eq!(
            registry.snapshot(100).sessions[0].remove_at_ms,
            Some(30_100)
        );

        // Completed 绿闪期间来子代理事件：移除计时必须保留，否则灯永远不消失。
        registry.apply_event(
            subagent_event("2", Provider::Claude, "one", AgentEventKind::Running, 200),
            30_000,
        );
        let session = &registry.snapshot(200).sessions[0];
        assert_eq!(session.phase, SessionPhase::Completed);
        assert_eq!(session.remove_at_ms, Some(30_100));

        assert!(registry.cleanup(30_100, DEFAULT_RUNNING_TIMEOUT_MS, |_| Some(false)));
        assert!(registry.snapshot(30_100).sessions.is_empty());
    }

    #[test]
    fn subagent_events_refresh_liveness_without_changing_phase() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", AgentEventKind::Running, 100),
            30_000,
        );
        // 子代理事件推进 last_seen：把 Running 超时窗口内本应到期的会话续住。
        registry.apply_event(
            subagent_event("2", Provider::Claude, "one", AgentEventKind::Running, 50_000),
            30_000,
        );
        let session = &registry.snapshot(50_000).sessions[0];
        assert_eq!(session.phase, SessionPhase::Running);
        assert_eq!(session.last_seen_at_ms, 50_000);

        // 以首次事件为准的 60s 超时点（100 + 60_000）上仍不该被清掉。
        let running_timeout = DEFAULT_RUNNING_TIMEOUT_MS;
        assert!(!registry.cleanup(100 + running_timeout + 1, running_timeout, |_| None));
        assert_eq!(registry.snapshot(100 + running_timeout + 1).sessions.len(), 1);
    }

    #[test]
    fn subagent_event_bootstraps_an_unseen_session_as_running() {
        let mut registry = StatusRegistry::new();
        // UFO 中途启动：只先收到子代理事件，也要把灯建出来（否则长时间没有灯）。
        registry.apply_event(
            subagent_event("1", Provider::Claude, "one", AgentEventKind::Running, 100),
            30_000,
        );
        let session = &registry.snapshot(100).sessions[0];
        assert_eq!(session.phase, SessionPhase::Running);
    }

    #[test]
    fn subagent_events_do_not_clear_other_waiting_sessions() {
        let mut registry = StatusRegistry::new();
        // 会话 A 进入 Waiting。
        registry.apply_event(event("1", Provider::Claude, "a", AgentEventKind::TurnFinished, 100), 30_000);
        // 会话 B 上只有子代理事件（UFO 中途启动的建档路径）：不得清掉 A 的黄灯。
        registry.apply_event(
            subagent_event("2", Provider::Claude, "b", AgentEventKind::Running, 200),
            30_000,
        );
        let snapshot = registry.snapshot(200);
        assert_eq!(snapshot.sessions.len(), 2);
        let a = snapshot
            .sessions
            .iter()
            .find(|session| session.session_id == "a")
            .expect("session a present");
        assert_eq!(a.phase, SessionPhase::Waiting);
    }

    /// 构造一条回填摘要（模拟从存档读出的历史值）。
    fn seed(started_at_ms: i64, prompt: Option<&str>, base_url: Option<&str>) -> StoredSeed {        StoredSeed {
            started_at_ms,
            prompt: prompt.map(str::to_string),
            base_url: base_url.map(str::to_string),
            tokens: TokenTotals::default(),
        }
    }

    #[test]
    fn seed_restores_started_prompt_and_base_url_on_new_session() {
        let mut registry = StatusRegistry::new();
        // 本次运行第一次见到该会话（如 --resume）：PreToolUse 不带 prompt/base_url。
        let applied = registry.apply_event_with_seed(
            event("1", Provider::Claude, "one", AgentEventKind::Running, 500),
            30_000,
            Some(&seed(100, Some("旧提问"), Some("http://127.0.0.1:15721"))),
        );
        assert!(applied);
        let session = &registry.snapshot(500).sessions[0];
        // started_at 回到存档值（灯位不跳），prompt/base_url 从回填续上。
        assert_eq!(session.started_at_ms, 100);
        assert_eq!(session.prompt.as_deref(), Some("旧提问"));
        assert_eq!(
            session.base_url.as_deref(),
            Some("http://127.0.0.1:15721")
        );
    }

    #[test]
    fn seed_does_not_override_a_live_session() {
        let mut registry = StatusRegistry::new();
        // 会话已在内存中（存活），回填不得覆盖新鲜值。
        registry.apply_event(
            event("1", Provider::Claude, "one", AgentEventKind::Running, 500),
            30_000,
        );
        registry.apply_event_with_seed(
            {
                let mut e = event("2", Provider::Claude, "one", AgentEventKind::Running, 600);
                e.prompt = None;
                e
            },
            30_000,
            Some(&seed(100, Some("陈旧提问"), Some("http://stale"))),
        );
        let session = &registry.snapshot(600).sessions[0];
        // started_at 仍是内存里的 500，不是存档的 100；prompt 也不被陈旧值污染。
        assert_eq!(session.started_at_ms, 500);
        assert_eq!(session.prompt, None);
        assert_eq!(session.base_url, None);
    }

    #[test]
    fn user_prompt_submit_overrides_restored_prompt() {
        let mut registry = StatusRegistry::new();
        registry.apply_event_with_seed(
            event("1", Provider::Claude, "one", AgentEventKind::Running, 500),
            30_000,
            Some(&seed(100, Some("旧提问"), None)),
        );
        // 新一次提交：覆盖回填来的旧提问（阶梯「事件 > 前驱 > 回填」）。
        let mut submit = event("2", Provider::Claude, "one", AgentEventKind::Running, 600);
        submit.prompt = Some("新提问".to_string());
        registry.apply_event_with_seed(submit, 30_000, None);
        assert_eq!(
            registry.snapshot(600).sessions[0].prompt.as_deref(),
            Some("新提问")
        );
    }

    #[test]
    fn seed_token_floor_is_seeded_into_totals() {
        let mut registry = StatusRegistry::new();
        let mut historic = seed(100, None, None);
        historic.tokens = TokenTotals {
            input_tokens: 1000,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 500,
            output_tokens: 200,
        };
        registry.apply_event_with_seed(
            event("1", Provider::Claude, "one", AgentEventKind::Running, 500),
            30_000,
            Some(&historic),
        );
        // 重扫完成前，快照就已带上历史 token（地板）。
        let session = &registry.snapshot(500).sessions[0];
        let totals = session.tokens.expect("seeded floor present");
        assert_eq!(totals.input_tokens, 1000);
        assert_eq!(totals.cache_read_input_tokens, 500);
    }

    #[test]
    fn zero_token_seed_does_not_create_a_zero_entry() {
        let mut registry = StatusRegistry::new();
        // 全零的历史 token 不应被灌进 totals —— 否则会把「未统计到」的 None 变成 Some(0)。
        registry.apply_event_with_seed(
            event("1", Provider::Claude, "one", AgentEventKind::Running, 500),
            30_000,
            Some(&seed(100, None, None)),
        );
        assert_eq!(registry.snapshot(500).sessions[0].tokens, None);
    }

    #[test]
    fn set_tokens_creates_entry_when_absent() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", AgentEventKind::Running, 100),
            30_000,
        );
        let totals = TokenTotals {
            input_tokens: 10,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
            output_tokens: 5,
        };
        assert!(registry.set_tokens("claude:one", totals));
        // 同值再写一次：无变化，返回 false 且不推进 revision。
        let revision = registry.snapshot(100).revision;
        assert!(!registry.set_tokens("claude:one", totals));
        assert_eq!(registry.snapshot(100).revision, revision);
    }

    #[test]
    fn set_tokens_never_regresses_below_the_floor() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", AgentEventKind::Running, 100),
            30_000,
        );
        registry.set_tokens(
            "claude:one",
            TokenTotals {
                input_tokens: 1000,
                cache_creation_input_tokens: 0,
                cache_read_input_tokens: 0,
                output_tokens: 0,
            },
        );
        // transcript 被截断后重扫：input 变小（200<1000）但 output 变大（50>0）。
        // 逐字段 max → input 保住地板 1000、output 增长到 50；确有变化所以返回 true。
        assert!(registry.set_tokens(
            "claude:one",
            TokenTotals {
                input_tokens: 200,
                cache_creation_input_tokens: 0,
                cache_read_input_tokens: 0,
                output_tokens: 50,
            }
        ));
        let totals = registry.snapshot(100).sessions[0].tokens.expect("present");
        assert_eq!(totals.input_tokens, 1000, "地板不得回退");
        assert_eq!(totals.output_tokens, 50, "更大的字段仍应增长");

        // 再刷同一批（已被 max 吸收成 1000/0/0/50）：合并后与现值相等 → 无变化、不白 bump revision。
        let revision = registry.snapshot(100).revision;
        assert!(!registry.set_tokens(
            "claude:one",
            TokenTotals {
                input_tokens: 200,
                cache_creation_input_tokens: 0,
                cache_read_input_tokens: 0,
                output_tokens: 50,
            }
        ));
        assert_eq!(registry.snapshot(100).revision, revision);
    }

    #[test]
    fn set_tokens_ignores_sessions_that_are_gone() {
        let mut registry = StatusRegistry::new();
        // 内存里没有该会话 → 不留孤儿 token 记录。
        assert!(!registry.set_tokens(
            "claude:ghost",
            TokenTotals {
                input_tokens: 1,
                ..Default::default()
            }
        ));
    }

    // ── 回归：跨会话清理、子代理归属、计时副作用、去重窗口 ──

    /// 取快照里某个会话（按 session_id），不存在就 panic。
    fn session_of(registry: &StatusRegistry, session_id: &str) -> SessionSnapshot {
        registry
            .snapshot(0)
            .sessions
            .into_iter()
            .find(|session| session.session_id == session_id)
            .unwrap_or_else(|| panic!("session {session_id} present"))
    }

    /// 主线程开始新一轮时，只清同 provider 的**空闲黄灯** —— 不能抹掉别的会话的审批等待。
    #[test]
    fn new_run_does_not_clear_another_sessions_permission_wait() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "a", AgentEventKind::WaitingForUser, 100),
            30_000,
        );
        let mut approval = event("2", Provider::Claude, "a", AgentEventKind::WaitingForUser, 120);
        approval.waiting_reason = Some("是否允许我访问官网".to_string());
        registry.apply_event(approval, 30_000);

        // 会话 B 主线程开始新一轮。
        registry.apply_event(
            event("3", Provider::Claude, "b", AgentEventKind::Running, 200),
            30_000,
        );

        let snapshot = registry.snapshot(200);
        assert_eq!(snapshot.sessions.len(), 2, "审批等待不得被新一轮清掉");
        let a = session_of(&registry, "a");
        assert_eq!(a.phase, SessionPhase::Waiting);
        // 私有状态与理由也都要原样保留。
        assert_eq!(
            registry.state.get("claude:a"),
            Some(&SessionState::WaitingForUser)
        );
        assert_eq!(a.waiting_reason.as_deref(), Some("是否允许我访问官网"));
    }

    /// 主线程 Stop 之后子代理仍在跑时对外也是 Running —— 但它**不是**「新一轮开始」，
    /// 不能顺手清掉别的会话的灯。
    #[test]
    fn background_running_does_not_clear_other_waiting_sessions() {
        let mut registry = StatusRegistry::new();
        // 会话 B：主线程开始新一轮，随后子代理在后台跑、主线程 Stop。
        registry.apply_event(
            event("1", Provider::Claude, "b", AgentEventKind::Running, 200),
            30_000,
        );
        // 会话 A：一轮跑完 → 空闲黄灯（保留到下一条指令）。
        registry.apply_event(
            event("2", Provider::Claude, "a", AgentEventKind::TurnFinished, 300),
            30_000,
        );
        registry.apply_event(
            subagent_event("3", Provider::Claude, "b", AgentEventKind::Running, 400),
            30_000,
        );
        registry.apply_event(
            event("4", Provider::Claude, "b", AgentEventKind::TurnFinished, 500),
            30_000,
        );
        assert_eq!(
            session_of(&registry, "b").phase,
            SessionPhase::Running,
            "后台还有子代理时对外仍是绿"
        );
        assert_eq!(
            registry.snapshot(500).sessions.len(),
            2,
            "后台运行不是「新一轮开始」，不得抹掉别的会话"
        );

        let a = session_of(&registry, "a");
        assert_eq!(a.phase, SessionPhase::Waiting, "别人的灯不能被后台运行抹掉");
        assert!(a.is_flashing, "A 的绿闪计时也要原样保留");
        assert_eq!(a.remove_at_ms, Some(30_300));
    }

    /// 子代理的工具失败（带 `agent_id` 的 `Failed`）只代表那个执行者出事：
    /// 不得把父会话打成红灯，也不得抹掉其它仍在跑的子代理。
    #[test]
    fn subagent_failure_does_not_redden_the_parent_session() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", AgentEventKind::Running, 100),
            30_000,
        );
        for (id, name, at) in [("agent-1", "2", 200), ("agent-2", "3", 300)] {
            let mut started =
                subagent_event(name, Provider::Claude, "one", AgentEventKind::Running, at);
            started.agent_id = Some(id.to_string());
            registry.apply_event(started, 30_000);
        }

        // agent-1 的工具报错（带错误正文）→ 它自己收工，父会话不受影响。
        let mut failed = subagent_event("4", Provider::Claude, "one", AgentEventKind::Failed, 400);
        failed.agent_id = Some("agent-1".to_string());
        failed.error_code = Some("tool blew up".to_string());
        registry.apply_event(failed, 30_000);

        let session = session_of(&registry, "one");
        assert_eq!(session.phase, SessionPhase::Running, "子代理出错不是父会话的错误");
        assert_eq!(session.error_code, None);
        assert_eq!(
            registry.subagents.get("claude:one").map(HashSet::len),
            Some(1),
            "另一个子代理仍在跑，不能被一起清掉"
        );

        // 主线程随后 Stop：后台还有活 → 保持绿，不落黄。
        registry.apply_event(
            event("5", Provider::Claude, "one", AgentEventKind::TurnFinished, 500),
            30_000,
        );
        assert_eq!(session_of(&registry, "one").phase, SessionPhase::Running);
    }

    /// 主线程出错期间与之后，后台子代理的生命周期事实都要如实保留。
    #[test]
    fn main_thread_error_keeps_background_subagent_facts() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", AgentEventKind::Running, 100),
            30_000,
        );
        registry.apply_event(
            subagent_event("2", Provider::Claude, "one", AgentEventKind::SubagentStarted, 200),
            30_000,
        );
        // 主线程本轮出错 → 红灯（子代理活动不得把红灯顶掉）。
        registry.apply_event(
            event("3", Provider::Claude, "one", AgentEventKind::Failed, 300),
            30_000,
        );
        assert_eq!(session_of(&registry, "one").phase, SessionPhase::Error);

        // 恢复：主线程继续跑，随后一轮结束 —— 后台还有活，灯保持绿而不是落黄。
        registry.apply_event(
            event("4", Provider::Claude, "one", AgentEventKind::Running, 400),
            30_000,
        );
        registry.apply_event(
            event("5", Provider::Claude, "one", AgentEventKind::TurnFinished, 500),
            30_000,
        );
        assert_eq!(
            session_of(&registry, "one").phase,
            SessionPhase::Running,
            "错误期间没丢掉「后台仍在跑」的事实"
        );

        // 后台收尾：这时才落回空闲黄。
        registry.apply_event(
            subagent_event("6", Provider::Claude, "one", AgentEventKind::SubagentStopped, 600),
            30_000,
        );
        let session = session_of(&registry, "one");
        assert_eq!(session.phase, SessionPhase::Waiting);
        assert!(!session.is_flashing);
    }

    /// 错误期间的后台收尾同样要被记下来：红灯结束、主线程恢复后不该还当作后台在跑。
    #[test]
    fn subagent_stop_during_an_error_is_still_recorded() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", AgentEventKind::Running, 100),
            30_000,
        );
        registry.apply_event(
            subagent_event("2", Provider::Claude, "one", AgentEventKind::SubagentStarted, 200),
            30_000,
        );
        registry.apply_event(
            event("3", Provider::Claude, "one", AgentEventKind::Failed, 300),
            30_000,
        );
        registry.apply_event(
            subagent_event("4", Provider::Claude, "one", AgentEventKind::SubagentStopped, 400),
            30_000,
        );
        assert_eq!(session_of(&registry, "one").phase, SessionPhase::Error);

        registry.apply_event(
            event("5", Provider::Claude, "one", AgentEventKind::Running, 500),
            30_000,
        );
        registry.apply_event(
            event("6", Provider::Claude, "one", AgentEventKind::TurnFinished, 600),
            30_000,
        );
        let session = session_of(&registry, "one");
        assert_eq!(
            session.phase,
            SessionPhase::Waiting,
            "后台已收尾：一轮结束就该落黄"
        );
        assert!(session.is_flashing);
    }

    /// 冷启动只见子代理活动：起点是**后台运行**，最后一个子代理收尾要能落回黄灯。
    #[test]
    fn cold_start_subagent_activity_lands_on_waiting_when_it_stops() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            subagent_event("1", Provider::Claude, "one", AgentEventKind::Running, 100),
            30_000,
        );
        assert_eq!(session_of(&registry, "one").phase, SessionPhase::Running);

        registry.apply_event(
            subagent_event("2", Provider::Claude, "one", AgentEventKind::SubagentStopped, 200),
            30_000,
        );
        let session = session_of(&registry, "one");
        assert_eq!(session.phase, SessionPhase::Waiting, "后台收尾后不能一直绿");
        assert!(!session.is_flashing, "收尾不该再补一次绿闪");
        assert_eq!(session.remove_at_ms, None);
    }

    /// 无法识别的事件不构成转灯信号：Stop 的绿闪计时必须原样保留。
    #[test]
    fn unknown_event_preserves_the_stop_flash_timer() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", AgentEventKind::TurnFinished, 100),
            30_000,
        );
        registry.apply_event(
            event("2", Provider::Claude, "one", AgentEventKind::Unknown, 200),
            30_000,
        );
        let session = session_of(&registry, "one");
        assert_eq!(session.phase, SessionPhase::Waiting);
        assert!(session.is_flashing, "Unknown 事件不得把绿闪掐掉");
        assert_eq!(session.remove_at_ms, Some(30_100));

        // 到期仍会正常落成黄常亮。
        assert!(registry.cleanup(30_100, DEFAULT_RUNNING_TIMEOUT_MS, |_| Some(true)));
        assert!(!session_of(&registry, "one").is_flashing);
    }

    /// 子代理序号属于**自己的序列**：不与主线程的水位互相比较，也不覆盖对外的主线程序号。
    #[test]
    fn subagent_sequence_is_compared_within_its_own_stream() {
        let mut registry = StatusRegistry::new();
        let mut main = event("1", Provider::Claude, "one", AgentEventKind::Running, 100);
        main.sequence = Some(10);
        registry.apply_event(main, 30_000);

        // 子代理的工具事件从自己的序号 1 开始：不得被主线程的水位 10 挡掉。
        let mut subagent = subagent_event("2", Provider::Claude, "one", AgentEventKind::Running, 200);
        subagent.sequence = Some(1);
        assert!(registry.apply_event(subagent, 30_000));

        let session = session_of(&registry, "one");
        assert_eq!(session.last_seen_at_ms, 200);
        assert_eq!(session.last_sequence, Some(10), "对外只暴露主线程的序号");
    }

    /// 乱序的子代理生命周期：已收尾的执行者不会被**更早**的开工事件重新唤醒。
    #[test]
    fn stale_subagent_start_does_not_resurrect_a_stopped_agent() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("0", Provider::Claude, "one", AgentEventKind::TurnFinished, 50),
            30_000,
        );
        let mut start =
            subagent_event("1", Provider::Claude, "one", AgentEventKind::SubagentStarted, 100);
        start.agent_id = Some("agent-1".to_string());
        start.sequence = Some(2);
        registry.apply_event(start, 30_000);

        let mut stop =
            subagent_event("2", Provider::Claude, "one", AgentEventKind::SubagentStopped, 200);
        stop.agent_id = Some("agent-1".to_string());
        stop.sequence = Some(3);
        registry.apply_event(stop, 30_000);
        assert_eq!(session_of(&registry, "one").phase, SessionPhase::Waiting);

        // 迟到的旧开工（序号 2 ≤ 已见的 3）：丢弃，否则灯会莫名又变绿。
        let mut stale = subagent_event("3", Provider::Claude, "one", AgentEventKind::Running, 300);
        stale.agent_id = Some("agent-1".to_string());
        stale.sequence = Some(2);
        assert!(!registry.apply_event(stale, 30_000));
        assert_eq!(session_of(&registry, "one").phase, SessionPhase::Waiting);
    }

    /// 不永久拉黑已停下的 agent_id：较晚的合法恢复必须能重新激活。
    #[test]
    fn a_later_legitimate_subagent_start_reactivates_the_agent() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("0", Provider::Claude, "one", AgentEventKind::TurnFinished, 50),
            30_000,
        );
        let mut start =
            subagent_event("1", Provider::Claude, "one", AgentEventKind::SubagentStarted, 100);
        start.agent_id = Some("agent-1".to_string());
        start.sequence = Some(2);
        registry.apply_event(start, 30_000);

        let mut stop =
            subagent_event("2", Provider::Claude, "one", AgentEventKind::SubagentStopped, 200);
        stop.agent_id = Some("agent-1".to_string());
        stop.sequence = Some(3);
        registry.apply_event(stop, 30_000);
        assert_eq!(session_of(&registry, "one").phase, SessionPhase::Waiting);

        // 同一个 agent_id 之后的合法开工（序号更新）：重新入集合 → 回到后台运行。
        let mut restart = subagent_event("3", Provider::Claude, "one", AgentEventKind::Running, 300);
        restart.agent_id = Some("agent-1".to_string());
        restart.sequence = Some(4);
        assert!(registry.apply_event(restart, 30_000));
        assert_eq!(session_of(&registry, "one").phase, SessionPhase::Running);
    }

    /// 去重窗口有界：装满后按 FIFO 淘汰最旧的，且重复命中不把条目推回队尾。
    #[test]
    fn seen_event_window_is_bounded_and_does_not_refresh_on_duplicates() {
        let mut registry = StatusRegistry::new();
        // 用 Unknown 事件填满窗口：它不驱灯色，避免顺带制造几千个会话状态。
        for index in 0..SEEN_EVENTS_CAPACITY {
            let id = format!("bulk-{index}");
            registry.apply_event(
                event(&id, Provider::Claude, &id, AgentEventKind::Unknown, 1_000),
                30_000,
            );
        }
        assert_eq!(registry.seen_events.len(), SEEN_EVENTS_CAPACITY);
        assert_eq!(registry.seen_order.len(), SEEN_EVENTS_CAPACITY);

        // 重复命中：不增长、也不改变队列顺序（FIFO 不是 LRU）。
        assert!(!registry.apply_event(
            event("bulk-0", Provider::Claude, "bulk-0", AgentEventKind::Unknown, 1_000),
            30_000
        ));
        assert_eq!(registry.seen_order.len(), SEEN_EVENTS_CAPACITY);
        assert_eq!(
            registry.seen_order.front().map(String::as_str),
            Some("claude:bulk-0:bulk-0")
        );

        // 再来一条新的：淘汰最旧的，容量不变。
        registry.apply_event(
            event("fresh", Provider::Claude, "fresh", AgentEventKind::Unknown, 1_000),
            30_000,
        );
        assert_eq!(registry.seen_events.len(), SEEN_EVENTS_CAPACITY);
        assert!(
            !registry.seen_events.contains("claude:bulk-0:bulk-0"),
            "最旧的一条被淘汰"
        );
        assert!(registry.seen_events.contains("claude:fresh:fresh"));
    }

    /// 会话被删掉后，窗口内重放它的事件不会凭空把灯点回来。
    #[test]
    fn replaying_events_of_a_removed_session_does_not_relight() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", AgentEventKind::Running, 100),
            30_000,
        );
        assert!(registry.dismiss_session("claude:one"));
        assert!(registry.snapshot(100).sessions.is_empty());

        // 同一条事件再消费一次（文件重复投递 / 存档回放）→ 被窗口挡住。
        assert!(!registry.apply_event(
            event("1", Provider::Claude, "one", AgentEventKind::Running, 200),
            30_000
        ));
        assert!(registry.snapshot(200).sessions.is_empty());
    }

    /// 删除会话要把私有记录一起丢掉：同一个 key 再次出现时不能读到上一世的状态。
    #[test]
    fn dismissing_a_session_forgets_its_private_state() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", AgentEventKind::WaitingForUser, 100),
            30_000,
        );
        registry.apply_event(
            subagent_event("2", Provider::Claude, "one", AgentEventKind::Running, 150),
            30_000,
        );
        assert!(registry.dismiss_session("claude:one"));
        assert!(!registry.state.contains_key("claude:one"));
        assert!(!registry.subagents.contains_key("claude:one"));
        assert!(!registry.sequence_watermarks.contains_key("claude:one"));
    }

    /// 跨会话清理触发时，被清掉的会话的私有记录同样要一起消失。
    #[test]
    fn clearing_idle_waiting_sessions_drops_their_private_state() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "a", AgentEventKind::TurnFinished, 100),
            30_000,
        );
        assert!(registry.state.contains_key("claude:a"));

        registry.apply_event(
            event("2", Provider::Claude, "b", AgentEventKind::Running, 200),
            30_000,
        );
        assert_eq!(registry.snapshot(200).sessions.len(), 1);
        assert!(!registry.state.contains_key("claude:a"));
        assert!(!registry.token_totals.contains_key("claude:a"));
    }
}
