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

/// `task/filter` 的已完成状态码（2 = Completed）。
const STATUS_COMPLETED: i64 = 2;

/// 拉取已完成任务往回调几天：只关心最近这段时间完成的任务（「完成情况」面板的窗口）。
const COMPLETED_LOOKBACK_DAYS: i64 = 7;

/// `task/undone` 兜底查询往前看的天数：接口要求区间不超过 14 天。
const UNDONE_LOOKBACK_DAYS: i64 = 13;

/// 一天有多少毫秒。
const DAY_MS: i64 = 86_400_000;

/// `call` 的 HTTP 方法。
///
/// 显式列出来而不是用「有没有请求体」推断：DELETE 也是无体的，靠 `body: None` 猜会把删除发成 GET。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// 读（无请求体）
    Get,
    /// 写（带 JSON 请求体）
    Post,
    /// 删（无请求体）
    Delete,
}

/// 发一次带鉴权的请求，返回响应体字符串。
///
/// @param token 个人 API Token（已从凭据管理器读出）
/// @param method HTTP 方法
/// @param path Open API 路径（以 `/` 开头，如 `/task/filter`）
/// @param body POST 的 JSON 请求体；无体请求（GET / DELETE）传 `None`
fn call(token: &str, method: Method, path: &str, body: Option<&str>) -> Result<String, String> {
    let url = format!("{BASE_URL}{path}");
    let config = ureq::config::Config::builder()
        .timeout_global(Some(Duration::from_secs(TIMEOUT_SEC)))
        // 状态码自己判断（4xx 也要读响应体，才能把服务端给的原因带回来）
        .http_status_as_error(false)
        .max_redirects(3)
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let authorization = format!("Bearer {token}");

    // 三种方法的构建器类型不同，但两边收尾都返回同一个 `Response`，所以能合在这里。
    let response = match method {
        Method::Post => agent
            .post(&url)
            .header("Content-Type", "application/json")
            .header("User-Agent", "UFO-Floating-Todo/1.0")
            .header("Accept", "application/json")
            .header("Authorization", &authorization)
            .send(body.unwrap_or("{}")),
        Method::Get => agent
            .get(&url)
            .header("User-Agent", "UFO-Floating-Todo/1.0")
            .header("Accept", "application/json")
            .header("Authorization", &authorization)
            .call(),
        Method::Delete => agent
            .delete(&url)
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
    let body = call(token, Method::Get, "/project", None)?;
    serde_json::from_str(&body).map_err(|err| format!("解析滴答清单列表失败: {err}"))
}

/// 按状态筛选任务：`POST /open/v1/task/filter`（只取未完成，最多 200 条）。
///
/// 不限定日期是有意的：这才是「我的未完成任务」的完整口径，不会漏掉没有截止时间的任务。
pub fn filter_open_tasks(token: &str, project_ids: &[String]) -> Result<Vec<RawTask>, String> {
    let body = filter_body(project_ids);
    let raw = call(token, Method::Post, "/task/filter", Some(&body))?;
    serde_json::from_str(&raw).map_err(|err| format!("解析滴答任务列表失败: {err}"))
}

/// 按日期区间取未完成任务：`POST /open/v1/task/undone`（起止时间必填，区间不超过 14 天）。
fn undone_tasks(token: &str, project_ids: &[String], now_ms: i64) -> Result<Vec<RawTask>, String> {
    let start = format_utc_datetime(now_ms - UNDONE_LOOKBACK_DAYS * DAY_MS);
    // 终点放到「明天同一时刻」：今天的任务不论几点截止都落在区间里。
    let end = format_utc_datetime(now_ms + DAY_MS);
    let body = undone_body(project_ids, &start, &end);
    let raw = call(token, Method::Post, "/task/undone", Some(&body))?;
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

/// 按状态 + 完成时间区间取任务：`POST /open/v1/task/filter`。
///
/// 完成时间下限给毫秒时间戳；上界不放（「近 N 天」只需要下限）。厂商若对
/// `fromCompletedTime` 的命名或语义有出入，本函数只负责把请求发出去，真正的「近 7 天」
/// 窗口仍由 `models::make_snapshot` 在客户端按 `completed_at_ms` 后置过滤兜底。
fn filter_completed_tasks(
    token: &str,
    project_ids: &[String],
    from_ms: i64,
) -> Result<Vec<RawTask>, String> {
    let body = completed_body(project_ids, from_ms);
    let raw = call(token, Method::Post, "/task/filter", Some(&body))?;
    serde_json::from_str(&raw).map_err(|err| format!("解析滴答任务列表失败: {err}"))
}

/// 拉取近 [`COMPLETED_LOOKBACK_DAYS`] 天完成的任务。
///
/// 失败对主流程**不致命**（「完成情况」是附加信息，漏了不该拖垮整份快照）——调用方决定
/// 是否容忍，这里只负责把 `Ok/Err` 传回去。
///
/// @param token 个人 API Token
/// @param project_ids 清单筛选（空 = 全部清单）
/// @param now_ms 当前毫秒时间戳（往回推完成时间的下限）
pub fn fetch_completed_tasks(
    token: &str,
    project_ids: &[String],
    now_ms: i64,
) -> Result<Vec<RawTask>, String> {
    filter_completed_tasks(token, project_ids, now_ms - COMPLETED_LOOKBACK_DAYS * DAY_MS)
}

/// 新建任务要提交的字段。
///
/// 参数一多就容易把顺序传错（标题 / 清单 / 优先级 / 日期全是短字符串或数字），
/// 收成一个结构体让调用方必须写字段名。
pub struct NewTask<'a> {
    /// 任务标题（调用方已 trim，非空）
    pub title: &'a str,
    /// 目标清单 id（收件箱固定为 `inbox`）
    pub project_id: &'a str,
    /// 优先级：5 高 / 3 中 / 1 低
    pub priority: i64,
    /// 截止时间串（`yyyy-MM-ddTHH:mm:ss+0800`；全天任务给当地零点）
    pub due_date: &'a str,
}

/// 新建一条任务：`POST /open/v1/task`（接口返回的是新建出来的任务，这里只要「成功」）。
///
/// 固定 `isAllDay: true`：本组件的「新增」就是「今天的任务」，不选时刻。
pub fn create_task(token: &str, task: &NewTask<'_>) -> Result<(), String> {
    let body = create_task_body(task);
    call(token, Method::Post, "/task", Some(&body))?;
    Ok(())
}

/// 完成一条任务：`POST /open/v1/project/{projectId}/task/{taskId}/complete`。
///
/// 路径里的两个 id 来自接口返回的数据，拼 URL 前先做百分号编码（见 [`encode_path_segment`]）。
pub fn complete_task(token: &str, project_id: &str, task_id: &str) -> Result<(), String> {
    let path = complete_path(project_id, task_id);
    call(token, Method::Post, &path, Some("{}"))?;
    Ok(())
}

/// 组装完成任务的路径。
fn complete_path(project_id: &str, task_id: &str) -> String {
    format!(
        "/project/{}/task/{}/complete",
        encode_path_segment(project_id),
        encode_path_segment(task_id)
    )
}

/// 重新打开（取消完成）一条任务：`POST /open/v1/task/{taskId}?projectId={projectId}` 把 status 置回未完成。
///
/// Open API **没有**专门的「取消完成」端点（complete 是单向的专用端点）：撤回完成态唯一的口子就是
/// 「更新任务」——把 `status` 从已完成([`STATUS_COMPLETED`] 2) 置回 [`STATUS_OPEN`](0)。
/// `projectId` 是「更新任务」的**必填查询参数**（与完成端点把它放在路径里不同），所以拼在 URL 上；
/// 请求体按 TickTask 的 Task 结构带上 id / projectId / status，服务器据此把这条任务改回未完成。
pub fn reopen_task(token: &str, project_id: &str, task_id: &str) -> Result<(), String> {
    let path = update_path(project_id, task_id);
    call(
        token,
        Method::Post,
        &path,
        Some(&reopen_body(project_id, task_id)),
    )?;
    Ok(())
}

/// 组装「取消完成」的请求体：id / projectId 供服务器定位，status 置回未完成。
fn reopen_body(project_id: &str, task_id: &str) -> String {
    serde_json::json!({
        "id": task_id,
        "projectId": project_id,
        "status": STATUS_OPEN,
    })
    .to_string()
}

/// 把一条任务的截止日期改到指定的一天（「顺延到今天」）。
///
/// 与 [`reopen_task`] 共用「更新任务」端点，只是请求体换了字段：带上新的 `dueDate` 并固定
/// `isAllDay: true` —— 顺延的语义是「挪到某一天」，和「新增任务」一样不挑时刻。
///
/// @param project_id 任务所属清单 id（更新端点的必填查询参数）
/// @param task_id 任务 id
/// @param due_date 新的截止串（`yyyy-MM-ddTHH:mm:ss±HHMM`，由调用方按用户时区算好）
pub fn postpone_task(
    token: &str,
    project_id: &str,
    task_id: &str,
    due_date: &str,
) -> Result<(), String> {
    let path = update_path(project_id, task_id);
    call(
        token,
        Method::Post,
        &path,
        Some(&postpone_body(project_id, task_id, due_date)),
    )?;
    Ok(())
}

/// 组装「顺延」的请求体：带上新的 `dueDate` 并固定 `isAllDay: true`。
fn postpone_body(project_id: &str, task_id: &str, due_date: &str) -> String {
    serde_json::json!({
        "id": task_id,
        "projectId": project_id,
        "dueDate": due_date,
        "isAllDay": true,
    })
    .to_string()
}

/// 删除一条任务：`DELETE /open/v1/project/{projectId}/task/{taskId}`。
///
/// 与「完成」同为 project 作用域，只是路径少一层 `/complete`、方法换成 DELETE。
pub fn delete_task(token: &str, project_id: &str, task_id: &str) -> Result<(), String> {
    let path = delete_path(project_id, task_id);
    call(token, Method::Delete, &path, None)?;
    Ok(())
}

/// 组装「更新任务」的路径：`/task/{taskId}?projectId={projectId}`（projectId 是必填查询参数）。
///
/// 取消完成与顺延都走它 —— 同一个端点，只是请求体不同。
fn update_path(project_id: &str, task_id: &str) -> String {
    format!(
        "/task/{}?projectId={}",
        encode_path_segment(task_id),
        encode_path_segment(project_id)
    )
}

/// 组装删除任务的路径：`/project/{projectId}/task/{taskId}`。
fn delete_path(project_id: &str, task_id: &str) -> String {
    format!(
        "/project/{}/task/{}",
        encode_path_segment(project_id),
        encode_path_segment(task_id)
    )
}

/// 把路径片段做最小百分号编码：只放行 URL 路径里安全的字符。
///
/// 清单 / 任务 id 目前都是十六进制串，正常不会命中；但它们是**外部数据**，直接拼进
/// URL 的路径段时留一个编码口子，免得将来出现 `/` 或空格把请求打到别的路径上。
fn encode_path_segment(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// 组装新建任务的请求体：标题 + 清单 + 优先级 + 全天截止。
fn create_task_body(task: &NewTask<'_>) -> String {
    serde_json::json!({
        "title": task.title,
        "projectId": task.project_id,
        "priority": task.priority,
        "dueDate": task.due_date,
        "isAllDay": true,
    })
    .to_string()
}

/// 组装 `task/filter` 的请求体：只要未完成；清单列表为空表示「全部清单」。
fn filter_body(project_ids: &[String]) -> String {
    let mut body = serde_json::json!({ "status": [STATUS_OPEN] });
    if !project_ids.is_empty() {
        body["projectIds"] = serde_json::json!(project_ids);
    }
    body.to_string()
}

/// 组装「已完成」的 `task/filter` 请求体：已完成状态 + 完成时间下限（毫秒）。
fn completed_body(project_ids: &[String], from_ms: i64) -> String {
    let mut body = serde_json::json!({ "status": [STATUS_COMPLETED], "fromCompletedTime": from_ms });
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
    fn completed_body_carries_status_and_completed_time_lower_bound() {
        let from_ms: i64 = 1_700_000_000_000;
        let body: serde_json::Value = serde_json::from_str(&completed_body(&[], from_ms)).unwrap();
        assert_eq!(
            body,
            serde_json::json!({ "status": [2], "fromCompletedTime": from_ms })
        );
    }

    #[test]
    fn completed_body_carries_project_ids_when_filtered() {
        let body: serde_json::Value =
            serde_json::from_str(&completed_body(&["inbox".to_string()], 5)).unwrap();
        assert_eq!(
            body,
            serde_json::json!({ "status": [2], "fromCompletedTime": 5, "projectIds": ["inbox"] })
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
    fn create_task_body_carries_title_project_priority_and_all_day_due_date() {
        let body: serde_json::Value = serde_json::from_str(&create_task_body(&NewTask {
            title: "写周报",
            project_id: "inbox",
            priority: 5,
            due_date: "2026-09-20T00:00:00+0800",
        }))
        .unwrap();
        assert_eq!(
            body,
            serde_json::json!({
                "title": "写周报",
                "projectId": "inbox",
                "priority": 5,
                "dueDate": "2026-09-20T00:00:00+0800",
                "isAllDay": true,
            })
        );
    }

    #[test]
    fn complete_path_points_at_the_project_scoped_endpoint() {
        assert_eq!(
            complete_path("p1", "t9"),
            "/project/p1/task/t9/complete"
        );
    }

    #[test]
    fn update_path_targets_the_update_endpoint_with_project_id_query() {
        assert_eq!(update_path("p1", "t9"), "/task/t9?projectId=p1");
    }

    #[test]
    fn postpone_body_carries_the_new_due_date_as_an_all_day_task() {
        // 顺延 = 复用「更新任务」端点，只换 dueDate / isAllDay 两个字段
        let body: serde_json::Value =
            serde_json::from_str(&postpone_body("p1", "t9", "2026-09-20T00:00:00+0800")).unwrap();
        assert_eq!(
            body,
            serde_json::json!({
                "id": "t9",
                "projectId": "p1",
                "dueDate": "2026-09-20T00:00:00+0800",
                "isAllDay": true,
            })
        );
    }

    #[test]
    fn delete_path_points_at_the_project_scoped_task() {
        assert_eq!(delete_path("p1", "t9"), "/project/p1/task/t9");
    }

    #[test]
    fn delete_path_percent_encodes_ids() {
        assert_eq!(delete_path("a/b", "t?1"), "/project/a%2Fb/task/t%3F1");
    }

    #[test]
    fn reopen_body_resets_status_to_open_with_ids() {
        // 取消完成 = 把 status 从已完成(2) 置回未完成(0)，并带上 id / projectId 供服务器识别
        let body: serde_json::Value = serde_json::from_str(&reopen_body("p1", "t9")).unwrap();
        assert_eq!(body, serde_json::json!({ "id": "t9", "projectId": "p1", "status": 0 }));
    }

    #[test]
    fn update_path_percent_encodes_ids() {
        assert_eq!(update_path("a/b", "t?1"), "/task/t%3F1?projectId=a%2Fb");
    }

    #[test]
    fn path_segments_are_percent_encoded() {
        // 常规 id 原样保留
        assert_eq!(encode_path_segment("69a850f41c20d2030e148fdf"), "69a850f41c20d2030e148fdf");
        // 危险字符被转义，不会改变路径结构
        assert_eq!(encode_path_segment("a/b c"), "a%2Fb%20c");
        assert_eq!(complete_path("a/b", "t?1"), "/project/a%2Fb/task/t%3F1/complete");
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
