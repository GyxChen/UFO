//! 插件相关的 IPC 命令。
//!
//! 授权口径（与 `capabilities/*.json` 一一对应）：
//! - **读**（`get_plugins` / `get_plugin_registry` / `report_plugin_load_ms`）给所有窗口 ——
//!   悬浮窗与提示窗要按台账挂载，必须能读；
//! - **写**（预览 / 安装 / 卸载 / 开关 / 重扫）只授主窗和它派出的面板窗。
//!
//! ⚠️ 全部读盘的命令都是 `async` + `spawn_blocking`：同步命令跑在主线程，
//! 一次冷扫描（几百个文件 × 每个包）就会把悬浮窗的拖拽卡住。

use std::collections::BTreeMap;
use std::path::Path;

use base64::Engine as _;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::capability;
use super::ble::{BleConnection, BleDevice, BleNotification, BleState};
use super::events::{self, EventBatch, EventState};
use super::fs::{FsState, FsUsage, PickedFile, PrivateFile, ZipEntry};
use super::log::{self, LogLine, LogPage, LogQuery, LogState};
use super::manifest;
use super::net::{self, FetchRequest, FetchResponse};
use super::paths;
use super::secrets::SecretState;
use super::serial::{SerialHandle, SerialPortRow, SerialState};
use super::store::{self, PluginRecord, StoreState};
use super::storage::{StorageState, StorageUsage};

/// 台账变更广播的事件名（前端 store 订阅它）。
pub const PLUGINS_CHANGED_EVENT: &str = "plugins-changed";

/// 一次读取 / 变更的返回：台账版本 + 全量记录。
///
/// 带上 `revision` 是为了让前端能丢弃迟到的那一份广播（与悬浮窗配置同一套做法）：
/// 安装与卸载几乎同时发生时，界面不该被旧记录盖回去。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerBroadcast {
    /// 台账版本（每次写盘 +1）。
    pub revision: u64,
    /// 全量记录（含校验失败的包）。
    pub records: Vec<PluginRecord>,
}

/// 取一份当前台账的广播体。
fn snapshot(store: &StoreState) -> LedgerBroadcast {
    LedgerBroadcast {
        revision: store.revision(),
        records: store.scan_records(),
    }
}

/// 把所有窗口都要用的装载输入一次取回：台账记录 + 能力注册表分两条命令，
/// 但都在同一批 IPC 里读（悬浮窗启动时先拿记录，再按需拿注册表）。
#[tauri::command]
pub async fn get_plugins(state: tauri::State<'_, StoreState>) -> Result<LedgerBroadcast, String> {
    let store = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || snapshot(&store))
        .await
        .map_err(|error| format!("插件扫描没跑完：{error}"))
}

/// 能力与挂载点的注册表（管理区徽标与装前披露都读它）。
#[tauri::command]
pub fn get_plugin_registry() -> Result<Vec<serde_json::Value>, String> {
    Ok(capability::facts_json())
}

/// 前端上报一次装载耗时（管理区那行的「装载 xx ms」读数）。
#[tauri::command]
pub fn report_plugin_load_ms(
    state: tauri::State<'_, StoreState>,
    id: String,
    load_ms: u64,
) -> Result<(), String> {
    if !paths::is_valid_plugin_id(&id) {
        return Err(format!("id「{id}」不合法"));
    }
    // 只当读数用，夹到 10 分钟足够，且防一个负数溢出成天文数字。
    state.report_load_ms(&id, load_ms.min(600_000));
    Ok(())
}

/// 校验一个目录能不能装（**不写盘**）：确认弹窗要先把逐条披露摊出来。
#[tauri::command]
pub async fn preview_plugin(
    state: tauri::State<'_, StoreState>,
    dir: String,
) -> Result<PluginRecord, String> {
    let store = state.inner().clone();
    let path = dir.trim().to_string();
    tauri::async_runtime::spawn_blocking(move || store.preview(&path))
        .await
        .map_err(|error| format!("校验没跑完：{error}"))?
}

/// 安装（本地目录 · 就地装载），并把新台账广播出去。
#[tauri::command]
pub async fn install_plugin(
    app: AppHandle,
    state: tauri::State<'_, StoreState>,
    dir: String,
) -> Result<LedgerBroadcast, String> {
    let store = state.inner().clone();
    let path = dir.trim().to_string();
    let done = tauri::async_runtime::spawn_blocking(move || {
        store.install_local(&path).map(|record| (record, snapshot(&store)))
    })
    .await
    .map_err(|error| format!("安装没跑完：{error}"))??;
    let broadcast = done.1;
    publish(&app, &broadcast);
    Ok(broadcast)
}

/// 卸载（界面上要走两步确认，这里只管落账）。
#[tauri::command]
pub async fn uninstall_plugin(
    app: AppHandle,
    state: tauri::State<'_, StoreState>,
    logs: tauri::State<'_, LogState>,
    events: tauri::State<'_, EventState>,
    storage: tauri::State<'_, StorageState>,
    files: tauri::State<'_, FsState>,
    secret_keys: tauri::State<'_, SecretState>,
    devices: tauri::State<'_, BleState>,
    ports: tauri::State<'_, SerialState>,
    id: String,
) -> Result<LedgerBroadcast, String> {
    let store = state.inner().clone();
    let log = logs.inner().clone();
    let bus = events.inner().clone();
    let kv = storage.inner().clone();
    let fs = files.inner().clone();
    let secrets = secret_keys.inner().clone();
    let ble = devices.inner().clone();
    let serial = ports.inner().clone();
    let broadcast = tauri::async_runtime::spawn_blocking(move || {
        store.uninstall(&id)?;
        // 环形缓冲、跟随、事件环与**存储分区**一起收：卸载就得什么都不留。
        log.forget(&id);
        bus.close(&id);
        kv.forget(&id);
        // 文件能力：句柄作废 + 私有目录整片删（承诺过「卸载即删」）。
        fs.revoke(&id);
        let _ = fs.forget(&id);
        // 密钥按名册删干净，否则会留下没人认领的凭据条目。
        let _ = secrets.forget(&id);
        // 外设：无线电与端口是活的系统资源，卸载时必须收回（断开是异步发的，不阻塞这次卸载）。
        ble.forget(&id);
        serial.forget(&id);
        Ok::<_, String>(snapshot(&store))
    })
    .await
    .map_err(|error| format!("卸载没跑完：{error}"))??;
    publish(&app, &broadcast);
    Ok(broadcast)
}

/// 开 / 关逐包热重载（只对本地源有效）。
#[tauri::command]
pub async fn set_plugin_dev_reload(
    app: AppHandle,
    state: tauri::State<'_, StoreState>,
    id: String,
    dev_reload: bool,
) -> Result<LedgerBroadcast, String> {
    let store = state.inner().clone();
    let broadcast = tauri::async_runtime::spawn_blocking(move || {
        store.set_dev_reload(&id, dev_reload).map(|_| snapshot(&store))
    })
    .await
    .map_err(|error| format!("改热重载开关没跑完：{error}"))??;
    publish(&app, &broadcast);
    Ok(broadcast)
}

/// 手动「重新加载」：重扫盘上的包并广播（作者改完清单不想重启 App 就点这个）。
#[tauri::command]
pub async fn reload_plugins(
    app: AppHandle,
    state: tauri::State<'_, StoreState>,
) -> Result<LedgerBroadcast, String> {
    let store = state.inner().clone();
    let broadcast = tauri::async_runtime::spawn_blocking(move || snapshot(&store))
        .await
        .map_err(|error| format!("重扫没跑完：{error}"))?;
    publish(&app, &broadcast);
    Ok(broadcast)
}

/// 把变更广播给所有窗口。
///
/// 事件发失败只记 stderr：台账已经落盘，任何窗口下次读都会拿到，不该因为 IPC 抖动报错给用户。
fn publish(app: &AppHandle, broadcast: &LedgerBroadcast) {
    if let Err(error) = app.emit(PLUGINS_CHANGED_EVENT, broadcast) {
        eprintln!("failed to broadcast plugin ledger: {error}");
    }
}

/// 插件写一条自己的日志（`ufo.log` / `host.log` 的落点，需求点名的「特例能力」）。
///
/// 留不留这一行由**台账里的调试开关**决定，不由调用方自报：关着就整条丢弃（零写盘、零广播）。
/// 授权给所有窗口 —— 挂载插件的不止主窗，日志必须能从它实际跑起来的那个窗口写进来。
#[tauri::command]
pub async fn plugin_log_write(
    app: AppHandle,
    state: tauri::State<'_, StoreState>,
    logs: tauri::State<'_, LogState>,
    id: String,
    level: String,
    msg: String,
    fields: Option<BTreeMap<String, serde_json::Value>>,
) -> Result<(), String> {
    let store = state.inner().clone();
    let log = logs.inner().clone();
    let line = LogLine { ts: store::now_ms(), level, msg, fields, trace_id: None };
    tauri::async_runtime::spawn_blocking(move || {
        let persist = store.dev_reload_of(&id);
        if let Some(stored) = log.write(&id, line, persist) {
            log::publish(&app, &log, &id, &[stored]);
        }
    })
    .await
    .map_err(|error| format!("日志没写完：{error}"))
}

/// 调试卡的「最近日志」：读内存环形缓冲，不碰盘（关着调试时它就是空的，这符合语义）。
#[tauri::command]
pub fn get_recent_plugin_logs(
    logs: tauri::State<'_, LogState>,
    id: String,
    limit: usize,
) -> Result<Vec<LogLine>, String> {
    if !paths::is_valid_plugin_id(&id) {
        return Err(format!("id「{id}」不合法"));
    }
    Ok(logs.recent(&id, limit.min(paths::LOG_RING_LINES)))
}

/// 独立日志窗的翻页：按盘读（新 → 旧），能翻到「关调试之前」写下的那些行。
#[tauri::command]
pub async fn get_plugin_logs(
    logs: tauri::State<'_, LogState>,
    id: String,
    query: LogQuery,
) -> Result<LogPage, String> {
    if !paths::is_valid_plugin_id(&id) {
        return Err(format!("id「{id}」不合法"));
    }
    let log = logs.inner().clone();
    tauri::async_runtime::spawn_blocking(move || log.page(&id, &query))
        .await
        .map_err(|error| format!("日志翻不动：{error}"))
}

/// 清空某个包的日志（环 + 盘）。界面上必须先二次确认：这是不可逆的。
#[tauri::command]
pub async fn clear_plugin_logs(
    app: AppHandle,
    logs: tauri::State<'_, LogState>,
    id: String,
) -> Result<usize, String> {
    let log = logs.inner().clone();
    let target = id.clone();
    let removed = tauri::async_runtime::spawn_blocking(move || log.clear(&target))
        .await
        .map_err(|error| format!("日志清不掉：{error}"))??;
    // 清完广播一次空批：窗口据此回到「共 0 条」，而不是留着旧计数继续显示。
    if let Err(error) = app.emit(log::PLUGIN_LOG_EVENT, serde_json::json!({ "pluginId": id, "lines": [] })) {
        eprintln!("failed to broadcast cleared plugin log: {error}");
    }
    Ok(removed)
}

/// 挂上 / 摘掉这个窗口的实时跟随。
///
/// ⚠️ 窗口 label 由 Tauri 注入（`tauri::Window`），不接受前端自报：
/// 一个页面冒充别人的 label 就能把别人的跟随顶掉。按「包 + 窗口」记账，
/// 所以主窗那一页收起不会把独立日志窗的跟随关掉。
#[tauri::command]
pub fn set_plugin_log_view(
    window: tauri::Window,
    logs: tauri::State<'_, LogState>,
    id: String,
    attached: bool,
) -> Result<(), String> {
    if !paths::is_valid_plugin_id(&id) {
        return Err(format!("id「{id}」不合法"));
    }
    logs.set_view(&id, window.label(), attached);
    Ok(())
}

/// 订阅灯态事件 —— **这道命令就是能力闸本身**。
///
/// 清单里没有 `events:agent` 就不开环形：之后 `poll` 拿不到任何东西（环的存在是唯一的凭据）。
/// 闸放在 Rust 是因为 capability 只认「哪个窗口在调」，而所有组件包的 UI 与宿主同窗；
/// 广播同理不能用 —— 一次 `app.emit` 等于把提问原文发给在场每一个包。
#[tauri::command]
pub async fn open_plugin_events(
    window: tauri::Window,
    state: tauri::State<'_, StoreState>,
    events: tauri::State<'_, EventState>,
    id: String,
) -> Result<Vec<String>, String> {
    let store = state.inner().clone();
    let bus = events.inner().clone();
    let target = id.clone();
    let label = window.label().to_string();
    tauri::async_runtime::spawn_blocking(move || {
        // 闸：清单没有 events:agent 就直接拒，连环形都不开。
        require_permission(&store, &target, "events:agent")?;
        bus.attach(&target, &label);
        Ok(events::TOPICS.iter().map(|topic| topic.to_string()).collect::<Vec<_>>())
    })
    .await
    .map_err(|error| format!("订阅没跑完：{error}"))?
}

/// 拉取本包 `since` 之后的事件（组件侧 `host.event()` 的传输层，一秒一次）。
#[tauri::command]
pub fn poll_plugin_events(
    events: tauri::State<'_, EventState>,
    id: String,
    since: u64,
) -> Result<EventBatch, String> {
    events
        .poll(&id, since)
        .ok_or_else(|| format!("组件「{id}」没订阅成功（清单要声明 events:agent）"))
}

/// 这个窗口不再盯这个包的事件（最后一个窗口撤了才丢环形）。
#[tauri::command]
pub fn close_plugin_events(
    window: tauri::Window,
    events: tauri::State<'_, EventState>,
    id: String,
) -> Result<(), String> {
    events.detach(&id, window.label());
    Ok(())
}

/// 按包查清单：有没有声明某项能力，有就返回它的清单。
///
/// 这是**所有能力命令的第一行**（`host.storage` / `host.http` / `host.event` 全走它）。
/// 放在 Rust 而不是前端：capability 只知道「哪个窗口在调」，而所有组件包与宿主同窗；
/// 前端的徽标是给人看的承诺，这一句才是真的闸。
///
/// `pub(crate)` 是为了协议层（`__pick` / `__fs`）也能过同一道闸 —— 那两条入口没有调用方身份，
/// 只能靠 URL 里的包 id 或句柄的归属回到这张表。
pub(crate) fn require_permission(
    store: &StoreState,
    id: &str,
    permission: &str,
) -> Result<manifest::Manifest, String> {
    if !paths::is_valid_plugin_id(id) {
        return Err(format!("id「{id}」不合法"));
    }
    let record = store
        .record_of(id)
        .ok_or_else(|| format!("没有 id 为「{id}」的已装组件"))?;
    let manifest = record
        .manifest
        .ok_or_else(|| format!("组件「{id}」的清单校验失败，能力「{permission}」用不了"))?;
    if !manifest.permissions.iter().any(|name| name == permission) {
        return Err(format!(
            "组件「{id}」没在清单 permissions 里声明 {permission}，用不了这个能力"
        ));
    }
    Ok(manifest)
}

/// 公共壳：先按 id 过能力闸，再把活丢到 blocking 线程。
///
/// 读盘与碰凭据都不能占主线程（AGENTS.md 的「非 async 命令跑在主线程」），
/// 而每一条能力命令的开头又都是同一句「查清单」—— 抽到这里，命令体就只剩它自己的那一行。
async fn gated<T, F>(
    state: &tauri::State<'_, StoreState>,
    permission: &'static str,
    id: String,
    work: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(String) -> Result<T, String> + Send + 'static,
{
    let store = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        require_permission(&store, &id, permission)?;
        work(id)
    })
    .await
    .map_err(|error| format!("能力调用没跑完：{error}"))?
}

// ---- 文件能力（`host.fs`）----

/// 弹一次原生文件对话框，把「用户亲手选中的这个文件」登记成一个会话内句柄。
///
/// ⚠️ 授权=这一次点击：句柄不落台账、不跨启动，卸载 / 重载即作废，
/// 所以界面上那句「每次都要重新选」不是客套，是这里的实现形状。
#[tauri::command]
pub async fn plugin_fs_pick(
    app: AppHandle,
    state: tauri::State<'_, StoreState>,
    files: tauri::State<'_, FsState>,
    id: String,
    title: Option<String>,
) -> Result<PickedFile, String> {
    let fs = files.inner().clone();
    gated(&state, "fs", id, move |target| {
        use tauri_plugin_dialog::DialogExt;
        let dialog = app.dialog().file();
        // 标题由作者给（「选一本书」这类），空就交给系统默认；截一下防一句长话挤爆对话框。
        let dialog = match title.as_deref() {
            Some(text) if !text.trim().is_empty() => dialog.set_title(paths::truncate_chars(text.trim(), 60)),
            _ => dialog,
        };
        // 刻意不加扩展名过滤：这一包的输入可能是 epub / zip / 无扩展名的数据文件，
        // 过滤条件是作者的事，不是宿主的。
        let picked = dialog
            .blocking_pick_file()
            .ok_or_else(|| "没有选择文件（对话框被取消了）".to_string())?;
        let path = match picked {
            tauri_plugin_dialog::FilePath::Path(path) => path,
            _ => return Err("只接受本地文件；网络地址请先自己取到本地".to_string()),
        };
        fs.grant(&target, path)
    })
    .await
}

/// 句柄此刻还指向一个读得到的文件吗（作者据此提示「请重新选」）。
#[tauri::command]
pub async fn plugin_fs_stat(
    state: tauri::State<'_, StoreState>,
    files: tauri::State<'_, FsState>,
    id: String,
    handle: String,
) -> Result<PickedFile, String> {
    let fs = files.inner().clone();
    gated(&state, "fs", id, move |target| fs.stat_pick(&target, &handle)).await
}

/// 列压缩包的目录表（epub / zip / docx…）。
#[tauri::command]
pub async fn plugin_fs_entries(
    state: tauri::State<'_, StoreState>,
    files: tauri::State<'_, FsState>,
    id: String,
    handle: String,
) -> Result<Vec<ZipEntry>, String> {
    let fs = files.inner().clone();
    gated(&state, "fs", id, move |target| fs.entries(&target, &handle)).await
}

/// 读选中文件的一段（base64）。大块请改用 `ufo-plugin://__pick/<句柄>` 让浏览器自己分段取。
#[tauri::command]
pub async fn plugin_fs_read(
    state: tauri::State<'_, StoreState>,
    files: tauri::State<'_, FsState>,
    id: String,
    handle: String,
    start: u64,
    len: u64,
) -> Result<String, String> {
    let fs = files.inner().clone();
    gated(&state, "fs", id, move |target| {
        fs.read_pick(&target, &handle, start, len)
            .map(|bytes| base64::prelude::BASE64_STANDARD.encode(bytes))
    })
    .await
}

/// 读压缩包中某一条目的一段（base64）。
#[tauri::command]
pub async fn plugin_fs_read_entry(
    state: tauri::State<'_, StoreState>,
    files: tauri::State<'_, FsState>,
    id: String,
    handle: String,
    entry: String,
    start: u64,
    len: u64,
) -> Result<String, String> {
    let fs = files.inner().clone();
    gated(&state, "fs", id, move |target| {
        fs.read_member(&target, &handle, &entry, start, len)
            .map(|bytes| base64::prelude::BASE64_STANDARD.encode(bytes))
    })
    .await
}

/// 写私有目录里的一个文件（`encoding` 只认 utf8 / base64）。
#[tauri::command]
pub async fn plugin_fs_write(
    state: tauri::State<'_, StoreState>,
    files: tauri::State<'_, FsState>,
    id: String,
    path: String,
    data: String,
    encoding: String,
) -> Result<PrivateFile, String> {
    let fs = files.inner().clone();
    gated(&state, "fs", id, move |target| {
        fs.write(&target, &path, &data, &encoding)
    })
    .await
}

/// 读私有目录里某个文件的一段（base64）。
#[tauri::command]
pub async fn plugin_fs_read_private(
    state: tauri::State<'_, StoreState>,
    files: tauri::State<'_, FsState>,
    id: String,
    path: String,
    start: u64,
    len: u64,
) -> Result<String, String> {
    let fs = files.inner().clone();
    gated(&state, "fs", id, move |target| {
        fs.read_private(&target, &path, start, len)
            .map(|bytes| base64::prelude::BASE64_STANDARD.encode(bytes))
    })
    .await
}

/// 列私有目录（递归，只给相对路径与大小）。
#[tauri::command]
pub async fn plugin_fs_list(
    state: tauri::State<'_, StoreState>,
    files: tauri::State<'_, FsState>,
    id: String,
) -> Result<Vec<PrivateFile>, String> {
    let fs = files.inner().clone();
    gated(&state, "fs", id, move |target| fs.list(&target)).await
}

/// 删私有目录里的一个文件。
#[tauri::command]
pub async fn plugin_fs_remove(
    state: tauri::State<'_, StoreState>,
    files: tauri::State<'_, FsState>,
    id: String,
    path: String,
) -> Result<bool, String> {
    let fs = files.inner().clone();
    gated(&state, "fs", id, move |target| fs.remove(&target, &path)).await
}

/// 某个包私有目录的占用读数（管理区详情常驻显示）。
///
/// 这条**不过能力闸**：它是给人看的账，调用方是宿主界面而不是组件本身
/// （与 `get_plugin_storage_usage` 同一口径，只校验 id）。它是读盘的，所以 async。
#[tauri::command]
pub async fn get_plugin_fs_usage(
    files: tauri::State<'_, FsState>,
    id: String,
) -> Result<FsUsage, String> {
    let fs = files.inner().clone();
    tauri::async_runtime::spawn_blocking(move || fs.usage(&id))
        .await
        .map_err(|error| format!("占用没读完：{error}"))?
}

// ---- 内嵌网页（`host.frame`）----

/// 问一句「这个 URL 允许被组件嵌进 iframe 吗」，通过就回规范化后的 host。
///
/// 这一条刻意**不新建窗口**：需求里的「webview 视图」在界面上就是一个 `<iframe>`，
/// 宿主只需要回答两个问题 —— 这个包声明过 `frame` 吗、这个地址是不是本机 / 元数据那一类。
/// 两句都在命令层判（与 `host.http` 同一支校验器），URL 原样交回给作者自己挂。
#[tauri::command]
pub async fn plugin_frame_allow(
    state: tauri::State<'_, StoreState>,
    id: String,
    url: String,
) -> Result<String, String> {
    gated(&state, "frame", id, move |_target| net::validated_frame_host(&url)).await
}

// ---- 开发期热重载（作者侧 `yarn dev` 的那一头）----

/// 所有开了热重载的包的**内容指纹**（包 id + 目录里最新的 mtime）。
///
/// 为什么是轮询一个指纹、而不是装一个文件监听器：
/// - 只有 dev 包参与，量很小（几个包 × 几百个文件的 mtime），一次几毫秒；
/// - 不为此引入 `notify` 这一条依赖；
/// - 作者的包目录常在 junction / 网盘 / WSL 挂载里，那些位置的原生事件本就不可靠，
///   而「问一次指纹」在哪儿都成立。
#[tauri::command]
pub async fn get_plugin_dev_digest(state: tauri::State<'_, StoreState>) -> Result<u64, String> {
    let store = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut digest: u64 = 0xcbf2_9ce4_8422_2325;
        for record in store.scan_records() {
            if !record.dev_reload {
                continue;
            }
            digest = digest_mix(digest, record.id.as_bytes());
            digest = digest_mix(digest, &newest_mtime(Path::new(&record.dir)).to_le_bytes());
        }
        Ok::<_, String>(digest)
    })
    .await
    .map_err(|error| format!("热重载指纹没算完：{error}"))?
}

/// FNV-1a 的一轮混合（足够把「哪个包 + 什么时候改过」压成一个会变的数）。
fn digest_mix(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// 目录里最新文件的修改时间（秒）；读不到回 0（不 panic，也不阻塞重载判定）。
fn newest_mtime(dir: &Path) -> u64 {
    let mut newest = 0u64;
    let mut stack: Vec<(std::path::PathBuf, usize)> = vec![(dir.to_path_buf(), 0)];
    while let Some((current, depth)) = stack.pop() {
        if depth > 6 {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                stack.push((path, depth + 1));
                continue;
            }
            let secs = path
                .metadata()
                .ok()
                .and_then(|meta| meta.modified().ok())
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|since| since.as_secs())
                .unwrap_or(0);
            if secs > newest {
                newest = secs;
            }
        }
    }
    newest
}

// ---- 蓝牙能力（`host.ble`）----
//
// 闸与其它能力同一句（`require_permission`），差别只在**执行位置**：
// btleplug 的调用本身是 async，所以这里先过闸（读的是内存里的台账，不占盘）再直接 `.await`；
// 用 `gated` 反而会把 async 活塞进 blocking 线程里再 `block_on`，绕一圈还容易自锁。

/// 扫一会儿蓝牙，返回扫到的外设（带连接用的句柄）。
#[tauri::command]
pub async fn plugin_ble_scan(
    state: tauri::State<'_, StoreState>,
    devices: tauri::State<'_, BleState>,
    id: String,
    ms: Option<u64>,
) -> Result<Vec<BleDevice>, String> {
    require_permission(state.inner(), &id, "ble")?;
    let ble = devices.inner().clone();
    ble.scan(&id, ms).await
}

/// 连上一个扫到的外设，顺手把 GATT 树读回来。
#[tauri::command]
pub async fn plugin_ble_connect(
    state: tauri::State<'_, StoreState>,
    devices: tauri::State<'_, BleState>,
    id: String,
    handle: String,
) -> Result<BleConnection, String> {
    require_permission(state.inner(), &id, "ble")?;
    let ble = devices.inner().clone();
    ble.connect(&id, &handle).await
}

/// 写一个特征（值走 base64；`with_response` 决定是否要回执）。
#[tauri::command]
pub async fn plugin_ble_write(
    state: tauri::State<'_, StoreState>,
    devices: tauri::State<'_, BleState>,
    id: String,
    handle: String,
    service: String,
    characteristic: String,
    data: String,
    with_response: bool,
) -> Result<usize, String> {
    require_permission(state.inner(), &id, "ble")?;
    let bytes = decode_base64(&data)?;
    let ble = devices.inner().clone();
    ble.write(&id, &handle, &service, &characteristic, &bytes, with_response)
        .await
}

/// 读一个特征（回 base64）。
#[tauri::command]
pub async fn plugin_ble_read(
    state: tauri::State<'_, StoreState>,
    devices: tauri::State<'_, BleState>,
    id: String,
    handle: String,
    service: String,
    characteristic: String,
) -> Result<String, String> {
    require_permission(state.inner(), &id, "ble")?;
    let ble = devices.inner().clone();
    let bytes = ble.read(&id, &handle, &service, &characteristic).await?;
    Ok(base64::prelude::BASE64_STANDARD.encode(bytes))
}

/// 开一条通知订阅。
#[tauri::command]
pub async fn plugin_ble_subscribe(
    state: tauri::State<'_, StoreState>,
    devices: tauri::State<'_, BleState>,
    id: String,
    handle: String,
    characteristic: String,
) -> Result<(), String> {
    require_permission(state.inner(), &id, "ble")?;
    let ble = devices.inner().clone();
    ble.subscribe(&id, &handle, &characteristic).await
}

/// 关一条通知订阅。
#[tauri::command]
pub async fn plugin_ble_unsubscribe(
    state: tauri::State<'_, StoreState>,
    devices: tauri::State<'_, BleState>,
    id: String,
    handle: String,
    characteristic: String,
) -> Result<(), String> {
    require_permission(state.inner(), &id, "ble")?;
    let ble = devices.inner().clone();
    ble.unsubscribe(&id, &handle, &characteristic).await
}

/// 取走通知（最旧的先给）。同步的环形搬运，走 `gated` 就够。
#[tauri::command]
pub async fn plugin_ble_notifications(
    state: tauri::State<'_, StoreState>,
    devices: tauri::State<'_, BleState>,
    id: String,
    handle: String,
    limit: usize,
) -> Result<Vec<BleNotification>, String> {
    let ble = devices.inner().clone();
    gated(&state, "ble", id, move |target| {
        ble.notifications(&target, &handle, limit)
    })
    .await
}

/// 断开一条连接。
#[tauri::command]
pub async fn plugin_ble_disconnect(
    state: tauri::State<'_, StoreState>,
    devices: tauri::State<'_, BleState>,
    id: String,
    handle: String,
) -> Result<bool, String> {
    require_permission(state.inner(), &id, "ble")?;
    let ble = devices.inner().clone();
    ble.disconnect(&id, &handle).await
}

/// 这个组件此刻连着哪些外设（管理区详情用）。
#[tauri::command]
pub async fn get_plugin_ble_held(
    state: tauri::State<'_, StoreState>,
    devices: tauri::State<'_, BleState>,
    id: String,
) -> Result<Vec<String>, String> {
    let ble = devices.inner().clone();
    gated(&state, "ble", id, move |target| Ok(ble.held(&target))).await
}

// ---- 串口能力（`host.serial`）----

/// 列出系统上的串口（不含任何句柄信息）。
#[tauri::command]
pub async fn plugin_serial_ports(
    state: tauri::State<'_, StoreState>,
    ports: tauri::State<'_, SerialState>,
    id: String,
) -> Result<Vec<SerialPortRow>, String> {
    let serial = ports.inner().clone();
    gated(&state, "serial", id, move |_| serial.list()).await
}

/// 打开一个端口（句柄在进程内有效；端口被占用期间别的程序拿不到它）。
#[tauri::command]
pub async fn plugin_serial_open(
    state: tauri::State<'_, StoreState>,
    ports: tauri::State<'_, SerialState>,
    id: String,
    path: String,
    baud: u32,
    timeout_ms: Option<u64>,
) -> Result<SerialHandle, String> {
    let serial = ports.inner().clone();
    gated(&state, "serial", id, move |target| {
        serial.open(&target, &path, baud, timeout_ms)
    })
    .await
}

/// 写一段字节（base64）。
#[tauri::command]
pub async fn plugin_serial_write(
    state: tauri::State<'_, StoreState>,
    ports: tauri::State<'_, SerialState>,
    id: String,
    handle: String,
    data: String,
) -> Result<usize, String> {
    let serial = ports.inner().clone();
    let bytes = decode_base64(&data)?;
    gated(&state, "serial", id, move |target| {
        serial.write(&target, &handle, &bytes)
    })
    .await
}

/// 读最多 `max` 字节（base64）；这一轮没数据回空串。
#[tauri::command]
pub async fn plugin_serial_read(
    state: tauri::State<'_, StoreState>,
    ports: tauri::State<'_, SerialState>,
    id: String,
    handle: String,
    max: usize,
) -> Result<String, String> {
    let serial = ports.inner().clone();
    gated(&state, "serial", id, move |target| {
        serial
            .read(&target, &handle, max)
            .map(|bytes| base64::prelude::BASE64_STANDARD.encode(bytes))
    })
    .await
}

/// 改一次超时（不必重开端口）。
#[tauri::command]
pub async fn plugin_serial_timeout(
    state: tauri::State<'_, StoreState>,
    ports: tauri::State<'_, SerialState>,
    id: String,
    handle: String,
    timeout_ms: u64,
) -> Result<u64, String> {
    let serial = ports.inner().clone();
    gated(&state, "serial", id, move |target| {
        serial.set_timeout(&target, &handle, timeout_ms)
    })
    .await
}

/// 关一个端口。
#[tauri::command]
pub async fn plugin_serial_close(
    state: tauri::State<'_, StoreState>,
    ports: tauri::State<'_, SerialState>,
    id: String,
    handle: String,
) -> Result<bool, String> {
    let serial = ports.inner().clone();
    gated(&state, "serial", id, move |target| {
        serial.close(&target, &handle)
    })
    .await
}

/// 这个组件此刻握着哪些端口（管理区详情用）。
#[tauri::command]
pub async fn get_plugin_serial_held(
    state: tauri::State<'_, StoreState>,
    ports: tauri::State<'_, SerialState>,
    id: String,
) -> Result<Vec<String>, String> {
    let serial = ports.inner().clone();
    gated(&state, "serial", id, move |target| Ok(serial.held(&target))).await
}

/// base64 → 字节（组件往外设发的都是这一串）。
fn decode_base64(text: &str) -> Result<Vec<u8>, String> {
    base64::prelude::BASE64_STANDARD
        .decode(text)
        .map_err(|_| "数据不是合法的 base64".to_string())
}

// ---- 密钥能力（`host.secrets`）----

/// 按名字取密钥值；没配过回 null（不是错误，组件据此提示用户去填）。
#[tauri::command]
pub async fn plugin_secret_get(
    state: tauri::State<'_, StoreState>,
    secrets: tauri::State<'_, SecretState>,
    id: String,
    name: String,
) -> Result<Option<String>, String> {
    let vault = secrets.inner().clone();
    gated(&state, "secrets", id, move |target| {
        vault.get(&target, &name)
    })
    .await
}

/// 写一个密钥（值只进 OS 凭据管理器；这里同时记下名字，卸载时按名册删干净）。
#[tauri::command]
pub async fn plugin_secret_set(
    state: tauri::State<'_, StoreState>,
    secrets: tauri::State<'_, SecretState>,
    id: String,
    name: String,
    value: String,
) -> Result<(), String> {
    let vault = secrets.inner().clone();
    gated(&state, "secrets", id, move |target| {
        vault.put(&target, &name, &value)
    })
    .await
}

/// 删一个密钥。
#[tauri::command]
pub async fn plugin_secret_remove(
    state: tauri::State<'_, StoreState>,
    secrets: tauri::State<'_, SecretState>,
    id: String,
    name: String,
) -> Result<(), String> {
    let vault = secrets.inner().clone();
    gated(&state, "secrets", id, move |target| {
        vault.remove(&target, &name)
    })
    .await
}

/// 这个包存过哪些密钥名（只有名字，绝不回值）。
#[tauri::command]
pub async fn get_plugin_secret_names(
    state: tauri::State<'_, StoreState>,
    secrets: tauri::State<'_, SecretState>,
    id: String,
) -> Result<Vec<String>, String> {
    let vault = secrets.inner().clone();
    gated(&state, "secrets", id, move |target| vault.names(&target)).await
}

/// 读一个键（`host.storage.get`）。没这个键返回 null。
#[tauri::command]
pub fn plugin_storage_get(
    state: tauri::State<'_, StoreState>,
    storage: tauri::State<'_, StorageState>,
    id: String,
    key: String,
) -> Result<Option<serde_json::Value>, String> {
    require_permission(state.inner(), &id, "storage")?;
    storage.get(&id, &key)
}

/// 写一个键（`host.storage.set`）。超配额时返回中文原因，值不落盘。
#[tauri::command]
pub fn plugin_storage_set(
    state: tauri::State<'_, StoreState>,
    storage: tauri::State<'_, StorageState>,
    id: String,
    key: String,
    value: serde_json::Value,
) -> Result<(), String> {
    require_permission(state.inner(), &id, "storage")?;
    storage.set(&id, &key, value)
}

/// 删一个键（`host.storage.remove`）。
#[tauri::command]
pub fn plugin_storage_remove(
    state: tauri::State<'_, StoreState>,
    storage: tauri::State<'_, StorageState>,
    id: String,
    key: String,
) -> Result<bool, String> {
    require_permission(state.inner(), &id, "storage")?;
    storage.remove(&id, &key)
}

/// 这个包存了哪些键（`host.storage.keys`）。
#[tauri::command]
pub fn plugin_storage_keys(
    state: tauri::State<'_, StoreState>,
    storage: tauri::State<'_, StorageState>,
    id: String,
) -> Result<Vec<String>, String> {
    require_permission(state.inner(), &id, "storage")?;
    storage.keys(&id)
}

/// 这个包的存储占用读数（管理区详情常驻显示）。
#[tauri::command]
pub fn get_plugin_storage_usage(
    storage: tauri::State<'_, StorageState>,
    id: String,
) -> Result<StorageUsage, String> {
    if !paths::is_valid_plugin_id(&id) {
        return Err(format!("id「{id}」不合法"));
    }
    storage.usage(&id)
}

/// 代发一次网络请求（`host.http`）。
///
/// 两道闸：清单声明 `http` → 命令层查；目标域名必须在**这份包**清单的 `hosts` 里；
/// 最后才是 `validate_http_url` 拦私网与元数据地址（顺序是「先查权限、再查表、最后开 socket」）。
#[tauri::command]
pub async fn plugin_http_fetch(
    state: tauri::State<'_, StoreState>,
    id: String,
    request: FetchRequest,
) -> Result<FetchResponse, String> {
    let store = state.inner().clone();
    let target = id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let manifest = require_permission(&store, &target, "http")?;
        net::fetch(&request, &manifest.hosts)
    })
    .await
    .map_err(|error| format!("请求没跑完：{error}"))?
}

/// 组合根用：确保 `plugins/` 存在（第一次写台账才不会 ENOENT），并读出台账。
pub fn load_store(app_data: &std::path::Path) -> StoreState {
    let root = paths::plugins_root(app_data);
    if let Err(error) = std::fs::create_dir_all(&root) {
        eprintln!("failed to create plugins root: {error}");
    }
    store::StoreState::load(app_data.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 广播体里的字段名必须与前端 `LedgerBroadcastJson` 对上（camelCase 一处都不要漏）。
    #[test]
    fn broadcast_json_uses_camel_case_keys() {
        let broadcast = LedgerBroadcast { revision: 3, records: Vec::new() };
        let value = serde_json::to_value(&broadcast).expect("广播体序列化不出来");
        assert_eq!(value.get("revision").and_then(|item| item.as_u64()), Some(3));
        assert!(value.get("records").map(|item| item.is_array()) == Some(true));
        // 蛇形键不该出现（出现了就是前端读不到的字段）。
        assert!(value.get("schema_version").is_none());
    }
}
