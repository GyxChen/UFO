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
        clear_status_log, dismiss_status_session, export_status_log, get_proxy_resolvers,
        get_status_log, get_status_snapshot, now_ms, set_status_log_view, set_tooltip_interactive,
        spawn_monitor, LogConfig, LogState, StatusState,
    },
};
use modules::weather::settings::adopt_legacy_fields as adopt_weather_legacy_fields;
use modules::weather::{
    delete_weather_secret, get_weather_secret_status, get_weather_snapshot, search_weather_city,
    set_weather_secret, test_weather_connection, WeatherState,
};
use modules::todo::{
    complete_todo_task, create_todo_task, delete_todo_task, delete_todo_token,
    get_todo_secret_status, get_todo_snapshot, list_todo_projects, postpone_todo_tasks,
    reopen_todo_task, set_todo_token, test_todo_connection, TodoState,
};
use mouse::is_left_mouse_pressed;

const MAIN_WINDOW: &str = "main";
/// 悬浮窗 label。除被 `UFO_DEVTOOLS` 点名开 devtools 外没有别处用到，故限定调试构建。
#[cfg(debug_assertions)]
const FLOATING_WINDOW: &str = "floating";
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
            // （见 modules/agent_status/settings.rs 与 modules/weather/settings.rs）。
            let module_state = ModuleState::load(
                module_config_path,
                &[adopt_legacy_fields, adopt_weather_legacy_fields],
            );
            // 组件配置联动：外壳不认识任何组件字段，各自在组合根登记「配置变了要做什么」。
            module_state.on_config_change(config_hook());
            // 已落盘的组件配置在启动时先取一次（此刻 StatusState 还没建好，钩子会空转）。
            let agent_status_settings = AgentStatusSettings::from_config(&module_state.snapshot());
            // 代理解析器：注册表 + 初值（从已存配置读出，避免重启后回落 none）。
            let proxy_state = ProxyState::new(None);
            proxy_state.set_selected(&agent_status_settings.proxy_resolver);
            // endpoint 档案登记表（主窗口「信息卡拓展」的数据源），与 StatusState 共享同一份。
            let profile_state = EndpointProfileState::load(app_data.join("endpoint-profiles.json"));
            // 凭据不在数据目录里（按 service 名前缀寻址，而前缀就是 App 标识），各组件搬自己那几条：
            // flow 的条目名从刚加载的档案 `secret_names` 反推，todo / weather 用的是固定名。
            modules::agent_status::flow::migrate_secrets_from_legacy(&profile_state);
            modules::todo::secrets::migrate_from_legacy();
            modules::weather::secrets::migrate_from_legacy();
            // 会话存档（SQLite）：跨重启续档。load 打开库、回填缓存并做一次过期清理。
            // 与 profiles 同样处理：clone 进 StatusState（monitor 线程写），再 manage 一份（命令读）。
            let session_store =
                SessionStoreState::load(app_data.join("session-records.db"), now_ms());
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
            app.manage(profile_state);
            app.manage(session_store);
            app.manage(module_state);
            app.manage(flow_state.clone());
            app.manage(push_state.clone());
            app.manage(WeatherState::default());
            app.manage(TodoState::default());
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
            #[cfg(debug_assertions)]
            let devtools_targets = devtools::DevtoolsTargets::from_env();
            #[cfg(debug_assertions)]
            for label in [MAIN_WINDOW, FLOATING_WINDOW, TOOLTIP_WINDOW] {
                if !devtools_targets.wants(label) {
                    continue;
                }
                if let Some(window) = app.get_webview_window(label) {
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
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Err(error) = window.hide() {
                    eprintln!("failed to hide {} window: {error}", window.label());
                }
            }
        })
        .plugin(tauri_plugin_shell::init())
        // 「导出转灯日志」要用原生另存为对话框拿目标路径：它走 IPC → RFD 自己弹窗，
        // 与 prevent-default 拦的那条通道（webview 的 alert/confirm/prompt）不是同一个东西。
        // ⚠️ 这条推理要靠运行时实测确认，别当既成事实：点一次导出、看原生框到底出不出来。
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_prevent_default::init())
        .invoke_handler(tauri::generate_handler![
            get_status_snapshot,
            dismiss_status_session,
            set_tooltip_interactive,
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
            clear_status_log,
            export_status_log,
            set_status_log_view,
            read_light_sound,
            is_left_mouse_pressed,
            get_floating_module_config,
            set_floating_module_config,
            get_weather_snapshot,
            search_weather_city,
            test_weather_connection,
            set_weather_secret,
            delete_weather_secret,
            get_weather_secret_status,
            get_todo_snapshot,
            create_todo_task,
            complete_todo_task,
            postpone_todo_tasks,
            delete_todo_task,
            reopen_todo_task,
            test_todo_connection,
            list_todo_projects,
            set_todo_token,
            delete_todo_token,
            get_todo_secret_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
