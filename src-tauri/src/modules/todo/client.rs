//! 滴答清单 Open API 客户端（`https://api.dida365.com/open/v1`）。
//!
//! 认证走个人 API Token（滴答网页版「设置 → 账户 → API Token」生成），请求头带
//! `Bearer` 令牌；Token 只从 OS 凭据管理器读，绝不进日志。出网收敛在这一个客户端里：
//! 统一超时 / 重定向控制 / 响应上限 / 错误文案。

use std::{io::Read, time::Duration};

use crate::modules::todo::models::{format_utc_datetime, RawProject, RawTask};

/// Open API 基址。
const BASE_URL: &str = "https://api.dida365.com/open/v1";

/// 单次响应体最大字节限制（512 KB）：一次最多 200 条任务，正常远小于它。
const MAX_RESPONSE_BYTES: usize = 512 * 1024;

/// 错误信息里回显的响应体长度上限（按字符计）：错误要看得懂，但不能把整页网关 HTML 塞进界面。
const ERROR_SNIPPET_CHARS: usize = 160;

/// 单次请求超时（秒）。
const TIMEOUT_SEC: u64 = 15;

/// `task/filter` 的未完成状态码（0 = Open / Normal）。
const STATUS_OPEN: i64 = 0;

/// `task/undone` 兜底查询往前看的天数：接口要求区间不超过 14 天。
const UNDONE_LOOKBACK_DAYS: i64 = 13;

/// 一天有多少毫秒。
const DAY_MS: i64 = 86_400_000;

/// 发一次带鉴权的请求，返回响应体字符串。
///
/// @param token 个人 API Token（已从凭据管理器读出）
/// @param path Open API 路径（以 `/` 开头，如 `/task/filter`）
/// @param body JSON 请求体；`None` = GET，`Some` = POST
fn call(token: &str, path: &str, body: Option<&str>) -> Result<String, String> {
    let url = format!("{BASE_URL}{path}");
    let config = ureq::config::Config::builder()
        .timeout_global(Some(Duration::from_secs(TIMEOUT_SEC)))
        // 状态码自己判断（4xx 也要读响应体，才能把服务端给的原因带回来）
        .http_status_as_error(false)
        .max_redirects(3)
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let authorization = format!("Bearer {token}");

    // GET / POST 的构建器类型不同，但两边收尾都返回同一个 `Response`，所以能合在这里。
    let response = match body {
        Some(payload) => agent
            .post(&url)
            .header("Content-Type", "application/json")
            .header("User-Agent", "UFO-Floating-Todo/1.0")
            .header("Accept", "application/json")
            .header("Authorization", &authorization)
            .send(payload),
        None => agent
            .get(&url)
            .header("User-Agent", "UFO-Floating-Todo/1.0")
            .header("Accept", "application/json")
            .header("Authorization", &authorization)
            .call(),
    }
    .map_err(|err| format!("滴答接口请求失败: {err}"))?;

    let status = response.status().as_u16();
    let mut reader = response
        .into_body()
        .into_reader()
        .take(MAX_RESPONSE_BYTES as u64);
    let mut body_text = String::new();
    reader
        .read_to_string(&mut body_text)
        .map_err(|err| format!("读取滴答响应失败: {err}"))?;

    if status == 401 || status == 403 {
        return Err("滴答 API Token 无效或已过期，请在设置中重新填写".to_string());
    }
    if !(200..300).contains(&status) {
        return Err(format!("滴答接口返回 HTTP {status}: {}", snippet(&body_text)));
    }
    Ok(body_text)
}

/// 拉取清单（项目）列表：`GET /open/v1/project`。
pub fn fetch_projects(token: &str) -> Result<Vec<RawProject>, String> {
    let body = call(token, "/project", None)?;
    serde_json::from_str(&body).map_err(|err| format!("解析滴答清单列表失败: {err}"))
}

/// 按状态筛选任务：`POST /open/v1/task/filter`（只取未完成，最多 200 条）。
///
/// 不限定日期是有意的：这才是「我的未完成任务」的完整口径，不会漏掉没有截止时间的任务。
pub fn filter_open_tasks(token: &str, project_ids: &[String]) -> Result<Vec<RawTask>, String> {
    let body = filter_body(project_ids);
    let raw = call(token, "/task/filter", Some(&body))?;
    serde_json::from_str(&raw).map_err(|err| format!("解析滴答任务列表失败: {err}"))
}

/// 按日期区间取未完成任务：`POST /open/v1/task/undone`（起止时间必填，区间不超过 14 天）。
fn undone_tasks(token: &str, project_ids: &[String], now_ms: i64) -> Result<Vec<RawTask>, String> {
    let start = format_utc_datetime(now_ms - UNDONE_LOOKBACK_DAYS * DAY_MS);
    // 终点放到「明天同一时刻」：今天的任务不论几点截止都落在区间里。
    let end = format_utc_datetime(now_ms + DAY_MS);
    let body = undone_body(project_ids, &start, &end);
    let raw = call(token, "/task/undone", Some(&body))?;
    serde_json::from_str(&raw).map_err(|err| format!("解析滴答任务列表失败: {err}"))
}

/// 拉取未完成任务：主路径 [`filter_open_tasks`]，失败时退到 [`undone_tasks`]。
///
/// 为什么要兜底：Open API 文档没把「只按状态过滤、不限日期」列成必填组合，万一服务端
/// 要求日期区间，主路径会直接报错 —— 那就用文档里语义更明确（但只能覆盖最近 14 天）
/// 的 undone 再试一次。两条路返回的都是同一个 Task 数组，解析完全共用；两条都失败时
/// 回报主路径的错误，因为那才是用户更可能撞上的那条。
///
/// @param token 个人 API Token
/// @param project_ids 清单筛选（空 = 全部清单）
/// @param now_ms 当前毫秒时间戳（用于算兜底查询的日期区间）
pub fn fetch_open_tasks(
    token: &str,
    project_ids: &[String],
    now_ms: i64,
) -> Result<Vec<RawTask>, String> {
    match filter_open_tasks(token, project_ids) {
        Ok(tasks) => Ok(tasks),
        Err(primary) => undone_tasks(token, project_ids, now_ms).map_err(|fallback| {
            eprintln!("dida undone fallback also failed: {fallback}");
            primary
        }),
    }
}

/// 组装 `task/filter` 的请求体：只要未完成；清单列表为空表示「全部清单」。
fn filter_body(project_ids: &[String]) -> String {
    let mut body = serde_json::json!({ "status": [STATUS_OPEN] });
    if !project_ids.is_empty() {
        body["projectIds"] = serde_json::json!(project_ids);
    }
    body.to_string()
}

/// 组装 `task/undone` 的请求体：起止时间必填，清单列表可选。
fn undone_body(project_ids: &[String], start: &str, end: &str) -> String {
    let mut body = serde_json::json!({ "startDate": start, "endDate": end });
    if !project_ids.is_empty() {
        body["projectIds"] = serde_json::json!(project_ids);
    }
    body.to_string()
}

/// 把出错的响应体压成一行并截断（见 [`ERROR_SNIPPET_CHARS`]），供错误信息回显。
fn snippet(body: &str) -> String {
    let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= ERROR_SNIPPET_CHARS {
        return flat;
    }
    let mut out: String = flat.chars().take(ERROR_SNIPPET_CHARS).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_body_only_carries_the_status_when_all_projects_are_watched() {
        let body: serde_json::Value = serde_json::from_str(&filter_body(&[])).unwrap();
        assert_eq!(body, serde_json::json!({ "status": [0] }));
    }

    #[test]
    fn filter_body_carries_project_ids_when_filtered() {
        let body: serde_json::Value =
            serde_json::from_str(&filter_body(&["p1".to_string()])).unwrap();
        assert_eq!(
            body,
            serde_json::json!({ "status": [0], "projectIds": ["p1"] })
        );
    }

    #[test]
    fn undone_body_carries_the_required_date_range() {
        let body: serde_json::Value = serde_json::from_str(&undone_body(
            &["inbox".to_string()],
            "2026-07-01T00:00:00+0000",
            "2026-07-14T23:59:59+0000",
        ))
        .unwrap();
        assert_eq!(
            body,
            serde_json::json!({
                "startDate": "2026-07-01T00:00:00+0000",
                "endDate": "2026-07-14T23:59:59+0000",
                "projectIds": ["inbox"],
            })
        );
    }

    #[test]
    fn error_snippet_is_single_line_and_truncated() {
        let body = format!("Unauthorized token expired {}", "x".repeat(400));
        let text = snippet(&body);
        // 压成一行：行数始终是 1（响应体里带换行时也不会把界面撑开）
        assert_eq!(text.lines().count(), 1);
        assert_eq!(text.chars().count(), ERROR_SNIPPET_CHARS + 1);
        assert!(text.ends_with('…'));
    }

    #[test]
    fn error_snippet_keeps_short_bodies_untouched() {
        assert_eq!(snippet("  not  found "), "not found");
    }
}
