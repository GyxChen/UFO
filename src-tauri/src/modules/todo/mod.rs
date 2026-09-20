//! 滴答待办组件（Rust 侧）。与前端 `src/modules/todo/` 一一对应。
//!
//! 职责：用 OS 凭据管理器里的个人 API Token 调滴答 Open API，拉出未完成任务，
//! 组装成快照广播给三个窗口（悬浮徽标 / 悬停信息卡 / 主窗设置预览）。
//! 网络与凭据读取一律放进 `spawn_blocking`，命令本身只做编排 —— 非 async 命令跑在
//! 主线程，在那儿做一次最坏 15 秒的网络请求会冻住整个窗口事件循环。

pub mod client;
pub mod models;
pub mod secrets;
pub mod settings;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use tauri::{AppHandle, Emitter, State};

use crate::floating::module_config::ModuleState;
use crate::modules::todo::models::{
    format_offset_datetime, make_snapshot, TodoProjectItem, TodoSnapshot,
};
use crate::modules::todo::secrets::{delete_token, get_token, set_token};
use crate::modules::todo::settings::TodoSettings;

/// 快照广播事件名（前端 store 监听它补齐数据）。
const SNAPSHOT_EVENT: &str = "todo-snapshot";

/// 快照缓存有效期（秒）：命令级去抖，避免每个窗口各读一次就真打一遍滴答接口。
const SNAPSHOT_TTL_SEC: i64 = 60;

/// 收件箱的固定清单 id（滴答约定）：清单筛选是「全部清单」时，新建任务落在这里。
const INBOX_PROJECT_ID: &str = "inbox";

/// 当前毫秒时间戳。
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// 缓存里的一条快照：过期时刻 + 抓它时的清单筛选口径 + 快照本体。
#[derive(Clone)]
struct CachedSnapshot {
    /// 过期时刻（毫秒）
    expire_at_ms: i64,
    /// 抓这份快照时的清单筛选（空串 = 全部清单）
    project_id: String,
    /// 快照本体
    snapshot: TodoSnapshot,
}

/// 带 TTL 与「筛选口径」双条件的快照缓存。
///
/// 为什么连口径一起存：用户改完清单筛选，旧快照是按另一个口径拉的，直接命中就等于
/// 「改完筛选先看到一份旧数据」。口径不一致 = 缓存未命中，自然重新拉。
#[derive(Clone, Default)]
pub struct TodoCache {
    inner: Arc<Mutex<Option<CachedSnapshot>>>,
}

impl TodoCache {
    /// 读缓存：未过期 **且** 清单筛选口径一致才算命中。
    fn get(&self, now_ms: i64, project_id: &str) -> Option<TodoSnapshot> {
        let guard = self.inner.lock().ok()?;
        let cached = guard.as_ref()?;
        if cached.expire_at_ms > now_ms && cached.project_id == project_id {
            return Some(cached.snapshot.clone());
        }
        None
    }

    /// 写缓存（有效期 [`SNAPSHOT_TTL_SEC`] 秒）。
    fn set(&self, now_ms: i64, project_id: &str, snapshot: TodoSnapshot) {
        if let Ok(mut guard) = self.inner.lock() {
            *guard = Some(CachedSnapshot {
                expire_at_ms: now_ms + SNAPSHOT_TTL_SEC * 1000,
                project_id: project_id.to_string(),
                snapshot,
            });
        }
    }

    /// 清空缓存：Token 变更时调用（旧快照是用旧凭据抓的，不能再给用户看）。
    pub fn clear(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            *guard = None;
        }
    }
}

/// 后端持有的待办共享状态。
#[derive(Clone, Default)]
pub struct TodoState {
    /// 未完成快照缓存
    pub cache: TodoCache,
}

/// 拉一次并组装快照（阻塞；调用方负责放进 `spawn_blocking`）。
///
/// 清单名只是展示信息：取不到就不算整体失败，任务本身照常列出来，名字退化成「清单」。
fn fetch_snapshot_internal(token: &str, project_ids: &[String]) -> Result<TodoSnapshot, String> {
    let now = now_ms();
    let open = client::fetch_open_tasks(token, project_ids, now)?;
    // 已完成是「完成情况」面板的附加信息：失败不影响未完成主链路，给空列表兜底。
    let completed = match client::fetch_completed_tasks(token, project_ids, now) {
        Ok(tasks) => tasks,
        Err(error) => {
            eprintln!("failed to load dida completed tasks: {error}");
            Vec::new()
        }
    };
    let names = match client::fetch_projects(token) {
        Ok(projects) => projects
            .into_iter()
            .map(|item| (item.id, item.name))
            .collect::<HashMap<_, _>>(),
        Err(error) => {
            eprintln!("failed to load dida project names: {error}");
            HashMap::new()
        }
    };
    Ok(make_snapshot(&open, &completed, &names, now))
}

/// 拉一次快照、写回缓存并广播给所有窗口（不走缓存）。
///
/// 这是「必须拿新数据」的那条路：手动刷新、以及新建 / 完成任务之后都要立刻看到结果。
/// 清单筛选从组件自己的配置块里读，命令的参数表里不必出现它。
async fn fetch_and_broadcast(
    app: &AppHandle,
    modules_state: &ModuleState,
    todo_state: &TodoState,
) -> Result<TodoSnapshot, String> {
    let settings = TodoSettings::from_config(&modules_state.snapshot());
    let cache = todo_state.cache.clone();
    let project_ids = settings.project_ids();
    let project_id = settings.project_id.clone();
    let snapshot = tauri::async_runtime::spawn_blocking(move || {
        let token = get_token()?.ok_or_else(|| "尚未配置滴答清单 API Token".to_string())?;
        let snapshot = fetch_snapshot_internal(&token, &project_ids)?;
        cache.set(now_ms(), &project_id, snapshot.clone());
        Ok::<TodoSnapshot, String>(snapshot)
    })
    .await
    .map_err(|error| format!("执行滴答拉取任务失败: {error}"))??;

    let _ = app.emit(SNAPSHOT_EVENT, &snapshot);
    Ok(snapshot)
}

/// Tauri Command: 获取未完成待办快照（三个窗口读数据都走它）。
///
/// 有新鲜缓存时直接返回（并顺带广播一次，让刚挂载的窗口也能补上数据）；
/// 否则拉取后广播 `todo-snapshot` 给所有窗口。
#[tauri::command]
pub async fn get_todo_snapshot(
    app: AppHandle,
    modules_state: State<'_, ModuleState>,
    todo_state: State<'_, TodoState>,
    force_refresh: Option<bool>,
) -> Result<TodoSnapshot, String> {
    let force = force_refresh.unwrap_or(false);

    if !force {
        let settings = TodoSettings::from_config(&modules_state.snapshot());
        if let Some(cached) = todo_state.cache.get(now_ms(), &settings.project_id) {
            let _ = app.emit(SNAPSHOT_EVENT, &cached);
            return Ok(cached);
        }
    }

    fetch_and_broadcast(&app, &modules_state, &todo_state).await
}

/// Tauri Command: 测试连接 / 手动刷新（设置页专用）：绕开缓存强制拉一次。
///
/// 直接复用 `get_todo_snapshot` 的 `force_refresh = true` 分支 —— 取数、广播、错误文案
/// 完全一样，没必要抄一遍。`#[tauri::command]` 只额外生成 IPC 包装，原函数照常可调。
#[tauri::command]
pub async fn test_todo_connection(
    app: AppHandle,
    modules_state: State<'_, ModuleState>,
    todo_state: State<'_, TodoState>,
) -> Result<TodoSnapshot, String> {
    fetch_and_broadcast(&app, &modules_state, &todo_state).await
}

/// Tauri Command: 拉取清单列表（设置页「清单筛选」下拉用；只有主窗口有权调用）。
#[tauri::command]
pub async fn list_todo_projects() -> Result<Vec<TodoProjectItem>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let token = get_token()?.ok_or_else(|| "尚未配置滴答清单 API Token".to_string())?;
        let projects = client::fetch_projects(&token)?;
        Ok::<Vec<TodoProjectItem>, String>(
            projects
                .into_iter()
                .filter(|item| !item.id.is_empty())
                .map(|item| TodoProjectItem {
                    id: item.id,
                    name: item.name,
                })
                .collect(),
        )
    })
    .await
    .map_err(|error| format!("拉取滴答清单任务失败: {error}"))?
}

/// Tauri Command: 保存个人 API Token 到 OS 凭据管理器（仅 main 窗口）。
///
/// 凭据一变，已有快照就是「用旧 Token 抓的」，必须就地作废，否则改完 Token 还要等一个
/// 缓存周期才生效（表现为「改了没反应」）。
#[tauri::command]
pub async fn set_todo_token(todo_state: State<'_, TodoState>, token: String) -> Result<(), String> {
    let cache = todo_state.cache.clone();
    tauri::async_runtime::spawn_blocking(move || set_token(&token))
        .await
        .map_err(|error| format!("保存 Token 任务失败: {error}"))??;
    cache.clear();
    Ok(())
}

/// Tauri Command: 删除已保存的 Token（仅 main 窗口）。
#[tauri::command]
pub async fn delete_todo_token(todo_state: State<'_, TodoState>) -> Result<(), String> {
    let cache = todo_state.cache.clone();
    tauri::async_runtime::spawn_blocking(delete_token)
        .await
        .map_err(|error| format!("删除 Token 任务失败: {error}"))??;
    cache.clear();
    Ok(())
}

/// Tauri Command: 查询是否已保存 Token（只回布尔，绝不回显明文）。
#[tauri::command]
pub async fn get_todo_secret_status() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || Ok(get_token()?.is_some()))
        .await
        .map_err(|error| format!("读取 Token 状态失败: {error}"))?
}

/// Tauri Command: 新建一条任务（设置页「新增任务」表单）。
///
/// 前端把「本机今天零点」的毫秒与当时的时区偏移一起传进来，这里只负责格式化成滴答要的
/// 日期串 —— 后端没有用户时区的可靠信息，在那边算「今天」可能算成昨天 / 明天。
/// 建完强制重拉并广播：三个窗口（含刚打开的主窗）立刻拿到一致的数据。
///
/// @param title 任务标题（trim 后不能为空）
/// @param priority 优先级：5 高 / 3 中 / 1 低（其它值一律按「中」处理）
/// @param due_at_ms 截止时刻（本机今天零点的 epoch 毫秒）
/// @param offset_minutes 本机时区相对 UTC 的分钟偏移（东八区 = 480）
#[tauri::command]
pub async fn create_todo_task(
    app: AppHandle,
    modules_state: State<'_, ModuleState>,
    todo_state: State<'_, TodoState>,
    title: String,
    priority: i64,
    due_at_ms: i64,
    offset_minutes: i64,
) -> Result<TodoSnapshot, String> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err("任务标题不能为空".to_string());
    }
    // 只认三档；其余取值按「中」兜底，别把奇怪的数字写进滴答
    let priority = match priority {
        5 => 5,
        1 => 1,
        _ => 3,
    };

    let settings = TodoSettings::from_config(&modules_state.snapshot());
    // 筛成「全部清单」时没有目标清单 —— 落到收件箱
    let project_id = if settings.project_id.is_empty() {
        INBOX_PROJECT_ID.to_string()
    } else {
        settings.project_id.clone()
    };
    let due_date = format_offset_datetime(due_at_ms, offset_minutes);

    let cache = todo_state.cache.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let token = get_token()?.ok_or_else(|| "尚未配置滴答清单 API Token".to_string())?;
        let task = client::NewTask {
            title: &title,
            project_id: &project_id,
            priority,
            due_date: &due_date,
        };
        client::create_task(&token, &task)?;
        // 旧快照不含这条新任务：先作废，免得并发读命中它
        cache.clear();
        Ok::<(), String>(())
    })
    .await
    .map_err(|error| format!("执行滴答创建任务失败: {error}"))??;

    fetch_and_broadcast(&app, &modules_state, &todo_state).await
}

/// 一条待办任务的定位：完成 / 顺延 / 删除都要「清单 id + 任务 id」两个都知道。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskTarget {
    /// 任务所属清单 id
    pub project_id: String,
    /// 任务 id
    pub task_id: String,
}

/// 清洗一批目标：trim 掉两端空白，并丢掉任务 id 为空的 —— 空 id 会拼出打到别处的路径。
fn sanitize_targets(targets: Vec<TaskTarget>) -> Vec<TaskTarget> {
    targets
        .into_iter()
        .map(|target| TaskTarget {
            project_id: target.project_id.trim().to_string(),
            task_id: target.task_id.trim().to_string(),
        })
        .filter(|target| !target.task_id.is_empty())
        .collect()
}

/// Tauri Command: 完成一条或多条任务（悬停信息卡点任务行 / 设置页点标题触发）。
///
/// 收一批是给「最后一个子目标完成，父目标自动完成」留的口子：前端算出要连带完成的祖先链后
/// 一枪发过来，这里在同一次重拉里收尾 —— 一条一次往返的话，每完成一条父目标都要重扫一遍
/// 未完成列表（200 条 × 3 个接口），树越深越慢。
///
/// 部分失败按「刷新后的真实状态」收场：只要有一条成功就重拉广播（成败由滴答说了算），
/// 一条都没成才把原文错误回给用户（此时状态没变，旧快照仍然有效）。
///
/// @param targets 要完成的任务，顺序即提交顺序（父目标应排在它的子目标之后）
#[tauri::command]
pub async fn complete_todo_task(
    app: AppHandle,
    modules_state: State<'_, ModuleState>,
    todo_state: State<'_, TodoState>,
    targets: Vec<TaskTarget>,
) -> Result<TodoSnapshot, String> {
    let targets = sanitize_targets(targets);
    if targets.is_empty() {
        return Err("任务 id 不能为空".to_string());
    }

    let (succeeded, first_error) = tauri::async_runtime::spawn_blocking(move || {
        let token = get_token()?.ok_or_else(|| "尚未配置滴答清单 API Token".to_string())?;
        let mut succeeded = 0usize;
        let mut first_error: Option<String> = None;
        for target in &targets {
            match client::complete_task(&token, &target.project_id, &target.task_id) {
                Ok(()) => succeeded += 1,
                Err(error) => {
                    eprintln!("failed to complete dida task {}: {error}", target.task_id);
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
        Ok::<(usize, Option<String>), String>((succeeded, first_error))
    })
    .await
    .map_err(|error| format!("执行滴答完成任务失败: {error}"))??;

    if succeeded == 0 {
        return Err(first_error.unwrap_or_else(|| "完成任务失败".to_string()));
    }
    // 至少有一条成功：旧快照一定不对了，作废后重拉
    todo_state.cache.clear();
    fetch_and_broadcast(&app, &modules_state, &todo_state).await
}

/// Tauri Command: 把一批任务的截止时间顺延到今天（设置页「完成情况」的右键菜单 / 「全部顺延」）。
///
/// 收一批是给「全部顺延」用的：逾期目标可能几十条，一条一次往返既慢又会触发 N 次重拉。
/// 收尾方式与「完成」一致：一条都没成才报错（状态没变），否则重拉（结果以滴答为准）。
///
/// @param targets 要顺延的任务
/// @param due_at_ms 新的截止时刻（前端算好的「本机今天零点」epoch 毫秒）
/// @param offset_minutes 本机时区相对 UTC 的分钟偏移（东八区 = 480）
#[tauri::command]
pub async fn postpone_todo_tasks(
    app: AppHandle,
    modules_state: State<'_, ModuleState>,
    todo_state: State<'_, TodoState>,
    targets: Vec<TaskTarget>,
    due_at_ms: i64,
    offset_minutes: i64,
) -> Result<TodoSnapshot, String> {
    let targets = sanitize_targets(targets);
    if targets.is_empty() {
        return Err("任务 id 不能为空".to_string());
    }
    // 时区换算跟前端配合的方式与「新增任务」一致：前端给绝对时刻 + 当时的偏移，这里只负责渲染。
    let due_date = format_offset_datetime(due_at_ms, offset_minutes);

    let (succeeded, first_error) = tauri::async_runtime::spawn_blocking(move || {
        let token = get_token()?.ok_or_else(|| "尚未配置滴答清单 API Token".to_string())?;
        let mut succeeded = 0usize;
        let mut first_error: Option<String> = None;
        for target in &targets {
            match client::postpone_task(
                &token,
                &target.project_id,
                &target.task_id,
                &due_date,
            ) {
                Ok(()) => succeeded += 1,
                Err(error) => {
                    eprintln!("failed to postpone dida task {}: {error}", target.task_id);
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
        Ok::<(usize, Option<String>), String>((succeeded, first_error))
    })
    .await
    .map_err(|error| format!("执行滴答顺延任务失败: {error}"))??;

    if succeeded == 0 {
        return Err(first_error.unwrap_or_else(|| "顺延任务失败".to_string()));
    }
    todo_state.cache.clear();
    fetch_and_broadcast(&app, &modules_state, &todo_state).await
}

/// Tauri Command: 删除一条任务（设置页「完成情况」的右键菜单）。
///
/// 删除**不可撤销**，所以前端必须先做二次确认再调这里；命令本身只负责「删掉 + 重拉广播」。
///
/// @param project_id 任务所属清单 id
/// @param task_id 任务 id
#[tauri::command]
pub async fn delete_todo_task(
    app: AppHandle,
    modules_state: State<'_, ModuleState>,
    todo_state: State<'_, TodoState>,
    project_id: String,
    task_id: String,
) -> Result<TodoSnapshot, String> {
    let task_id = task_id.trim().to_string();
    if task_id.is_empty() {
        return Err("任务 id 不能为空".to_string());
    }
    let project_id = project_id.trim().to_string();

    tauri::async_runtime::spawn_blocking(move || {
        let token = get_token()?.ok_or_else(|| "尚未配置滴答清单 API Token".to_string())?;
        client::delete_task(&token, &project_id, &task_id)?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|error| format!("执行滴答删除任务失败: {error}"))??;

    // 那条任务连同它的子目标一起没了，旧快照必然过期
    todo_state.cache.clear();
    fetch_and_broadcast(&app, &modules_state, &todo_state).await
}

/// Tauri Command: 取消完成（重新打开）一条任务（设置页「完成情况」里点已完成任务触发）。
///
/// Open API 没有专门的「取消完成」端点，走「更新任务」把 status 置回未完成（见 `client::reopen_task`）。
/// 与完成对称：成功后强制重拉并广播 —— 那一行会从「已完成」组消失、重新出现在「未完成」组，
/// 三档计数与悬浮窗徽标同步更新。错误（网络 / 鉴权 / 已被删除）原样回给前端展示。
///
/// @param project_id 任务所属清单 id
/// @param task_id 任务 id
#[tauri::command]
pub async fn reopen_todo_task(
    app: AppHandle,
    modules_state: State<'_, ModuleState>,
    todo_state: State<'_, TodoState>,
    project_id: String,
    task_id: String,
) -> Result<TodoSnapshot, String> {
    let task_id = task_id.trim().to_string();
    if task_id.is_empty() {
        return Err("任务 id 不能为空".to_string());
    }
    let project_id = project_id.trim().to_string();

    let cache = todo_state.cache.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let token = get_token()?.ok_or_else(|| "尚未配置滴答清单 API Token".to_string())?;
        client::reopen_task(&token, &project_id, &task_id)?;
        cache.clear();
        Ok::<(), String>(())
    })
    .await
    .map_err(|error| format!("执行滴答取消完成任务失败: {error}"))??;

    fetch_and_broadcast(&app, &modules_state, &todo_state).await
}
