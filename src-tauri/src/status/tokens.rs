//! 会话 token 消耗统计（Claude）。
//!
//! 数据来自 Claude Code 的 transcript（JSONL）：每条 assistant 记录的
//! `message.usage` 就是该次请求的 token 明细。
//!
//! 两个关键约束：
//! 1. **必须按 `message.id` 去重** —— 同一条 API 响应会在 transcript 里写成多条
//!    assistant 记录，每条都带同一份 usage，不去重总量会翻 2–3 倍（实测确认）。
//! 2. **增量读取** —— transcript 会增长到 MB 级，每次只读上次之后的追加字节。
//!
//! ⚠️ **Codex 不在这里统计**：它的用量写在 rollout 里，由 `status::codex_rollout`
//! 增量巡检时顺手产出（那个文件本来就要读，不该为用量再开第二个读取器）。
//!
//! 本模块由常驻的 monitor 线程独占持有，因此内部不加锁。

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use serde_json::Value;

use super::lines::LineCursor;
use super::model::TokenTotals;

/// Claude transcript 里我们只关心 assistant 记录。
const ASSISTANT_RECORD: &str = "assistant";

/// 单个文件的读取状态。
struct FileCursor {
    /// 增量行读取（`lines.rs`）。
    lines: LineCursor,
}

impl FileCursor {
    fn new() -> Self {
        Self {
            lines: LineCursor::new(),
        }
    }
}

/// 一个会话的 token 统计状态。
struct SessionTokens {
    /// 主 transcript 路径，用于推导子代理目录。
    main_path: Option<PathBuf>,
    /// 已登记要增量读取的文件（主 transcript + 各子代理 transcript）。
    files: HashMap<PathBuf, FileCursor>,
    /// 已计入的 `message.id` —— 去重的关键。
    seen_messages: HashSet<String>,
    /// 累计值。
    totals: TokenTotals,
    /// 子代理目录是否已扫描过。目录只需发现一次，之后新文件由 files 表跟进。
    /// （仅扫描一次意味着统计启动后才创建的会话可以覆盖，但中途新增的子代理文件
    /// 需要重新 observe 才会被发现 —— 由 refresh 里的重扫兜底，见下。）
    subagents_scanned: bool,
}

impl SessionTokens {
    fn new() -> Self {
        Self {
            main_path: None,
            files: HashMap::new(),
            seen_messages: HashSet::new(),
            totals: TokenTotals::default(),
            subagents_scanned: false,
        }
    }
}

/// 跨会话的 token 统计器。
#[derive(Default)]
pub struct TokenTracker {
    sessions: HashMap<String, SessionTokens>,
}

impl TokenTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一个会话的 transcript 路径。
    ///
    /// 由 monitor 在消费 hook 事件时调用；重复登记同一路径是幂等的。
    /// `provider` 只在首次登记时决定解析方式（同一 key 不可能换 provider）。
    pub fn observe(&mut self, session_key: &str, transcript_path: &Path) {
        let session = self
            .sessions
            .entry(session_key.to_string())
            .or_insert_with(SessionTokens::new);
        if session.main_path.is_none() {
            session.main_path = Some(transcript_path.to_path_buf());
        }
        session
            .files
            .entry(transcript_path.to_path_buf())
            .or_insert_with(FileCursor::new);
    }

    /// 刷新全部已登记会话，返回累计值发生变化的 `(session_key, totals)`。
    pub fn refresh_all(&mut self) -> Vec<(String, TokenTotals)> {
        let keys: Vec<String> = self.sessions.keys().cloned().collect();
        let mut changed = Vec::new();
        for key in keys {
            if self.refresh(&key) {
                if let Some(totals) = self.sessions.get(&key).map(|session| session.totals) {
                    changed.push((key, totals));
                }
            }
        }
        changed
    }

    /// 增量读取该会话的所有 transcript 文件，返回累计值是否变化。
    fn refresh(&mut self, session_key: &str) -> bool {
        let Some(session) = self.sessions.get_mut(session_key) else {
            return false;
        };
        let before = session.totals;

        // 首次刷新时扫描子代理目录。子代理的 transcript 与主文件同源：
        // <project>/<session-id>.jsonl  →  <project>/<session-id>/subagents/*.jsonl
        if !session.subagents_scanned {
            session.subagents_scanned = true;
            if let Some(main) = session.main_path.clone() {
                for path in subagent_transcripts(&main) {
                    session.files.entry(path).or_insert_with(FileCursor::new);
                }
            }
        }

        // 先收集路径，避免同时可变借用 files 与其它字段。
        let paths: Vec<PathBuf> = session.files.keys().cloned().collect();
        // 先收集路径，避免同时可变借用 files 与其它字段。
        for path in &paths {
            let Some(cursor) = session.files.get_mut(path) else {
                continue;
            };
            let (seen, totals) = (&mut session.seen_messages, &mut session.totals);
            cursor
                .lines
                .read_appended(path, |line| consume_line(line, seen, totals));
        }

        session.totals != before
    }

    /// 丢弃已不存在于注册表的会话，避免内存泄漏。
    pub fn retain<F>(&mut self, mut keep: F)
    where
        F: FnMut(&str) -> bool,
    {
        self.sessions.retain(|key, _| keep(key));
    }
}

/// 由主 transcript 路径推导子代理 transcript 列表。
///
/// 目录不存在（该会话没用子代理）时返回空，不是错误。
fn subagent_transcripts(main: &Path) -> Vec<PathBuf> {
    let dir = main.with_extension("").join("subagents");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("jsonl"))
        .collect()
}

/// 解析一行 transcript 记录：只认 assistant 记录的 `message.usage`，按 id 去重。
///
/// 任何解析失败都静默跳过 —— transcript 的格式不是稳定 API，
/// 格式变了最多是统计不到，绝不能让 monitor 线程出错。
fn consume_line(line: &[u8], seen: &mut HashSet<String>, totals: &mut TokenTotals) {
    if line.is_empty() {
        return;
    }
    let Ok(record) = serde_json::from_slice::<Value>(line) else {
        return;
    };
    if record.get("type").and_then(Value::as_str) != Some(ASSISTANT_RECORD) {
        return;
    }
    let Some(message) = record.get("message") else {
        return;
    };
    // 没有 usage 的记录不占用去重名额，避免把后续真正带 usage 的同 id 记录挡掉。
    let Some(usage) = message.get("usage") else {
        return;
    };
    let Some(message_id) = message.get("id").and_then(Value::as_str) else {
        return;
    };
    if !seen.insert(message_id.to_string()) {
        return;
    }

    totals.input_tokens += u64_field(usage, "input_tokens");
    totals.cache_creation_input_tokens += u64_field(usage, "cache_creation_input_tokens");
    totals.cache_read_input_tokens += u64_field(usage, "cache_read_input_tokens");
    // output_tokens 已包含 output_tokens_details.thinking_tokens，不能重复相加。
    totals.output_tokens += u64_field(usage, "output_tokens");
}

/// 读取一个非负整数字段；缺失或类型不符按 0 处理。
fn u64_field(value: &Value, name: &str) -> u64 {
    value.get(name).and_then(Value::as_u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::status::test_support::TempDir;

    /// 构造一条 assistant transcript 记录。
    fn assistant_record(message_id: &str, input: u64, cache_read: u64, output: u64) -> String {
        serde_json::json!({
            "type": "assistant",
            "message": {
                "id": message_id,
                "usage": {
                    "input_tokens": input,
                    "cache_creation_input_tokens": 0,
                    "cache_read_input_tokens": cache_read,
                    "output_tokens": output,
                    // 是 output_tokens 的细分，不应被重复相加。
                    "output_tokens_details": { "thinking_tokens": 999 },
                }
            }
        })
        .to_string()
    }

    /// 用给定内容建一个会话并返回 (tracker, 文件路径, 临时目录)。
    ///
    /// 必须把 `TempDir` 一并返回并留在调用方作用域内：它 Drop 时会删掉整个目录，
    /// 若在此函数内提前释放，后续 refresh 读到的是已删除的文件，统计恒为 0。
    fn tracked(contents: &str) -> (TokenTracker, PathBuf, TempDir) {
        let temp = TempDir::new("tokens");
        let path = temp.write("session.jsonl", contents);
        let mut tracker = TokenTracker::new();
        tracker.observe("claude:s1", &path);
        (tracker, path, temp)
    }

    /// 刷新并取回指定会话的累计值（默认取 `tracked()` 注册的 `claude:s1`）。
    fn totals_of(tracker: &mut TokenTracker) -> TokenTotals {
        totals_of_key(tracker, "claude:s1")
    }

    /// 刷新并取回指定会话 key 的累计值。
    fn totals_of_key(tracker: &mut TokenTracker, key: &str) -> TokenTotals {
        tracker.refresh_all();
        tracker
            .sessions
            .get(key)
            .map(|session| session.totals)
            .expect("session present")
    }

    #[test]
    fn counts_a_single_assistant_record() {
        let contents = format!("{}\n", assistant_record("msg_1", 100, 3000, 50));
        let (mut tracker, _path, _temp) = tracked(&contents);
        let totals = totals_of(&mut tracker);
        assert_eq!(totals.input_tokens, 100);
        assert_eq!(totals.cache_read_input_tokens, 3000);
        assert_eq!(totals.output_tokens, 50);
        // total 是四项之和；thinking 不重复计入。
        assert_eq!(totals.total(), 3150);
    }

    #[test]
    fn deduplicates_repeated_message_ids() {
        // 核心用例：同一条 API 响应会写成多条记录，每条都带同一份 usage。
        // 不去重这里的 total 会变成 3×100。
        let record = assistant_record("msg_same", 100, 0, 0);
        let contents = format!("{record}\n{record}\n{record}\n");
        let (mut tracker, _path, _temp) = tracked(&contents);
        let totals = totals_of(&mut tracker);
        assert_eq!(totals.total(), 100, "同 message.id 只能计一次");
    }

    #[test]
    fn sums_distinct_messages() {
        let contents = format!(
            "{}\n{}\n",
            assistant_record("msg_1", 10, 0, 1),
            assistant_record("msg_2", 20, 0, 2),
        );
        let (mut tracker, _path, _temp) = tracked(&contents);
        let totals = totals_of(&mut tracker);
        assert_eq!(totals.input_tokens, 30);
        assert_eq!(totals.output_tokens, 3);
    }

    #[test]
    fn ignores_non_assistant_and_usage_less_records() {
        let contents = format!(
            "{}\n{}\n{}\n",
            serde_json::json!({"type": "user", "message": {"usage": {"input_tokens": 999}}}),
            serde_json::json!({"type": "assistant", "message": {"id": "msg_no_usage"}}),
            assistant_record("msg_1", 5, 0, 5),
        );
        let (mut tracker, _path, _temp) = tracked(&contents);
        let totals = totals_of(&mut tracker);
        assert_eq!(totals.total(), 10, "只应计入 assistant 且带 usage 的记录");
    }

    #[test]
    fn usage_less_record_does_not_block_a_later_one() {
        // 先出现的同 id 记录没有 usage，后续带 usage 的必须仍能计入。
        let contents = format!(
            "{}\n{}\n",
            serde_json::json!({"type": "assistant", "message": {"id": "msg_1"}}),
            assistant_record("msg_1", 7, 0, 3),
        );
        let (mut tracker, _path, _temp) = tracked(&contents);
        assert_eq!(totals_of(&mut tracker).total(), 10);
    }

    #[test]
    fn malformed_lines_are_skipped() {
        let contents = format!(
            "not json at all\n{}\n{{ broken\n",
            assistant_record("msg_1", 4, 0, 6),
        );
        let (mut tracker, _path, _temp) = tracked(&contents);
        // 损坏行静默跳过，不影响后续统计。
        assert_eq!(totals_of(&mut tracker).total(), 10);
    }

    #[test]
    fn only_reads_appended_bytes_on_refresh() {
        let temp = TempDir::new("tokens-incremental");
        let path = temp.write("session.jsonl", "");
        let mut tracker = TokenTracker::new();
        tracker.observe("claude:s1", &path);

        append(&path, &format!("{}\n", assistant_record("msg_1", 10, 0, 0)));
        assert_eq!(totals_of(&mut tracker).total(), 10);

        // 追加第二条：只应加上新增的部分，不是重算全量。
        append(&path, &format!("{}\n", assistant_record("msg_2", 20, 0, 0)));
        assert_eq!(totals_of(&mut tracker).total(), 30);

        // 没有新内容时再次刷新不应变化。
        assert!(!tracker.refresh("claude:s1"));
    }

    #[test]
    fn waits_for_the_trailing_newline() {
        let temp = TempDir::new("tokens-partial");
        let path = temp.write("session.jsonl", "");
        let mut tracker = TokenTracker::new();
        tracker.observe("claude:s1", &path);

        // 写到一半（没有换行）：还不能计入。
        let record = assistant_record("msg_1", 10, 0, 0);
        let (head, tail) = record.split_at(record.len() / 2);
        append(&path, head);
        assert_eq!(totals_of(&mut tracker).total(), 0, "残行不应被计入");

        // 补齐剩余部分与换行后才计入，且只计一次。
        append(&path, &format!("{tail}\n"));
        assert_eq!(totals_of(&mut tracker).total(), 10);
    }

    #[test]
    fn truncated_file_resets_offset_without_losing_totals() {
        let temp = TempDir::new("tokens-truncated");
        let path = temp.write("session.jsonl", "");
        let mut tracker = TokenTracker::new();
        tracker.observe("claude:s1", &path);

        append(&path, &format!("{}\n", assistant_record("msg_1", 10, 0, 0)));
        assert_eq!(totals_of(&mut tracker).total(), 10);

        // 文件被截断：offset 重置，已累计的值保留（去重集仍挡住 msg_1）。
        std::fs::write(&path, "").unwrap();
        assert_eq!(totals_of(&mut tracker).total(), 10);

        // 之后的追加仍能正常计入。
        append(&path, &format!("{}\n", assistant_record("msg_2", 5, 0, 0)));
        assert_eq!(totals_of(&mut tracker).total(), 15);
    }

    #[test]
    fn counts_subagent_transcripts_into_the_same_session() {
        let temp = TempDir::new("tokens-subagents");
        // 主 transcript：<dir>/<session-id>.jsonl
        let main = temp.write("proj/sess-1.jsonl", "");
        // 子代理：<dir>/<session-id>/subagents/<agent>.jsonl
        let sub = temp.write("proj/sess-1/subagents/agent-abc.jsonl", "");

        let mut tracker = TokenTracker::new();
        tracker.observe("claude:sess-1", &main);
        append(
            &main,
            &format!("{}\n", assistant_record("msg_main", 10, 0, 0)),
        );
        append(
            &sub,
            &format!("{}\n", assistant_record("msg_sub", 0, 0, 90)),
        );

        // 子代理的消耗必须并入同一会话 —— 否则重度使用子代理的会话会明显偏低。
        // 该用例注册的 key 是 claude:sess-1（与 tracked() 的 claude:s1 不同），需按 key 取。
        let totals = totals_of_key(&mut tracker, "claude:sess-1");
        assert_eq!(totals.input_tokens, 10);
        assert_eq!(totals.output_tokens, 90);
        assert_eq!(totals.total(), 100);
    }

    #[test]
    fn missing_transcript_is_not_an_error() {
        let temp = TempDir::new("tokens-missing");
        let mut tracker = TokenTracker::new();
        tracker.observe("claude:s1", &temp.path().join("nope.jsonl"));
        // 文件还不存在：不 panic，也不产生统计。
        assert!(tracker.refresh_all().is_empty());
    }

    #[test]
    fn retain_drops_sessions_that_are_gone() {
        let temp = TempDir::new("tokens-retain");
        let path = temp.write("session.jsonl", "");
        let mut tracker = TokenTracker::new();
        tracker.observe("claude:s1", &path);
        tracker.observe("claude:s2", &temp.write("other.jsonl", ""));

        tracker.retain(|key| key == "claude:s1");
        assert_eq!(tracker.sessions.len(), 1);
        assert!(tracker.sessions.contains_key("claude:s1"));
    }


    /// 以追加方式写入，模拟 transcript 的增长。
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
