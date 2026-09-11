use std::collections::{HashMap, HashSet};

use super::model::{
    session_key, AgentEvent, SessionPhase, SessionSnapshot, StatusCounts, StatusSnapshot,
    StatusSource, SCHEMA_VERSION,
};

pub struct StatusRegistry {
    sessions: HashMap<String, SessionSnapshot>,
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
            seen_events: HashSet::new(),
            revision: 0,
            source_health: "healthy".to_string(),
        }
    }

    pub fn mark_source_degraded(&mut self) {
        self.source_health = "degraded".to_string();
    }

    pub fn apply_event(&mut self, event: AgentEvent, retention_ms: u64) -> bool {
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
        let (previous_error, previous_started_at, previous_pid, previous_error_code) =
            match self.sessions.get(&key) {
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
                        previous.phase == SessionPhase::Error,
                        Some(previous.started_at_ms),
                        previous.pid,
                        previous.error_code.clone(),
                    )
                }
                None => (false, None, None, None),
            };

        let phase_for_update = phase_for_event(&event);
        let phase = if previous_error && phase_for_update != SessionPhase::Error {
            SessionPhase::Error
        } else {
            phase_for_update
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

        let started_at_ms = previous_started_at.unwrap_or(now);
        let completed_at_ms = if phase == SessionPhase::Completed {
            Some(now)
        } else {
            None
        };
        let remove_at_ms = completed_at_ms
            .map(|completed| completed.saturating_add(retention_ms.min(i64::MAX as u64) as i64));

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
                is_flashing: phase == SessionPhase::Completed && retention_ms > 0,
                pid: event.pid.or(previous_pid),
                error_code: event.error_code.or(previous_error_code),
                last_sequence: event.sequence,
                termination_confirmed: phase == SessionPhase::Completed,
            },
        );
        self.revision = self.revision.saturating_add(1);
        true
    }

    pub fn cleanup<F>(&mut self, now: i64, mut is_alive: F) -> bool
    where
        F: FnMut(&SessionSnapshot) -> Option<bool>,
    {
        let before = self.sessions.len();
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
                // Running / Error / Unknown: process liveness + short timeout.
                _ => {
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

        let changed = before != self.sessions.len();
        if changed {
            self.revision = self.revision.saturating_add(1);
        }
        changed
    }

    pub fn snapshot(&self, now: i64) -> StatusSnapshot {
        let mut sessions: Vec<_> = self.sessions.values().cloned().collect();
        sessions.sort_by(|left, right| {
            right
                .last_seen_at_ms
                .cmp(&left.last_seen_at_ms)
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

/// Maximum time (ms) an Error/Unknown session can stay without new events
/// before being cleaned up.
const ERROR_TIMEOUT_MS: i64 = 60 * 1000; // 1 minute

/// Failsafe timeout (ms) for Waiting sessions — normally they are cleared
/// when the next run starts, but this prevents leaks if no run ever starts.
const WAITING_TIMEOUT_MS: i64 = 30 * 60 * 1000; // 30 minutes

fn phase_for_event(event: &AgentEvent) -> SessionPhase {
    // Error from payload fields takes priority regardless of event name.
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
        // ── Agent actively working (green + animation) ──
        "session_start" | "session_started" | "setup" | "user_prompt_submit"
        | "user_prompt_expansion" | "pre_tool_use" | "post_tool_use" | "post_tool_use_failure"
        | "post_tool_batch" | "permission_request" | "subagent_start" | "subagent_stop"
        | "task_created" | "message_display" | "pre_compact" | "post_compact"
        | "pre_model_switch" | "post_model_switch" | "elicit" | "elicit_result"
        | "instructions_loaded" | "config_change" | "cwd_changed" | "directory_added"
        | "file_changed" | "worktree_create" | "worktree_remove" => SessionPhase::Running,

        // ── Waiting for user input (yellow) ──
        "notification" | "permission_denied" => SessionPhase::Waiting,

        // ── Agent finished responding, waiting for next input (yellow) ──
        // These stay visible until the next run clears them.
        "stop" | "task_completed" | "teammate_idle" => SessionPhase::Waiting,

        // ── Session ended (green, no animation, with retention) ──
        "session_end" | "session_ended" | "session_completed" | "session_done" => {
            if event.outcome.as_deref().is_none_or(|outcome| {
                matches!(
                    outcome.to_ascii_lowercase().as_str(),
                    "success" | "succeeded" | "completed" | "ok" | "done"
                )
            }) {
                SessionPhase::Completed
            } else {
                SessionPhase::Error
            }
        }

        // ── API / turn failure (red) ──
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

fn normalize_event_name(value: &str) -> String {
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
        }
    }

    #[test]
    fn keeps_multiple_provider_sessions_separate() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(event("1", Provider::Claude, "same", "start", 1), 30_000);
        registry.apply_event(event("2", Provider::Codex, "same", "start", 2), 30_000);

        let snapshot = registry.snapshot(3);
        assert_eq!(snapshot.sessions.len(), 2);
        assert_eq!(snapshot.sessions[0].key, "codex:same");
        assert_eq!(snapshot.sessions[1].key, "claude:same");
    }

    #[test]
    fn completed_sessions_flash_until_retention_expires() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", "session_end", 100),
            30_000,
        );

        assert!(!registry.cleanup(30_099, |_| Some(false)));
        assert!(registry.cleanup(30_100, |_| Some(false)));
        assert!(registry.snapshot(30_100).sessions.is_empty());
    }

    #[test]
    fn dead_error_sessions_are_removed_by_liveness_check() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(event("1", Provider::Claude, "one", "error", 100), 30_000);

        assert!(registry.cleanup(200, |_| Some(false)));
        assert!(registry.snapshot(200).sessions.is_empty());
    }

    #[test]
    fn stop_and_notification_mean_waiting_not_success() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(event("1", Provider::Claude, "one", "Stop", 100), 30_000);
        assert_eq!(
            registry.snapshot(100).sessions[0].phase,
            SessionPhase::Waiting
        );

        registry.apply_event(
            event("2", Provider::Claude, "one", "Notification", 200),
            30_000,
        );
        assert_eq!(
            registry.snapshot(200).sessions[0].phase,
            SessionPhase::Waiting
        );
    }

    #[test]
    fn session_end_is_the_completed_signal() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", "SessionEnd", 100),
            30_000,
        );
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
        assert!(!registry.cleanup(200, |_| Some(false)));
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
        assert!(!registry.cleanup(200, |_| None));
        assert_eq!(registry.snapshot(200).sessions.len(), 1);
    }

    #[test]
    fn stale_error_sessions_are_removed_after_timeout() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(event("1", Provider::Claude, "one", "error", 100), 30_000);
        // Error sessions are kept while within the timeout window.
        assert!(!registry.cleanup(200, |_| None));
        assert_eq!(registry.snapshot(200).sessions.len(), 1);
        // After 60s without new events, stale error sessions are removed.
        let stale_time = 100 + super::ERROR_TIMEOUT_MS + 1;
        assert!(registry.cleanup(stale_time, |_| None));
        assert!(registry.snapshot(stale_time).sessions.is_empty());
    }

    #[test]
    fn error_state_is_protected_from_non_error_events() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(event("1", Provider::Claude, "one", "error", 100), 30_000);
        registry.apply_event(event("2", Provider::Claude, "one", "start", 200), 30_000);
        assert_eq!(
            registry.snapshot(200).sessions[0].phase,
            SessionPhase::Error
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

    #[test]
    fn notification_maps_to_waiting() {
        let mut registry = StatusRegistry::new();
        registry.apply_event(
            event("1", Provider::Claude, "one", "Notification", 100),
            30_000,
        );
        assert_eq!(
            registry.snapshot(100).sessions[0].phase,
            SessionPhase::Waiting
        );
    }
}
