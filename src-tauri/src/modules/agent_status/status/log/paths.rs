//! 转灯日志的落点与文件名：**全是纯函数**，hook 与 GUI 各自独立算出同一个活动文件。
//!
//! 之所以能做到「无协商」：文件名由「UTC 日期 + 当日序号」唯一决定，而序号的规则只依赖
//! 目录里已有什么文件、各自多大 —— 两个进程在同一时刻读到同一份目录，必然选出同一个序号。
//! 于是 hook（不能开 SQLite、不能起 IPC）只管按规则追加，GUI 只管按规则回收。
//!
//! ⚠️ 分文件用 **UTC**，UI 显示用本地时区。混用的代价已经被 `hook.rs` 那个「函数名叫
//! `chrono_local_now`、算的其实是 UTC 秒」踩过一次，这里把口径写在类型上。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 一条日志在盘上的坐标：哪一天的第几个文件 + 该行在文件里的**起始字节偏移**。
///
/// 为什么用它当「行身份」而不是行号：日志文件只追加，已写行的起始偏移永不移动，所以这个
/// 三元组稳定可比；而行号要回答「倒数第 N 行」必须先数完整个文件，且跨文件不可比。
/// 它同时是**翻页游标**（倒序扫描从某个坐标往回走）与**去重键**（实时广播的行与按盘读回的
/// 行第一次拥有同一个身份），前端 `:key` 与合并去重都只用 [`LogPosition::id`]。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LogPosition {
    /// UTC 日期（`YYYY-MM-DD`），与文件名同源。
    pub day: String,
    /// 当日文件序号（文件名里那个 `.<seq>`，与内存游标 `seq` 无关）。
    pub file_seq: u32,
    /// 该行在文件里的起始字节偏移（不含上一行的换行符）。
    pub offset: u64,
}

impl LogPosition {
    /// 稳定身份串：前端与后端都只比这个值，不在别处手拼三元组（拼法一改就去重失效）。
    pub fn id(&self) -> String {
        format!("{}.{}.{}", self.day, self.file_seq, self.offset)
    }
}

/// 日志目录：与 inbox 同级，跟着 `status_dir` 走（`--status-dir` 覆盖时一起覆盖）。
pub fn log_dir(status_dir: &Path) -> PathBuf {
    status_dir.join("log")
}

/// 文件名前缀与后缀：解析与构造共用同一份常量，避免两处各写一遍字面量。
const PREFIX: &str = "transitions-";
const SUFFIX: &str = ".jsonl";

/// 某个时刻属于哪一天（UTC，`YYYY-MM-DD`）。
pub fn utc_day(ts_ms: i64) -> String {
    day_of_index(ts_ms.div_euclid(86_400_000))
}

/// 某个日志文件属于哪一天、当日第几个。
pub fn file_name(day: &str, seq: u32) -> String {
    format!("{PREFIX}{day}.{seq}{SUFFIX}")
}

/// 从文件名反解出 `(日期, 序号)`；不是本系统写的文件一律返回 None（不去碰别人的东西）。
pub fn parse_file_name(name: &str) -> Option<(String, u32)> {
    let rest = name.strip_prefix(PREFIX)?;
    let rest = rest.strip_suffix(SUFFIX)?;
    // 日期里只有短横线，所以第一个点就是分隔符；`split_once` 两侧都不带它
    // （`split_at(dot)` 会把点留在日期上、`split_at(dot + 1)` 会把它留在另一边，两种都错）。
    let (day, seq) = rest.split_once('.')?;
    if !is_real_day(day) {
        return None;
    }
    seq.parse::<u32>().ok().map(|seq| (day.to_string(), seq))
}

/// ISO 日期串的形状校验（不做日历校验，日历校验在 [`is_real_day`]）。
fn is_iso_day(day: &str) -> bool {
    day.len() == 10
        && day.bytes().enumerate().all(|(index, byte)| match index {
            4 | 7 => byte == b'-',
            _ => byte.is_ascii_digit(),
        })
}

/// 是不是一个**真实存在**的日期（`2026-99-99` 形状对但日历上没有这种日子）。
///
/// 靠「天数 → 年月日 → 天数」往返回环判定，因此不需要再手写一份月份天数表；
/// 认不出的文件名一律不当成自己的日志 —— 轮转会把认出来的删掉，误判的代价是删错文件。
fn is_real_day(day: &str) -> bool {
    if !is_iso_day(day) {
        return false;
    }
    let Some(index) = days_from_civil_str(day) else {
        return false;
    };
    day_of_index(index) == day
}

/// 第 N 天（自 1970-01-01）对应的 ISO 日期。
fn day_of_index(index: i64) -> String {
    let (year, month, day) = civil_from_days(index);
    format!("{year:04}-{month:02}-{day:02}")
}

/// 该删掉的旧文件：**严格早于** `today` 且已超过保留天数。
///
/// 当天的文件永不删除 —— 它可能正被别的 hook 进程持有追加句柄，unlink 会让那条信号
/// 从此写进一个没人读的 inode。按整天回收因此是安全下限。
pub fn is_expired(name: &str, today: &str, retention_days: u32) -> bool {
    let Some((day, _)) = parse_file_name(name) else {
        return false;
    };
    // 当天的文件永不删（见类型文档）；`String` 与 `&str` 之间只有相等比较的 impl，
    // 大小比较要先统一成 `&str`。
    if day.as_str() >= today {
        return false;
    }
    match (days_from_civil_str(&day), days_from_civil_str(today)) {
        (Some(file), Some(now)) => now.saturating_sub(file) >= i64::from(retention_days),
        _ => false,
    }
}

/// 选出**当前该写**的当日序号：**今天已存在的最大序号且还没超限**者；都超限则当日最大序号 +1。
///
/// 纯函数，入参是 `(文件名, 字节数)` 列表 —— 读目录与读大小留给调用方，这样单测可以
/// 直接喂假数据验证规则（包括「0 号满了」「0 与 1 号都满了」这类中间态）。
///
/// 写者因此**自己就会换文件**（open 时顺手建出下一个序号），GUI 不需要参与切文件、
/// 只管删过期整天文件与超额部分 —— 少一个「谁负责轮转」的进程间协议。
pub fn pick_active_seq(files: &[(String, u64)], today: &str, max_file_bytes: u64) -> u32 {
    let mut best: Option<u32> = None;
    let mut largest_of_today: Option<u32> = None;
    for (name, size) in files {
        let Some((day, seq)) = parse_file_name(name) else {
            continue;
        };
        if day != today {
            continue;
        }
        largest_of_today = Some(largest_of_today.map_or(seq, |current: u32| current.max(seq)));
        if size < &max_file_bytes && best.is_none_or(|current| seq > current) {
            best = Some(seq);
        }
    }
    // 今天一个文件都没有时从 0 起；有但都满了则往后开一个新文件（序号只增不减，
    // 不会出现「回头写已经被 tailer 读完的旧文件」导致的漏读）。
    best.or(largest_of_today.map(|seq| seq + 1))
        .unwrap_or_default()
}

/// 总量超上限时该删哪些文件：**从最旧的整天开始**，删到低于上限即停。
///
/// 活动文件（今天、`active_seq`）永不进名单 —— 它可能被别的 hook 进程持有追加句柄。
/// 今天的其它序号文件排在最后（排序键是 `(日期, 序号)`，天然如此），因为它们的信号
/// 最新、最可能正是用户要找的那一段。
pub fn evict_for_total_cap(
    files: &[(String, u64)],
    today: &str,
    active_seq: u32,
    max_total_bytes: u64,
) -> Vec<String> {
    let mut total: u64 = files
        .iter()
        .filter(|(name, _)| parse_file_name(name).is_some())
        .map(|(_, size)| *size)
        .sum();
    if total <= max_total_bytes {
        return Vec::new();
    }
    let mut ordered: Vec<&(String, u64)> = files
        .iter()
        .filter(|(name, _)| parse_file_name(name).is_some())
        .collect();
    ordered.sort_by(|(left, _), (right, _)| {
        let left_key = parse_file_name(left).unwrap_or_default();
        let right_key = parse_file_name(right).unwrap_or_default();
        left_key.cmp(&right_key)
    });
    let mut victims = Vec::new();
    for (name, size) in ordered {
        if total <= max_total_bytes {
            break;
        }
        let (day, seq) = parse_file_name(name).unwrap_or_default();
        if day == today && seq == active_seq {
            continue;
        }
        total = total.saturating_sub(*size);
        victims.push(name.clone());
    }
    victims
}

/// 把「自 1970-01-01 起的天数」换成 UTC 年月日。
///
/// Howard Hinnant 的 `civil_from_days`：纯整数、零依赖（本项目**没有** chrono 直接依赖，
/// 拉一个只为算日期不划算），对 1970 之前的负数也成立。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// `civil_from_days` 的逆运算（用于算两个日期串相差几天）。
fn days_from_civil_str(day: &str) -> Option<i64> {
    let year: i64 = day.get(0..4)?.parse().ok()?;
    let month: i64 = day.get(5..7)?.parse().ok()?;
    let dom: i64 = day.get(8..10)?.parse().ok()?;
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400; // [0, 399]
    let mp = if month > 2 { month - 3 } else { month + 9 }; // [0, 11]
    let doy = (153 * mp + 2) / 5 + dom - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    Some(era * 146_097 + doe - 719_468)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(day: &str, seq: u32, size: u64) -> (String, u64) {
        (file_name(day, seq), size)
    }

    /// 2026-09-27T00:00:00Z（本项目当前日期）：= (20454 + 269) 天，20454 是 2026-01-01。
    const SEPT27: i64 = 1_790_467_200_000;

    #[test]
    fn utc_day_maps_millis_to_the_right_date() {
        assert_eq!(utc_day(0), "1970-01-01");
        // 2000-01-01 与 2026-09-27 都跨了世纪边界，正是算法最容易错的地方。
        assert_eq!(utc_day(946_684_800_000), "2000-01-01");
        assert_eq!(utc_day(SEPT27), "2026-09-27");
        // 一天内的最后一毫秒仍是同一天；跨零点才换文件。
        assert_eq!(utc_day(SEPT27 + 86_399_999), "2026-09-27");
        assert_eq!(utc_day(SEPT27 + 86_400_000), "2026-09-28");
    }

    #[test]
    fn day_round_trips_through_both_directions() {
        for ts in [0_i64, 1, 946_684_800_000, SEPT27, 1_767_225_600_000] {
            let day = utc_day(ts);
            let index = ts.div_euclid(86_400_000);
            assert_eq!(days_from_civil_str(&day), Some(index), "{day}");
        }
    }

    #[test]
    fn file_names_parse_only_our_own_shape() {
        assert_eq!(
            parse_file_name("transitions-2026-09-27.3.jsonl"),
            Some(("2026-09-27".to_string(), 3))
        );
        // 不认识的形状一律不动：hook-debug.log、临时文件、用户手放的东西。
        assert_eq!(parse_file_name("hook-debug.log"), None);
        assert_eq!(parse_file_name("transitions-2026-09-27.jsonl"), None);
        assert_eq!(parse_file_name("transitions-2026-99-99.0.jsonl"), None);
        assert_eq!(parse_file_name("session-records.db"), None);
    }

    #[test]
    fn active_seq_prefers_the_largest_uncapped_file_of_today() {
        let today = "2026-09-27";
        let max = 1000;
        assert_eq!(pick_active_seq(&[], today, max), 0);
        assert_eq!(
            pick_active_seq(&[file(today, 0, 10)], today, max),
            0,
            "没满就继续往 0 号追加"
        );
        assert_eq!(
            pick_active_seq(&[file(today, 0, 1000)], today, max),
            1,
            "0 号满了：写者自己开 1 号，不回头写已经满的文件"
        );
        assert_eq!(
            pick_active_seq(&[file(today, 0, 1000), file(today, 1, 5)], today, max),
            1,
            "0 号满了就用 1 号，之后稳定在 1 号（不会跳到 2）"
        );
        assert_eq!(
            pick_active_seq(&[file(today, 0, 1000), file(today, 1, 1000)], today, max),
            2,
            "都满就往后开"
        );
        assert_eq!(
            pick_active_seq(&[file("2026-09-26", 4, 5)], today, max),
            0,
            "昨天的序号不占用今天的"
        );
        assert_eq!(
            pick_active_seq(
                &[file("2026-09-26", 4, 1000), file(today, 0, 5)],
                today,
                max
            ),
            0,
            "今天的可用序号优先，昨天的满了也不影响"
        );
    }

    #[test]
    fn total_cap_evicts_oldest_first_and_never_the_active_file() {
        let today = "2026-09-27";
        let files = vec![
            file("2026-09-25", 0, 800),
            file("2026-09-26", 0, 800),
            file(today, 0, 800),
            file(today, 1, 100),
        ];
        let victims = evict_for_total_cap(&files, today, 1, 1000);
        assert_eq!(
            victims,
            vec![file_name("2026-09-25", 0), file_name("2026-09-26", 0)],
            "从最旧整天开始删，够就停手"
        );
        assert!(
            !victims.contains(&file_name(today, 1)),
            "活动文件永不进删除名单"
        );
        assert!(victims.len() < 3, "删到低于上限即可，不该把今天的清空");
        assert!(
            evict_for_total_cap(&files, today, 1, 100_000).is_empty(),
            "没超上限什么都不做"
        );
    }

    #[test]
    fn retention_deletes_only_whole_older_days() {
        let today = "2026-09-27";
        assert!(!is_expired(&file_name(today, 0), today, 1));
        assert!(
            !is_expired(&file_name("2026-09-26", 0), today, 30),
            "在保留期内"
        );
        assert!(is_expired(&file_name("2026-09-26", 0), today, 1));
        assert!(is_expired(&file_name("2026-08-27", 2), today, 14));
        // 跨月、跨年边界最容易算错天数：2025-12-31 距 2026-09-27 是 270 天。
        assert!(is_expired(&file_name("2025-12-31", 0), today, 200));
        assert!(
            !is_expired(&file_name("2025-12-31", 0), today, 300),
            "没过保留天数就不该删"
        );
        assert!(!is_expired(&file_name("2026-09-01", 0), today, 30));
        // 认不出的文件名永不删。
        assert!(!is_expired("hook-debug.log", today, 1));
    }

    #[test]
    fn log_dir_sits_next_to_inbox() {
        let dir = log_dir(Path::new("/x/status"));
        assert_eq!(dir, PathBuf::from("/x/status/log"));
    }

    /**
     * 行身份的两条性质：跨天可比、同行稳定。
     *
     * 翻页游标与去重键都建在这两条上 —— 序号在跨天时从 0 重来，所以必须按 `(日期, 序号, 偏移)`
     * 比；而「实时那一路」与「按盘读回那一路」要能对上，同一行算两次就得给出同一个串。
     */
    #[test]
    fn position_orders_across_days_and_stays_stable_for_one_line() {
        let older = LogPosition {
            day: "2026-09-27".to_string(),
            file_seq: 2,
            offset: 900,
        };
        let newer = LogPosition {
            day: "2026-09-28".to_string(),
            file_seq: 0,
            offset: 0,
        };
        // 排序口径与文件名一致：`(日期, 序号, 偏移)` 字典序即写入序（比较发生在 `scan.rs`）。
        let older_key = (older.day.as_str(), older.file_seq, older.offset);
        let newer_key = (newer.day.as_str(), newer.file_seq, newer.offset);
        assert!(older_key < newer_key, "新一天从 0 号重来，但整体仍比昨天更新");
        assert_eq!(older.id(), "2026-09-27.2.900");
        assert_eq!(older.clone().id(), older.id(), "同一行算两次必须得到同一个身份");
        assert_ne!(older.id(), newer.id());
    }
}
