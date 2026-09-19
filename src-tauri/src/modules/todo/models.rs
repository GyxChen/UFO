//! 滴答清单 Open API 的数据模型，以及「原始任务 → 悬浮窗 / 信息卡可用快照」的转换。
//!
//! 解析只取展示要用的字段；日期一律换算成**绝对的 UTC 毫秒**。「今天 / 明天 / 已逾期」
//! 这类相对文案由前端按用户本机时区算 —— 后端没有（也不该有）用户时区的可靠信息，
//! 在这里猜等于给跨时区用户显示错的日期。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// 一条未完成任务（已完成 / 已放弃的会在 [`build_item`] 里被滤掉）。
///
/// 序列化给前端的字段名与前端 `stores/todo.ts` 的 `TodoTaskItem` 一一对应。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TodoTaskItem {
    /// 任务 id
    pub id: String,
    /// 任务标题（已 trim；空标题的任务会被丢弃）
    pub title: String,
    /// 所属清单 id
    pub project_id: String,
    /// 所属清单名（取自 `GET /open/v1/project`；查不到时为「收件箱」/「清单」）
    pub project_name: String,
    /// 优先级：5 高 / 3 中 / 1 低 / 0 无
    pub priority: i64,
    /// 截止时间（UTC epoch 毫秒）；未设置截止为 null
    pub due_at_ms: Option<i64>,
    /// 是否有具体时刻（false = 只到天粒度，界面不显示时分）
    pub has_time: bool,
    /// 子任务（勾选清单）总条数
    pub items_total: i64,
    /// 子任务已完成条数
    pub items_done: i64,
}

/// 待办快照：未完成列表整体，广播给悬浮窗 / 主窗 / 提示窗。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TodoSnapshot {
    /// 排好序的未完成任务（见 [`make_snapshot`] 的口径）
    pub list: Vec<TodoTaskItem>,
    /// 未完成任务总数（= `list` 长度；滴答单次最多返回 200 条）
    pub total_incomplete: i64,
    /// 快照构建时刻（毫秒）；前端以它为参照算「今天 / 明天 / 已逾期」
    pub updated_at_ms: i64,
}

/// 清单（项目）列表项，序列化给设置页的「清单筛选」下拉。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TodoProjectItem {
    /// 清单 id（收件箱固定为 `inbox`）
    pub id: String,
    /// 清单名
    pub name: String,
}

/// 滴答原始任务：`task/filter`、`task/undone`、`project/{id}/data` 共用同一个 Task 结构。
///
/// 字段名刻意保持滴答 JSON 的驼峰（`#[serde(rename)]`），缺失一律走默认值 —— 接口
/// 只回写过来的字段，少的字段不该让整条任务解析失败。
#[derive(Debug, Default, Deserialize)]
pub struct RawTask {
    /// 任务 id
    #[serde(default)]
    pub id: String,
    /// 任务标题
    #[serde(default)]
    pub title: String,
    /// 所属清单 id
    #[serde(default, rename = "projectId")]
    pub project_id: String,
    /// 优先级：5 高 / 3 中 / 1 低 / 0 无
    #[serde(default)]
    pub priority: i64,
    /// 截止时间：`2026-09-19T09:30:00.000+0800`（也可能只有日期部分）
    #[serde(default, rename = "dueDate")]
    pub due_date: Option<String>,
    /// 是否全天任务（滴答对「只选日期」的任务置 true）
    #[serde(default, rename = "isAllDay")]
    pub is_all_day: bool,
    /// 状态：-1 已放弃 / 0 未完成 / 2 已完成
    #[serde(default)]
    pub status: i64,
    /// 完成时刻；有值即视为已完成
    #[serde(default, rename = "completedTime")]
    pub completed_time: Option<String>,
    /// 子任务（勾选清单）
    #[serde(default)]
    pub items: Vec<RawChecklistItem>,
}

/// 子任务条目：只关心它有没有被勾掉。
#[derive(Debug, Default, Deserialize)]
pub struct RawChecklistItem {
    /// 子任务状态：≥ 2 视为已完成
    #[serde(default)]
    pub status: i64,
}

/// 滴答原始清单：`GET /open/v1/project`。
#[derive(Debug, Default, Deserialize)]
pub struct RawProject {
    /// 清单 id
    #[serde(default)]
    pub id: String,
    /// 清单名
    #[serde(default)]
    pub name: String,
}

/// 是否不该展示：已完成（状态 ≥ 2，或带完成时刻）或已放弃（状态 < 0）。
fn is_dropped(task: &RawTask) -> bool {
    let completed = task
        .completed_time
        .as_deref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    completed || task.status < 0 || task.status >= 2
}

/// 天数 → (年, 月, 日)：Howard Hinnant 的 civil_from_days，与 [`days_from_civil`] 互逆。
///
/// 只为「把 epoch 毫秒格式化成接口要的日期串」这一步，不值得拉进整个日期库。
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 { shifted } else { shifted - 146_096 } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// (年, 月, 日) → 天数：Hinnant 的 days_from_civil，以 1970-01-01 为 0。
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// 取字符串开头连续的 ASCII 数字（用来读小时数）。
fn leading_digits(text: &str) -> &str {
    let end = text
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(text.len());
    &text[..end]
}

/// 解析 `HHMM` 形式的时区偏移为分钟。
fn parse_offset_minutes(text: &str) -> i64 {
    let digits: String = text
        .chars()
        .filter(char::is_ascii_digit)
        .take(4)
        .collect();
    let hours = digits.get(0..2).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
    let minutes = digits.get(2..4).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
    hours * 60 + minutes.clamp(0, 59)
}

/// 把滴答的日期文本解析成 UTC epoch 毫秒。
///
/// 支持两种形态（宽松）：
/// - 仅日期：`2026-09-19`（按 UTC 零点算）
/// - 完整时间戳：`2026-09-19T09:30:00.000+0800` / `...Z`（时区偏移折算回 UTC）
///
/// 返回 `None` 表示无法解析 —— 调用方按「未设置截止」处理，绝不 panic（release 是 abort）。
fn parse_due_date_ms(raw: &str) -> Option<i64> {
    let text = raw.trim();
    if text.len() < 10 || text.eq_ignore_ascii_case("null") {
        return None;
    }
    // 用 get 切片而不是 split_at：接口回的是外部数据，第 10 个字节可能落在多字节
    // 字符中间（release 是 panic = abort，一次坏数据不能带走整个 App）。
    let date = text.get(0..10)?;
    let rest = text.get(10..)?;
    let year = date.get(0..4)?.parse::<i64>().ok()?;
    let month = date.get(5..7)?.parse::<i64>().ok()?;
    let day = date.get(8..10)?.parse::<i64>().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let mut seconds_of_day = 0i64;
    let mut offset_minutes = 0i64;
    if let Some(time) = rest.strip_prefix('T') {
        let hours = leading_digits(time).parse::<i64>().ok()?;
        let minutes = time.get(3..5).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
        let seconds = time.get(6..8).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
        seconds_of_day = hours * 3600 + minutes * 60 + seconds;

        if time.contains('Z') {
            offset_minutes = 0;
        } else if let Some(index) = time.find('+') {
            offset_minutes = parse_offset_minutes(&time[index + 1..]);
        } else if let Some(index) = time.find('-') {
            offset_minutes = -parse_offset_minutes(&time[index + 1..]);
        }
    }

    let days = days_from_civil(year, month, day);
    Some((days * 86_400 + seconds_of_day) * 1_000 - offset_minutes * 60 * 1_000)
}

/// 判断一条截止时间是否「只到天」。
///
/// 滴答对「只选日期」的任务把时刻写成 `00:00:00`（时区还可能是 UTC），这时若按时分显示，
/// 用户会看到自己从没设过的 `08:00` —— 所以这种情况按天粒度展示。
fn is_date_only(raw: &str, is_all_day: bool) -> bool {
    if is_all_day {
        return true;
    }
    match raw.trim().split_once('T') {
        Some((_, time)) => time.starts_with("00:00:00"),
        None => true,
    }
}

/// 优先级 → 排序权重（越大越靠前）：高(5) > 中(3) > 低(1) > 无。
fn priority_rank(priority: i64) -> u8 {
    match priority {
        5 => 3,
        3 => 2,
        1 => 1,
        _ => 0,
    }
}

/// 把一条原始任务转成展示项；不该展示（已完成 / 已放弃）或标题为空时返回 `None`。
fn build_item(task: &RawTask, project_names: &HashMap<String, String>) -> Option<TodoTaskItem> {
    if is_dropped(task) {
        return None;
    }
    let title = task.title.trim();
    if title.is_empty() {
        return None;
    }

    let due_text = task
        .due_date
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    Some(TodoTaskItem {
        id: task.id.clone(),
        title: title.to_string(),
        project_id: task.project_id.clone(),
        project_name: project_names
            .get(&task.project_id)
            .cloned()
            .unwrap_or_else(|| fallback_project_name(&task.project_id)),
        priority: task.priority,
        due_at_ms: due_text.and_then(parse_due_date_ms),
        has_time: due_text
            .map(|value| !is_date_only(value, task.is_all_day))
            .unwrap_or(false),
        items_total: task.items.len() as i64,
        items_done: task
            .items
            .iter()
            .filter(|item| item.status >= 2)
            .count() as i64,
    })
}

/// 清单名查不到时的兜底：收件箱有专用 id，其余统一叫「清单」。
fn fallback_project_name(project_id: &str) -> String {
    if project_id == "inbox" {
        "收件箱".to_string()
    } else {
        "清单".to_string()
    }
}

/// 把原始任务数组组装成快照。
///
/// 排序口径（都按绝对时间，不依赖时区）：
/// 1. 有截止时间的在前，且按截止时间从早到晚 —— 逾期最久的自然排在最上面；
/// 2. 同一天里优先级高的在前；
/// 3. 再同则按标题，保证顺序稳定（同样的数据每次渲染顺序一致，列表不会自己跳）。
///
/// @param tasks 滴答返回的原始任务
/// @param project_names 清单 id → 清单名（拿不到就退化成「收件箱」/「清单」）
/// @param now_ms 快照构建时刻（毫秒）
pub fn make_snapshot(
    tasks: &[RawTask],
    project_names: &HashMap<String, String>,
    now_ms: i64,
) -> TodoSnapshot {
    let mut list: Vec<TodoTaskItem> = tasks
        .iter()
        .filter_map(|task| build_item(task, project_names))
        .collect();

    list.sort_by(|left, right| {
        let by_due = match (left.due_at_ms, right.due_at_ms) {
            (Some(a), Some(b)) => a.cmp(&b),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        };
        by_due
            .then_with(|| priority_rank(right.priority).cmp(&priority_rank(left.priority)))
            .then_with(|| left.title.cmp(&right.title))
    });

    TodoSnapshot {
        total_incomplete: list.len() as i64,
        list,
        updated_at_ms: now_ms,
    }
}

/// 把 epoch 毫秒格式化成滴答接口要的 `yyyy-MM-ddTHH:mm:ss+0000`（UTC）。
///
/// 只在 `task/undone` 兜底查询里用到（那个接口的起止时间是必填）。
pub fn format_utc_datetime(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let ms_of_day = ms.rem_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    let seconds = ms_of_day / 1_000;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}+0000",
        seconds / 3600,
        (seconds % 3600) / 60,
        seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一条原始任务，只覆盖测试关心的字段。
    fn task(title: &str, priority: i64, due: Option<&str>) -> RawTask {
        RawTask {
            id: format!("id-{title}"),
            title: title.to_string(),
            project_id: "p1".to_string(),
            priority,
            due_date: due.map(str::to_string),
            ..RawTask::default()
        }
    }

    fn names() -> HashMap<String, String> {
        let mut map = HashMap::new();
        map.insert("p1".to_string(), "工作".to_string());
        map
    }

    #[test]
    fn parses_timestamp_with_timezone_offset() {
        // 2026-09-19T20:30:00+0800 = 2026-09-19 12:30 UTC
        let day_ms = days_from_civil(2026, 9, 19) * 86_400_000;
        assert_eq!(
            parse_due_date_ms("2026-09-19T20:30:00.000+0800"),
            Some(day_ms + 12 * 3_600_000 + 30 * 60_000)
        );
    }

    #[test]
    fn parses_plain_date_and_zulu_timestamp() {
        let day_ms = days_from_civil(2026, 9, 19) * 86_400_000;
        assert_eq!(parse_due_date_ms("2026-09-19"), Some(day_ms));
        assert_eq!(
            parse_due_date_ms("2026-09-19T00:00:00.000Z"),
            Some(day_ms)
        );
    }

    #[test]
    fn unparsable_dates_are_treated_as_no_due_date() {
        assert_eq!(parse_due_date_ms(""), None);
        assert_eq!(parse_due_date_ms("null"), None);
        assert_eq!(parse_due_date_ms("2026-13-40"), None);
        assert_eq!(parse_due_date_ms("2026-09-19Txx:00:00+0000"), None);
    }

    #[test]
    fn date_only_detection_covers_all_day_and_midnight() {
        assert!(is_date_only("2026-09-19T00:00:00.000+0000", false));
        assert!(is_date_only("2026-09-19", false));
        assert!(is_date_only("2026-09-19T09:30:00.000+0800", true));
        assert!(!is_date_only("2026-09-19T09:30:00.000+0800", false));
    }

    #[test]
    fn drops_completed_abandoned_and_untitled_tasks() {
        let mut completed = task("已完成", 5, None);
        completed.status = 2;
        let mut abandoned = task("已放弃", 5, None);
        abandoned.status = -1;
        let mut completed_by_time = task("有完成时刻", 5, None);
        completed_by_time.completed_time = Some("2026-09-19T10:00:00+0800".to_string());
        let untitled = task("   ", 0, None);

        let snapshot = make_snapshot(
            &[completed, abandoned, completed_by_time, untitled],
            &names(),
            0,
        );
        assert_eq!(snapshot.total_incomplete, 0);
    }

    #[test]
    fn sorts_by_due_date_then_priority() {
        // 逾期最久的排最前；没有截止时间的排最后
        let tasks = vec![
            task("无截止-高", 5, None),
            task("后天", 0, Some("2026-09-21T09:00:00.000+0800")),
            task("明天-低", 1, Some("2026-09-20T09:00:00.000+0800")),
            task("昨天", 0, Some("2026-09-18T09:00:00.000+0800")),
            task("明天-中", 3, Some("2026-09-20T09:00:00.000+0800")),
        ];

        let snapshot = make_snapshot(&tasks, &names(), 0);
        let titles: Vec<&str> = snapshot.list.iter().map(|item| item.title.as_str()).collect();
        assert_eq!(
            titles,
            vec!["昨天", "明天-中", "明天-低", "后天", "无截止-高"]
        );
    }

    #[test]
    fn carries_project_name_and_checklist_progress() {
        let mut item = task("写周报", 3, Some("2026-09-19T09:00:00.000+0800"));
        item.items = vec![
            RawChecklistItem { status: 0 },
            RawChecklistItem { status: 2 },
            RawChecklistItem { status: 3 },
        ];

        let snapshot = make_snapshot(&[item], &names(), 1_700_000_000_000);
        let first = &snapshot.list[0];
        assert_eq!(first.project_name, "工作");
        assert_eq!(first.items_total, 3);
        assert_eq!(first.items_done, 2);
        assert!(first.has_time);
        assert_eq!(snapshot.updated_at_ms, 1_700_000_000_000);
    }

    #[test]
    fn unknown_project_falls_back_to_readable_names() {
        let mut inbox_task = task("收件箱任务", 0, None);
        inbox_task.project_id = "inbox".to_string();
        let mut orphan = task("没名字的清单", 0, None);
        orphan.project_id = "ghost".to_string();

        let snapshot = make_snapshot(&[inbox_task, orphan], &HashMap::new(), 0);
        let names: Vec<&str> = snapshot.list.iter().map(|item| item.project_name.as_str()).collect();
        assert!(names.contains(&"收件箱"));
        assert!(names.contains(&"清单"));
    }

    #[test]
    fn formats_utc_datetime_for_the_fallback_query() {
        // 1970-01-01T00:00:00+0000
        assert_eq!(format_utc_datetime(0), "1970-01-01T00:00:00+0000");
        // 往返一致：格式化 → 解析应回到同一时刻
        let sample = days_from_civil(2026, 7, 14) * 86_400_000 + 23 * 3_600_000 + 59 * 60_000 + 59_000;
        assert_eq!(parse_due_date_ms(&format_utc_datetime(sample)), Some(sample));
    }
}
