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
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};

use super::super::model::now_ms;
use super::config::{LogConfig, LogLevel};
use super::paths::{
    evict_for_total_cap, file_name, is_expired, log_dir, parse_file_name, pick_active_seq,
    utc_day, LogPosition,
};
use super::record::{LogEntry, LogReason, LogStage, LogWriter};
use super::sink::Sink;

/// 日志目录的占用读数：设置页常驻显示它，用户才知道这套账现在吃了多少盘。
///
/// 存在的理由是一条诚实性约束：保留天数是界面上唯一的旋钮，但体积上限那条硬界仍在背后
/// 削最旧的整天 —— 只说「保留 N 天」会变成一个会被静默打破的承诺。
#[derive(Debug, Clone, Default, Serialize)]
pub struct LogDiskUsage {
    /// 认识的文件合计字节数（不认识的名称不计入，与 [`super::paths::parse_file_name`] 同口径）。
    pub bytes: u64,
    /// 认识的文件个数。
    pub files: usize,
    /// 最早一天的日期（`YYYY-MM-DD`）；目录空时为 `None`。
    pub oldest_day: Option<String>,
    /// 本次运行内是否删过「还没到保留天数、只因超出体积上限而被削掉」的整天文件。
    /// ⚠️ 只说本次运行：进程启动前删掉的那次无从知道，宁可少说不猜。
    pub cut_by_size_cap: bool,
}

/// 占用刷新的最小间隔：与 `sweep()` 的节流同量级，免得每轮（500ms）都去 `read_dir`。
const USAGE_REFRESH_MS: i64 = 5_000;

/// 同一条查询两次计数之间的最小间隔：一次计数 = 把该条件涉及的目录整个翻完，
/// 节流 + 缓存是它唯一的成本防线（否则「换一次会话」就是一次全目录扫描）。
const COUNT_MIN_INTERVAL_MS: i64 = 3_000;
/// 计数缓存的条数（按查询的 JSON 认，超出逐出最旧的）。
const COUNT_CACHE_SLOTS: usize = 4;

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
    /// 进过内存的行数水位：面板只拿它判断「这一轮有没有新行」，不参与身份、翻页与计数。
    next_seq: u64,
    /// tail 游标：None = 还一次都没读过。
    cursor: Option<Cursor>,
    /// 此刻正在看日志的窗口 label：空 = 没人看，关着时不广播（盘上照样在写，读侧随时能按盘分页）。
    ///
    /// ⚠️ 按 label 记账而不是一个 bool：主窗的「会话记录」页与独立的转灯日志窗会各自
    /// attach / detach，单一开关被其中一方关掉时会把另一方也带成「没人看」（实时行就此停住）。
    viewers: Vec<String>,
    /// 上次回收文件的时刻（与 monitor 的 10 秒清理同频）。
    last_sweep: i64,
    /// 目录占用读数：由 [`LogState::refresh_usage`] 按 `USAGE_REFRESH_MS` 刷新，读侧只取这份。
    usage: LogDiskUsage,
    /// 上次刷新占用的时刻；`0` = 强制下一条来更新（`clear` / 删过文件后这么置）。
    usage_at: i64,
    /// 本次运行内是否因超出体积上限删过还没过保留期的文件（粘住，直到清空或重启）。
    size_cut: bool,
    /// 计数缓存：`(查询的 JSON, 结果)`。一次计数 = 把该条件涉及的目录翻完，是最贵的读，
    /// 所以同一条查询在 [`COUNT_MIN_INTERVAL_MS`] 内直接回这份缓存（`as_of_ms` 一起回，
    /// 界面自己显示「截至」）。有新行落盘 / 删过文件 / 清空 ⇒ 整份作废。
    counts: Vec<(String, StatusLogCount)>,
    /// 是否已经做过启动播种（第一次只读文件尾巴，不把整个保留期灌进内存）。
    seeded: bool,
}

impl Runtime {
    /// 把本轮 tail 到的一条记进「待广播」，并把水位往前推一格。
    ///
    /// ⚠️ 这里**不再留一份内存副本**：分页、计数、导出全部按盘读，环形缓冲就成了只写不读的
    /// 死结构（原先正是它给导出封顶）。`next_seq` 只剩「有没有新行进来过」这一个用途。
    fn record(&mut self, entry: LogEntry, pos: LogPosition, fresh: &mut Vec<StatusLogEntry>) {
        self.next_seq = self.next_seq.saturating_add(1);
        fresh.push(StatusLogEntry {
            id: pos.id(),
            entry,
        });
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
        let mut fresh = Vec::new();
        for (day, seq, size) in files {
            // 本文件的起点。拿它和游标比「谁在谁前面」，才知道这个文件读完了没有。
            let position = Cursor {
                day: day.clone(),
                seq,
                offset: 0,
            };
            let from = match runtime.cursor.clone() {
                // ⚠️ 方向必须是「本文件的起点 < 游标」，别写成 `cursor.precedes(&position)`：
                //   那样跳过的会是**比游标更新**的文件。跨天之后今天的文件永远读不到，
                //   游标还会被更早那个文件的位置顶回去，环形缓冲从此冻在旧账上 —— 盘上一路在写、
                //   面板却「本会话 0 条」。（同一天内看不出来，所以只有第二天才会暴露。）
                Some(cursor) if position.precedes(&cursor) => continue,
                // 游标正停在这个文件上：从上次断点继续（夹到当前大小，防被截断后越界）。
                Some(cursor) if cursor.day == day && cursor.seq == seq => cursor.offset.min(size),
                // 比游标更新的文件（跨天了 / 当天另起了一个），或游标还没建立：从文件头整份读。
                _ => 0,
            };
            if size <= from {
                continue;
            }
            let (lines, consumed) = read_from(
                &dir.join(file_name(&day, seq)),
                from,
                size,
                READ_WINDOW_BYTES,
            );
            for (offset, line) in lines {
                if let Some(entry) = parse_line(&line) {
                    // 每条都带着它刚被读出来的盘上坐标：实时广播与按盘分页因此是同一个身份。
                    runtime.record(
                        entry,
                        LogPosition {
                            day: day.clone(),
                            file_seq: seq,
                            offset,
                        },
                        &mut fresh,
                    );
                }
            }
            runtime.cursor = Some(Cursor { day, seq, offset: consumed });
        }
        // 盘上多了行 ⇒ 「截至某刻共 N 条」已经过期，缓存整份作废。
        // ⚠️ 这里必须在**已持有的守卫**上直接清：std Mutex 不可重入，调 `self.void_counts()`
        //   会当场自死锁（表现是 `cargo test` 一片挂住，而不是报错）。
        if !fresh.is_empty() {
            runtime.counts.clear();
        }
        fresh
    }

    /// 按条件取一页（面板与导出共用）。返回**新→旧**，`before_seq` 是上一页最后一条的游标。
    pub fn page(&self, query: &StatusLogQuery) -> StatusLogPage {
        // 前端传 0（或认不出的值）时按缺省页大小走，别让面板一次刷出空列表。
        let limit = usize::from(if query.limit == 0 { default_limit() } else { query.limit });
        let disk = super::scan::read_page(
            &self.dir(),
            query,
            limit,
            super::scan::SCAN_BYTE_BUDGET,
            super::scan::SCAN_BLOCK_BYTES,
        );
        let (head, usage) = match self.runtime.lock() {
            Ok(runtime) => (runtime.next_seq, runtime.usage.clone()),
            Err(poisoned) => (poisoned.into_inner().next_seq, LogDiskUsage::default()),
        };
        StatusLogPage {
            head,
            dir: self.dir().to_string_lossy().to_string(),
            usage,
            next_before: disk.next_before,
            exhausted: disk.exhausted,
            scanned_bytes: disk.scanned_bytes,
            entries: disk
                .entries
                .into_iter()
                .map(|(position, entry)| StatusLogEntry {
                    id: position.id(),
                    entry,
                })
                .collect(),
        }
    }

    /// 按盘精确计数（同一条查询在 [`COUNT_MIN_INTERVAL_MS`] 内走缓存）。
    ///
    /// 为什么不塞进 [`page`]：分页天生早停（凑满 `limit` 就返回），要出「命中总数」就必须
    /// 一路扫到目录头 —— 那等于把最贵的操作绑死在每次点击上。独立成一条命令才好控制时机、
    /// 才好缓存、也才好让界面显示「计数中…」。
    pub fn count(&self, query: &StatusLogQuery) -> StatusLogCount {
        let key = serde_json::to_string(query).unwrap_or_default();
        let now = now_ms();
        if let Ok(runtime) = self.runtime.lock() {
            if let Some((_, cached)) = runtime.counts.iter().find(|(name, _)| *name == key) {
                if now.saturating_sub(cached.as_of_ms) < COUNT_MIN_INTERVAL_MS {
                    return cached.clone();
                }
            }
        }
        let stats = super::scan::count_matching(&self.dir(), query, super::scan::SCAN_BYTE_BUDGET);
        let fresh = StatusLogCount {
            total: stats.matched,
            as_of_ms: now,
            scanned_bytes: stats.scanned_bytes,
        };
        if let Ok(mut runtime) = self.runtime.lock() {
            runtime.counts.retain(|(name, _)| *name != key);
            runtime.counts.push((key, fresh.clone()));
            // 只留最近几条：面板会换过滤条件，攒一堆用不到的键只是占地方。
            while runtime.counts.len() > COUNT_CACHE_SLOTS {
                runtime.counts.remove(0);
            }
        }
        fresh
    }

    /// 作废计数缓存：盘上多了行、少了文件或被清空，「截至某刻共 N 条」就不再是最新事实。
    fn void_counts(&self) {
        if let Ok(mut runtime) = self.runtime.lock() {
            runtime.counts.clear();
        }
    }

    /// 把一批新条目包成「一页」（实时广播用）。
    ///
    /// 形状与 [`page`] **完全一致** —— 前端因此只需要一种数据结构：首屏拉取与增量推送
    /// 走同一个 `applyPage`，不必为 live 路径再写一套解析。
    ///
    /// ⚠️ 增量包里的 `next_before` / `exhausted` / `scanned_bytes` **没有意义**（这几条不是
    /// 「从某个锚点往回翻」翻出来的），前端合并时只取 `entries`，翻页锚点一律以分页结果为准。
    pub fn page_of(&self, entries: &[StatusLogEntry]) -> StatusLogPage {
        let (head, usage) = match self.runtime.lock() {
            Ok(runtime) => (runtime.next_seq, runtime.usage.clone()),
            Err(poisoned) => (poisoned.into_inner().next_seq, LogDiskUsage::default()),
        };
        StatusLogPage {
            head,
            dir: self.dir().to_string_lossy().to_string(),
            usage,
            next_before: None,
            exhausted: false,
            scanned_bytes: 0,
            entries: entries.to_vec(),
        }
    }

    /// 面板开关：按调用方窗口 label 记账，只有还有窗口开着才广播（省掉没人看时的 IPC）。
    ///
    /// 同一个 label 重复 attach 只记一份，这样一次 detach 就能真的把它摘掉。
    pub fn set_viewer(&self, label: &str, attached: bool) {
        if let Ok(mut runtime) = self.runtime.lock() {
            if attached {
                if !runtime.viewers.iter().any(|item| item == label) {
                    runtime.viewers.push(label.to_string());
                }
            } else {
                runtime.viewers.retain(|item| item != label);
            }
        }
    }

    /// 是否还有窗口在看（monitor 据此决定要不要发那一批）。
    pub fn viewer_attached(&self) -> bool {
        self.runtime
            .lock()
            .map(|runtime| !runtime.viewers.is_empty())
            .unwrap_or(false)
    }

    /// 刷新日志目录的占用读数（monitor 每轮调一次，内部按 `USAGE_REFRESH_MS` 节流）。
    ///
    /// ⚠️ 这份数**不跟档位走**：日志关着只是不再写，盘上已有的文件仍会按保留期与体积上限
    /// 被回收，所以设置页的占用读数不能跟着 `level = off` 一起停更。
    pub fn refresh_usage(&self) {
        let now = now_ms();
        {
            let Ok(runtime) = self.runtime.lock() else {
                return;
            };
            if now.saturating_sub(runtime.usage_at) < USAGE_REFRESH_MS {
                return;
            }
        }
        // `list_files` 只认本系统写的文件，因此合计与回收口径一致（不会把别人的文件算进来）。
        let files = list_files(&self.dir());
        let bytes: u64 = files.iter().map(|(_, size)| *size).sum();
        let oldest_day = files
            .iter()
            .filter_map(|(name, _)| parse_file_name(name).map(|(day, _)| day))
            .min();
        let Ok(mut runtime) = self.runtime.lock() else {
            return;
        };
        let size_cut = runtime.size_cut;
        runtime.usage = LogDiskUsage {
            bytes,
            files: files.len(),
            oldest_day,
            cut_by_size_cap: size_cut,
        };
        runtime.usage_at = now;
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
            // `viewers` 是「哪个窗口正开着」的运行时事实，跟盘上有没有内容无关：
            // 跟着 default 归零的话，用户点一次「清空日志」后面板就再也收不到实时行了。
            cleared.viewers = std::mem::take(&mut runtime.viewers);
            cleared.cursor = parse_file_name(&next).map(|(day, seq)| Cursor { day, seq, offset: 0 });
            *runtime = cleared;
        }
        // 清空改动了盘上的量：让下一次刷新立刻重算（`usage_at` 已随 `Runtime::default()` 归零），
        // 否则命令紧接着返回的那一页仍带着清空前的占用数字。
        self.refresh_usage();
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
        // 过了保留期的整天：这批是"用户说的保留期到了"，删它不算食言。
        let expired: HashSet<String> = files
            .iter()
            .filter(|(name, _)| is_expired(name, &today, config.retention_days))
            .map(|(name, _)| name.clone())
            .collect();
        let mut victims: Vec<String> = expired.iter().cloned().collect();
        let active_seq = parse_file_name(&active).map_or(0, |(_, seq)| seq);
        // 体积上限那条名单里可能混着 expired 的，两路合并后照常去重。
        victims.extend(evict_for_total_cap(&files, &today, active_seq, config.max_total_bytes));
        victims.sort();
        victims.dedup();
        let mut removed = 0;
        let mut size_cut = false;
        for name in victims {
            if name == active {
                continue;
            }
            match fs::remove_file(dir.join(&name)) {
                Ok(()) => {
                    removed += 1;
                    // 没到保留期却被删 = 是体积上限动的手。这件事必须记下来：设置页只有
                    // 「保留 N 天」一个旋钮，不记就等于让用户以为删掉的账还在。
                    if !expired.contains(&name) {
                        size_cut = true;
                    }
                }
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
        if removed > 0 || size_cut {
            if let Ok(mut runtime) = self.runtime.lock() {
                runtime.size_cut |= size_cut;
                // 盘上的量变了：让下一轮 `refresh_usage` 立刻重算，而不是再等满节流窗口。
                runtime.usage_at = 0;
            }
            // 删过文件 ⇒ 计数同样过期。
            self.void_counts();
        }
        removed
    }

    /// 导出一份诊断文件：**按当前条件把盘上命中的行全部写出去**。
    ///
    /// `path = Some(...)`：写到用户在「另存为」对话框里选的位置；`None`：落到
    /// `<日志目录>/export/diagnose-*.jsonl`，作为对话框不可用时的退路。
    ///
    /// ⚠️ 数据源是磁盘，不是内存环形缓冲：原先这里调 `page()`，于是「导出」被环形容量
    /// 悄悄封顶（默认 5000 条）—— 用户以为拿到了完整账，其实只有最近那一段。现在逐文件
    /// 正序流式写（`scan::scan_forward` + `BufWriter`），条数上界由目录的保留策略决定。
    /// 表头只写「开打前就知道的事」（机器、配置、条件、覆盖范围）；条数要扫完才知道，
    /// 所以它在返回值里给界面，不在表头里留一个骗人的占位。
    pub fn export_to(&self, path: Option<String>, query: &StatusLogQuery) -> Result<ExportedLog, String> {
        // 生产入口用完整预算；单测走 `_with_budget` 来验证「被截断」这条分支真的会说出去。
        self.export_to_with_budget(path, query, super::scan::SCAN_BYTE_BUDGET)
    }

    /// 同上，但扫描预算由调用方给（只为了能把「预算截断 ⇒ 结果不完整」这条路径测出来）。
    fn export_to_with_budget(
        &self,
        path: Option<String>,
        query: &StatusLogQuery,
        budget: u64,
    ) -> Result<ExportedLog, String> {
        let dir = self.dir();
        let target = match path {
            Some(raw) => validate_export_target(&raw, &dir)?,
            None => {
                let export_dir = dir.join("export");
                fs::create_dir_all(&export_dir)
                    .map_err(|error| format!("无法创建导出目录：{error}"))?;
                export_dir.join(format!(
                    "diagnose-{}-{now}.jsonl",
                    utc_day(now_ms()),
                    now = now_ms()
                ))
            }
        };
        let header = serde_json::json!({
            "kind": "ufo-status-log-export",
            "exported_at_ms": now_ms(),
            "app_version": env!("CARGO_PKG_VERSION"),
            "log_dir": dir.to_string_lossy(),
            "config": self.config(),
            "query": query,
            // 覆盖范围说死：导出的边界是盘上的保留策略，不再是环形容量。
            "coverage": "按 query 扫过日志目录内全部命中行（受 retention_days / max_total_bytes 约束）",
        });
        let file = File::create(&target).map_err(|error| format!("导出文件创建失败：{error}"))?;
        let mut writer = BufWriter::with_capacity(64 * 1024, file);
        // 写失败只记第一次：盘都写坏了，后面每条都会失败，重复赋值没有意义，也不许在这里 panic。
        let mut failure: Option<String> = None;
        if let Err(error) = writer
            .write_all(header.to_string().as_bytes())
            .and_then(|_| writer.write_all(b"\n"))
        {
            failure = Some(format!("导出写入失败：{error}"));
        }
        // 文件按**旧→新**写（与面板的新→旧相反）：发给人看的时间线顺着读才不别扭。
        let stats = super::scan::scan_forward(&dir, query, budget, |_, entry| {
            if failure.is_some() {
                return;
            }
            let line = entry.line();
            if let Err(error) = writer
                .write_all(line.as_bytes())
                .and_then(|_| writer.write_all(b"\n"))
            {
                failure = Some(format!("导出写入失败：{error}"));
            }
        });
        if failure.is_none() {
            if let Err(error) = writer.flush() {
                failure = Some(format!("导出写入失败：{error}"));
            }
        }
        drop(writer);
        if let Some(error) = failure {
            // 失败不留半个文件：用户拿到的要么是一份完整的账，要么什么都没有。
            let _ = fs::remove_file(&target);
            return Err(error);
        }
        Ok(ExportedLog {
            path: target.to_string_lossy().to_string(),
            entries: stats.matched,
            scanned_bytes: stats.scanned_bytes,
            stopped_by_budget: stats.stopped_by_budget,
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

/// 一轮里最多读多少字节：积压再多也不至于把 monitor 卡在一轮里。
pub(super) const READ_WINDOW_BYTES: u64 = 4 * 1024 * 1024;

/// 从文件的 `from` 字节读到 `size`（单次最多 `window` 字节）：返回**行**（各带它的起始
/// 绝对偏移）与新的读取位置。
///
/// ⚠️ 两条推进规则方向相反，都必须在：
/// - 尾部**半行**留给下一轮（hook 可能正在写），所以 `consumed` 只推进到最后一个换行之后 ——
///   推过了头，那条记录就永久丢了（本项目靠这条纪律避过一次漏读）。
/// - 但**整个窗口一个换行都没有**时不能原地不动：那是一条比窗口还长的行，游标不动就等于
///   tailer 每轮重读同一窗、从此再也跟不到新行。此时按「后面还有没有字节」区分两者 ——
///   读满了窗口还没换行 ⇒ 超长行，整窗当坏行跳过；正好读到文件尾 ⇒ 是在写的半行，不动。
pub(super) fn read_from(path: &Path, from: u64, size: u64, window: u64) -> (Vec<(u64, String)>, u64) {
    let Ok(mut file) = File::open(path) else {
        return (Vec::new(), from);
    };
    if file.seek(SeekFrom::Start(from)).is_err() {
        return (Vec::new(), from);
    }
    let want = size.saturating_sub(from).min(window) as usize;
    let mut buffer = vec![0u8; want];
    let read = file.read(&mut buffer).unwrap_or(0);
    let chunk = &buffer[..read];
    // `split_inclusive` 让每段自带结尾换行，于是「这段占多少字节」可以直接累进偏移；
    // 最后一段通常没有换行（那正是正在被追加的半行），照旧交出去由 `parse_line` 判生死。
    let mut lines: Vec<(u64, String)> = Vec::new();
    let mut cursor = from;
    for piece in chunk.split_inclusive(|byte| *byte == b'\n') {
        let body = match piece.split_last() {
            Some((b'\n', rest)) => rest,
            _ => piece,
        };
        if !body.iter().all(|byte| byte.is_ascii_whitespace()) {
            if let Ok(line) = String::from_utf8(body.to_vec()) {
                lines.push((cursor, line));
            }
        }
        cursor = cursor.saturating_add(piece.len() as u64);
    }
    let consumed = if chunk.last().is_some_and(|byte| *byte == b'\n') {
        from + read as u64
    } else {
        match chunk.iter().rposition(|byte| *byte == b'\n') {
            Some(index) => from + index as u64 + 1,
            // 窗口读满还没有换行 ⇒ 后面仍有字节，这是一条比窗口还长的行：整窗当坏行跳过。
            None if (from + read as u64) < size => from + read as u64,
            // 正好读到文件尾 ⇒ 是正在被追加的半行，一个字节都不算读过，留给下一轮。
            None => from,
        }
    };
    (lines, consumed)
}

/// 反序列化一行；坏行直接丢（写到一半被截断的残留），**不报错、不重试**。
pub(super) fn parse_line(line: &str) -> Option<LogEntry> {
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
    /// 上一页最后一条的**盘上坐标**：只取比它更旧的。`None` = 从最新一条开始。
    ///
    /// ⚠️ 这里不留「进程内游标」那种锚点：它在重启后归零，跨会话翻页会串到别的行上去。
    #[serde(default)]
    pub before: Option<LogPosition>,
    /// 一页多少条 —— **只有分页读它**：导出与计数忽略它（它们要的是盘上全部命中）。
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
            before: None,
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
    ///
    /// 按盘扫描（`scan.rs`）也用它做最终判定：那边只走子串预筛，真判在这里。
    pub(super) fn matches(&self, entry: &LogEntry) -> bool {
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
    /// 环形缓冲的水位：只用来判断「这一轮有没有新行进过内存」，不参与身份、翻页与计数。
    pub head: u64,
    /// 日志目录：面板常驻显示它，用户才知道去哪翻历史。
    pub dir: String,
    /// 目录占用读数：设置页与浮层都靠它说清"这套账吃了多少盘、最早到哪天"。
    pub usage: LogDiskUsage,
    /// 下一页的锚点（本页**最后一条**的盘上坐标）；实时增量包里没有意义，为 `None`。
    pub next_before: Option<LogPosition>,
    /// 已经翻到目录头：面板据此收起「看更早的」，否则它会一直点、一直空。
    pub exhausted: bool,
    /// 为凑齐这一页读了多少字节（极端目录上「这一页为什么慢」的唯一线索）。
    pub scanned_bytes: u64,
    /// 命中的条目，**新→旧**。
    pub entries: Vec<StatusLogEntry>,
}

/// 一条日志加上它的盘上身份。
#[derive(Debug, Clone, Serialize)]
pub struct StatusLogEntry {
    /// 行身份（[`LogPosition::id`]）：前端 `:key`、合并去重、选中比对都只用它。
    ///
    /// ⚠️ 原先这里还带一个进程内 `seq` 当翻页游标 —— 盘上坐标能同时当身份与游标，
    /// 两个字段留着就会有人拿错那个（重启归零的 `seq` 去翻页会串到别的行上）。
    pub id: String,
    #[serde(flatten)]
    pub entry: LogEntry,
}

/// 一次按盘计数的结果。
#[derive(Debug, Clone, Serialize)]
pub struct StatusLogCount {
    /// 命中总条数 —— **精确值**，不是下界，所以界面再也不许出现「N+ / 约」。
    pub total: u64,
    /// 这次扫描开始的时刻：计数是一个瞬时快照，界面必须说成「截至 HH:MM:SS 共 N 条」。
    /// 有了这个时刻，"会过期的精确数"就变成"永远为真的历史事实"，不需要任何含糊写法。
    pub as_of_ms: i64,
    /// 实际扫描字节（诊断「这个数为什么慢」）。
    pub scanned_bytes: u64,
}

/// 一次导出的结果。
#[derive(Debug, Clone, Serialize)]
pub struct ExportedLog {
    /// 写在哪：用户选的绝对路径，或（未给路径时）日志目录内的 `export/` 子目录。
    pub path: String,
    /// 实际写到文件里多少条 = **盘上命中总数**（不再是「内存环形装得下的那一段」）。
    pub entries: u64,
    /// 为拿到这份结果读了多少字节：判「这次导出为什么慢」的唯一线索。
    pub scanned_bytes: u64,
    /// 撞到扫描预算 ⇒ 这份**不完整**。正常情况下恒为 false，为真时界面必须说出来。
    pub stopped_by_budget: bool,
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
            // ⚠️ 时刻必须跟着真实时钟：条目落进哪一天的文件由 `ts_ms` 决定，而 `clear()`
            // 是按 `utc_day(now)` 建活动文件的。夹具写死日期会让「清空后还读得回来」那条
            // 测试变成「日期恰好与今天同一天才成立」。
            ts_ms: now_ms(),
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
        // 广播用的形状与查询结果一致：条目外面套一层**盘上身份**（不是进程内游标）。
        assert_eq!(fresh[0].entry.stage, LogStage::LightTransition);
        assert!(
            !fresh[0].id.is_empty(),
            "增量行也要带盘上坐标，否则前端没法把它和分页结果认成同一条"
        );
        assert_eq!(log.drain().len(), 0, "游标已推过去，第二轮不该重复");
        let page = log.page(&StatusLogQuery::default());
        assert_eq!(page.entries.len(), 2);
        assert_eq!(
            page.entries[0].entry.stage,
            LogStage::KernelVerdict,
            "面板顺序是新→旧"
        );
        // 翻页：锚点 = 本页最后一条的盘上坐标，下一页严格取比它更旧的（锚点自身不再出现）。
        let first = log.page(&StatusLogQuery {
            limit: 1,
            ..Default::default()
        });
        assert_eq!(first.entries.len(), 1);
        assert_eq!(first.head, 2, "head 是环形水位：前端靠它判断有没有新行进来");
        let anchor = first.next_before.clone().expect("分页必须给出下一页的锚点");
        let older = log.page(&StatusLogQuery {
            before: Some(anchor),
            limit: 10,
            ..Default::default()
        });
        assert_eq!(older.entries.len(), 1);
        assert_eq!(older.entries[0].entry.stage, LogStage::LightTransition);
        assert!(
            older.exhausted,
            "翻到目录头要声明到底了，否则「看更早的」会一直点、一直空"
        );
        // 再翻一次就该是空的（同一页反复捞回来会让「跟随最新」永远停在旧数据上）。
        assert!(log
            .page(&StatusLogQuery {
                before: older.next_before.clone(),
                limit: 10,
                ..Default::default()
            })
            .entries
            .is_empty());
    }

    /// 计数按盘数、同查询走缓存、有新行就作废。
    #[test]
    fn count_is_exact_on_disk_and_cached_until_new_lines_arrive() {
        let temp = TempDir::new("log-runtime-count-cache");
        let log = LogState::new(temp.path().to_path_buf(), recording());
        log.report_batch(&(0..12).map(|_| entry(LogStage::LightTransition, None)).collect::<Vec<_>>());
        assert_eq!(log.drain().len(), 12, "先把这 12 行 tail 掉，水位才对齐");
        let query = StatusLogQuery::default();
        assert_eq!(log.count(&query).total, 12, "精确数，不是下界 —— 界面因此不许写「12+」");

        // 同一条查询 3 秒内第二次问：该命中缓存（`as_of_ms` 一模一样，也不该多扫一遍盘）。
        let first = log.count(&query);
        let second = log.count(&query);
        assert_eq!(second.as_of_ms, first.as_of_ms, "节流窗口内该回同一份缓存");
        assert_eq!(second.scanned_bytes, first.scanned_bytes, "缓存命中时不该重新扫盘");

        // 又落一条 ⇒ 「截至那刻共 N 条」过期，缓存必须整份作废、重新扫。
        log.report(entry(LogStage::SessionReaped, None));
        assert_eq!(log.drain().len(), 1);
        let refreshed = log.count(&query);
        assert_eq!(refreshed.total, 13);
        assert_ne!(refreshed.as_of_ms, first.as_of_ms, "该给出一个新的「截至」时刻");

        // 换一条查询（按会话过滤）也要各算各的，不共用一份缓存。
        let by_key = StatusLogQuery {
            key: Some("claude:s1".to_string()),
            ..Default::default()
        };
        assert_eq!(log.count(&by_key).total, 13, "夹具里的行都带同一个 key");
        let other = StatusLogQuery {
            key: Some("codex:nope".to_string()),
            ..Default::default()
        };
        assert_eq!(log.count(&other).total, 0);
    }

    #[test]
    fn an_older_day_file_must_not_starve_the_current_one() {
        // 跨天才暴露的那条：目录里同时有「更早一天」和「今天」两个文件时，
        // 播种只该管最新那个，旧文件既不该整份灌进内存，更不该把游标顶回去、
        // 让今天的行永远读不到（面板表现为「本会话 0 条」，而盘上一直在写）。
        let temp = TempDir::new("log-runtime-two-days");
        let log = LogState::new(temp.path().to_path_buf(), recording());
        log.report(entry(LogStage::LightTransition, Some(LogReason::PhaseChanged)));
        log.report(entry(LogStage::KernelVerdict, Some(LogReason::StaleSequence)));

        let stale: String = (0..3)
            .map(|_| serde_json::to_string(&entry(LogStage::IdleLanternCleared, None)).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        fs::write(log.dir().join(file_name("2000-01-01", 0)), stale).expect("种一个更早一天的文件");

        let fresh = log.drain();
        assert_eq!(
            fresh
                .iter()
                .filter(|item| item.entry.stage == LogStage::IdleLanternCleared)
                .count(),
            0,
            "播种只看最新文件的尾巴：旧一天的行不该被整份灌进这一轮"
        );
        assert_eq!(fresh.len(), 2, "今天的两行必须进这一轮");

        // 第二轮：新写的一行要读得回来。游标一旦被旧文件顶回去，这一轮就永远是 0。
        log.report(entry(LogStage::SessionReaped, None));
        assert_eq!(log.drain().len(), 1, "跨天后的第二轮还得跟得上");
        // 按盘分页会把「更早一天」那三行一起给出 —— 这正是改造的目的（能翻到昨天），
        // 与 tailer 的播种范围是两件事：后者只决定「哪几行算本轮新增」。
        let page = log.page(&StatusLogQuery {
            key: Some("claude:s1".to_string()),
            limit: 50,
            ..Default::default()
        });
        assert_eq!(page.entries.len(), 6, "今天三条 + 种下的旧一天三条，同属这个会话的都该捞到");
        assert!(
            page.entries[0].entry.stage == LogStage::SessionReaped,
            "顺序仍是新→旧"
        );
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

    /// 占用读数的基本形状：几个文件、多少字节、最早哪天，没删过就不许宣称削过。
    #[test]
    fn usage_reports_bytes_files_and_the_oldest_day() {
        let temp = TempDir::new("log-runtime-usage");
        let log = LogState::new(temp.path().to_path_buf(), recording());
        log.refresh_usage();
        let empty = log.page(&StatusLogQuery::default()).usage;
        assert_eq!(empty.files, 0, "空目录该报 0 个文件");
        assert_eq!(empty.oldest_day, None, "没有文件就没有「最早一天」");

        // ⚠️ 刷新按 `USAGE_REFRESH_MS` 节流：上面那一次已经把 `usage_at` 拨到当下，
        // 所以「写过之后」要换一份运行态来验，否则第二次刷新被节流挡掉、读数还是旧的。
        let written_temp = TempDir::new("log-runtime-usage-written");
        let written = LogState::new(written_temp.path().to_path_buf(), recording());
        written.report(entry(LogStage::LightTransition, Some(LogReason::PhaseChanged)));
        written.refresh_usage();
        let usage = written.page(&StatusLogQuery::default()).usage;
        assert_eq!(usage.files, 1);
        assert!(usage.bytes > 0, "写过一行就该有字节数");
        assert_eq!(usage.oldest_day.as_deref(), Some(utc_day(now_ms()).as_str()));
        assert!(!usage.cut_by_size_cap, "一次都没删过，不该宣称被容量削过");
    }

    /**
     * 「没到保留期就被删」必须与「到期了才删」分得开 —— 界面上只有天数这一个旋钮，
     * 把容量线的删除说成天线的结果，等于让用户以为他要的 90 天还在。
     */
    #[test]
    fn a_size_eviction_is_flagged_but_a_retention_one_is_not() {
        let yesterday = utc_day(now_ms() - 86_400_000);

        // 留 90 天：昨天那个文件远没到期，只有超出总量上限才会动它。
        let temp = TempDir::new("log-runtime-usage-cut");
        let log = LogState::new(
            temp.path().to_path_buf(),
            LogConfig {
                retention_days: 90,
                max_total_bytes: 64 * 1024,
                ..recording()
            },
        );
        log.report(entry(LogStage::LightTransition, Some(LogReason::PhaseChanged)));
        fs::write(
            log.dir().join(file_name(&yesterday, 0)),
            "x".repeat(100 * 1024),
        )
        .expect("种一个比总量上限还大的昨天文件");
        assert_eq!(log.sweep(), 1, "总量超上限：从最旧的整天开始删");
        log.refresh_usage();
        assert!(
            log.page(&StatusLogQuery::default()).usage.cut_by_size_cap,
            "删的是没到期的一天 = 容量线动的手，必须报出来"
        );

        // 同一天的文件，换成「到期了」的情形：同样的删除动作，含义完全不同。
        let temp2 = TempDir::new("log-runtime-usage-expired");
        let log2 = LogState::new(
            temp2.path().to_path_buf(),
            LogConfig {
                retention_days: 1,
                max_total_bytes: 512 * 1024 * 1024,
                ..recording()
            },
        );
        // 这一份运行态一次都没写过（目录还不存在），所以先把日志目录建出来再种文件。
        fs::create_dir_all(log2.dir()).expect("日志目录");
        fs::write(log2.dir().join(file_name(&yesterday, 0)), "x").expect("种一个昨天的文件");
        assert_eq!(log2.sweep(), 1, "过保留期该删掉昨天");
        log2.refresh_usage();
        assert!(
            !log2.page(&StatusLogQuery::default()).usage.cut_by_size_cap,
            "到期的删除不该算容量削的"
        );
    }

    /// 清空是一次「重新开始」：占用归零，粘住的容量削标记也不该再挂着。
    #[test]
    fn clear_resets_usage_and_the_size_cut_flag() {
        let temp = TempDir::new("log-runtime-usage-clear");
        let yesterday = utc_day(now_ms() - 86_400_000);
        let log = LogState::new(
            temp.path().to_path_buf(),
            LogConfig {
                retention_days: 90,
                max_total_bytes: 64 * 1024,
                ..recording()
            },
        );
        fs::create_dir_all(log.dir()).expect("日志目录");
        fs::write(
            log.dir().join(file_name(&yesterday, 0)),
            "x".repeat(70 * 1024),
        )
        .expect("种一个必须被容量线削掉的文件");
        assert_eq!(log.sweep(), 1);
        log.refresh_usage();
        assert!(log.page(&StatusLogQuery::default()).usage.cut_by_size_cap);

        log.clear();
        let usage = log.page(&StatusLogQuery::default()).usage;
        assert!(!usage.cut_by_size_cap, "清空之后不该还欠着一次容量回收的说法");
        assert_eq!(usage.files, 1, "只剩那个新建的空活动文件");
        assert_eq!(usage.bytes, 0);
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
        log.set_viewer("main", true);
        assert!(log.viewer_attached());
    }

    /// 主窗的「会话记录」页与独立的转灯日志窗各自 attach / detach：
    /// 任何一方关掉都不能把另一方也带成「没人看」，全关掉才真的停广播。
    #[test]
    fn viewers_are_counted_per_window_label() {
        let temp = TempDir::new("log-runtime-viewers");
        let log = LogState::new(temp.path().to_path_buf(), recording());
        log.set_viewer("main", true);
        log.set_viewer("status-log", true);
        log.set_viewer("main", false);
        assert!(log.viewer_attached(), "日志窗还开着，实时行不能停");
        log.set_viewer("status-log", false);
        assert!(!log.viewer_attached(), "两个窗口都关了才该停广播");

        // 同一个 label 重复 attach 只记一份：一次 detach 就该真摘掉。
        log.set_viewer("main", true);
        log.set_viewer("main", true);
        log.set_viewer("main", false);
        assert!(!log.viewer_attached());
    }

    /// 清空只动盘上的内容，不动「谁正在看」：否则面板挂着也再也收不到实时行。
    #[test]
    fn clear_keeps_the_attached_viewers() {
        let temp = TempDir::new("log-runtime-clear-viewers");
        let log = LogState::new(temp.path().to_path_buf(), recording());
        log.set_viewer("status-log", true);
        log.report(entry(LogStage::HookAttempt, None));
        log.clear();
        assert!(log.page(&StatusLogQuery::default()).entries.is_empty());
        assert!(log.viewer_attached(), "清空不该把窗口那侧的订阅一起抹掉");
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
        let (lines, consumed) = read_from(&path, 0, size, READ_WINDOW_BYTES);
        assert_eq!(lines.len(), 2, "两段文本都交出去，坏的那段由 parse_line 丢");
        // `{"a":1}` 是 7 字节 + 换行 = 8：只推进到换行之后，剩下半行留给下次。
        assert_eq!(consumed, 8, "半行不能被算成已读，否则那条永久丢");
        // 第一条的起始偏移就是 0，第二条落在换行之后 —— 行身份全靠这两个数。
        assert_eq!(lines[0].0, 0);
        assert_eq!(lines[1].0, 8);
    }

    /**
     * 一条比窗口还长的行**不许把 tailer 卡死**：游标必须往前爬，否则每轮重读同一窗，
     * 后面所有新行就永远读不到了（这是 `read_from` 旧实现里的真实停滞形态）。
     */
    #[test]
    fn a_line_longer_than_the_window_does_not_stall_the_reader() {
        let temp = TempDir::new("log-runtime-longline");
        let path = temp.path().join("long.jsonl");
        // 一条 100 字节、中间一个换行都没有的行，后面跟着一条正常的。
        let long = "x".repeat(100);
        fs::write(&path, format!("{long}\n\"tail\":1\n")).unwrap();
        let size = fs::metadata(&path).unwrap().len();
        let mut offset = 0;
        let mut rounds = 0;
        loop {
            let (_lines, consumed) = read_from(&path, offset, size, 16);
            rounds += 1;
            if rounds > 40 {
                panic!("扫描没在合理轮数内走完：窗口推进逻辑又停摆了");
            }
            if consumed <= offset {
                break;
            }
            offset = consumed;
        }
        assert_eq!(offset, size, "超长行也要一路爬到文件尾");
    }

    /**
     * 导出的边界是**盘**，不是一页、也不是当年的内存环形缓冲。
     *
     * 这是用户点名的第一条：原先导出走 `page()`，于是它被环形容量（默认 5000）悄悄封顶 ——
     * 用户以为拿到完整账。现在写 600 行：一页只有 50 条，计数与导出都必须是 600。
     */
    #[test]
    fn export_writes_every_matching_line_on_disk_beyond_one_page() {
        let temp = TempDir::new("log-runtime-export-disk");
        let log = LogState::new(temp.path().to_path_buf(), recording());
        let lines: Vec<LogEntry> = (0..600)
            .map(|index| {
                entry(LogStage::LightTransition, Some(LogReason::PhaseChanged))
                    .with_detail(serde_json::json!({ "n": index }))
            })
            .collect();
        log.report_batch(&lines);
        assert_eq!(log.drain().len(), 600, "600 行都该被 tail 回来");

        let page = log.page(&StatusLogQuery {
            limit: 50,
            ..Default::default()
        });
        assert_eq!(page.entries.len(), 50, "一页就是 50 条");
        assert!(!page.exhausted, "还有更旧的没翻完，面板要留着「看更早的」");
        assert_eq!(
            log.count(&StatusLogQuery::default()).total,
            600,
            "计数按盘数，与环形容量无关"
        );

        let exported = log
            .export_to(None, &StatusLogQuery::default())
            .expect("导出成功");
        assert_eq!(exported.entries, 600, "导出按盘算，不受环形容量封顶");
        assert!(!exported.stopped_by_budget, "没撞预算就不许自称不完整");

        let raw = fs::read_to_string(&exported.path).expect("文件在");
        let rows: Vec<&str> = raw.lines().collect();
        assert_eq!(rows.len(), 601, "表头 + 600 行");
        let header: serde_json::Value = serde_json::from_str(rows[0]).unwrap();
        assert!(header.get("coverage").is_some(), "表头要写明覆盖范围");
        assert!(
            header.get("dropped_from_ring").is_none(),
            "导出与环形容量无关，留着那句等于宣称假因果"
        );
        // 文件按旧→新写：最后一条是环形早就挤掉的那一批的末尾。
        let last: LogEntry = serde_json::from_str(rows[600]).unwrap();
        assert_eq!(last.stage, LogStage::LightTransition);
    }

    /// 实时那一路与按盘那一路必须给出**同一个身份**，否则前端合并去重是空谈。
    #[test]
    fn ring_entries_carry_the_same_position_the_scanner_computes() {
        let temp = TempDir::new("log-runtime-same-id");
        let log = LogState::new(temp.path().to_path_buf(), recording());
        log.report_batch(&[
            entry(LogStage::LightTransition, Some(LogReason::PhaseChanged)),
            entry(LogStage::KernelVerdict, Some(LogReason::StaleSequence)),
        ]);
        let live: Vec<String> = log.drain().into_iter().map(|item| item.id).collect();
        assert_eq!(live.len(), 2);

        let mut scanned: Vec<String> = Vec::new();
        let stats = super::super::scan::scan_forward(
            &log.dir(),
            &StatusLogQuery::default(),
            u64::MAX,
            |position, _| scanned.push(position.id()),
        );
        assert_eq!(scanned, live, "同一行两条路算出的坐标必须逐字相等");
        assert_eq!(stats.matched, 2);
        assert!(stats.files > 0);
    }

    /// 写不出去就不许留下半个文件：用户拿到的要么完整、要么什么都没有。
    #[test]
    fn a_failed_export_leaves_no_file_behind() {
        let temp = TempDir::new("log-runtime-export-fail");
        let log = LogState::new(temp.path().to_path_buf(), recording());
        log.report(entry(LogStage::HookAttempt, None));
        // 父目录还不存在 ⇒ `File::create` 必然失败。
        let target = temp.path().join("nope").join("out.jsonl");
        let error = log
            .export_to(Some(target.to_string_lossy().to_string()), &StatusLogQuery::default())
            .expect_err("父目录不存在，创建必须失败");
        assert!(!error.is_empty(), "错误文案要能看懂为什么失败");
        assert!(!target.exists(), "失败之后不该在那儿留下东西");
    }

    /// 被扫描预算截断时，结果必须自己说明"这不完整"。
    #[test]
    fn an_export_cut_by_budget_says_so() {
        let temp = TempDir::new("log-runtime-export-budget");
        let log = LogState::new(temp.path().to_path_buf(), recording());
        log.report(entry(LogStage::LightTransition, Some(LogReason::PhaseChanged)));
        let today = utc_day(now_ms());
        fs::write(
            log.dir().join(file_name(&today, 1)),
            fs::read_to_string(log.dir().join(file_name(&today, 0)))
                .expect("第一个文件在")
                .as_bytes(),
        )
        .expect("种第二个文件");
        assert_eq!(list_files(&log.dir()).len(), 2, "两个文件才走得完「换文件时撞预算」这条路");

        let exported = log
            .export_to_with_budget(None, &StatusLogQuery::default(), 1)
            .expect("预算截断不是错误，是一份不完整的结果");
        assert!(exported.stopped_by_budget, "1 字节的预算必须被判定为不完整");
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
