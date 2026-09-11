use tauri::{
    menu::{Menu, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconEvent},
    AppHandle, Manager, Runtime, WindowEvent,
};

mod module_config;
mod status;

use module_config::ModuleState;
pub use status::try_run_status_hook;
use status::{get_status_snapshot, spawn_monitor, StatusState};

const MAIN_WINDOW: &str = "main";
const FLOATING_WINDOW: &str = "floating";
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
            let app_data = app.path().app_data_dir()?;
            let status_dir = app_data.join("status");
            let module_config_path = app_data.join("floating-modules.json");
            let module_state = ModuleState::load(module_config_path);
            let status_state = StatusState::new(status_dir, module_state.retention_ms());
            app.manage(status_state.clone());
            app.manage(module_state);
            spawn_monitor(app.handle().clone(), status_state);

            let show = MenuItemBuilder::with_id(SHOW_MENU_ITEM, "打开主体").build(app)?;
            let hide = MenuItemBuilder::with_id(HIDE_MENU_ITEM, "隐藏主体").build(app)?;
            let quit = MenuItemBuilder::with_id(QUIT_MENU_ITEM, "退出").build(app)?;
            let menu = Menu::with_items(app, &[&show, &hide, &quit])?;

            if let Some(tray) = app.tray_by_id(TRAY_ICON) {
                tray.set_menu(Some(menu))?;
            }

            #[cfg(debug_assertions)]
            if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
                window.open_devtools();
            }
            //悬浮窗的devtools,如果不用直接注释掉即可
            #[cfg(debug_assertions)]
            if let Some(window) = app.get_webview_window(FLOATING_WINDOW) {
                window.open_devtools();
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
        .plugin(tauri_plugin_prevent_default::init())
        .invoke_handler(tauri::generate_handler![
            get_status_snapshot,
            module_config::get_floating_module_config,
            module_config::set_floating_module_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
