//! Codex rollout 补漏：hook 覆盖不到的「一轮以错误结束」。
//!
//! Codex 的 12 个 hook 事件里**没有错误类事件**（Claude 有 `StopFailure`），实测
//! 「重试耗尽」这类以 error 结束的轮次连 `Stop` 都不触发 —— 于是红灯在 Codex 会话上
//! 原本永远不可达（灯只会停在绿灯，直到超时被清掉）。
//!
//! 这里回到 Codex 自己写的 rollout（路径由 hook 的 `transcript_path` 给出）补一脚：
//! 读到 `event_msg` / `task_complete` 带 `error` 就合成一条 `Failed` 事件喂给内核。
//! 识别规则在兼容层（`compat::codex::rollout_turn_error`），本模块只管增量巡检。
//!
//! ⚠️ 两个约束：
//! 1. **只巡检已登记的会话**（hook 事件里带过 `transcript_path` 的那些）—— 没有会话时
//!    零 I/O，既不会自己去发现文件，也不会在 agent 全停时还在读盘。
//! 2. 它与 token 统计读同一个文件、共用同一套游标语义（`lines.rs`），
//!    因此**只在 monitor 线程里读**，与 hook 进程不争锁。
//!
//! ⚠️ 已知的时序窗口：rollout 是 Codex 落盘后才被我们读到，最坏比真实发生晚一个
//! 巡检周期（约 500ms）。若用户在这 500ms 内已经发出下一条 prompt，红灯可能短暂盖住
//! 新一轮的绿灯 —— 下一条 hook 事件（新轮次立刻就会发）会把它纠正回来。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::compat::codex;
use super::lines::LineCursor;
use super::model::{now_ms, AgentEvent};

/// 被巡检的一个 Codex 会话。
struct WatchedSession {
    /// 会话 id：合成的内核事件要用它（rollout 的 `task_complete` 记录里没有该字段）。
    session_id: String,
    /// 已登记的 rollout 文件 → 增量游标。
    files: HashMap<PathBuf, LineCursor>,
    /// 已上报过的轮 id，同一轮只报一次。
    seen_turns: HashSet<String>,
}

/// 跨会话的 rollout 错误巡检器。
#[derive(Default)]
pub(crate) struct CodexRolloutWatcher {
    sessions: HashMap<String, WatchedSession>,
}

impl CodexRolloutWatcher {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// 登记某会话的 rollout 路径；重复登记同一路径是幂等的。
    ///
    /// 由 monitor 在消费 hook 事件时调用（与 token 统计同一处、同一时机）。
    pub(crate) fn observe(&mut self, session_key: &str, session_id: &str, transcript_path: &Path) {
        let session = self
            .sessions
            .entry(session_key.to_string())
            .or_insert_with(|| WatchedSession {
                session_id: session_id.to_string(),
                files: HashMap::new(),
                seen_turns: HashSet::new(),
            });
        session
            .files
            .entry(transcript_path.to_path_buf())
            .or_insert_with(LineCursor::new);
    }

    /// 增量巡检所有已登记会话，返回本轮**新发现**的「错误结束」事件。
    ///
    /// 同一轮（同一 `turn_id`）重复读到不会重复产出 —— 文件被截断重扫时靠这里兜住。
    pub(crate) fn poll(&mut self) -> Vec<AgentEvent> {
        let received_at_ms = now_ms();
        let mut events = Vec::new();
        for session in self.sessions.values_mut() {
            for (path, cursor) in session.files.iter_mut() {
                // 先收进局部：闭包只捕获局部，避免与 cursor 抢可变借用。
                let mut found = Vec::new();
                cursor.read_appended(path, |line| {
                    if let Some(error) = codex::rollout_turn_error(line) {
                        found.push(error);
                    }
                });
                for error in found {
                    if !session.seen_turns.insert(error.turn_id.clone()) {
                        continue;
                    }
                    events.push(error.into_event(&session.session_id, received_at_ms));
                }
            }
        }
        events
    }

    /// 丢弃已不存在于注册表的会话，避免内存泄漏（与 token 统计同一时机调用）。
    pub(crate) fn retain<F>(&mut self, mut keep: F)
    where
        F: FnMut(&str) -> bool,
    {
        self.sessions.retain(|key, _| keep(key));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::status::model::{AgentEventKind, Provider};
    use crate::status::test_support::TempDir;

    /// 构造一条**失败**轮次的 task_complete 记录（照抄真实 rollout 的形态）。
    fn task_complete_error(turn_id: &str, message: &str) -> String {
        serde_json::json!({
            "timestamp": "2026-09-17T15:36:35.178Z",
            "ordinal": 29,
            "type": "event_msg",
            "payload": {
                "type": "task_complete",
                "turn_id": turn_id,
                "last_agent_message": null,
                "error": {
                    "message": message,
                    "codex_error_info": {"response_too_many_failed_attempts": {"http_status_code": 429}}
                },
                "started_at": 1789659285,
            }
        })
        .to_string()
    }

    /// 构造一条**正常完成**的 task_complete（不带 error 字段）。
    fn task_complete_ok(turn_id: &str) -> String {
        serde_json::json!({
            "timestamp": "2026-09-17T13:17:44.771Z",
            "ordinal": 8,
            "type": "event_msg",
            "payload": {"type": "task_complete", "turn_id": turn_id, "last_agent_message": "done"},
        })
        .to_string()
    }

    /// 建一个已登记的巡检器，返回 (watcher, rollout 路径, 临时目录)。
    fn tracked(contents: &str) -> (CodexRolloutWatcher, PathBuf, TempDir) {
        let temp = TempDir::new("codex-rollout");
        let path = temp.write("rollout.jsonl", contents);
        let mut watcher = CodexRolloutWatcher::new();
        watcher.observe("codex:thr-1", "thr-1", &path);
        (watcher, path, temp)
    }

    #[test]
    fn reports_a_failed_turn_as_a_red_light_event() {
        let contents = format!(
            "{}\n",
            task_complete_error("turn-1", "exceeded retry limit, last status: 429 Too Many Requests")
        );
        let (mut watcher, _path, _temp) = tracked(&contents);

        let events = watcher.poll();
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.provider, Provider::Codex);
        assert_eq!(event.session_id, "thr-1");
        assert_eq!(event.kind, AgentEventKind::Failed);
        // 错误正文进 error_code（红灯期间内核会保留它）。
        assert!(event
            .error_code
            .as_deref()
            .is_some_and(|code| code.contains("exceeded retry limit")));
        // 标出这不是 hook 事件。
        assert_eq!(event.event, "task_complete(error)");
    }

    #[test]
    fn a_failed_turn_is_reported_only_once() {
        let contents = format!("{}\n", task_complete_error("turn-1", "429"));
        let (mut watcher, _path, _temp) = tracked(&contents);

        assert_eq!(watcher.poll().len(), 1);
        // 没有新增内容：不重复报。
        assert!(watcher.poll().is_empty());
        // 文件被截断重扫（长度回退）也不重复报 —— 靠 turn_id 去重。
        let path = _temp.path().join("rollout.jsonl");
        std::fs::write(&path, format!("{}\n", task_complete_error("turn-1", "429"))).unwrap();
        assert!(watcher.poll().is_empty());
    }

    #[test]
    fn each_failed_turn_is_reported_separately() {
        let contents = format!("{}\n", task_complete_error("turn-1", "429"));
        let (mut watcher, path, _temp) = tracked(&contents);
        assert_eq!(watcher.poll().len(), 1);

        // 追加一轮新的失败：单独上报。
        append(&path, &format!("{}\n", task_complete_error("turn-2", "502")));
        let events = watcher.poll();
        assert_eq!(events.len(), 1);
        assert!(events[0].event_id.contains("turn-2"));
    }

    #[test]
    fn successful_turns_are_ignored() {
        // 正常完成的 task_complete 不带 error —— hook 的 Stop 已经覆盖，这里不能重复报。
        let contents = format!("{}\n", task_complete_ok("turn-1"));
        let (mut watcher, _path, _temp) = tracked(&contents);
        assert!(watcher.poll().is_empty());
    }

    #[test]
    fn retries_that_did_not_end_the_turn_are_ignored() {
        // 中间的重试只写别的事件，不写 task_complete；把「重试后成功」误报成错误最伤。
        let noise = serde_json::json!({
            "type": "event_msg",
            "payload": {"type": "stream_error", "message": "retrying (attempt 2)"}
        })
        .to_string();
        let contents = format!("{noise}\n{}\n", task_complete_ok("turn-1"));
        let (mut watcher, _path, _temp) = tracked(&contents);
        assert!(watcher.poll().is_empty());
    }

    #[test]
    fn half_written_records_are_held_back() {
        // 写到一半的记录不能当完整记录解析（否则可能读到截断的 JSON 而误判）。
        let line = task_complete_error("turn-1", "429");
        let temp = TempDir::new("codex-rollout-partial");
        let path = temp.write("rollout.jsonl", &line[..line.len() / 2]);
        let mut watcher = CodexRolloutWatcher::new();
        watcher.observe("codex:thr-1", "thr-1", &path);
        assert!(watcher.poll().is_empty());

        // 补齐后半行后才上报。
        append(&path, &format!("{}\n", &line[line.len() / 2..]));
        assert_eq!(watcher.poll().len(), 1);
    }

    #[test]
    fn missing_rollout_is_not_an_error() {
        let temp = TempDir::new("codex-rollout-missing");
        let mut watcher = CodexRolloutWatcher::new();
        watcher.observe("codex:thr-1", "thr-1", &temp.path().join("nope.jsonl"));
        // transcript_path 可能为 null / 文件尚未落盘：不 panic，也不产出事件。
        assert!(watcher.poll().is_empty());
    }

    #[test]
    fn retain_drops_sessions_that_are_gone() {
        let temp = TempDir::new("codex-rollout-retain");
        let mut watcher = CodexRolloutWatcher::new();
        watcher.observe("codex:s1", "s1", &temp.write("a.jsonl", ""));
        watcher.observe("codex:s2", "s2", &temp.write("b.jsonl", ""));

        watcher.retain(|key| key == "codex:s1");
        assert_eq!(watcher.sessions.len(), 1);
        assert!(watcher.sessions.contains_key("codex:s1"));
    }

    /// 以追加方式写入，模拟 rollout 的增长。
    fn append(path: &Path, contents: &str) {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open for append");
        file.write_all(contents.as_bytes()).expect("append");
    }
}
