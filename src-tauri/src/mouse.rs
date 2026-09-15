//! 物理鼠标按键状态查询 —— 供悬浮窗拖拽后的贴边判定「左键是否仍按住」。
//!
//! 为什么需要它：Windows 上 `startDragging()` 会把指针交给原生模态拖拽循环，
//! 松开时 WebView2 收不到 pointerup（tao 只在 `WM_EXITSIZEMOVE` 里向主窗口
//! post 合成的 `WM_LBUTTONUP`，webview 内容拿不到），前端无法靠 DOM 事件判断松手，
//! 只能轮询系统级按键状态。`GetAsyncKeyState` 查询的是全局物理状态，
//! 与捕获、焦点、所属进程都无关，正合此用。

/// 鼠标左键当前是否处于按下状态；`None` 表示该平台不支持此查询。
#[tauri::command]
pub fn is_left_mouse_pressed() -> Option<bool> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
        use windows::Win32::UI::Input::KeyboardAndMouse::VK_LBUTTON;
        // 返回值最高位（0x8000）= 查询瞬间是否按下；低位是一次性变化计数，忽略。
        let state = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) };
        Some((state as u16) & 0x8000 != 0)
    }
    // 非 Windows 平台没有「原生拖拽吞 pointerup」问题，前端 pointerup 路径可靠；
    // 返回 None 让前端停用轮询、保持 DOM 事件驱动。
    #[cfg(not(windows))]
    {
        None
    }
}
