use std::sync::Arc;

use tauri::{
    menu::{Menu, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconEvent},
    AppHandle, Manager, Runtime, WindowEvent,
};

// devtools 开关只服务于调试构建（`open_devtools` 本身也只在 debug_assertions 下存在），
// release 里整个模块不参与编译，省掉一份没人用的解析代码。
#[cfg(debug_assertions)]
mod devtools;
/// 悬浮窗外壳（Rust 侧）：组件的配置存取与转发 —— 不认识任何组件。
mod floating;
/// App 标识改名后的一次性搬迁：文件由外壳整体补缺，凭据由组合根点名各组件搬自己的那几条。
mod migrate;
/// 悬浮窗组件在 Rust 侧的家：一个组件一个目录（与前端 `src/modules/` 一一对应）。
mod modules;
mod mouse;
/// 独立面板窗（脚本编辑 / 各类日志）的宿主侧：按需建窗、载荷交接、销毁回报。
mod panel_window;
/// 插件（组件包）：清单契约、能力注册表、装载协议与安装记录。
mod plugin;

// 组合根：只有这里点名具体组件（登记它的配置钩子、运行时状态与命令），
// 外壳与其它组件都不认识它。
use floating::module_config::{
    get_floating_module_config, set_floating_module_config, ModuleState,
};
/// 状态 hook 二进制（`ufo --status-hook`）的入口，由 `main.rs` 调用，故从 crate 根导出。
pub use modules::agent_status::status::try_run_status_hook;
use modules::agent_status::{
    endpoint_profiles::{
        add_endpoint_profile, delete_endpoint_profile, get_endpoint_profiles, EndpointProfileState,
    },
    flow::{
        delete_flow_secret, get_flow_results, get_flow_snapshot, run_flow_test, save_endpoint_flow,
        set_flow_secret, spawn_worker, FlowState, KeyringSecrets, UreqFetch,
    },
    proxy::ProxyState,
    push::{
        get_push_status, spawn_worker as spawn_push_worker, test_push_provider, PushState,
    },
    session_store::{
        clear_session_records, delete_session_record, get_session_records, SessionStoreState,
    },
    settings::{adopt_legacy_fields, config_hook, AgentStatusSettings},
    sound::read_light_sound,
    status::{
        clear_status_log, count_status_log, dismiss_status_session, export_status_log,
        get_proxy_resolvers, get_status_log, get_status_snapshot, now_ms, set_status_log_view,
        set_tooltip_interactive, spawn_monitor, LogConfig, LogState, StatusState,
    },
};
use mouse::is_left_mouse_pressed;
use plugin::commands::{
    clear_plugin_logs, close_plugin_events, get_plugin_ble_held, get_plugin_dev_digest,
    get_plugin_fs_usage, get_plugin_logs, get_plugin_registry, get_plugin_secret_names,
    get_plugin_serial_held, get_plugin_storage_usage, get_plugins, get_recent_plugin_logs,
    install_plugin, open_plugin_events, plugin_ble_connect, plugin_ble_disconnect,
    plugin_ble_notifications, plugin_ble_read, plugin_ble_scan, plugin_ble_subscribe,
    plugin_ble_unsubscribe, plugin_ble_write, plugin_frame_allow, plugin_fs_entries,
    plugin_fs_list, plugin_fs_pick, plugin_fs_read, plugin_fs_read_entry, plugin_fs_read_private,
    plugin_fs_remove, plugin_fs_stat, plugin_fs_write, plugin_http_fetch, plugin_log_write,
    plugin_secret_get, plugin_secret_remove, plugin_secret_set, plugin_serial_close,
    plugin_serial_open, plugin_serial_ports, plugin_serial_read, plugin_serial_timeout,
    plugin_serial_write, plugin_storage_get, plugin_storage_keys, plugin_storage_remove,
    plugin_storage_set, poll_plugin_events, preview_plugin, reload_plugins,
    report_plugin_load_ms, set_plugin_dev_reload, set_plugin_log_view,
    uninstall_plugin,
};
use panel_window::{open_panel_window, take_panel_payload, PanelWindowState};

/// 常驻三窗：关掉只是隐藏（进程与窗口都要留着，随时再叫出来）。
/// **不在这份名单里的窗口 = 按需建的面板窗**，关窗即销毁，于是它的 renderer 进程跟着释放。
const PERSISTENT_WINDOWS: [&str; 3] = [MAIN_WINDOW, "floating", TOOLTIP_WINDOW];

const MAIN_WINDOW: &str = "main";
/// 信息提示窗 label：点击穿透与 devtools 例外都要点名它，故留在非调试构建里。
const TOOLTIP_WINDOW: &str = "tooltip";
const TRAY_ICON: &str = "main";
const SHOW_MENU_ITEM: &str = "show";
const HIDE_MENU_ITEM: &str = "hide";
const QUIT_MENU_ITEM: &str = "quit";

fn show_main<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return Ok(());
    };

    window.unminimize()?;
    window.show()?;
    window.set_focus()
}

fn hide_main<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        window.hide()?;
    }
    Ok(())
}

/// 把枚举变成它序列化出来的那个小写串（如 `SessionPhase::Running` → `running`）。
///
/// 插件事件是对外形状，不该出现 `Debug` 的花括号与变体名；拿不到就回 `unknown`，
/// 宁可给一个明确读不懂的值也不 unwrap（release 是 `panic = abort`）。
fn enum_token<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|token| token.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // ⚠️ 必须排在第一次读 `app_data_dir()` 之前：晚一步，下面所有状态都按空目录加载，
            //   旧配置 / 会话存档这一轮就用不上了（下次启动才生效，用户看到的是「设置全丢了」）。
            migrate::copy_app_data(app.handle());
            let app_data = app.path().app_data_dir()?;
            let status_dir = app_data.join("status");
            let module_config_path = app_data.join("floating-modules.json");
            // 组件配置：外壳只负责存盘 + 转发，旧版字段由各组件自己认领
            // （见 modules/agent_status/settings.rs；组件包不在这条链上，它读写自己的存储分区）。
            let module_state = ModuleState::load(module_config_path, &[adopt_legacy_fields]);
            // 组件配置联动：外壳不认识任何组件字段，各自在组合根登记「配置变了要做什么」。
            module_state.on_config_change(config_hook());
            // 已落盘的组件配置在启动时先取一次（此刻 StatusState 还没建好，钩子会空转）。
            let agent_status_settings = AgentStatusSettings::from_config(&module_state.snapshot());
            // 代理解析器：注册表 + 初值（从已存配置读出，避免重启后回落 none）。
            let proxy_state = ProxyState::new(None);
            proxy_state.set_selected(&agent_status_settings.proxy_resolver);
            // endpoint 档案登记表（主窗口「信息卡拓展」的数据源），与 StatusState 共享同一份。
            let profile_state = EndpointProfileState::load(app_data.join("endpoint-profiles.json"));
            // 凭据不在数据目录里（按 service 名前缀寻址，而前缀就是 App 标识）：
            // flow 的条目名从刚加载的档案 `secret_names` 反推。
            modules::agent_status::flow::migrate_secrets_from_legacy(&profile_state);
            // 会话存档（SQLite）：跨重启续档。load 打开库、回填缓存并做一次过期清理。
            // 与 profiles 同样处理：clone 进 StatusState（monitor 线程写），再 manage 一份（命令读）。
            let session_store =
                SessionStoreState::load(app_data.join("session-records.db"), now_ms());
            // 插件安装记录：`load_store` 里先确保 `plugins/` 在盘上存在（第一次写安装记录才不会被
            // ENOENT 挡掉），再读安装记录。这里只读内存，扫描留给命令层的 spawn_blocking。
            let plugin_store = plugin::commands::load_store(&app_data);
            // 协议 handler 在 build 之前就注册好了（见下面的 register_…），它只能在运行时
            // 从这份静态引用拿安装记录；所以这里必须紧跟在 load_store 之后注入。
            plugin::protocol::attach(plugin_store.clone());
            // 信息卡拓展的流程脚本运行态：HTTP/密钥用生产实现；worker 线程消费调度队列。
            let (flow_state, flow_rx) =
                FlowState::new(Arc::new(UreqFetch), Arc::new(KeyringSecrets));
            // 消息推送运行态：只注入 HTTP 后端（主题不是密钥，没有凭据要管），投递跑在自己的 worker 线程。
            // 启动初值直接从已存配置取（与 flow 一样，配置钩子此刻还空转）。
            let (push_state, push_rx) = PushState::with_production_backends();
            push_state.configure(modules::agent_status::push::config::PushConfig::from_block(
                module_state.snapshot().module_settings.get("agent-status"),
            ));
            // 转灯日志运行态：落点基准与 inbox 同为 `status_dir`（日志在它下面的 `log/`），
            // 初值同样从已存配置取（配置钩子此刻还空转）。
            let log_state = LogState::new(
                status_dir.clone(),
                LogConfig::from_block(
                    module_state.snapshot().module_settings.get("agent-status"),
                ),
            );
            let status_state = StatusState::new(
                status_dir,
                agent_status_settings.completed_retention_ms,
                agent_status_settings.running_timeout_ms,
                proxy_state,
                profile_state.clone(),
                session_store.clone(),
                flow_state.clone(),
                push_state.clone(),
                log_state,
            );
            app.manage(status_state.clone());
            // 面板窗宿主状态：待取载荷与归属表（谁开的这扇窗，销毁就回报给谁）。
            app.manage(PanelWindowState::default());
            app.manage(profile_state);
            app.manage(session_store);
            app.manage(module_state);
            app.manage(flow_state.clone());
            app.manage(push_state.clone());
            app.manage(plugin_store);
            // 插件日志运行态：环形缓冲 + 按包的 JSONL 落点（落点基准与安装记录同一个 `plugins/`）。
            app.manage(plugin::log::LogState::new(app_data.clone()));
            // 灯态事件总线：组件包「按包拉取」，闸在 `open_plugin_events` 那道命令里
            // （不广播 —— 广播等于把提问原文发给同窗的每个包）。
            let plugin_events = plugin::events::EventState::new();
            app.manage(plugin_events.clone());
            // 组件包的存储分区（一包含一个 JSON 文件，卸载即删）。
            app.manage(plugin::storage::StorageState::new(app_data.clone()));
            // 文件能力运行态：私有目录的落点 + 「用户选中的文件」的会话内句柄表。
            // 协议层的 `__pick` / `__fs` 两条入口要从这里读，所以顺手交给它一份静态引用。
            let plugin_fs = plugin::fs::FsState::new(app_data.clone());
            plugin::protocol::attach_fs(plugin_fs.clone());
            app.manage(plugin_fs);
            // 密钥能力运行态：值只进出 OS 凭据管理器，盘上只留「这个包用过哪些名字」。
            app.manage(plugin::secrets::SecretState::new(app_data.clone()));
            // 外设能力运行态：蓝牙连接与串口端口都是**活的系统资源**，必须常驻宿主
            // （组件包会随热重载 / 窗口隐藏重建，重建就断连是不可接受的）。
            app.manage(plugin::ble::BleState::new());
            app.manage(plugin::serial::SerialState::new());
            // 巡检线程：读状态快照换算成「每盏灯的读数」。没人订阅时整轮跳过，
            // 所以一个包都没装的情况下这条线程完全不查快照。
            {
                let status_for_events = app.state::<StatusState>().inner().clone();
                plugin::events::spawn_loop(plugin_events.clone(), move || {
                    let snapshot = status_for_events.snapshot_with_upstream();
                    let samples = snapshot
                        .sessions
                        .iter()
                        .map(|session| plugin::events::LightSample {
                            key: session.key.clone(),
                            provider: enum_token(&session.provider),
                            session_id: session.session_id.clone(),
                            phase: enum_token(&session.phase),
                            endpoint: session
                                .upstream_url
                                .clone()
                                .or_else(|| session.base_url.clone()),
                            prompt: session.prompt.clone(),
                            started_at_ms: session.started_at_ms,
                            last_seen_at_ms: session.last_seen_at_ms,
                            error_code: session.error_code.clone(),
                            waiting_reason: session.waiting_reason.clone(),
                        })
                        .collect();
                    (samples, now_ms() as i64)
                });
            }
            spawn_monitor(app.handle().clone(), status_state);
            spawn_worker(app.handle().clone(), flow_state, flow_rx);
            spawn_push_worker(app.handle().clone(), push_state, push_rx);

            let show = MenuItemBuilder::with_id(SHOW_MENU_ITEM, "打开主体").build(app)?;
            let hide = MenuItemBuilder::with_id(HIDE_MENU_ITEM, "隐藏主体").build(app)?;
            let quit = MenuItemBuilder::with_id(QUIT_MENU_ITEM, "退出").build(app)?;
            let menu = Menu::with_items(app, &[&show, &hide, &quit])?;

            if let Some(tray) = app.tray_by_id(TRAY_ICON) {
                tray.set_menu(Some(menu))?;
            }

            // 是否打开各窗口的 devtools 由环境变量 `UFO_DEVTOOLS` 决定（默认全关，见 devtools.rs）。
            // 只在调试构建下有意义：`open_devtools` 本身仅在 debug_assertions 或开 `devtools`
            // feature 时才存在，release 二进制里这段会被整个编译掉。
            // ⚠️ 遍历**当下真实存在的窗口**而不是抄一份 label 清单：配置里加窗口时这里不用跟着改，
            // 也避免清单漏项导致新窗口永远开不出 devtools。
            #[cfg(debug_assertions)]
            let devtools_targets = devtools::DevtoolsTargets::from_env();
            // 存一份给按需建窗的那条路用：面板窗是运行时才建的，下面这一轮遍历不到它。
            #[cfg(debug_assertions)]
            app.manage(devtools_targets.clone());
            #[cfg(debug_assertions)]
            for (label, window) in app.webview_windows() {
                if devtools_targets.wants(&label) {
                    window.open_devtools();
                }
            }

            // 信息提示窗只做展示：设为点击穿透，避免遮挡或拦截下方窗口的点击。
            // Rust 侧调用不经过 capability 校验，因此无需授予 set-ignore-cursor-events 权限。
            //
            // 例外 —— 点击穿透（Win32 的 WS_EX_TRANSPARENT）会让页面收不到鼠标移动，
            // 配合配置里的 focusable:false（WS_EX_NOACTIVATE）窗口也无法激活，
            // 两者叠加会让 devtools 的元素检查选取器失效（悬停不高亮、点不中）。
            // 因此仅当 `UFO_DEVTOOLS` 点名了 tooltip 时才放开这两个限制去检查它，
            // 其余情况（含调试构建但没给 tooltip 开 devtools）保持 release 的穿透行为。
            if let Some(window) = app.get_webview_window(TOOLTIP_WINDOW) {
                #[cfg(debug_assertions)]
                let inspecting_tooltip = devtools_targets.wants(TOOLTIP_WINDOW);
                #[cfg(not(debug_assertions))]
                let inspecting_tooltip = false;

                if inspecting_tooltip {
                    if let Err(error) = window.set_focusable(true) {
                        eprintln!("failed to make tooltip window focusable: {error}");
                    }
                } else if let Err(error) = window.set_ignore_cursor_events(true) {
                    eprintln!("failed to make tooltip window click-through: {error}");
                }
            }

            Ok(())
        })
        .on_menu_event(|app, event| match event.id.as_ref() {
            SHOW_MENU_ITEM => {
                if let Err(error) = show_main(app) {
                    eprintln!("failed to show main window: {error}");
                }
            }
            HIDE_MENU_ITEM => {
                if let Err(error) = hide_main(app) {
                    eprintln!("failed to hide main window: {error}");
                }
            }
            QUIT_MENU_ITEM => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|app, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
            | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => {
                if let Err(error) = show_main(app) {
                    eprintln!("failed to show main window: {error}");
                }
            }
            _ => {}
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                // 常驻三窗关窗 = 隐藏（进程与窗口都留着，随时再叫）。
                // 面板窗**不拦**：让它真的关掉，它那个 renderer 进程才会被释放（这是改按需建窗的全部意义）。
                if PERSISTENT_WINDOWS.contains(&window.label()) {
                    api.prevent_close();
                    if let Err(error) = window.hide() {
                        eprintln!("failed to hide {} window: {error}", window.label());
                    }
                }
            }
            WindowEvent::Destroyed => {
                let label = window.label();
                if PERSISTENT_WINDOWS.contains(&label) {
                    return;
                }
                // 前端在被销毁时来不及自己收尾，两份「谁在看」的账必须由这里摘掉：
                // 转灯日志按窗口 label 记账，插件日志按「包 + 窗口」记账。
                if let Some(state) = window.try_state::<StatusState>() {
                    state.log.set_viewer(label, false);
                }
                if let Some(state) = window.try_state::<plugin::log::LogState>() {
                    state.drop_window_viewers(label);
                }
                // 最后清宿主账并回报发起方（主窗据此解锁内嵌编辑框）。
                if let Some(state) = window.try_state::<PanelWindowState>() {
                    state.note_destroyed(window.app_handle(), label);
                }
            }
            _ => {}
        })
        .plugin(tauri_plugin_shell::init())
        // 「导出转灯日志」要用原生另存为对话框拿目标路径：它走 IPC → RFD 自己弹窗，
        // 与 prevent-default 拦的那条通道（webview 的 alert/confirm/prompt）不是同一个东西。
        // ⚠️ 这条推理要靠运行时实测确认，别当既成事实：点一次导出、看原生框到底出不出来。
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_prevent_default::init())
        // 插件产物：`ufo-plugin://<id>/<包内相对路径>`（Windows 上呈递成
        // `http://ufo-plugin.localhost/<id>/<path>`，handler 里两种形状都认）。
        // IO 丢给 blocking 线程：同步 handler 会占住协议派发，一次大文件读能拖住整窗加载。
        .register_asynchronous_uri_scheme_protocol(
            plugin::protocol::SCHEME,
            |_ctx, request, responder| {
                tauri::async_runtime::spawn_blocking(move || {
                    let method = request.method().clone();
                    let uri = request.uri().to_string();
                    let headers = request.headers().clone();
                    responder.respond(plugin::protocol::handle(&method, &uri, &headers));
                });
            },
        )
        .invoke_handler(tauri::generate_handler![
            get_status_snapshot,
            dismiss_status_session,
            set_tooltip_interactive,
            open_panel_window,
            take_panel_payload,
            get_proxy_resolvers,
            get_endpoint_profiles,
            add_endpoint_profile,
            delete_endpoint_profile,
            get_flow_results,
            get_flow_snapshot,
            save_endpoint_flow,
            run_flow_test,
            set_flow_secret,
            delete_flow_secret,
            get_push_status,
            test_push_provider,
            get_session_records,
            delete_session_record,
            clear_session_records,
            get_status_log,
            count_status_log,
            clear_status_log,
            export_status_log,
            set_status_log_view,
            read_light_sound,
            is_left_mouse_pressed,
            get_floating_module_config,
            set_floating_module_config,
            get_plugins,
            get_plugin_registry,
            report_plugin_load_ms,
            preview_plugin,
            install_plugin,
            uninstall_plugin,
            set_plugin_dev_reload,
            reload_plugins,
            plugin_log_write,
            get_recent_plugin_logs,
            get_plugin_logs,
            clear_plugin_logs,
            set_plugin_log_view,
            open_plugin_events,
            poll_plugin_events,
            close_plugin_events,
            plugin_storage_get,
            plugin_storage_set,
            plugin_storage_remove,
            plugin_storage_keys,
            get_plugin_storage_usage,
            plugin_http_fetch,
            plugin_fs_pick,
            plugin_fs_stat,
            plugin_fs_entries,
            plugin_fs_read,
            plugin_fs_read_entry,
            plugin_fs_write,
            plugin_fs_read_private,
            plugin_fs_list,
            plugin_fs_remove,
            get_plugin_fs_usage,
            plugin_secret_get,
            plugin_secret_set,
            plugin_secret_remove,
            get_plugin_secret_names,
            plugin_frame_allow,
            get_plugin_dev_digest,
            plugin_ble_scan,
            plugin_ble_connect,
            plugin_ble_write,
            plugin_ble_read,
            plugin_ble_subscribe,
            plugin_ble_unsubscribe,
            plugin_ble_notifications,
            plugin_ble_disconnect,
            get_plugin_ble_held,
            plugin_serial_ports,
            plugin_serial_open,
            plugin_serial_write,
            plugin_serial_read,
            plugin_serial_timeout,
            plugin_serial_close,
            get_plugin_serial_held,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
