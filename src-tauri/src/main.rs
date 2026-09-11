// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(exit_code) = tauri_app_lib::try_run_status_hook(&args) {
        std::process::exit(exit_code);
    }

    tauri_app_lib::run()
}
