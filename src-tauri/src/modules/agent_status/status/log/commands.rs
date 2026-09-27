//! 面板读日志 / 清空 / 导出 / 跟随的 tauri 命令。
//!
//! 权限拆分照会话记录那一套：**读命令给三个窗口**（悬浮窗也可能要看，见
//! `capabilities/default.json`），**清空与导出只给主窗**（`capabilities/main-module-settings.json`）。
//!
//! ⚠️ 这些命令一律**不碰 registry、不碰灯态**：只做「把已经落盘的账拿出来给人看」。

use tauri::State;

use super::runtime::{ExportedLog, StatusLogPage, StatusLogQuery};
use crate::modules::agent_status::status::StatusState;

/// 取一页转灯日志。缺省查询 = 最近一页全量。
#[tauri::command]
pub fn get_status_log(
    state: State<'_, StatusState>,
    query: Option<StatusLogQuery>,
) -> StatusLogPage {
    state.log.page(&query.unwrap_or_default())
}

/// 清空日志：另起一个新文件、删掉旧的、内存清零，并回一份空页让面板立刻收敛。
#[tauri::command]
pub fn clear_status_log(state: State<'_, StatusState>) -> StatusLogPage {
    state.log.clear();
    state.log.page(&StatusLogQuery::default())
}

/// 按当前过滤条件导出一份诊断文件。
///
/// `path` 来自前端的原生「另存为」对话框：给了就写到那里，没给（或用户取消、对话框不可用）
/// 就落到日志目录内的 `export/` 子目录。路径校验在 `LogState::export_to`，
/// 这条命令是**唯一**能按调用方给的地点写文件的地方，只授权给主窗。
#[tauri::command]
pub fn export_status_log(
    state: State<'_, StatusState>,
    path: Option<String>,
    query: Option<StatusLogQuery>,
) -> Result<ExportedLog, String> {
    state.log.export_to(path, &query.unwrap_or_default())
}

/// 面板挂载 / 卸载时上报，决定 monitor 要不要每轮发那一批（关着时零 IPC）。
#[tauri::command]
pub fn set_status_log_view(state: State<'_, StatusState>, attached: bool) {
    state.log.set_viewer(attached);
}
