//! GUI 侧的日志运行态：配置热更新、增量 tail、内存环形缓冲、按天/按量回收、导出。
//!
//! # 为什么读端只有一条路
//!
//! 主进程自己写的行**也走同一个文件**，内存环形缓冲只由 tailer 喂 —— 于是「谁写了什么」
//! 在系统里只有一份真相，不需要「内存一份 + 文件一份」再去对账。这与
//! [`crate::modules::agent_status::status::codex_rollout`] 增量读 rollout 字节是同一个套路。
//!
//! # 为什么 hook 不需要跟 GUI 协商文件名
//!
//! 该写哪个文件完全由目录当前内容决定（见 [`super::paths::pick_active_seq`]），
//! 两个进程在同一刻读到同一份目录必然算出同一个名字。因此 hook 只管追加，
//! 本文件只管**读**（tail）与**删**（回收），三方零协调。

use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};

use super::super::model::now_ms;
use super::config::{LogConfig, LogLevel};
use super::paths::{
    evict_for_total_cap, file_name, is_expired, log_dir, parse_file_name, pick_active_seq, utc_day,
};
use super::record::{LogEntry, LogReason, LogStage, LogWriter};
use super::sink::Sink;

/// 环形缓冲里的一条：带一个单调递增的游标号，供前端「向上翻页」当 cursor 用。
#[derive(Debug, Clone)]
struct Ringed {
    /// 内存内单调序号（重启后从 0 重新起，因此只在本进程内有意义）。
    seq: u64,
    entry: LogEntry,
}

/// tail 游标：读到「哪一天的第几个文件的哪个字节」为止。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Cursor {
    day: String,
    seq: u32,
    offset: u64,
}

impl Cursor {
    /// `(日期, 序号)` 意义上的「更早」：日期是 ISO、序号单调递增，字典序即写入序。
    fn precedes(&self, other: &Cursor) -> bool {
        (self.day.as_str(), self.seq) < (other.day.as_str(), other.seq)
    }
}

/// 日志运行态的可变部分。只在 monitor 线程写、命令侧读，因此整体一把锁。
#[derive(Debug, Default)]
struct Runtime {
    /// 内存里留着的最近若干条（容量来自配置 `ring_capacity`）。
    ring: VecDeque<Ringed>,
    /// 下一条要发的游标号。
    next_seq: u64,
    /// 因为超出环形容量而被挤掉的历史条数（面板要说明「上面还有 N 条」）。
    dropped: u64,
    /// tail 游标：None = 还一次都没读过。
    cursor: Option<Cursor>,
    /// 面板是否开着：关着时不广播（环形缓冲照收，成本极低）。
    viewer: bool,
    /// 上次回收文件的时刻（与 monitor 的 10 秒清理同频）。
    last_sweep: i64,
    /// 是否已经做过启动播种（第一次只读文件尾巴，不把整个保留期灌进内存）。
    seeded: bool,
}

impl Runtime {
    /// 入环形缓冲，并把这条算作「本轮新增」（供广播，带着它的游标号）。满了就挤掉最旧的。
    fn push(&mut self, entry: LogEntry, capacity: usize, fresh: &mut Vec<StatusLogEntry>) {
        let seq = self.next_seq;
        self.next_seq = seq.saturating_add(1);
        fresh.push(StatusLogEntry { seq, entry: entry.clone() });
        self.ring.push_back(Ringed { seq, entry });
        while self.ring.len() > capacity {
            self.ring.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
    }
}

/// 跨线程共享的日志运行态：放进 `StatusState`，命令侧与 monitor 各拿一份克隆。
#[derive(Clone)]
pub struct LogState {
    /// 状态目录（日志目录由它派生，与 inbox 同级）。
    status_dir: PathBuf,
    /// 可热更新的配置：与 `retention_ms` 同一套写法（`Arc<Mutex<..>>`，改完即时生效）。
    config: Arc<Mutex<LogConfig>>,
    /// 可变运行态。
    runtime: Arc<Mutex<Runtime>>,
}

impl LogState {
    /// 建运行态（启动时读一次已落盘配置）。
    pub fn new(status_dir: PathBuf, config: LogConfig) -> Self {
        Self {
            status_dir,
            config: Arc::new(Mutex::new(config)),
            runtime: Arc::new(Mutex::new(Runtime::default())),
        }
    }

    /// 日志目录（面板要常驻显示它：`--status-dir` 覆盖时那些行落在别处）。
    pub fn dir(&self) -> PathBuf {
        log_dir(&self.status_dir)
    }

    /// 当前配置的一份拷贝。
    pub fn config(&self) -> LogConfig {
        self.config
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// 配置热更新（由组件配置钩子调用）。返回是否真的有变化。
    ///
    /// 「关闭」是唯一必须**强行**留痕的动作：关掉之后再没有一行写得进去，若不在关的
    /// 那一刻记一笔，事后就分不清「用户主动关了日志」与「日志坏了没记上」—— 这两种
    /// 结论指向完全相反的排障方向。开启与改档位走普通路径（那时本来就写得进去）。
    pub fn configure(&self, next: LogConfig) -> bool {
        let wrote_before = self.config().writes();
        let changed = {
            let mut guard = match self.config.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            let changed = guard.differs(&next);
            if changed {
                *guard = next.clone();
            }
            changed
        };
        if !changed {
            return false;
        }
        let level = super::record::wire_name(&next.level);
        if next.writes() {
            self.report(
                LogEntry::decided(LogWriter::Ui, LogStage::LogMaintenance, LogReason::LogOpened)
                    .with_detail(serde_json::json!({ "level": level })),
            );
        } else if wrote_before {
            self.force(LogEntry::decided(
                LogWriter::Ui,
                LogStage::LogMaintenance,
                LogReason::SinkDisabled,
            ));
        }
        true
    }

    /// 无视档位强制写一行。
    ///
    /// 只有一个正当用途：记录「日志被关闭了」这件事本身。其它地方一律走 [`report`]。
    fn force(&self, entry: LogEntry) {
        let config = self.config();
        Sink::for_status_dir(&self.status_dir, config.max_file_bytes).report(entry);
    }

    /// 当前档位是否覆盖 `needed`。埋点前的短路用它：日志关闭时**连条目都不构造**。
    pub fn covers(&self, needed: LogLevel) -> bool {
        self.config().covers(needed)
    }

    /// 写一条裁决级日志。**不返回结果** —— 写不进日志绝不能影响灯态。
    pub fn report(&self, entry: LogEntry) {
        self.report_at(LogLevel::Minimal, entry);
    }

    /// 写一条属于某个档位的日志。
    pub fn report_at(&self, needed: LogLevel, entry: LogEntry) {
        if !self.covers(needed) {
            return;
        }
        let config = self.config();
        Sink::for_status_dir(&self.status_dir, config.max_file_bytes).report(entry);
    }

    /// 写一批（monitor 一轮一次，省掉重复的 open/write）。
    pub fn report_batch(&self, entries: &[LogEntry]) {
        if entries.is_empty() || !self.covers(LogLevel::Minimal) {
            return;
        }
        let config = self.config();
        let _ = Sink::for_status_dir(&self.status_dir, config.max_file_bytes).append(entries);
    }

    /// **每轮调一次**：把目录里新写入的行读进环形缓冲，返回本轮新增的条目（供广播）。
    ///
    /// 顺序很关键 —— 必须在所有埋点之后调，否则本轮自己写的行要等下一轮才进内存。
    pub fn drain(&self) -> Vec<StatusLogEntry> {
        let config = self.config();
        if !config.writes() {
            return Vec::new();
        }
        let sink = Sink::for_status_dir(&self.status_dir, config.max_file_bytes);
        // 目录里所有认识的文件，按 `(日期, 序号)` 排好 —— 这就是写入顺序。
        let mut files: Vec<(String, u32, u64)> = sink
            .listing()
            .into_iter()
            .filter_map(|(name, size)| parse_file_name(&name).map(|(day, seq)| (day, seq, size)))
            .collect();
        files.sort_by(|left, right| (&left.0, left.1).cmp(&(&right.0, right.1)));
        let dir = self.dir();
        let Ok(mut runtime) = self.runtime.lock() else {
            return Vec::new();
        };
        // 启动播种：只读最新那个文件的尾巴，避免把整个保留期灌进内存。
        if !runtime.seeded {
            runtime.seeded = true;
            if let Some((day, seq, size)) = files.last() {
                runtime.cursor = Some(Cursor {
                    day: day.clone(),
                    seq: *seq,
                    offset: size.saturating_sub(config.live_tail_bytes),
                });
            }
        }
        let capacity = config.ring_capacity.max(1) as usize;
        let mut fresh = Vec::new();
        for (day, seq, size) in files {
            let from = match runtime.cursor.clone() {
                // 游标已经在这个文件之后：它早就读完了。
                Some(cursor) if cursor.precedes(&Cursor { day: day.clone(), seq, offset: 0 }) => {
                    continue;
                }
                // 游标正停在这个文件上：从上次断点继续（夹到当前大小，防被截断后越界）。
                Some(cursor) if cursor.day == day && cursor.seq == seq => cursor.offset.min(size),
                // 游标还没建立（目录第一次出现文件）：整份读。
                _ => 0,
            };
            if size <= from {
                continue;
            }
            let (lines, consumed) = read_from(&dir.join(file_name(&day, seq)), from, size);
            for line in lines {
                if let Some(entry) = parse_line(&line) {
                    runtime.push(entry, capacity, &mut fresh);
                }
            }
            runtime.cursor = Some(Cursor { day, seq, offset: consumed });
        }
        fresh
    }

    /// 按条件取一页（面板与导出共用）。返回**新→旧**，`before_seq` 是上一页最后一条的游标。
    pub fn page(&self, query: &StatusLogQuery) -> StatusLogPage {
        let Ok(runtime) = self.runtime.lock() else {
            return StatusLogPage::empty(self.dir());
        };
        // 前端传 0（或认不出的值）时按缺省页大小走，别让面板一次刷出空列表。
        let limit = usize::from(if query.limit == 0 { default_limit() } else { query.limit });
        let mut entries = Vec::with_capacity(limit.min(runtime.ring.len()));
        for item in runtime.ring.iter().rev() {
            if query.before_seq.is_some_and(|before| item.seq >= before) {
                continue;
            }
            if !query.matches(&item.entry) {
                continue;
            }
            entries.push(StatusLogEntry {
                seq: item.seq,
                entry: item.entry.clone(),
            });
            if entries.len() >= limit {
                break;
            }
        }
        StatusLogPage {
            head: runtime.next_seq,
            total: runtime.ring.len(),
            dropped: runtime.dropped,
            dir: self.dir().to_string_lossy().to_string(),
            entries,
        }
    }

    /// 把一批新条目包成「一页」（实时广播用）。
    ///
    /// 形状与 [`page`] **完全一致** —— 前端因此只需要一种数据结构：首屏拉取与增量推送
    /// 走同一个 `applyList`，不必为 live 路径再写一套解析。
    pub fn page_of(&self, entries: &[StatusLogEntry]) -> StatusLogPage {
        let Ok(runtime) = self.runtime.lock() else {
            return StatusLogPage::empty(self.dir());
        };
        StatusLogPage {
            head: runtime.next_seq,
            total: runtime.ring.len(),
            dropped: runtime.dropped,
            dir: self.dir().to_string_lossy().to_string(),
            entries: entries.to_vec(),
        }
    }

    /// 面板开关：只有开着才广播（省掉没人看时的 IPC）。
    pub fn set_viewer(&self, attached: bool) {
        if let Ok(mut runtime) = self.runtime.lock() {
            runtime.viewer = attached;
        }
    }

    /// 面板是否开着（monitor 据此决定要不要发那一批）。
    pub fn viewer_attached(&self) -> bool {
        self.runtime
            .lock()
            .map(|runtime| runtime.viewer)
            .unwrap_or(false)
    }

    /// 清空：**另起一个全新的活动文件**，其余日志文件删除、内存清零。
    ///
    /// ⚠️ 已知代价：抬升序号后，仍在按旧序号写入的 hook 进程会把那几条落进一个即将被删的
    /// 文件 —— 手动清空时丢几条在途信号是可接受的（与「会话记录的清除全部只删归档」同理，
    /// 这里做不到的是「保证一条不漏」）。
    pub fn clear(&self) {
        let config = self.config();
        let dir = self.dir();
        let today = utc_day(now_ms());
        let files = list_files(&dir);
        // 活动序号再 +1：旧文件里全是要删的东西，写者按规则会自然跟到新文件。
        let next = file_name(
            &today,
            pick_active_seq(&files, &today, config.max_file_bytes).saturating_add(1),
        );
        touch(&dir.join(&next));
        for (name, _) in &files {
            if name != &next {
                let _ = fs::remove_file(dir.join(name));
            }
        }
        if let Ok(mut runtime) = self.runtime.lock() {
            let mut cleared = Runtime::default();
            cleared.seeded = true;
            cleared.cursor = parse_file_name(&next).map(|(day, seq)| Cursor { day, seq, offset: 0 });
            *runtime = cleared;
        }
    }

    /// 按天与按量回收：**当天的活动文件永不删**（可能正被 hook 持有追加句柄）。
    ///
    /// 与 monitor 的 10 秒清理同频调用。返回被删的文件数，供调用方记一条 `LogRotated`。
    pub fn sweep(&self) -> usize {
        let config = self.config();
        let now = now_ms();
        let due = {
            let Ok(runtime) = self.runtime.lock() else {
                return 0;
            };
            now.saturating_sub(runtime.last_sweep) >= 5_000
        };
        if !due {
            return 0;
        }
        if let Ok(mut runtime) = self.runtime.lock() {
            runtime.last_sweep = now;
        }
        let dir = self.dir();
        let today = utc_day(now);
        let files = list_files(&dir);
        let active = file_name(
            &today,
            pick_active_seq(&files, &today, config.max_file_bytes),
        );
        let mut victims: Vec<String> = files
            .iter()
            .filter(|(name, _)| is_expired(name, &today, config.retention_days))
            .map(|(name, _)| name.clone())
            .collect();
        let active_seq = parse_file_name(&active).map_or(0, |(_, seq)| seq);
        victims.extend(evict_for_total_cap(&files, &today, active_seq, config.max_total_bytes));
        victims.sort();
        victims.dedup();
        let mut removed = 0;
        for name in victims {
            if name == active {
                continue;
            }
            match fs::remove_file(dir.join(&name)) {
                Ok(()) => removed += 1,
                // 文件被占用：本轮放弃，不重试也不报错（下一轮还会再来）。
                Err(_) => self.report(
                    LogEntry::decided(
                        LogWriter::Monitor,
                        LogStage::LogMaintenance,
                        LogReason::LogRotationSkippedFileBusy,
                    )
                    .with_detail(serde_json::json!({ "file": name })),
                ),
            }
        }
        removed
    }

    /// 导出一份诊断文件。
    ///
    /// `path = Some(...)`：写到用户在「另存为」对话框里选的位置；`None`：落到
    /// `<日志目录>/export/diagnose-*.jsonl`，作为对话框不可用时的退路。
    /// 表头写明「哪台机器、什么配置、按什么条件导出」，否则一份日志发出去没法判读。
    pub fn export_to(&self, path: Option<String>, query: &StatusLogQuery) -> Result<ExportedLog, String> {
        let page = self.page(query);
        let target = match path {
            Some(raw) => validate_export_target(&raw, &self.dir())?,
            None => {
                let dir = self.dir().join("export");
                fs::create_dir_all(&dir).map_err(|error| format!("无法创建导出目录：{error}"))?;
                dir.join(format!("diagnose-{}-{now}.jsonl", utc_day(now_ms()), now = now_ms()))
            }
        };
        let header = serde_json::json!({
            "kind": "ufo-status-log-export",
            "exported_at_ms": now_ms(),
            "app_version": env!("CARGO_PKG_VERSION"),
            "log_dir": page.dir,
            "config": self.config(),
            "query": query,
            "entries": page.entries.len(),
            "dropped_from_ring": page.dropped,
        });
        let mut body = header.to_string();
        body.push('\n');
        // 文件按**旧→新**写（与面板的新→旧相反）：发给人看的时间线顺着读才不别扭。
        for item in page.entries.iter().rev() {
            body.push_str(&item.entry.line());
            body.push('\n');
        }
        fs::write(&target, body).map_err(|error| format!("导出写入失败：{error}"))?;
        Ok(ExportedLog {
            path: target.to_string_lossy().to_string(),
            entries: page.entries.len(),
        })
    }
}

/// 导出允许的尾部：都必须是纯文本，写错后缀会让用户以为拿到的是能直接看的日志。
const EXPORT_SUFFIXES: [&str; 3] = ["jsonl", "log", "txt"];

/// 校验「另存为」交回来的目标路径 —— 这是命令接受调用方路径后**唯一**的防线。
///
/// 命令只授权给主窗（`capabilities/main-module-settings.json`），但既然路径来自参数，
/// 就得假设它可能被传错：
/// - 必须绝对路径：相对路径的基准是进程 cwd，文件会落到意想不到的地方；
/// - 后缀限定在 [`EXPORT_SUFFIXES`]；
/// - 拒绝 UNC（`\\主机\共享`）：一次写出可能变成凭据外发；
/// - 拒绝落在日志目录内：那里正被 tailer 增量读，覆盖活动文件等于把正在写的账截断。
fn validate_export_target(raw: &str, log_dir: &Path) -> Result<PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("导出路径不能为空".to_string());
    }
    if trimmed.starts_with(r"\\") {
        return Err("不支持导出到网络路径（UNC）".to_string());
    }
    let path = Path::new(trimmed);
    if !path.is_absolute() {
        return Err("导出路径必须是绝对路径".to_string());
    }
    let suffix_ok = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| EXPORT_SUFFIXES.contains(&ext.to_ascii_lowercase().as_str()));
    if !suffix_ok {
        return Err(format!("导出文件后缀只能是 {}", EXPORT_SUFFIXES.join(" / ")));
    }
    // 日志目录内一律拒绝：那里的文件名由 [`super::paths`] 约定，混进别的文件会干扰 tail 与回收。
    // ⚠️ 两条都要查，一条都挡不全：
    // - 字面比 —— 目标在一个**还没建出来**的子目录里时 canonicalize 会失败，只有字面比挡得住；
    // - 真实比 —— `..` 或目录链接绕行时，字面比会误放行，所以按 canonicalize 后的路径再比一次。
    // 父目录不存在而两条都判不出时按「不在内」放行：宁可少挡，也不许把用户选的正常路径顶回去。
    if under_dir(path, log_dir) {
        return Err("不能导出到日志目录内：那会干扰正在写入的日志".to_string());
    }
    let real_dir = log_dir.canonicalize().ok();
    let real_parent = path.parent().and_then(|parent| parent.canonicalize().ok());
    if let (Some(real_dir), Some(real_parent)) = (real_dir.as_deref(), real_parent) {
        if under_dir(&real_parent, real_dir) {
            return Err("不能导出到日志目录内：那会覆盖正在写入的活动日志".to_string());
        }
    }
    Ok(path.to_path_buf())
}

/// `path` 是否落在 `dir` 之下（按路径分量比，不按字符串前缀 —— 后者会把 `status-log` 当成 `status` 的子目录）。
fn under_dir(path: &Path, dir: &Path) -> bool {
    path.strip_prefix(dir).is_ok()
}

/// 从文件的 `from` 字节读到 `size`：返回**完整行**与新的读取位置。
///
/// ⚠️ 半行必须留给下一轮（hook 可能正在写），因此 `consumed` 只推进到最后一个换行符之后；
/// 否则下一轮从半行中间开始读，那条记录就永久丢了。
fn read_from(path: &Path, from: u64, size: u64) -> (Vec<String>, u64) {
    let Ok(mut file) = File::open(path) else {
        return (Vec::new(), from);
    };
    if file.seek(SeekFrom::Start(from)).is_err() {
        return (Vec::new(), from);
    }
    // 一轮最多读 4 MiB：积压再多也不至于把 monitor 卡在一轮里。
    let want = size.saturating_sub(from).min(4 * 1024 * 1024) as usize;
    let mut buffer = vec![0u8; want];
    let read = file.read(&mut buffer).unwrap_or(0);
    let chunk = &buffer[..read];
    let lines = chunk
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.iter().all(|byte| byte.is_ascii_whitespace()))
        .filter_map(|line| String::from_utf8(line.to_vec()).ok())
        .collect();
    let consumed = if chunk.last().is_some_and(|byte| *byte == b'\n') {
        from + read as u64
    } else {
        // 没有结尾换行：只认到最后一个换行符为止，前面没有换行就一个字节都不算读过。
        from + chunk
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index as u64 + 1)
    };
    (lines, consumed)
}

/// 反序列化一行；坏行直接丢（写到一半被截断的残留），**不报错、不重试**。
fn parse_line(line: &str) -> Option<LogEntry> {
    serde_json::from_str(line).ok()
}

/// 列目录里的 `(文件名, 字节数)`，只认本系统写的文件。
fn list_files(dir: &Path) -> Vec<(String, u64)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if parse_file_name(&name).is_none() {
            continue;
        }
        let size = fs::metadata(entry.path()).map(|meta| meta.len()).unwrap_or(0);
        files.push((name, size));
    }
    files
}

/// 建一个空文件（写入方下次算序号时就会落在它上面）。失败就忽略。
fn touch(path: &Path) {
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.flush();
    }
}

/// 查询条件（命令侧入参；全部可选，缺省 = 全量最近一页）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusLogQuery {
    /// 上一页最后一条的游标：只取比它更旧的。
    #[serde(default)]
    pub before_seq: Option<u64>,
    /// 一页多少条。
    #[serde(default = "default_limit")]
    pub limit: u16,
    /// 只看某个会话（`provider:session_id`）。
    #[serde(default)]
    pub key: Option<String>,
    /// 只看某条信号的完整链路。
    #[serde(default)]
    pub trace_id: Option<String>,
    /// 只看某个环节。
    #[serde(default)]
    pub stage: Option<String>,
    /// 只看某个理由。
    #[serde(default)]
    pub reason: Option<String>,
    /// 只看「没落成灯变化」的那些（被拒、被丢、被摘、只记账）。
    #[serde(default)]
    pub only_rejected: Option<bool>,
    /// 只要这个时刻之后的。
    #[serde(default)]
    pub since_ms: Option<i64>,
}

/// 缺省页大小：够看几分钟的流水，又不至于一次传几 MB。
fn default_limit() -> u16 {
    200
}

/// ⚠️ 手写而不是 `#[derive(Default)]`：derive 会把 `limit` 填成 `0`，而 serde 的
/// `#[serde(default = "default_limit")]` 填的是 200 —— 两个「缺省」不一致时，
/// Rust 侧构造的查询（导出、内部复用）会静默变成「一页 0 条」。
impl Default for StatusLogQuery {
    fn default() -> Self {
        Self {
            before_seq: None,
            limit: default_limit(),
            key: None,
            trace_id: None,
            stage: None,
            reason: None,
            only_rejected: None,
            since_ms: None,
        }
    }
}

impl StatusLogQuery {
    /// 判断一条日志是否命中过滤。前端传的是线协议字符串，**认不出的取值不匹配任何东西**
    /// （而不是「忽略该条件」—— 那会让一个拼错的过滤静默变成全量，比空列表更难查）。
    fn matches(&self, entry: &LogEntry) -> bool {
        if self.key.as_deref().is_some_and(|key| entry.key.as_deref() != Some(key)) {
            return false;
        }
        if self
            .trace_id
            .as_deref()
            .is_some_and(|trace| entry.trace_id.as_deref() != Some(trace))
        {
            return false;
        }
        if self
            .stage
            .as_deref()
            .is_some_and(|stage| wire(&entry.stage) != stage)
        {
            return false;
        }
        if self.reason.as_deref().is_some_and(|reason| {
            entry.reason.map(wire_of_reason).as_deref() != Some(reason)
        }) {
            return false;
        }
        if self.only_rejected == Some(true) && !is_rejection(entry) {
            return false;
        }
        if self.since_ms.is_some_and(|since| entry.ts_ms < since) {
            return false;
        }
        true
    }
}

/// 一页日志（命令返回值，也是实时广播的载荷）。
#[derive(Debug, Clone, Serialize)]
pub struct StatusLogPage {
    /// 当前环形缓冲的游标水位：前端拿它当「最新位置」，翻页时回填 `before_seq`。
    pub head: u64,
    /// 内存里现在留了多少条。
    pub total: usize,
    /// 被环形容量挤掉多少条（面板据此说明「更早的只能去文件里看」）。
    pub dropped: u64,
    /// 日志目录：面板常驻显示它，用户才知道去哪翻历史。
    pub dir: String,
    /// 命中的条目，**新→旧**。
    pub entries: Vec<StatusLogEntry>,
}

impl StatusLogPage {
    /// 取不到运行态时的空页（前端永远拿到一个结构完整的对象）。
    fn empty(dir: PathBuf) -> Self {
        Self {
            head: 0,
            total: 0,
            dropped: 0,
            dir: dir.to_string_lossy().to_string(),
            entries: Vec::new(),
        }
    }
}

/// 一条日志加上它的内存游标号（分页与「跟随最新」都靠它）。
#[derive(Debug, Clone, Serialize)]
pub struct StatusLogEntry {
    pub seq: u64,
    #[serde(flatten)]
    pub entry: LogEntry,
}

/// 一次导出的结果。
#[derive(Debug, Clone, Serialize)]
pub struct ExportedLog {
    /// 写在哪：用户选的绝对路径，或（未给路径时）日志目录内的 `export/` 子目录。
    pub path: String,
    /// 带出去多少条。
    pub entries: usize,
}

/// 取枚举的线协议字符串：统一走 serde，避免手抄一份 match 与枚举漂移。
fn wire<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// 同上，理由版。
fn wire_of_reason(reason: LogReason) -> String {
    wire(&reason)
}

/// 「这条信号没落成灯变化」的判据 —— 面板那个「只看被拒 / 被丢」开关走这里。
///
/// ⚠️ `CompatVerdict` 整块**不算**被拒：它同时承载「归一成功」的正常事件，
/// 只有那两个忽略/拒绝的理由才算。
fn is_rejection(entry: &LogEntry) -> bool {
    if matches!(
        entry.reason,
        Some(LogReason::AdapterIgnoredEvent | LogReason::AdapterRejected)
    ) {
        return true;
    }
    matches!(
        entry.stage,
        LogStage::KernelVerdict
            | LogStage::RolloutDropped
            | LogStage::InboxQuarantine
            | LogStage::SessionReaped
            | LogStage::IdleLanternCleared
            | LogStage::LightUnchanged
            | LogStage::SubagentAliveOnly
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::agent_status::status::test_support::TempDir;

    /// 造一条填好了会话坐标的日志（测试里到处要用，抽出来免得每处写 15 个字段）。
    fn entry(stage: LogStage, reason: Option<LogReason>) -> LogEntry {
        LogEntry {
            ts_ms: 1_758_940_800_000,
            writer: LogWriter::Monitor,
            stage,
            reason,
            trace_id: None,
            event_id: None,
            provider: Some("claude".to_string()),
            session_id: Some("s1".to_string()),
            key: Some("claude:s1".to_string()),
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

    /// 测试用的「正在记」配置。
    ///
    /// 产品默认是**关**（理由见 [`LogConfig`] 的默认值注释：开关语义要对得上、明细级会落用户文本），
    /// 而这一组测试验的是 tail / 环形缓冲 / 过滤 / 回收，必须先真的写下东西才有东西可读。
    fn recording() -> LogConfig {
        LogConfig {
            level: LogLevel::Normal,
            hook_enabled: true,
            ..LogConfig::default()
        }
    }

    #[test]
    fn writing_and_reading_round_trips_through_the_file() {
        let temp = TempDir::new("log-runtime-roundtrip");
        let log = LogState::new(temp.path().to_path_buf(), recording());
        log.report(entry(LogStage::LightTransition, Some(LogReason::PhaseChanged)));
        log.report(entry(LogStage::KernelVerdict, Some(LogReason::StaleSequence)));
        let fresh = log.drain();
        assert_eq!(fresh.len(), 2, "两条都该被 tail 回来");
        // 广播用的形状与查询结果一致：条目外面套一层游标号。
        assert_eq!(fresh[0].entry.stage, LogStage::LightTransition);
        assert_eq!(fresh[0].seq, 0, "游标号按写入顺序递增");
        assert_eq!(log.drain().len(), 0, "游标已推过去，第二轮不该重复");
        let page = log.page(&StatusLogQuery::default());
        assert_eq!(page.entries.len(), 2);
        assert_eq!(page.total, 2);
        assert_eq!(
            page.entries[0].entry.stage,
            LogStage::KernelVerdict,
            "面板顺序是新→旧"
        );
        // 翻页：`before_seq` 是「只取比它更旧的」，所以要拿第一页**末尾那条**的游标当锚点。
        let first = log.page(&StatusLogQuery {
            limit: 1,
            ..Default::default()
        });
        assert_eq!(first.entries.len(), 1);
        assert_eq!(first.head, 2, "head 是环形水位：前端靠它判断有没有新数据");
        let older = log.page(&StatusLogQuery {
            before_seq: Some(first.entries[0].seq),
            limit: 10,
            ..Default::default()
        });
        assert_eq!(older.entries.len(), 1);
        assert_eq!(older.entries[0].entry.stage, LogStage::LightTransition);
        // 翻到底就该是空的 —— 同一页反复捞回来会让「跟随最新」永远停在旧数据上。
        assert!(log
            .page(&StatusLogQuery {
                before_seq: Some(older.entries[0].seq),
                limit: 10,
                ..Default::default()
            })
            .entries
            .is_empty());
    }

    #[test]
    fn filters_narrow_by_key_stage_reason_and_rejection() {
        let temp = TempDir::new("log-runtime-filter");
        let log = LogState::new(temp.path().to_path_buf(), recording());
        log.report(entry(LogStage::LightTransition, Some(LogReason::PhaseChanged)));
        log.report(entry(LogStage::KernelVerdict, Some(LogReason::StaleSequence)));
        log.report(entry(LogStage::CompatVerdict, Some(LogReason::AdapterIgnoredEvent)));
        let _ = log.drain();
        let by_stage = log.page(&StatusLogQuery {
            stage: Some("kernel_verdict".to_string()),
            limit: 10,
            ..Default::default()
        });
        assert_eq!(by_stage.entries.len(), 1);
        assert_eq!(
            log.page(&StatusLogQuery {
                stage: Some("not_a_stage".to_string()),
                limit: 10,
                ..Default::default()
            })
            .entries
            .len(),
            0,
            "认不出的取值不该匹配上任何东西（拼错的过滤不能静默变成全量）"
        );
        assert_eq!(
            log.page(&StatusLogQuery {
                only_rejected: Some(true),
                limit: 10,
                ..Default::default()
            })
            .entries
            .len(),
            2,
            "被拒 + 被静默忽略，但不含归一成功的那条"
        );
        assert_eq!(
            log.page(&StatusLogQuery {
                reason: Some("stale_sequence".to_string()),
                limit: 10,
                ..Default::default()
            })
            .entries
            .len(),
            1
        );
        assert_eq!(
            log.page(&StatusLogQuery {
                key: Some("codex:s1".to_string()),
                limit: 10,
                ..Default::default()
            })
            .entries
            .len(),
            0
        );
    }

    #[test]
    fn clear_starts_a_new_file_and_forgets_history() {
        let temp = TempDir::new("log-runtime-clear");
        let log = LogState::new(temp.path().to_path_buf(), recording());
        log.report(entry(LogStage::HookAttempt, None));
        let _ = log.drain();
        assert_eq!(log.page(&StatusLogQuery::default()).entries.len(), 1);
        log.clear();
        assert_eq!(log.page(&StatusLogQuery::default()).entries.len(), 0);
        assert_eq!(
            list_files(&log.dir())
                .into_iter()
                .filter(|(name, size)| size > &0)
                .count(),
            0,
            "旧文件都该清掉，只剩一个空的活动文件"
        );
        // 清空后继续写要能读回来（游标跟着新文件走，否则面板从此不再更新）。
        log.report(entry(LogStage::LightTransition, Some(LogReason::PhaseChanged)));
        assert_eq!(log.drain().len(), 1);
    }

    #[test]
    fn off_level_writes_nothing_at_all() {
        let temp = TempDir::new("log-runtime-off");
        let mut config = LogConfig::default();
        config.level = LogLevel::Off;
        let log = LogState::new(temp.path().to_path_buf(), config);
        assert!(!log.covers(LogLevel::Minimal));
        log.report(entry(LogStage::HookAttempt, None));
        let _ = log.drain();
        assert!(log.page(&StatusLogQuery::default()).entries.is_empty());
        assert!(list_files(&log.dir()).is_empty(), "off 档一行都不该写");
    }

    #[test]
    fn viewer_flag_only_gates_broadcasting_not_recording() {
        let temp = TempDir::new("log-runtime-viewer");
        let log = LogState::new(temp.path().to_path_buf(), recording());
        assert!(!log.viewer_attached());
        log.report(entry(LogStage::HookAttempt, None));
        assert_eq!(log.drain().len(), 1, "没人看的时候照样记账");
        log.set_viewer(true);
        assert!(log.viewer_attached());
    }

    #[test]
    fn an_unwritable_directory_is_silent_not_fatal() {
        // 只验「不 panic、也不误报有数据」：日志再坏也不能把主进程带下去。
        // 用「文件底下再开目录」造一个必然失败的落点 —— 跨平台都成立，
        // 也不会像绝对路径那样在 Windows 上真的建到别的盘去。
        let temp = TempDir::new("log-runtime-blocked");
        let blocker = temp.path().join("blocker");
        fs::write(&blocker, b"x").expect("占位文件");
        let log = LogState::new(blocker.join("nested"), recording());
        log.report(entry(LogStage::HookAttempt, None));
        assert!(log.drain().is_empty());
        assert_eq!(log.sweep(), 0);
        log.clear();
        assert!(log.page(&StatusLogQuery::default()).entries.is_empty());
        assert!(log.export_to(None, &StatusLogQuery::default()).is_err());
    }

    #[test]
    fn export_writes_a_header_then_oldest_first() {
        let temp = TempDir::new("log-runtime-export");
        let log = LogState::new(temp.path().to_path_buf(), recording());
        log.report(entry(LogStage::HookAttempt, None));
        log.report(entry(LogStage::LightTransition, Some(LogReason::PhaseChanged)));
        let _ = log.drain();
        let exported = log
            .export_to(None, &StatusLogQuery::default())
            .expect("export ok");
        assert_eq!(exported.entries, 2);
        let raw = fs::read_to_string(&exported.path).expect("file exists");
        let lines: Vec<&str> = raw.lines().collect();
        assert_eq!(lines.len(), 3);
        let header: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(header["kind"], "ufo-status-log-export");
        assert!(header.get("config").is_some(), "表头要带当刻配置");
        assert!(exported.path.contains("export"), "只可能落在 export 子目录");
        // 文件里旧→新，与面板相反：时间线顺着读才不别扭。
        let first: LogEntry = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(first.stage, LogStage::HookAttempt);
    }

    #[test]
    fn export_writes_to_the_path_the_user_picked() {
        let temp = TempDir::new("log-runtime-export-picked");
        let log = LogState::new(temp.path().to_path_buf(), recording());
        log.report(entry(LogStage::HookAttempt, None));
        let _ = log.drain();
        let target = temp.path().join("诊断包.jsonl");
        let exported = log
            .export_to(
                Some(target.to_string_lossy().to_string()),
                &StatusLogQuery::default(),
            )
            .expect("写到用户选的位置");
        assert_eq!(exported.path, target.to_string_lossy());
        assert_eq!(exported.entries, 1);
        assert_eq!(fs::read_to_string(&target).unwrap().lines().count(), 2);
    }

    #[test]
    fn export_rejects_every_path_shape_that_could_write_somewhere_surprising() {
        let temp = TempDir::new("log-runtime-export-guard");
        let log = LogState::new(temp.path().to_path_buf(), recording());
        log.report(entry(LogStage::HookAttempt, None));
        let _ = log.drain();
        // 落点基准要问运行态自己（`<status_dir>/log`），手拼会指到别处，测试就成了空断言。
        let log_dir = log.dir();
        let query = StatusLogQuery::default();
        // 相对路径（基准是进程 cwd）、UNC（可能变成凭据外发）、非文本后缀（双击就跑东西）。
        for bad in [
            "out.jsonl".to_string(),
            r"..\..\outside.jsonl".to_string(),
            r"\\server\share\dump.jsonl".to_string(),
            temp.path().join("诊断包.exe").to_string_lossy().to_string(),
        ] {
            let error = log
                .export_to(Some(bad.clone()), &query)
                .expect_err(&format!("{bad} 必须被挡掉"));
            assert!(!error.is_empty(), "错误文本要能看懂为什么被挡");
        }
        // 日志目录内：直接放、以及放进一个还没建出来的子目录，两条都必须挡住。
        for inside in [
            log_dir.join("sneaky.jsonl"),
            log_dir.join("export").join("sneaky.jsonl"),
        ] {
            assert!(
                log.export_to(Some(inside.to_string_lossy().to_string()), &query)
                    .is_err(),
                "落在日志目录内的路径必须被拒：{:?}",
                inside
            );
        }
        // 被拒之后不能留下半个文件，环形缓冲也还在（导出失败不许把日志本身弄丢）。
        let strays: Vec<_> = list_files(&log_dir)
            .into_iter()
            .filter(|(name, _)| name.contains("sneaky"))
            .collect();
        assert!(strays.is_empty(), "被拒的路径不该在盘上留东西：{strays:?}");
        assert_eq!(log.page(&query).entries.len(), 1);
    }

    #[test]
    fn half_a_line_is_left_for_the_next_round() {
        let temp = TempDir::new("log-runtime-halfline");
        let path = temp.path().join("half.jsonl");
        fs::write(&path, b"{\"a\":1}\n{\"b\":2").unwrap();
        let size = fs::metadata(&path).unwrap().len();
        let (lines, consumed) = read_from(&path, 0, size);
        assert_eq!(lines.len(), 2, "两段文本都交出去，坏的那段由 parse_line 丢");
        // `{"a":1}` 是 7 字节 + 换行 = 8：只推进到换行之后，剩下半行留给下次。
        assert_eq!(consumed, 8, "半行不能被算成已读，否则那条永久丢");
    }

    #[test]
    fn ring_drops_oldest_and_counts_it() {
        let mut runtime = Runtime::default();
        for _ in 0..105 {
            let mut fresh = Vec::new();
            runtime.push(entry(LogStage::HookAttempt, None), 100, &mut fresh);
        }
        assert_eq!(runtime.ring.len(), 100);
        assert_eq!(runtime.dropped, 5);
        assert_eq!(runtime.next_seq, 105);
        assert_eq!(runtime.cursor, None, "记账不该动 tail 游标");
    }

    #[test]
    fn cursor_ordering_is_by_day_then_seq() {
        let old = Cursor {
            day: "2026-09-26".to_string(),
            seq: 9,
            offset: 0,
        };
        let new = Cursor {
            day: "2026-09-27".to_string(),
            seq: 0,
            offset: 0,
        };
        assert!(old.precedes(&new));
        assert!(!new.precedes(&old));
    }
}
