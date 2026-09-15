use std::collections::{HashMap, HashSet};

use super::model::{
    session_key, AgentEvent, SessionPhase, SessionSnapshot, StatusCounts, StatusSnapshot,
    StatusSource, StoredSeed, TokenTotals, SCHEMA_VERSION,
};

pub struct StatusRegistry {
    sessions: HashMap<String, SessionSnapshot>,
    /// 各会话累计的 token；由 monitor 的 TokenTracker 写入。
    /// 它不是事件派生字段，所以在 snapshot() 时合并进 SessionSnapshot。
    token_totals: HashMap<String, TokenTotals>,
    seen_events: HashSet<String>,
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
            token_totals: HashMap::new(),
            seen_events: HashSet::new(),
            revision: 0,
            source_health: "healthy".to_string(),
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
            || !self.seen_events.insert(event_key)
        {
            return false;
        }

        let now = event.received_at_ms;
        let key = session_key(event.provider, &event.session_id);

        // Extract previous session data before any mutable borrow.
        let (
            previous_phase,
            previous_started_at,
            previous_pid,
            previous_error_code,
            previous_base_url,
            previous_prompt,
        ) = match self.sessions.get(&key) {
            Some(previous) => {
                if event
                    .sequence
                    .zip(previous.last_sequence)
                    .is_some_and(|(incoming, last)| incoming <= last)
                {
                    return false;
                }
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
                )
            }
            None => (None, None, None, None, None, None),
        };

        // 红灯不粘：事件映射出的阶段即最终阶段。
        // 任何能转灯的事件（Stop→黄、SessionEnd→绿闪、工具事件→绿等）都允许把红灯带走；
        // 唯一例外是 Unknown —— 无法识别的事件不构成转灯信号，保持原红灯。
        let mapped_phase = phase_for_event(&event);
        let phase = if previous_phase == Some(SessionPhase::Error)
            && mapped_phase == SessionPhase::Unknown
        {
            SessionPhase::Error
        } else {
            mapped_phase
        };

        // When a new run starts (Running phase), clear old Waiting sessions
        // for the same provider. This implements "completed state stays until next run".
        if phase == SessionPhase::Running {
            let provider = event.provider;
            let current_session = &event.session_id;
            self.sessions.retain(|_, session| {
                !(session.provider == provider
                    && session.phase == SessionPhase::Waiting
                    && session.session_id != *current_session)
            });
        }

        // 回填只在「内存里没有该会话」时启用（previous_started_at 为该分支恒 None 的判据，
        // 上面 match 已经查过一次 self.sessions.get(&key)，不再多查一次哈希）。
        let seed = seed.filter(|_| previous_started_at.is_none());

        let started_at_ms = previous_started_at
            // 本次运行没见过这个会话 → 用存档里的创建时刻：--resume 的会话因此保住原灯位
            // （snapshot() 按 started_at_ms 升序排指示灯）。
            .or_else(|| seed.map(|seed| seed.started_at_ms))
            .unwrap_or(now);
        let event_name = normalize_event_name(&event.event);
        // Stop：一轮执行完成 → Waiting，但先绿闪，retention 后落成黄常亮。
        // SessionEnd：Completed，绿闪后移除。
        // 其它 Waiting（权限/Elicitation 等）：直接黄灯，不闪。
        let is_stop_turn = event_name == "stop";
        let flash_deadline = if retention_ms > 0
            && (phase == SessionPhase::Completed
                || (phase == SessionPhase::Waiting && is_stop_turn))
        {
            Some(now.saturating_add(retention_ms.min(i64::MAX as u64) as i64))
        } else {
            None
        };
        let completed_at_ms = if phase == SessionPhase::Completed {
            Some(now)
        } else {
            None
        };
        // Completed：remove_at = 移除时间；Waiting+Stop：remove_at = 绿闪结束时间（不移除）。
        let remove_at_ms = flash_deadline;

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
                completed_at_ms,
                remove_at_ms,
                is_flashing: flash_deadline.is_some(),
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
                // prompt 同理：只有 UserPromptSubmit 携带，同轮内的后续事件继承，
                // 直到下一次提交才被覆盖。第三档回填让 resume 的会话立刻有「最近提问」。
                prompt: event
                    .prompt
                    .or(previous_prompt)
                    .or_else(|| seed.and_then(|seed| seed.prompt.clone())),
                // token 不是事件派生字段，由 snapshot() 从 token_totals 合并进来。
                tokens: None,
                last_sequence: event.sequence,
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

        // 会话消失后同步丢弃其 token 记录，避免长期运行下无限增长。
        let live: HashSet<String> = self.sessions.keys().cloned().collect();
        self.token_totals.retain(|key, _| live.contains(key));

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

/// 将 hook 事件映射为会话阶段。
///
/// 灯色语义：
/// - Running → 绿常亮（正在执行）
/// - Waiting → 黄灯（等待用户操作，对话未结束）
///   - 其中 Stop 会额外先绿闪，retention 后落成黄常亮（见 apply_event / cleanup）
/// - Error → 红灯（接口/不可抗力错误）
/// - Completed → 绿闪后移除（对话已完成，非错误结束）
fn phase_for_event(event: &AgentEvent) -> SessionPhase {
    // Error from payload fields takes priority regardless of event name.
    // SessionEnd.reason（clear/resume/...）不是失败 outcome，不会命中这里。
    if event.error_code.is_some()
        || event.outcome.as_deref().is_some_and(|outcome| {
            matches!(
                outcome.to_ascii_lowercase().as_str(),
                "error" | "failed" | "failure" | "cancelled" | "denied"
            )
        })
    {
        return SessionPhase::Error;
    }

    let event_name = normalize_event_name(&event.event);
    match event_name.as_str() {
        // ── 正在执行（绿常亮） ──
        "session_start" | "session_started" | "setup" | "user_prompt_submit"
        | "user_prompt_expansion" | "pre_tool_use" | "post_tool_use" | "post_tool_use_failure"
        | "post_tool_batch" | "subagent_start" | "subagent_stop" | "task_created"
        | "message_display" | "pre_compact" | "post_compact" | "pre_model_switch"
        | "post_model_switch" | "elicit_result" | "elicitation_result"
        | "instructions_loaded" | "config_change" | "cwd_changed" | "directory_added"
        | "file_changed" | "worktree_create" | "worktree_remove"
        // 非「等人操作」信号，保持执行链路语义
        | "permission_denied" | "task_completed" | "teammate_idle" => SessionPhase::Running,

        // ── 等待用户操作（黄灯） ──
        // Stop：一轮回复结束，等下一条 prompt
        // PermissionRequest / Elicitation：弹窗等人选择或输入
        "stop" | "permission_request" | "elicit" | "elicitation" => SessionPhase::Waiting,

        // Notification：仅等待类 notification_type 进黄灯，其它保持 Running
        "notification" => {
            if is_waiting_notification(event.notification_type.as_deref()) {
                SessionPhase::Waiting
            } else {
                SessionPhase::Running
            }
        }

        // ── 对话结束（绿闪）；SessionEnd 只有 reason，不再按 success 分流 ──
        "session_end" | "session_ended" | "session_completed" | "session_done" => {
            SessionPhase::Completed
        }

        // ── 本轮 API/模型错误（红灯）；会话通常仍可继续 ──
        "stop_failure" => SessionPhase::Error,

        // ── Fallback for unknown events ──
        _ => {
            if event_name.contains("error") || event_name.contains("fail") {
                return SessionPhase::Error;
            }
            if event_name.contains("start") || event_name.contains("tool") {
                return SessionPhase::Running;
            }
            if event_name.contains("stop") || event_name.contains("wait") {
                return SessionPhase::Waiting;
            }
            SessionPhase::Unknown
        }
    }
}

/// 判断 Notification 是否属于「等待用户操作」类型。
fn is_waiting_notification(notification_type: Option<&str>) -> bool {
    notification_type
        .map(|value| value.to_ascii_lowercase())
        .is_some_and(|value| {
            matches!(
                value.as_str(),
                "idle_prompt"
                    | "permission_prompt"
                    | "elicitation_dialog"
                    | "agent_needs_input"
            )
        })
}

pub(super) fn normalize_event_name(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len() + 4);
    let mut prev = '\0';
    for character in value.chars() {
        if character.is_ascii_uppercase() && prev.is_ascii_lowercase() {
            normalized.push('_');
        }
        if matches!(character, '-' | '.' | ':') {
            normalized.push('_');
        } else {
            normalized.push(character.to_ascii_lowercase());
        }
        prev = character;
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::model::Provider;

    fn event(
        id: &str,
        provider: Provider,
        session_id: &str,
        name: &str,
        received: i64,
    ) -> AgentEvent {
        AgentEvent {
            schema_version: SCHEMA_VERSION,
            event_id: id.to_string(),
            provider,
            session_id: session_id.to_string(),
            event: name.to_string(),
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
        }
    }

    /// 构造带 notification_type 的事件，用于 Notification 分流测试。
    fn notification_event(
        id: &str,
        session_id: &str,
        notification_type: Option<&str>,
        received: i64,
    ) -> AgentEvent {
        let mut event = event(id, Provider::Claude, session_id, "Notification", received);
        event.notification_type = notification_type.map(str::to_string);
        event
    }

    #[test]
    fn keeps_multiple_provider_sessions_separate() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(event("1", Provider::Claude, "same", "start", 1), 30_000);
        registry.apply_event(event("2", Provider::Codex, "same", "start", 2), 30_000);

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
        registry.apply_event(event("1", Provider::Claude, "s1", "start", 100), 30_000);
        registry.apply_event(event("2", Provider::Claude, "s2", "start", 200), 30_000);
        registry.apply_event(event("3", Provider::Claude, "s3", "start", 300), 30_000);
        // s1 收到新事件：last_seen 最新，但仍应排在最前（按 started 而非 last_seen）。
        registry.apply_event(
            event("4", Provider::Claude, "s1", "PreToolUse", 999),
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
            event("1", Provider::Claude, "one", "session_end", 100),
            30_000,
        );

        assert!(!registry.cleanup(30_099, DEFAULT_RUNNING_TIMEOUT_MS, |_| Some(false)));
        assert!(registry.cleanup(30_100, DEFAULT_RUNNING_TIMEOUT_MS, |_| Some(false)));
        assert!(registry.snapshot(30_100).sessions.is_empty());
    }

    #[test]
    fn dead_error_sessions_are_removed_by_liveness_check() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(event("1", Provider::Claude, "one", "error", 100), 30_000);

        assert!(registry.cleanup(200, DEFAULT_RUNNING_TIMEOUT_MS, |_| Some(false)));
        assert!(registry.snapshot(200).sessions.is_empty());
    }

    #[test]
    fn stop_means_waiting_with_green_flash_then_settles_yellow() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(event("1", Provider::Claude, "one", "Stop", 100), 30_000);
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
            event("1", Provider::Claude, "one", "PermissionRequest", 100),
            30_000,
        );
        let session = &registry.snapshot(100).sessions[0];
        assert_eq!(session.phase, SessionPhase::Waiting);
        assert!(!session.is_flashing);
        assert_eq!(session.remove_at_ms, None);
    }

    #[test]
    fn permission_request_and_elicitation_mean_waiting() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", "PermissionRequest", 100),
            30_000,
        );
        assert_eq!(
            registry.snapshot(100).sessions[0].phase,
            SessionPhase::Waiting
        );

        registry.apply_event(
            event("2", Provider::Claude, "one", "Elicitation", 200),
            30_000,
        );
        assert_eq!(
            registry.snapshot(200).sessions[0].phase,
            SessionPhase::Waiting
        );
    }

    #[test]
    fn waiting_notification_types_map_to_waiting() {
        let mut registry = StatusRegistry::new();
        for (idx, notification_type) in [
            "idle_prompt",
            "permission_prompt",
            "elicitation_dialog",
            "agent_needs_input",
        ]
        .into_iter()
        .enumerate()
        {
            let received = 100 + idx as i64;
            registry.apply_event(
                notification_event(
                    &format!("n-{idx}"),
                    &format!("sess-{idx}"),
                    Some(notification_type),
                    received,
                ),
                30_000,
            );
            let session = registry
                .snapshot(received)
                .sessions
                .into_iter()
                .find(|session| session.session_id == format!("sess-{idx}"))
                .expect("session present");
            assert_eq!(session.phase, SessionPhase::Waiting);
        }
    }

    #[test]
    fn non_waiting_or_missing_notification_type_maps_to_running() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            notification_event("1", "one", Some("auth_success"), 100),
            30_000,
        );
        assert_eq!(
            registry.snapshot(100).sessions[0].phase,
            SessionPhase::Running
        );

        registry.apply_event(notification_event("2", "two", None, 200), 30_000);
        assert_eq!(
            registry.snapshot(200).sessions[0].phase,
            SessionPhase::Running
        );
    }

    #[test]
    fn permission_denied_and_task_completed_stay_running() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", "PermissionDenied", 100),
            30_000,
        );
        assert_eq!(
            registry.snapshot(100).sessions[0].phase,
            SessionPhase::Running
        );

        registry.apply_event(
            event("2", Provider::Claude, "one", "TaskCompleted", 200),
            30_000,
        );
        assert_eq!(
            registry.snapshot(200).sessions[0].phase,
            SessionPhase::Running
        );
    }

    #[test]
    fn session_end_is_the_completed_signal_regardless_of_reason() {
        let mut registry = StatusRegistry::new();
        let mut end = event("1", Provider::Claude, "one", "SessionEnd", 100);
        // SDK SessionEnd 只有 reason（如 clear），没有 success outcome。
        end.outcome = Some("clear".to_string());
        registry.apply_event(end, 30_000);
        let session = &registry.snapshot(100).sessions[0];
        assert_eq!(session.phase, SessionPhase::Completed);
        assert!(session.is_flashing);
        assert_eq!(session.remove_at_ms, Some(30_100));
    }

    #[test]
    fn event_ids_are_scoped_to_the_session() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(event("same", Provider::Claude, "one", "start", 1), 30_000);
        registry.apply_event(event("same", Provider::Claude, "two", "start", 2), 30_000);
        assert_eq!(registry.snapshot(2).sessions.len(), 2);
    }

    #[test]
    fn stale_sequences_and_completed_sessions_are_ignored() {
        let mut registry = StatusRegistry::new();
        let mut first = event("1", Provider::Claude, "one", "start", 100);
        first.sequence = Some(2);
        registry.apply_event(first, 30_000);

        let mut stale = event("2", Provider::Claude, "one", "error", 200);
        stale.sequence = Some(1);
        assert!(!registry.apply_event(stale, 30_000));
        assert_eq!(
            registry.snapshot(200).sessions[0].phase,
            SessionPhase::Running
        );

        registry.apply_event(
            event("3", Provider::Claude, "one", "session_end", 300),
            30_000,
        );
        assert!(!registry.apply_event(event("4", Provider::Claude, "one", "start", 400), 30_000));
        assert_eq!(
            registry.snapshot(400).sessions[0].phase,
            SessionPhase::Completed
        );
    }

    #[test]
    fn duplicate_events_do_not_change_revision() {
        let mut registry = StatusRegistry::new();
        let first = event("1", Provider::Claude, "one", "start", 100);
        assert!(registry.apply_event(first.clone(), 30_000));
        assert!(!registry.apply_event(first, 30_000));
        assert_eq!(registry.snapshot(100).revision, 1);
    }

    #[test]
    fn wrong_schema_version_is_rejected() {
        let mut registry = StatusRegistry::new();
        let mut bad = event("1", Provider::Claude, "one", "start", 100);
        bad.schema_version = 99;
        assert!(!registry.apply_event(bad, 30_000));
        assert!(registry.snapshot(100).sessions.is_empty());
    }

    #[test]
    fn empty_event_id_is_rejected() {
        let mut registry = StatusRegistry::new();
        let bad = event("", Provider::Claude, "one", "start", 100);
        assert!(!registry.apply_event(bad, 30_000));
        assert!(registry.snapshot(100).sessions.is_empty());
    }

    #[test]
    fn empty_session_id_is_rejected() {
        let mut registry = StatusRegistry::new();
        let bad = event("1", Provider::Claude, "", "start", 100);
        assert!(!registry.apply_event(bad, 30_000));
        assert!(registry.snapshot(100).sessions.is_empty());
    }

    #[test]
    fn custom_retention_affects_remove_at() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", "session_end", 100),
            60_000,
        );
        let session = &registry.snapshot(100).sessions[0];
        assert_eq!(session.remove_at_ms, Some(60_100));
    }

    #[test]
    fn liveness_check_preserves_completed_green_lights() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", "session_end", 100),
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
            event("1", Provider::Claude, "one", "session_end", 100),
            30_000,
        );
        assert!(!registry.cleanup(200, DEFAULT_RUNNING_TIMEOUT_MS, |_| None));
        assert_eq!(registry.snapshot(200).sessions.len(), 1);
    }

    #[test]
    fn stale_error_sessions_are_removed_after_timeout() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(event("1", Provider::Claude, "one", "error", 100), 30_000);
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
            event("1", Provider::Claude, "one", "PreToolUse", 100),
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
        let mut failure = event("1", Provider::Claude, "one", "StopFailure", 100);
        failure.error_code = Some("overloaded".to_string());
        registry.apply_event(failure, 30_000);
        assert_eq!(
            registry.snapshot(100).sessions[0].error_code.as_deref(),
            Some("overloaded")
        );

        // 红灯不粘：后续执行事件可以把红灯带回绿灯，并清掉残留的错误码。
        registry.apply_event(
            event("2", Provider::Claude, "one", "PreToolUse", 200),
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
            event("1", Provider::Claude, "one", "StopFailure", 100),
            30_000,
        );
        assert_eq!(
            registry.snapshot(100).sessions[0].phase,
            SessionPhase::Error
        );

        // 红灯期间收到 Stop：允许转灯 → Waiting 绿闪，随后落黄。
        registry.apply_event(event("2", Provider::Claude, "one", "Stop", 200), 30_000);
        let session = &registry.snapshot(200).sessions[0];
        assert_eq!(session.phase, SessionPhase::Waiting);
        assert!(session.is_flashing);
    }

    #[test]
    fn session_end_after_error_completes_and_removes() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", "StopFailure", 100),
            30_000,
        );
        // 红灯期间会话结束：允许转灯 → Completed 绿闪，retention 后移除。
        registry.apply_event(
            event("2", Provider::Claude, "one", "SessionEnd", 200),
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
        registry.apply_event(event("1", Provider::Claude, "one", "error", 100), 30_000);
        // 无法识别的事件不构成转灯信号，红灯保持。
        registry.apply_event(
            event("2", Provider::Claude, "one", "SomethingWeird", 200),
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
            event("1", Provider::Claude, "one", "StopFailure", 100),
            30_000,
        );
        assert_eq!(
            registry.snapshot(100).sessions[0].phase,
            SessionPhase::Error
        );

        // StopFailure 后用户再发 prompt，应恢复 Running。
        registry.apply_event(
            event("2", Provider::Claude, "one", "UserPromptSubmit", 200),
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
        let mut first = event("1", Provider::Claude, "one", "PreToolUse", 100);
        first.base_url = Some("http://127.0.0.1:15721".to_string());
        registry.apply_event(first, 30_000);
        assert_eq!(
            registry.snapshot(100).sessions[0].base_url.as_deref(),
            Some("http://127.0.0.1:15721")
        );

        // 后续事件没带 base_url：应继承，而不是被抹成 null。
        registry.apply_event(
            event("2", Provider::Claude, "one", "PostToolUse", 200),
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
        let mut first = event("1", Provider::Claude, "one", "PreToolUse", 100);
        first.base_url = Some("http://127.0.0.1:15721".to_string());
        registry.apply_event(first, 30_000);

        let mut second = event("2", Provider::Claude, "one", "PostToolUse", 200);
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
        let mut submit = event("1", Provider::Claude, "one", "UserPromptSubmit", 100);
        submit.prompt = Some("第一个问题".to_string());
        registry.apply_event(submit, 30_000);
        assert_eq!(
            registry.snapshot(100).sessions[0].prompt.as_deref(),
            Some("第一个问题")
        );

        // 同一轮的后续事件不带 prompt：应继承，直到下一次提交。
        registry.apply_event(
            event("2", Provider::Claude, "one", "PreToolUse", 200),
            30_000,
        );
        assert_eq!(
            registry.snapshot(200).sessions[0].prompt.as_deref(),
            Some("第一个问题")
        );

        // 下一次提交：覆盖为新提问。
        let mut next = event("3", Provider::Claude, "one", "UserPromptSubmit", 300);
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
        registry.apply_event(event("1", Provider::Claude, "one", "session_end", 100), 0);
        let session = &registry.snapshot(100).sessions[0];
        assert!(!session.is_flashing);
    }

    #[test]
    fn normalize_event_name_handles_camel_case() {
        assert_eq!(normalize_event_name("TurnCompleted"), "turn_completed");
        assert_eq!(normalize_event_name("SessionEnd"), "session_end");
    }

    #[test]
    fn normalize_event_name_handles_separators() {
        assert_eq!(normalize_event_name("session-end"), "session_end");
        assert_eq!(normalize_event_name("session.end"), "session_end");
        assert_eq!(normalize_event_name("session:end"), "session_end");
    }

    #[test]
    fn normalize_event_name_lowercases() {
        assert_eq!(normalize_event_name("STOP"), "stop");
        assert_eq!(normalize_event_name("Start"), "start");
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
                    "start",
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
            event("1", Provider::Claude, "sess-1", "PreToolUse", 100),
            30_000,
        );
        registry.apply_event(event("2", Provider::Claude, "sess-1", "Stop", 200), 30_000);
        assert_eq!(registry.snapshot(200).sessions.len(), 1);
        assert_eq!(
            registry.snapshot(200).sessions[0].phase,
            SessionPhase::Waiting
        );

        // Session 2 starts: old Waiting session should be cleared.
        registry.apply_event(
            event("3", Provider::Claude, "sess-2", "UserPromptSubmit", 300),
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
        registry.apply_event(event("1", Provider::Claude, "c1", "Stop", 100), 30_000);
        assert_eq!(registry.snapshot(100).sessions.len(), 1);
        // Codex session starts: should NOT clear Claude's Waiting session.
        registry.apply_event(
            event("2", Provider::Codex, "x1", "UserPromptSubmit", 200),
            30_000,
        );
        assert_eq!(registry.snapshot(200).sessions.len(), 2);
    }

    /// 构造一条回填摘要（模拟从存档读出的历史值）。
    fn seed(started_at_ms: i64, prompt: Option<&str>, base_url: Option<&str>) -> StoredSeed {
        StoredSeed {
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
            event("1", Provider::Claude, "one", "PreToolUse", 500),
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
            event("1", Provider::Claude, "one", "UserPromptSubmit", 500),
            30_000,
        );
        registry.apply_event_with_seed(
            {
                let mut e = event("2", Provider::Claude, "one", "PreToolUse", 600);
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
            event("1", Provider::Claude, "one", "PreToolUse", 500),
            30_000,
            Some(&seed(100, Some("旧提问"), None)),
        );
        // 新一次提交：覆盖回填来的旧提问（阶梯「事件 > 前驱 > 回填」）。
        let mut submit = event("2", Provider::Claude, "one", "UserPromptSubmit", 600);
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
            event("1", Provider::Claude, "one", "PreToolUse", 500),
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
            event("1", Provider::Claude, "one", "PreToolUse", 500),
            30_000,
            Some(&seed(100, None, None)),
        );
        assert_eq!(registry.snapshot(500).sessions[0].tokens, None);
    }

    #[test]
    fn set_tokens_creates_entry_when_absent() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", "PreToolUse", 100),
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
            event("1", Provider::Claude, "one", "PreToolUse", 100),
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

    #[test]
    fn stop_failure_maps_to_error() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", "StopFailure", 100),
            30_000,
        );
        assert_eq!(
            registry.snapshot(100).sessions[0].phase,
            SessionPhase::Error
        );
    }
}
