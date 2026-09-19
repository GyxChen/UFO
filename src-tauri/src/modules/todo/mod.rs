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

use tauri::{AppHandle, Emitter, State};

use crate::floating::module_config::ModuleState;
use crate::modules::todo::models::{make_snapshot, TodoProjectItem, TodoSnapshot};
use crate::modules::todo::secrets::{delete_token, get_token, set_token};
use crate::modules::todo::settings::TodoSettings;

/// 快照广播事件名（前端 store 监听它补齐数据）。
const SNAPSHOT_EVENT: &str = "todo-snapshot";

/// 快照缓存有效期（秒）：命令级去抖，避免每个窗口各读一次就真打一遍滴答接口。
const SNAPSHOT_TTL_SEC: i64 = 60;

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
    let tasks = client::fetch_open_tasks(token, project_ids, now)?;
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
    Ok(make_snapshot(&tasks, &names, now))
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
    // 清单筛选是组件自己的配置：组件自己读，命令的参数表里不必出现它。
    let settings = TodoSettings::from_config(&modules_state.snapshot());

    if !force {
        if let Some(cached) = todo_state.cache.get(now_ms(), &settings.project_id) {
            let _ = app.emit(SNAPSHOT_EVENT, &cached);
            return Ok(cached);
        }
    }

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
    get_todo_snapshot(app, modules_state, todo_state, Some(true)).await
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
