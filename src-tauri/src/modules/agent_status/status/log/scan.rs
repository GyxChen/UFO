//! 按盘扫描：把「从目录头翻到尾读 JSONL」这件事收在一处。
//!
//! tailer（`runtime.rs::drain`）只关心「新增的那一截」，而下面这几件事都要**整个目录翻一遍**：
//! 导出（旧→新全量）、精确计数、按盘倒序分页。它们的 IO 形态一样（逐文件、按窗口读、只认
//! 完整行、先子串预筛再解析），共用一套函数就不会三份各自漂移。
//!
//! ⚠️ 两条纪律：① 零驻留 —— 命中一条就回调一条，绝不在内存里攒（目录可以被配到几百 MiB）；
//! ② 全程不 panic —— release 是 `panic = "abort"`，一次索引越界会带走整个 App。

use std::fs;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use super::paths::{parse_file_name, LogPosition};
use super::record::LogEntry;
use super::runtime::{parse_line, read_from, StatusLogQuery, READ_WINDOW_BYTES};

/// 倒序扫描的缺省块长（256 KiB）：一次 seek + 一次 read 的窗口。
pub(super) const SCAN_BLOCK_BYTES: u64 = 256 * 1024;
/// 块装不出任何完整行时的放大上限：超过它（= 单行比这还长）就放弃这个文件，绝不空转。
pub(super) const SCAN_BLOCK_MAX: u64 = 4 * 1024 * 1024;

/// 一次扫描最多读多少字节：兜住「有人把 `max_total_bytes` 配到 512 MiB」的极端目录，
/// 免得一次点击把后台线程钉死几十秒。正常情况下永远不会触顶。
pub(super) const SCAN_BYTE_BUDGET: u64 = 512 * 1024 * 1024;

/// 一个「本系统写的」日志文件：坐标 + 完整路径 + 当前字节数。
pub(super) struct ScannedFile {
    /// 文件名里的 UTC 日期。
    pub(super) day: String,
    /// 文件名里的当日序号。
    pub(super) file_seq: u32,
    /// 完整路径。
    pub(super) path: PathBuf,
    /// 扫描开始时看到的字节长度（文件可能正在被追加，多出来的部分留给下一轮）。
    pub(super) size: u64,
}

/// 一次扫描的账：命中多少条、实际读了多少字节、走了几个文件、有没有被预算截断。
#[derive(Debug, Clone, Default)]
pub(super) struct ScanStats {
    /// 命中并回调了多少条。
    pub(super) matched: u64,
    /// 实际读过的字节数（诊断「这个数为什么慢」用）。
    pub(super) scanned_bytes: u64,
    /// 看了几个文件。
    pub(super) files: u64,
    /// ⚠️ 为真 = 结果不完整（撞到 [`SCAN_BYTE_BUDGET`]）。导出的反馈文案要把它说出来。
    pub(super) stopped_by_budget: bool,
}

/// 目录里认识的文件，按写入序 `(日期, 序号)` 升序（= 旧→新）。
///
/// 读目录失败 = 返回空（一次盘都没读过），而不是报错：日志坏了不许影响任何别的功能。
pub(super) fn files_in_order(dir: &Path) -> Vec<ScannedFile> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        // 只认本系统写的名字（`transitions-<日>.<序号>.jsonl`）：别人的文件一律不动。
        let Some((day, file_seq)) = parse_file_name(&name) else {
            continue;
        };
        let path = entry.path();
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        files.push(ScannedFile {
            day,
            file_seq,
            path,
            size: metadata.len(),
        });
    }
    files.sort_by(|left, right| {
        (left.day.as_str(), left.file_seq).cmp(&(right.day.as_str(), right.file_seq))
    });
    files
}

/// 只有「这段文本在 JSON 字符串里必然原样出现」时才敢拿它做子串预筛。
///
/// ⚠️ 含 `"` `\\` 等非安全字符的 needle 会被 serde 转义，子串比对就会漏匹配 ——
/// 预筛的判错方向必须是「多解析几行」而不是「少报几条」，所以这类值直接放弃预筛。
fn safe_to_prefilter(needle: &str) -> bool {
    !needle.is_empty()
        && needle.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-' | b'/')
        })
}

/// 这一行**有没有可能**命中查询（只图快，不做最终判定；最终判定仍走 `StatusLogQuery::matches`）。
///
/// 整目录扫描时这一步是数量级的差别：按当前实测约 97% 的行属于同一个会话之外或被过滤的
/// 噪声类，一次 `contains` 就把它们挡在 JSON 解析之前。
fn might_match(line: &str, query: &StatusLogQuery) -> bool {
    if let Some(key) = query.key.as_deref() {
        if safe_to_prefilter(key) && !line.contains(key) {
            return false;
        }
    }
    if let Some(trace) = query.trace_id.as_deref() {
        if safe_to_prefilter(trace) && !line.contains(trace) {
            return false;
        }
    }
    // 枚举取值是闭合的小写 snake_case，序列化后必然是 `"stage":"xxx"`，永远可以预筛。
    if let Some(stage) = query.stage.as_deref() {
        if !line.contains(&format!("\"stage\":\"{stage}\"")) {
            return false;
        }
    }
    if let Some(reason) = query.reason.as_deref() {
        if !line.contains(&format!("\"reason\":\"{reason}\"")) {
            return false;
        }
    }
    true
}

/// 正序全量扫描：每命中一条就回调 `(盘上坐标, 条目)`。零驻留、不 panic、不报错。
///
/// 三个边界口径与 tailer 完全一致（同一个 `read_from`）：
/// - 尾部半行不认（正在被 hook 追加的那条，本轮读它会把残段当记录）；
/// - 坏行直接丢（`parse_line` 返回 None）；
/// - 游标推不动就停在这个文件（宁可少扫一个文件，也不许在循环里空转）。
pub(super) fn scan_forward<F>(
    dir: &Path,
    query: &StatusLogQuery,
    budget: u64,
    mut on_entry: F,
) -> ScanStats
where
    F: FnMut(LogPosition, LogEntry),
{
    let mut stats = ScanStats::default();
    for file in files_in_order(dir) {
        stats.files = stats.files.saturating_add(1);
        let mut offset: u64 = 0;
        while offset < file.size {
            if stats.scanned_bytes >= budget {
                stats.stopped_by_budget = true;
                return stats;
            }
            let (lines, consumed) = read_from(&file.path, offset, file.size, READ_WINDOW_BYTES);
            if consumed <= offset {
                // 没有推进（读失败 / 尾部半行停在 EOF）：再转一轮就是死循环，这个文件到此为止。
                break;
            }
            stats.scanned_bytes = stats.scanned_bytes.saturating_add(consumed - offset);
            for (start, line) in lines {
                if !might_match(&line, query) {
                    continue;
                }
                let Some(entry) = parse_line(&line) else {
                    continue;
                };
                if !query.matches(&entry) {
                    continue;
                }
                stats.matched = stats.matched.saturating_add(1);
                on_entry(
                    LogPosition {
                        day: file.day.clone(),
                        file_seq: file.file_seq,
                        offset: start,
                    },
                    entry,
                );
            }
            offset = consumed;
        }
    }
    stats
}

/// 只数不攒：精确计数的正序全量扫（`matched` 就是答案，回调什么都不做）。
pub(super) fn count_matching(dir: &Path, query: &StatusLogQuery, budget: u64) -> ScanStats {
    scan_forward(dir, query, budget, |_, _| {})
}

// ── 倒序分页 ───────────────────────────────────────────────────────────────────
//
// 视图要「新→旧」，而 JSONL 是追加正序写的，所以这里整套 IO 形态与上面相反：
// 从最后一个文件的**尾部换行边界**起，一块一块往回吃。三条不变式一旦写错，表现是
// 「翻页重复或漏行」，真机要翻很久才看得出来 —— 所以块边界那几条单测是前置，不是补充。

/// 一次倒序取块的结果。
struct ScanStep {
    /// 块内的完整行（**块内也是新→旧**），各带它的起始绝对偏移。
    lines: Vec<(u64, String)>,
    /// 下一轮该从哪个偏移往回取 —— 一定落在某行的起始处（不变式）。
    next_hi: u64,
    /// 这个块装不出任何完整行（单行太长 / 读不到）：调用方放大块长重试，或放弃本文件。
    blocked: bool,
}

/// 读 `[lo, hi)` 整段。读不满（文件被截短 / 出错）返回 None ⇒ 调用方放弃，绝不猜。
fn read_span(file: &mut File, lo: u64, hi: u64) -> Option<Vec<u8>> {
    let want = usize::try_from(hi.saturating_sub(lo)).ok()?;
    if want == 0 {
        return Some(Vec::new());
    }
    if file.seek(SeekFrom::Start(lo)).is_err() {
        return None;
    }
    let mut buffer = vec![0u8; want];
    let read = file.read(&mut buffer).unwrap_or(0);
    (read == want).then(|| buffer[..read].to_vec())
}

/// 文件里「最后一个换行之后」的位置 = 尾部半行的起点，也是**倒序扫描唯一合法的上界**。
///
/// ⚠️ 倒序永远不许碰那半行（hook 可能正写到一半），所以起手必须把上界归到换行边界上。
/// 返回 `None` = 这个文件里一行都没写完（空文件，或整份都是残段），调用方跳过它继续翻更旧的。
fn tail_boundary(path: &Path, size: u64) -> Option<u64> {
    if size == 0 {
        return None;
    }
    let Ok(mut file) = File::open(path) else {
        return None;
    };
    let mut hi = size;
    while hi > 0 {
        let start = hi.saturating_sub(SCAN_BLOCK_BYTES);
        let chunk = read_span(&mut file, start, hi)?;
        if let Some(index) = chunk.iter().rposition(|byte| *byte == b'\n') {
            return Some(start + index as u64 + 1);
        }
        hi = start;
    }
    None
}

/// 从 `index` 往回找第一个「至少写完了一行」的文件，返回它的下标与倒序上界。
///
/// 一个都没有 = 整个目录没有任何可读的行 ⇒ 分页到底了。
fn first_readable(files: &[ScannedFile], index: usize) -> Option<(usize, u64)> {
    if files.is_empty() {
        return None;
    }
    let mut cursor = index.min(files.len() - 1);
    loop {
        let file = files.get(cursor)?;
        if let Some(bound) = tail_boundary(&file.path, file.size) {
            return Some((cursor, bound));
        }
        if cursor == 0 {
            return None;
        }
        cursor -= 1;
    }
}

/// 从 `hi` 往回吃一个块，交出块内**完整**的行（块内也是新→旧）。
///
/// 不变式：进来的 `hi` 一定是某一行的起始偏移（由 [`tail_boundary`] 归一，或由上一轮的
/// `next_hi` 给出）。于是块头那一刀必然切在行中间 —— 那段残头属于**更旧的一行**，
/// 本轮不 emit，`next_hi` 停在它之后，下一轮从更早的位置把它的行首一起读进来：不重也不漏。
fn read_block_backwards(file: &mut File, hi: u64, block: u64) -> ScanStep {
    let start = hi.saturating_sub(block);
    let Some(chunk) = read_span(file, start, hi) else {
        return ScanStep { lines: Vec::new(), next_hi: hi, blocked: true };
    };
    // `head` = 块内第一段「完整行」的起点（块相对）。start 就在文件头时，整段都是完整的。
    let head = if start == 0 {
        0
    } else {
        match chunk.iter().position(|byte| *byte == b'\n') {
            Some(index) => index + 1,
            // 整块连一个换行都没有：这一行比块还长，交给调用方放大。
            None => return ScanStep { lines: Vec::new(), next_hi: hi, blocked: true },
        }
    };
    if head >= chunk.len() {
        // 唯一的换行就压在末字节 ⇒ 本块只装得下「上一行的尾巴」，装不出任何完整行。
        return ScanStep { lines: Vec::new(), next_hi: hi, blocked: true };
    }
    let mut lines: Vec<(u64, String)> = Vec::new();
    let mut cursor = start + head as u64;
    for piece in chunk[head..].split_inclusive(|byte| *byte == b'\n') {
        let body = match piece.split_last() {
            Some((b'\n', rest)) => rest,
            _ => piece,
        };
        if !body.iter().all(|byte| byte.is_ascii_whitespace()) {
            if let Ok(text) = String::from_utf8(body.to_vec()) {
                lines.push((cursor, text));
            }
        }
        cursor = cursor.saturating_add(piece.len() as u64);
    }
    lines.reverse();
    ScanStep { lines, next_hi: start + head as u64, blocked: false }
}

/// 一次倒序分页的结果。
pub(super) struct DiskPage {
    /// 命中的行，**新→旧**，各带盘上坐标。
    pub(super) entries: Vec<(LogPosition, LogEntry)>,
    /// 下一页的锚点（= 本页最后一条的坐标）。
    pub(super) next_before: Option<LogPosition>,
    /// 已经翻到目录头，没有更旧的了。
    pub(super) exhausted: bool,
    /// 为凑齐这一页读了多少字节（面板可用来解释「这一页为什么慢」）。
    pub(super) scanned_bytes: u64,
}

/// 按盘**倒序**取一页：`before = None` 从最新文件的尾开始，`Some(pos)` 严格取比它更旧的。
///
/// 语义与内存版逐字一致（「只取比锚点更旧的」，锚点那条本身不会再出现），但**没有条数上限** ——
/// 一路能翻到目录头，能看多少由盘上的保留策略决定，不再由环形容量决定。
///
/// 撞到 `budget` 时返回「还没到底」（`exhausted = false`）而不是报错：宁可让用户再点一次
/// 「看更早的」什么也没多出来，也不许把一次极端扫描说成读取失败。
///
/// `block` 是回向取块的窗口大小：生产传 [`SCAN_BLOCK_BYTES`]，单测故意传小值来逼它跨块走完
/// （否则块边界那一刀的重复/漏行永远测不到）。
pub(super) fn read_page(
    dir: &Path,
    query: &StatusLogQuery,
    limit: usize,
    budget: u64,
    block: u64,
) -> DiskPage {
    let files = files_in_order(dir);
    let started = match query.before.as_ref() {
        // 没有锚点：从最新那个可读文件的尾巴起。
        None => first_readable(&files, files.len().saturating_sub(1)),
        Some(pos) => {
            let key = (pos.day.as_str(), pos.file_seq);
            match files.iter().rposition(|file| (file.day.as_str(), file.file_seq) == key) {
                // 锚点所在文件还在：从锚点那一行的起始偏移往回走，锚点自身读不到（严格更旧）。
                Some(index) => files
                    .get(index)
                    .map(|file| (index, pos.offset.min(file.size))),
                // 文件已被回收 / 清空删掉：退化成「锚点之前最后一个还存在的文件」。
                // ⚠️ 不许返回空列表了事 —— 那会被读成「这条会话没有日志」。
                None => files
                    .iter()
                    .rposition(|file| (file.day.as_str(), file.file_seq) < key)
                    .and_then(|index| {
                        tail_boundary(&files[index].path, files[index].size).map(|bound| (index, bound))
                    }),
            }
        }
    };
    let Some((mut index, mut hi)) = started else {
        return DiskPage {
            entries: Vec::new(),
            next_before: None,
            exhausted: true,
            scanned_bytes: 0,
        };
    };

    let mut out: Vec<(LogPosition, LogEntry)> = Vec::new();
    let mut scanned: u64 = 0;
    // 放大只在「这个块装不出完整行」期间有效，跨过一道坎就收回初值（小窗口尤其重要）。
    let initial_block = block.max(1);
    let mut block = initial_block;
    loop {
        if out.len() >= limit {
            return page_of(out, scanned, false);
        }
        if scanned >= budget {
            return page_of(out, scanned, false);
        }
        if hi == 0 {
            // 这个文件翻到头了 ⇒ 换更旧的一个；没有更旧的了就是到底。
            if index == 0 {
                return page_of(out, scanned, true);
            }
            match first_readable(&files, index - 1) {
                Some((next_index, bound)) => {
                    index = next_index;
                    hi = bound;
                    block = initial_block;
                }
                None => return page_of(out, scanned, true),
            }
            continue;
        }
        let Some(file) = files.get(index) else {
            return page_of(out, scanned, true);
        };
        let Ok(mut handle) = File::open(&file.path) else {
            // 打不开就翻更旧的一个文件：一次读不到不能把整页判死。
            if index == 0 {
                return page_of(out, scanned, true);
            }
            match first_readable(&files, index - 1) {
                Some((next_index, bound)) => {
                    index = next_index;
                    hi = bound;
                    block = initial_block;
                }
                None => return page_of(out, scanned, true),
            }
            continue;
        };
        let step = read_block_backwards(&mut handle, hi, block);
        if step.blocked {
            if block < SCAN_BLOCK_MAX {
                block = block.saturating_mul(4).min(SCAN_BLOCK_MAX);
                continue;
            }
            // 单行比放大上限还长：这个文件只能放弃（继续翻更旧的，不许原地空转）。
            if index == 0 {
                return page_of(out, scanned, true);
            }
            match first_readable(&files, index - 1) {
                Some((next_index, bound)) => {
                    index = next_index;
                    hi = bound;
                }
                None => return page_of(out, scanned, true),
            }
            block = initial_block;
            continue;
        }
        block = initial_block;
        scanned = scanned.saturating_add(hi.saturating_sub(step.next_hi));
        for (offset, line) in step.lines {
            if !might_match(&line, query) {
                continue;
            }
            let Some(entry) = parse_line(&line) else {
                continue;
            };
            if !query.matches(&entry) {
                continue;
            }
            out.push((
                LogPosition {
                    day: file.day.clone(),
                    file_seq: file.file_seq,
                    offset,
                },
                entry,
            ));
            if out.len() >= limit {
                break;
            }
        }
        hi = step.next_hi;
    }
}

/// 把攒到的行包成一页，并算好下一页的锚点。
fn page_of(out: Vec<(LogPosition, LogEntry)>, scanned: u64, exhausted: bool) -> DiskPage {
    let next_before = out.last().map(|(position, _)| position.clone());
    DiskPage {
        entries: out,
        next_before,
        exhausted,
        scanned_bytes: scanned,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::agent_status::status::log::paths::{file_name, log_dir, utc_day};
    use crate::modules::agent_status::status::log::record::{LogStage, LogWriter};
    use crate::modules::agent_status::status::log::runtime::StatusLogQuery;
    use crate::modules::agent_status::status::model::now_ms;
    use crate::modules::agent_status::status::test_support::TempDir;

    /// 造一条可直接写盘的日志行。
    ///
    /// ⚠️ 时间戳跟着真实时钟：文件名由 `utc_day(now)` 算，夹具写死日期会让测试「只有当天能跑」。
    fn entry(index: u32) -> LogEntry {
        LogEntry::new(LogWriter::Monitor, LogStage::LightTransition)
            .with_key("claude:s1")
            .with_detail(serde_json::json!({ "n": index }))
    }

    /// 把若干条写成一个日志文件，返回它的 `(日期, 序号)`。
    fn write_file(dir: &Path, day: &str, seq: u32, lines: &[LogEntry]) -> PathBuf {
        let body: String = lines.iter().map(|line| format!("{}\n", line.line())).collect();
        let path = dir.join(file_name(day, seq));
        fs::write(&path, body).expect("写日志文件");
        path
    }

    /// 单测里的窗口故意取小：大窗口一个块就装完整个文件，块边界那一刀永远测不到。
    const TEST_BLOCK: u64 = 64;

    fn today() -> String {
        utc_day(now_ms())
    }

    #[test]
    fn reverse_page_walks_newest_first_across_many_blocks() {
        let temp = TempDir::new("log-scan-reverse-order");
        let dir = log_dir(temp.path());
        fs::create_dir_all(&dir).expect("日志目录");
        let lines: Vec<LogEntry> = (0..5).map(entry).collect();
        write_file(&dir, &today(), 0, &lines);

        let page = read_page(&dir, &StatusLogQuery::default(), 5, u64::MAX, TEST_BLOCK);
        assert_eq!(page.entries.len(), 5, "5 行都要被翻出来");
        let seen: Vec<u32> = page
            .entries
            .iter()
            .filter_map(|(_, item)| item.detail.as_ref().and_then(|d| d.get("n"))?.as_u64().map(|n| n as u32))
            .collect();
        assert_eq!(seen, vec![4, 3, 2, 1, 0], "顺序必须是新→旧，即使它跨了好几个 64 字节的块");
        // 偏移也要严格递减 —— 行身份就是靠它。
        let offsets: Vec<u64> = page.entries.iter().map(|(pos, _)| pos.offset).collect();
        assert!(offsets.windows(2).all(|pair| pair[0] > pair[1]), "{offsets:?}");
    }

    #[test]
    fn tail_of_a_half_written_line_is_never_returned() {
        let temp = TempDir::new("log-scan-half-line");
        let dir = log_dir(temp.path());
        fs::create_dir_all(&dir).expect("日志目录");
        // 尾部那段没有换行的「正在被 hook 写」的残段，倒序扫描必须整段绕过。
        let path = dir.join(file_name(&today(), 0));
        fs::write(&path, format!("{}\n{}\nPARTIAL", entry(0).line(), entry(1).line())).expect("写半个文件");

        let page = read_page(&dir, &StatusLogQuery::default(), 10, u64::MAX, TEST_BLOCK);
        assert_eq!(page.entries.len(), 2, "半行不算一条记录");
        assert!(page
            .entries
            .iter()
            .all(|(_, item)| item.stage == LogStage::LightTransition));
    }

    #[test]
    fn page_boundary_neither_duplicates_nor_skips_the_anchor_row() {
        let temp = TempDir::new("log-scan-anchor");
        let dir = log_dir(temp.path());
        fs::create_dir_all(&dir).expect("日志目录");
        let lines: Vec<LogEntry> = (0..6).map(entry).collect();
        write_file(&dir, &today(), 0, &lines);

        // 逐页翻到底：把整份账一条不漏地拿回来，才算「翻页」这件事成立。
        let mut before: Option<LogPosition> = None;
        let mut ids: Vec<String> = Vec::new();
        let mut offsets: Vec<u64> = Vec::new();
        let mut pages = 0;
        loop {
            let query = StatusLogQuery {
                before: before.clone(),
                ..Default::default()
            };
            let page = read_page(&dir, &query, 2, u64::MAX, TEST_BLOCK);
            pages += 1;
            assert!(pages <= 10, "六行分两页翻，最多三页就该到底");
            for (position, _) in &page.entries {
                ids.push(position.id());
                offsets.push(position.offset);
            }
            if page.exhausted {
                break;
            }
            before = page.next_before.clone();
            assert!(before.is_some(), "没到底就必须给出下一页的锚点");
        }

        assert_eq!(ids.len(), 6, "六行都要翻到，一条不许漏");
        assert_eq!(
            ids.iter().collect::<std::collections::HashSet<_>>().len(),
            6,
            "同一条不许在两页里各出现一次"
        );
        assert!(
            offsets.windows(2).all(|pair| pair[0] > pair[1]),
            "整体必须严格新→旧：{offsets:?}"
        );
    }

    #[test]
    fn a_filter_matching_nothing_terminates_and_reports_exhausted() {
        let temp = TempDir::new("log-scan-no-match");
        let dir = log_dir(temp.path());
        fs::create_dir_all(&dir).expect("日志目录");
        let lines: Vec<LogEntry> = (0..4).map(entry).collect();
        write_file(&dir, &today(), 0, &lines);

        let page = read_page(
            &dir,
            &StatusLogQuery {
                stage: Some("not_a_stage".to_string()),
                ..Default::default()
            },
            10,
            u64::MAX,
            TEST_BLOCK,
        );
        assert!(page.entries.is_empty());
        assert!(page.exhausted, "永远翻不完 = 面板会一直给「看更早的」按钮");
    }

    #[test]
    fn an_overlong_line_does_not_stall_the_reverse_walk() {
        let temp = TempDir::new("log-scan-overlong");
        let dir = log_dir(temp.path());
        fs::create_dir_all(&dir).expect("日志目录");
        let path = dir.join(file_name(&today(), 0));
        // 一条 300 字节的超长行（远超 64 字节的测试窗口），后面跟一条正常的。
        let long = "x".repeat(300);
        fs::write(&path, format!("{long}\n{}\n", entry(7).line())).expect("写超长行");

        let page = read_page(&dir, &StatusLogQuery::default(), 10, u64::MAX, TEST_BLOCK);
        // 超长行本身解析不出记录（被当坏行跳过），但它**后面的**那条必须读得到，且不能死循环。
        assert_eq!(page.entries.len(), 1, "正常那条要翻得回来");
    }

    #[test]
    fn a_missing_anchor_falls_back_to_a_surviving_older_file() {
        let temp = TempDir::new("log-scan-missing-anchor");
        let dir = log_dir(temp.path());
        fs::create_dir_all(&dir).expect("日志目录");
        let lines: Vec<LogEntry> = (0..3).map(entry).collect();
        write_file(&dir, &today(), 0, &lines);

        // 锚点指向一个「比现存文件都新」但已经不存在的序号（被回收 / 被清空换号都会这样）。
        let ghost = LogPosition {
            day: today(),
            file_seq: 9,
            offset: 128,
        };
        let page = read_page(
            &dir,
            &StatusLogQuery {
                before: Some(ghost),
                ..Default::default()
            },
            10,
            u64::MAX,
            TEST_BLOCK,
        );
        assert_eq!(
            page.entries.len(),
            3,
            "锚点文件没了要退化成翻更旧的文件，而不是把面板顶成空列表（那会被读成「这条会话没有日志」）"
        );
    }

    #[test]
    fn count_matching_is_exact_and_ignores_the_ring() {
        let temp = TempDir::new("log-scan-count");
        let dir = log_dir(temp.path());
        fs::create_dir_all(&dir).expect("日志目录");
        let lines: Vec<LogEntry> = (0..12).map(entry).collect();
        write_file(&dir, &today(), 0, &lines);
        let stats = count_matching(&dir, &StatusLogQuery::default(), SCAN_BYTE_BUDGET);
        assert_eq!(stats.matched, 12, "计数按盘数，与内存环形容量无关");
        assert_eq!(stats.files, 1);
        assert!(!stats.stopped_by_budget);
    }
}
