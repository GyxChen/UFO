//! 插件日志通道：`ufo.log` / `host.log` 的唯一去处。
//!
//! 三条口径来自需求与 UI mock 的 04/05 段，都有对应判据：
//! 1. **只有插件自己调 `ufo.log` 才会有日志**（没有第二条来源，也没有自动采集）；
//! 2. **关掉调试就什么都不留** —— 不进环形缓冲、不写盘、不 emit（与转灯日志同一口径：没人看时零写盘）；
//! 3. **日志是敏感数据的第二出口** —— 组件可以 `ufo.log('info', prompt)`，所以导出走两步确认、
//!    界面上必须常驻说明这句话（见管理区与日志窗的文案）。
//!
//! 形状：内存里每包一个环形缓冲（`LOG_RING_LINES`），盘上按 UTC 日一个 JSONL 文件，
//! 调试卡的「最近 3 行」读环形缓冲，独立日志窗的翻页读盘（关调试后仍能翻到之前写的）。

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use super::paths;

/// 日志批次广播的事件名（调试卡与独立日志窗都订阅它）。
pub const PLUGIN_LOG_EVENT: &str = "plugin-log";

/// 一条日志。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LogLine {
    /// 毫秒时间戳。
    pub ts: u64,
    /// 级别：只认 `info` / `warn` / `error`，别的整条拒收（不夹成默认值，避免「看起来像 info」）。
    pub level: String,
    /// 正文（按 `LOG_LINE_CHARS` 截断）。
    pub msg: String,
    /// 结构化字段：只留标量（string / number / bool），对象与数组一律丢掉。
    pub fields: Option<BTreeMap<String, serde_json::Value>>,
    /// 盘上坐标（`<日>#<行序号>`），导出与「同一次调用的上下游」用它定位。
    pub trace_id: Option<String>,
}

/// 一页查询的结果。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogPage {
    /// 这一页的行（新 → 旧）。
    pub lines: Vec<LogLine>,
    /// 命中总数（不含 limit/offset）。
    pub total: usize,
    /// 本次请求的偏移。
    pub offset: usize,
}

/// 查询条件。
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LogQuery {
    /// 只看某一级别；空串 = 全部。
    pub level: String,
    /// 从新到旧的偏移。
    pub offset: usize,
    /// 这一页取多少行。
    pub limit: usize,
}

/// 运行态。
#[derive(Clone, Default)]
pub struct LogState(Arc<Mutex<Inner>>);

#[derive(Debug, Default)]
struct Inner {
    /// app-data（日志落在 `plugins/<id>/logs/` 下）。
    app_data: PathBuf,
    /// 每包的环形缓冲。
    rings: BTreeMap<String, VecDeque<LogLine>>,
    /// 每个包当前挂着哪些窗口（没挂就不 emit，省三窗 IPC）。
    viewers: BTreeMap<String, BTreeSet<String>>,
}

impl LogState {
    /// 建运行态（组合根在 setup 里调，包目录不必已存在）。
    pub fn new(app_data: PathBuf) -> Self {
        Self(Arc::new(Mutex::new(Inner {
            app_data,
            rings: BTreeMap::new(),
            viewers: BTreeMap::new(),
        })))
    }

    /// 写一条日志：校验级别与体积，进环形缓冲、落盘。
    ///
    /// `persist` 由调用方按「这个包现在开着调试吗」给：假 = 整条丢弃（什么都不留）。
    /// 返回写进去的那条（含 traceId），调用方据此广播。
    pub fn write(&self, id: &str, line: LogLine, persist: bool) -> Option<LogLine> {
        if !persist {
            return None;
        }
        if !paths::is_valid_plugin_id(id) || !is_known_level(&line.level) {
            return None;
        }
        let msg = paths::truncate_chars(&line.msg, paths::LOG_LINE_CHARS);
        let fields = keep_scalars(line.fields);
        let day = day_of(line.ts);
        let (app_data, seq) = {
            let mut guard = lock(&self.0);
            let ring = guard.rings.entry(id.to_string()).or_default();
            if ring.len() >= paths::LOG_RING_LINES {
                ring.pop_front();
            }
            let seq = ring.len() + 1;
            ring.push_back(LogLine {
                ts: line.ts,
                level: line.level.clone(),
                msg: msg.clone(),
                fields: fields.clone(),
                trace_id: Some(format!("{}#{}", day, seq)),
            });
            (guard.app_data.clone(), seq)
        };
        let stored = LogLine {
            ts: line.ts,
            level: line.level,
            msg,
            fields,
            trace_id: Some(format!("{}#{}", day, seq)),
        };
        append_line(&paths::plugin_log_dir(&app_data, id), &stored);
        Some(stored)
    }

    /// 环形缓冲的最后 N 行（新 → 旧）：调试卡那「最近 3 行」读它，不碰盘。
    pub fn recent(&self, id: &str, limit: usize) -> Vec<LogLine> {
        let guard = lock(&self.0);
        let Some(ring) = guard.rings.get(id) else {
            return Vec::new();
        };
        let take = limit.min(ring.len());
        ring.iter().rev().take(take).cloned().collect()
    }

    /// 按盘分页（新 → 旧），供独立日志窗用。
    ///
    /// ⚠️ 读的是「关调试之前写下的那些」：环只有 200 行、盘可以有很多天，
    /// 所以窗口翻页必须走盘，不能拿内存里的东西当全部历史。
    pub fn page(&self, id: &str, query: &LogQuery) -> LogPage {
        let app_data = lock(&self.0).app_data.clone();
        scan_page(&paths::plugin_log_dir(&app_data, id), query)
    }

    /// 清空某个包的日志（环 + 盘）。
    pub fn clear(&self, id: &str) -> Result<usize, String> {
        if !paths::is_valid_plugin_id(id) {
            return Err(format!("id「{id}」不合法"));
        }
        let app_data = lock(&self.0).app_data.clone();
        let dir = paths::plugin_log_dir(&app_data, id);
        let removed = list_day_files(&dir).len();
        let mut guard = lock(&self.0);
        let cleared = guard.rings.remove(id).map(|ring| ring.len()).unwrap_or(0);
        guard.viewers.remove(id);
        drop(guard);
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(|error| format!("日志删不掉：{error}"))?;
        }
        Ok(cleared.max(removed))
    }

    /// 挂上 / 摘掉某个窗口的实时跟随（按「包 + 窗口 label」记账，同转灯日志那条修法）。
    ///
    /// 每个窗口各自记一份：主窗那一页收起不会把这扇独立窗的跟随关掉。
    pub fn set_view(&self, id: &str, window: &str, attached: bool) {
        let mut guard = lock(&self.0);
        let viewers = guard.viewers.entry(id.to_string()).or_default();
        if attached {
            viewers.insert(window.to_string());
        } else {
            viewers.remove(window);
        }
    }

    /// 摘掉某个窗口在**所有包**上的跟随：面板窗是被销毁的（前端来不及自己 detach），
    /// 由组合根在 `WindowEvent::Destroyed` 上调这里，否则「没人看还每轮 emit」会一直挂着。
    pub fn drop_window_viewers(&self, window: &str) {
        let mut guard = lock(&self.0);
        for viewers in guard.viewers.values_mut() {
            viewers.remove(window);
        }
        // 空集合留着只会让 `has_viewer` 多绕一次，顺手收掉。
        guard.viewers.retain(|_, viewers| !viewers.is_empty());
    }

    /// 卸载 / 停用时收干净：连环带跟随一起丢，别让残留行在下次装同名包时冒出来。
    pub fn forget(&self, id: &str) {
        let mut guard = lock(&self.0);
        guard.rings.remove(id);
        guard.viewers.remove(id);
    }

    /// 有没有人在这看（写盘之外的 emit 判据）。
    fn has_viewer(&self, id: &str) -> bool {
        lock(&self.0)
            .viewers
            .get(id)
            .map(|viewers| !viewers.is_empty())
            .unwrap_or(false)
    }
}

/// 级别白名单：未知级别整条拒收（夹成 info 会让「看不懂的日志」伪装成正常日志）。
fn is_known_level(level: &str) -> bool {
    matches!(level, "info" | "warn" | "error")
}

/// 只留标量字段：数组 / 对象 / null 一律丢（一条 dump 能把整行挤没，也不该进盘）。
fn keep_scalars(
    fields: Option<BTreeMap<String, serde_json::Value>>,
) -> Option<BTreeMap<String, serde_json::Value>> {
    let mut kept: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    for (key, value) in fields.unwrap_or_default() {
        let scalar = matches!(
            value,
            serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::String(_)
        );
        if scalar && key.chars().count() <= paths::STORAGE_MAX_KEY_CHARS {
            kept.insert(key, value);
        }
    }
    if kept.is_empty() {
        None
    } else {
        Some(kept)
    }
}

/// 毫秒时间戳 → UTC 日（`YYYY-MM-DD`），日志文件按它分片。
fn day_of(ts_ms: u64) -> String {
    let days = (ts_ms / 1000 / 86_400) as i64;
    // Civil-from-days（Howard Hinnant）：不引 chrono，只要一个稳定分片名。
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097).max(0);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{:02}-{:02}", if m <= 2 { y + 1 } else { y }, m, d)
}

/// 追加一行到当天的 JSONL；超过每包上限就不再写（宁可不写也不吃满磁盘）。
fn append_line(dir: &Path, line: &LogLine) {
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let file = dir.join(format!("{}.jsonl", day_of(line.ts)));
    if fs::metadata(&file).map(|meta| meta.len() > paths::LOG_FILE_BYTES).unwrap_or(false) {
        return;
    }
    let Ok(text) = serde_json::to_string(line) else {
        return;
    };
    match OpenOptions::new().create(true).append(true).open(&file) {
        Ok(mut handle) => {
            // ⚠️ 正文与换行必须**一次** `write_all`：分两次写时，几扇窗同一瞬间追加会交错成
            // 「A 的正文 + B 的正文 + 一个换行」，那份 JSONL 从此按行解析不动（实测粘出过两条）。
            let _ = handle.write_all(format!("{text}\n").as_bytes());
        }
        Err(_) => {}
    }
}

/// 列出当天的日志文件（按文件名升序 = 按日期升序）。
fn list_day_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = Vec::new();
    let Ok(reader) = fs::read_dir(dir) else {
        return files;
    };
    for entry in reader.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        // 只认 `<YYYY-MM-DD>.jsonl`：别的文件（残留的 .tmp 之类）不参与查询。
        if name.len() == 16 && name.ends_with(".jsonl") && path.is_file() {
            files.push(path);
        }
    }
    files.sort();
    files
}

/// 从新到旧扫盘，取一页 + 命中总数。
fn scan_page(dir: &Path, query: &LogQuery) -> LogPage {
    let files = list_day_files(dir);
    let mut matched: Vec<LogLine> = Vec::new();
    for file in files.iter().rev() {
        let Ok(text) = fs::read_to_string(file) else {
            continue;
        };
        let mut lines: Vec<LogLine> = Vec::new();
        for raw in text.lines().rev() {
            if let Ok(line) = serde_json::from_str::<LogLine>(raw) {
                lines.push(line);
            }
        }
        // 文件内部也是新 → 旧。
        for line in lines {
            if !query.level.is_empty() && line.level != query.level {
                continue;
            }
            matched.push(line);
        }
    }
    let total = matched.len();
    let limit = if query.limit == 0 {
        paths::LOG_PAGE_LINES
    } else {
        query.limit.min(paths::LOG_PAGE_LINES)
    };
    let lines = matched.drain(query.offset.min(total)..).take(limit).collect();
    LogPage { lines, total, offset: query.offset }
}

/// 广播一批新行给挂在窗口外的 webview（没人看就直接返回）。
pub fn publish(app: &AppHandle, state: &LogState, plugin_id: &str, lines: &[LogLine]) {
    if lines.is_empty() || !state.has_viewer(plugin_id) {
        return;
    }
    let payload = serde_json::json!({ "pluginId": plugin_id, "lines": lines });
    if let Err(error) = app.emit(PLUGIN_LOG_EVENT, payload) {
        eprintln!("failed to broadcast plugin log: {error}");
    }
}

/// 取一把锁（中毒也照用）。
fn lock<T>(value: &Arc<Mutex<T>>) -> MutexGuard<'_, T> {
    match value.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 隔离的临时 app-data。
    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ufo-plugin-log-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("临时目录建不出来");
        dir
    }

    /// 造一条日志。
    fn line(level: &str, msg: &str, ts: u64) -> LogLine {
        LogLine { ts, level: level.to_string(), msg: msg.to_string(), fields: None, trace_id: None }
    }

    #[test]
    fn concurrent_appends_stay_line_oriented() {
        // 三扇窗同时 `activate` 就是同时往同一个文件追加：交错一次，整份历史就按行读不出来了。
        let root = temp_root("race");
        let dir = paths::plugin_log_dir(&root, "racer");
        let workers: Vec<_> = (0..8u32)
            .map(|id| {
                let dir = dir.clone();
                std::thread::spawn(move || {
                    for index in 0..64u32 {
                        append_line(&dir, &line("info", &format!("t{id}-{index}"), 1_700_000_000_000));
                    }
                })
            })
            .collect();
        for worker in workers {
            let _ = worker.join();
        }

        let mut seen = 0usize;
        for file in list_day_files(&dir) {
            let text = fs::read_to_string(file).expect("日志文件读不出来");
            for raw in text.lines().filter(|row| !row.is_empty()) {
                let parsed: LogLine = serde_json::from_str(raw).expect("有一行不是完整 JSON：追加被交错写坏了");
                assert!(parsed.msg.starts_with('t'), "读出一行不像记录的：{raw}");
                seen += 1;
            }
        }
        assert_eq!(seen, 8 * 64, "行数对不上：有记录被粘在一起或写丢了");
    }

    #[test]
    fn persist_off_keeps_the_ledger_empty() {
        // 「关调试就零写盘」是需求点名的口径，这里断言的是「整条丢弃」而不是「只丢盘」。
        let root = temp_root("off");
        let state = LogState::new(root.clone());
        assert!(state.write("x", line("info", "不该留下", 1_700_000_000_000), false).is_none());
        assert!(state.recent("x", 3).is_empty());
        assert!(!paths::plugin_log_dir(&root, "x").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn unknown_levels_and_ids_are_dropped() {
        let root = temp_root("levels");
        let state = LogState::new(root.clone());
        assert!(state.write("ok", line("debug", "不认识级别", 1), true).is_none());
        assert!(state.write("Bad Id", line("info", "id 不合法", 1), true).is_none());
        assert!(state.recent("ok", 3).is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn ring_is_capped_and_reads_back_newest_first() {
        let root = temp_root("ring");
        let state = LogState::new(root.clone());
        for index in 0..(paths::LOG_RING_LINES + 20) {
            state.write("p", line("info", &format!("第 {} 条", index), 1_700_000_000_000 + index as u64), true);
        }
        let recent = state.recent("p", 3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].msg, format!("第 {} 条", paths::LOG_RING_LINES + 19));
        assert_eq!(state.recent("p", 999).len(), paths::LOG_RING_LINES, "环不许涨过头");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn long_messages_are_truncated() {
        let root = temp_root("truncate");
        let state = LogState::new(root.clone());
        let long = "字".repeat(paths::LOG_LINE_CHARS + 500);
        let stored = state.write("p", line("warn", &long, 1_700_000_000_000), true).expect("该收下");
        assert_eq!(stored.msg.chars().count(), paths::LOG_LINE_CHARS);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn only_scalar_fields_survive() {
        let root = temp_root("fields");
        let state = LogState::new(root.clone());
        let mut fields: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        fields.insert("ms".to_string(), serde_json::json!(214));
        fields.insert("cache_hit".to_string(), serde_json::json!(false));
        fields.insert("body".to_string(), serde_json::json!({ "大": "对象" }));
        fields.insert("list".to_string(), serde_json::json!([1, 2]));
        let stored = state
            .write(
                "p",
                LogLine { ts: 1_700_000_000_000, level: "info".to_string(), msg: "轮询完成".to_string(), fields: Some(fields), trace_id: None },
                true,
            )
            .expect("该收下");
        let kept = stored.fields.expect("有标量就该留着");
        assert_eq!(kept.len(), 2, "只该剩下标量：{:?}", kept);
        assert!(kept.contains_key("ms"));
        assert!(kept.contains_key("cache_hit"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn pages_read_from_disk_newest_first_with_level_filter() {
        let root = temp_root("page");
        let state = LogState::new(root.clone());
        for index in 0..5 {
            let level = if index % 2 == 0 { "info" } else { "error" };
            state.write("p", line(level, &format!("第 {} 条", index), 1_700_000_000_000 + index), true);
        }
        let all = state.page("p", &LogQuery { level: String::new(), offset: 0, limit: 3 });
        assert_eq!(all.total, 5);
        assert_eq!(all.lines.len(), 3);
        assert_eq!(all.lines[0].msg, "第 4 条", "必须新 → 旧");
        let second = state.page("p", &LogQuery { level: String::new(), offset: 3, limit: 3 });
        assert_eq!(second.lines.len(), 2);
        let errors = state.page("p", &LogQuery { level: "error".to_string(), offset: 0, limit: 10 });
        assert_eq!(errors.total, 2);
        assert!(errors.lines.iter().all(|item| item.level == "error"));
        let beyond = state.page("p", &LogQuery { level: String::new(), offset: 99, limit: 10 });
        assert!(beyond.lines.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn clear_drops_ring_and_files() {
        let root = temp_root("clear");
        let state = LogState::new(root.clone());
        state.write("p", line("info", "一条", 1_700_000_000_000), true);
        assert!(paths::plugin_log_dir(&root, "p").exists());
        let removed = state.clear("p").expect("清得掉");
        assert_eq!(removed, 1);
        assert!(!paths::plugin_log_dir(&root, "p").exists());
        assert!(state.recent("p", 3).is_empty());
        // 没日志的包：清空是幂等的（重复点「清空」不该报错），但 id 不合法必须拒。
        assert_eq!(state.clear("p").expect("重复清空要无事可做"), 0);
        assert!(state
            .clear("Bad Id")
            .expect_err("不合法的 id 不能拿去删目录")
            .contains("不合法"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn viewers_are_counted_per_window_and_detach_only_itself() {
        let root = temp_root("viewers");
        let state = LogState::new(root.clone());
        state.set_view("p", "main", true);
        state.set_view("p", "plugin-log", true);
        assert!(state.has_viewer("p"));
        // 主窗那一页收起不该把这扇独立窗的跟随关掉。
        state.set_view("main", "main", true);
        state.set_view("p", "main", false);
        assert!(state.has_viewer("p"));
        state.set_view("p", "plugin-log", false);
        assert!(!state.has_viewer("p"));
        state.set_view("p", "plugin-log", true);
        // 面板窗是被销毁的（前端来不及自己 detach）：宿主在 Destroyed 里按窗口一次性摘掉它在
        // **所有包**上的跟随，别留下「没人看还每轮 emit」，也不许顺手关掉主窗那一份。
        state.set_view("q", "plugin-log", true);
        state.set_view("q", "main", true);
        state.drop_window_viewers("plugin-log");
        assert!(!state.has_viewer("p"), "窗口销毁后 p 包不再有人跟");
        assert!(state.has_viewer("q"), "但 q 包上主窗那份跟随要留着");
        state.forget("p");
        assert!(!state.has_viewer("p"), "卸载 / 停用后要连跟随一起收");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn day_names_are_stable_utc_slices() {
        // 1970-01-01 起第 0 天与 2023-06-15（19523 天）。
        assert_eq!(day_of(0), "1970-01-01");
        assert_eq!(day_of(1_686_796_800_000), "2023-06-15");
        // 一天的边界不能跨到前一天去分片。
        assert_eq!(day_of(86_399_999), "1970-01-01");
        assert_eq!(day_of(86_400_000), "1970-01-02");
    }

    #[test]
    fn day_files_are_the_only_queryable_sources() {
        let root = temp_root("stray");
        let dir = paths::plugin_log_dir(&root, "p");
        fs::create_dir_all(&dir).expect("目录建不出来");
        fs::write(dir.join("2023-06-15.jsonl"), "{\"ts\":1,\"level\":\"info\",\"msg\":\"盘上的一条\"}\n").expect("写不出");
        fs::write(dir.join("leftover.tmp"), "junk").expect("写不出");
        assert_eq!(list_day_files(&dir).len(), 1, "只认 <日期>.jsonl");
        let page = scan_page(&dir, &LogQuery { level: String::new(), offset: 0, limit: 10 });
        assert_eq!(page.total, 1);
        assert_eq!(page.lines[0].msg, "盘上的一条");
        let _ = fs::remove_dir_all(&root);
    }
}
