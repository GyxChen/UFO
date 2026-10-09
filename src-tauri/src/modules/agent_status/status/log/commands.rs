//! 面板读日志 / 清空 / 导出 / 跟随的 tauri 命令。
//!
//! 权限拆分照会话记录那一套：**读命令给所有窗口**（悬浮窗也可能要看，见
//! `capabilities/default.json`），**清空 / 导出 / 计数 / 订阅只给主窗与它派出的独立面板窗**
//! （`capabilities/main-module-settings.json`）。
//!
//! ⚠️ 这些命令一律**不碰 registry、不碰灯态**：只做「把已经落盘的账拿出来给人看」。

use tauri::State;

use super::runtime::{ExportedLog, StatusLogCount, StatusLogPage, StatusLogQuery};
use crate::modules::agent_status::status::StatusState;

/// 取一页转灯日志（**按盘倒序**，缺省查询 = 最新一页）。
///
/// ⚠️ `async` + `spawn_blocking`：一页数据现在是从 JSONL 倒着翻出来的，窄过滤也可能逼着
/// 它扫完整个目录；非 async 命令跑在**主线程**，那样会把窗口事件循环冻住。
#[tauri::command]
pub async fn get_status_log(
    state: State<'_, StatusState>,
    query: Option<StatusLogQuery>,
) -> Result<StatusLogPage, String> {
    let log = state.log.clone();
    let request = query.unwrap_or_default();
    let job = tauri::async_runtime::spawn_blocking(move || Ok(log.page(&request)));
    match job.await {
        Ok(result) => result,
        Err(_) => Err("读取转灯日志的任务异常退出".to_string()),
    }
}

/// 按条件精确计数（一次全目录扫描，同查询 3 秒内走缓存）。
///
/// 存在的理由是把「本会话有多少条」从**内存视图的条数**里解出来：界面不许再用「N+」这种
/// 含糊写法，就必须给一个按盘数出来的精确值 + 它是截至哪一刻的数。
#[tauri::command]
pub async fn count_status_log(
    state: State<'_, StatusState>,
    query: Option<StatusLogQuery>,
) -> Result<StatusLogCount, String> {
    let log = state.log.clone();
    // 计数忽略 `limit`：它问的是总数，不是某一页。
    let request = StatusLogQuery {
        limit: 0,
        ..query.unwrap_or_default()
    };
    let job = tauri::async_runtime::spawn_blocking(move || Ok(log.count(&request)));
    match job.await {
        Ok(result) => result,
        Err(_) => Err("统计转灯日志的任务异常退出".to_string()),
    }
}

/// 清空日志：另起一个新文件、删掉旧的、内存清零，并回一份空页让面板立刻收敛。
#[tauri::command]
pub fn clear_status_log(state: State<'_, StatusState>) -> StatusLogPage {
    state.log.clear();
    state.log.page(&StatusLogQuery::default())
}

/// 按当前过滤条件导出一份诊断文件（写到哪都行，条数以**盘上**命中为准）。
///
/// `path` 来自前端的原生「另存为」对话框：给了就写到那里，没给（或用户取消、对话框不可用）
/// 就落到日志目录内的 `export/` 子目录。路径校验在 `LogState::export_to`，这条命令是**唯一**
/// 能按调用方给的地点写文件的地方，因此只授权给设置侧的窗口（主窗 + 独立转灯日志窗）。
///
/// ⚠️ 声明为 `async` 并把扫描丢进 `spawn_blocking`：导出要翻完整个日志目录（`max_total_bytes`
/// 可以被配到几百 MiB）再逐行写出，而非 async 命令跑在**主线程** —— 一次导出就能把窗口事件
/// 循环冻住几秒到几十秒。带引用入参的 async 命令必须返回 `Result`（tauri 宏要求）。
#[tauri::command]
pub async fn export_status_log(
    state: State<'_, StatusState>,
    path: Option<String>,
    query: Option<StatusLogQuery>,
) -> Result<ExportedLog, String> {
    // `State<'_, _>` 是借用，进不了 `'static` 闭包：先取出 owned 的共享句柄再 move 进去。
    let log = state.log.clone();
    let request = query.unwrap_or_default();
    let job = tauri::async_runtime::spawn_blocking(move || log.export_to(path, &request));
    // blocking 池里的任务不 panic（`export_to` 全 `Result` 返回），但 join 失败要兜成错误，绝不 expect。
    // 注意这里不再套一层 `Ok`：`spawn_blocking` 的 `Ok(_)` 里装的就是 `export_to` 的那个 `Result`。
    match job.await {
        Ok(result) => result,
        Err(_) => Err("导出任务异常退出".to_string()),
    }
}

/// 面板挂载 / 卸载时上报，决定 monitor 要不要发那一批（关着时零 IPC）。
///
/// ⚠️ 记账按**调用方窗口的 label**（`tauri::Window` 由 tauri 注入，前端不必传参）：
/// 主窗的「会话记录」页与独立的转灯日志窗都会调这条命令，共用一个开关时任何一方
/// 关掉都会让另一方停止跟随最新（见 `LogState::set_viewer`）。
#[tauri::command]
pub fn set_status_log_view(window: tauri::Window, state: State<'_, StatusState>, attached: bool) {
    state.log.set_viewer(window.label(), attached);
}
