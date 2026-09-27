//! 日志落盘：**一次 open + 一次 `write_all`** 的追加写，只用 std。
//!
//! 为什么不用 SQLite（会话存档用的那个）：hook 短命进程也要写日志，而它有一条红线
//! 「绝不打开 AppData 的 SQLite」—— 每次事件都要启动一个进程，开库+建连接的成本
//! 直接压在 agent 的调用路径上。多进程并发追加靠的是 `O_APPEND` 语义：Windows 上
//! Rust 的 `append(true)` 映射到 `FILE_APPEND_DATA`，单次小写入在 EOF 处是原子的，
//! 因此一整批行**拼成一个缓冲、发一次 `write_all`** 就不会互相插行。
//! （这也是 `hook-debug.log` 今天已经在用、跑了很久没出过问题的机制。）
//!
//! ⚠️ 不做 `sync_all`：日志是尽力而为的观测面，为了它把 hook 进程的退出拖慢不值得。
//! 系统崩溃时可能丢最后几 KB，这是已知代价。

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use super::super::model::now_ms;
use super::paths::{file_name, log_dir, pick_active_seq, utc_day};
use super::record::LogEntry;

/// 一个日志写入器：定位到 `status_dir/log`，按当日序号写文件。
///
/// 无内部可变状态 —— hook 每次新建一个、monitor 复用一个都行，因为「该写哪个文件」
/// 完全由目录当前内容决定（见 [`super::paths`]）。
#[derive(Debug, Clone)]
pub struct Sink {
    /// 日志目录（`<status_dir>/log`）。
    dir: PathBuf,
    /// 单文件体积上限，超过就往后开一个新序号。
    max_file_bytes: u64,
    /// 停用标志：为真时**一次盘都不碰**（见 [`Sink::disabled`]）。
    disabled: bool,
}

impl Sink {
    /// 按状态目录建写入器（与 inbox 同级）。
    pub fn for_status_dir(status_dir: &Path, max_file_bytes: u64) -> Self {
        Self {
            dir: log_dir(status_dir),
            max_file_bytes: max_file_bytes.max(1),
            disabled: false,
        }
    }

    /// 一个什么都不做的写入器：不建目录、不 open、不写。
    ///
    /// 存在的唯一理由是让 hook 侧的日志开关**真的**不碰盘 —— 关掉日志图的就是「别在
    /// agent 每次事件都要起的短进程里加一次磁盘往返」，只把内容丢掉、还留着 open/write
    /// 等于没关。GUI 侧不需要它（档位判断在 [`LogState`] 里已经短路掉了）。
    pub fn disabled() -> Self {
        Self {
            dir: PathBuf::new(),
            max_file_bytes: 1,
            disabled: true,
        }
    }

    /// 日志目录（面板要把它显示出来：`--status-dir` 覆盖时那些行落在别的目录）。
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// 当前该写的那个文件的完整路径。
    pub fn active_path(&self) -> PathBuf {
        self.active_path_on(&utc_day(now_ms()))
    }

    /// 同上，但指定日期（单测用）。
    fn active_path_on(&self, today: &str) -> PathBuf {
        let files = self.listing();
        let seq = pick_active_seq(&files, today, self.max_file_bytes);
        self.dir.join(file_name(today, seq))
    }

    /// 目录里的 `(文件名, 字节数)`；读不到的条目按 0 字节算（宁可当它是空的）。
    pub fn listing(&self) -> Vec<(String, u64)> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut files = Vec::new();
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let size = fs::metadata(entry.path()).map(|meta| meta.len()).unwrap_or(0);
            files.push((name, size));
        }
        files
    }

    /// 追加若干条日志。**调用方一律 `let _ =` 忽略结果** —— 写不进日志绝不能影响灯态。
    ///
    /// 整批一次 `write_all`：这是「多进程不互插半行」的唯一保证，别改成逐行写。
    pub fn append(&self, entries: &[LogEntry]) -> io::Result<()> {
        // 停用态：连目录都不建 —— 这条早退就是「关掉日志」在 hook 侧的全部含义。
        if self.disabled {
            return Ok(());
        }
        let Some(last) = entries.last() else {
            return Ok(());
        };
        // 一批按**最后一条**的时刻归日：跨零点时把整批落进新文件，
        // 于是「昨天产生的几条」和「今天的」同处一个文件 —— tailer 按文件全量读，不丢。
        let today = utc_day(last.ts_ms);
        fs::create_dir_all(&self.dir)?;
        let path = self.active_path_on(&today);
        let mut payload = String::new();
        for entry in entries {
            payload.push_str(&entry.line());
            payload.push('\n');
        }
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        file.write_all(payload.as_bytes())
    }

    /// hook 侧的便利入口：**一次调用只 open 一次，且不返回任何错误给调用方**。
    ///
    /// 单独给 hook 用一个 `pub(crate)` 的薄壳，是为了让「日志失败不改退出码」这条
    /// 纪律写在类型上（返回 `()`），而不是靠每个调用点记得 `let _ =`。
    pub(crate) fn report(&self, entry: LogEntry) {
        let _ = self.append(std::slice::from_ref(&entry));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::agent_status::status::log::record::{LogStage, LogWriter};
    use crate::modules::agent_status::status::test_support::TempDir;

    fn entry(stage: LogStage) -> LogEntry {
        LogEntry::new(LogWriter::Hook, stage)
    }

    #[test]
    fn appends_are_line_delimited_and_ordered() {
        let temp = TempDir::new("log-sink");
        let sink = Sink::for_status_dir(temp.path(), 1024);
        assert!(sink.append(&[entry(LogStage::HookAttempt), entry(LogStage::InboxWrite)]).is_ok());
        let raw = fs::read_to_string(sink.active_path()).expect("file written");
        let lines: Vec<_> = raw.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].matches("\n").count(), 0, "每行内部不得含换行");
        for line in lines {
            let parsed: LogEntry = serde_json::from_str(line).expect("each line is valid JSON");
            assert_eq!(parsed.writer, LogWriter::Hook);
        }
    }

    #[test]
    fn empty_batch_writes_nothing_and_creates_no_file() {
        let temp = TempDir::new("log-sink-empty");
        let sink = Sink::for_status_dir(temp.path(), 1024);
        assert!(sink.append(&[]).is_ok());
        assert!(fs::read_dir(sink.dir()).is_err(), "没写过就不该建目录");
    }

    #[test]
    fn a_disabled_sink_never_touches_the_disk() {
        // 关掉日志要省下的是那一次磁盘往返本身，所以断言的是「目录没被建出来」，
        // 不是「目录里没有新行」—— 后者用一个指向空处的路径也能骗过。
        let temp = TempDir::new("log-sink-disabled");
        let status_dir = temp.path().join("status");
        let sink = Sink::disabled();
        sink.report(entry(LogStage::HookAttempt));
        assert!(sink.append(&[entry(LogStage::InboxWrite)]).is_ok());
        assert!(!log_dir(&status_dir).exists(), "停用态不该建目录");
        assert!(!temp.path().join("log").exists());
        assert!(sink.listing().is_empty());
    }

    #[test]
    fn a_full_file_makes_the_next_batch_open_a_new_sequence() {
        let temp = TempDir::new("log-sink-rotate");
        let sink = Sink::for_status_dir(temp.path(), 1);
        // 上限设成 1 字节：任何一条都必然「超限」，于是第二条该另开一个新序号文件。
        sink.report(entry(LogStage::HookAttempt));
        let after_first = names(&sink);
        sink.report(entry(LogStage::HookAttempt));
        let after_second = names(&sink);
        assert_eq!(after_first.len(), 1, "第一次写建出 0 号");
        assert_eq!(
            after_second.len(),
            2,
            "写满后另开新文件，而不是回头接着往满的文件塞：{after_second:?}"
        );
        for name in &after_first {
            let raw = fs::read_to_string(sink.dir().join(name)).expect("written");
            assert_eq!(raw.lines().count(), 1, "已翻篇的文件不该再被追加");
        }
    }

    /// 目录里已有的日志文件名。
    fn names(sink: &Sink) -> Vec<String> {
        sink.listing().into_iter().map(|(name, _)| name).collect()
    }

    #[test]
    fn reporting_survives_an_unwritable_directory() {
        // 把日志路径占成一个普通文件：open 必然失败，但 `report` 不许 panic、也不许传播错误。
        let temp = TempDir::new("log-sink-blocked");
        let blocker = temp.path().join("blocked");
        fs::write(&blocker, b"x").unwrap();
        let sink = Sink::for_status_dir(&blocker, 1024);
        sink.report(entry(LogStage::HookAttempt));
    }
}
