//! 独立面板窗的宿主侧：按需建窗、把「这一次打开」的数据交给窗口、销毁时回报发起方。
//!
//! ## 为什么不在 `tauri.conf.json` 里预先声明
//! 面板窗（脚本编辑 / 转灯日志 / 插件日志）平时根本不显示，但**声明即建窗**：WebView2 会为
//! 每扇窗分配一个 renderer 进程，实测每个 85~105 MB，三扇隐藏窗就是 ~270 MB 常驻。
//! 所以这里改成「要开才建、关掉即销毁」，隐藏窗口不再占内存。
//!
//! ## 与「声明 + 隐藏 + 按需显示」的旧口径差在哪
//! 窗口不再预先存在 ⇒ 「先 show 再 emitTo」有竞争（新窗的 JS 还没挂监听）。因此数据走**两段**：
//! 1. 建窗时把载荷暂存进 [`PanelWindowState::pending`]，窗口挂载后调 `take_panel_payload` 取一次（一次性）；
//! 2. 窗口已经活着时直接 `emit_to` 过去（它的监听器在）。
//! 两条都发同一份数据，重复应用是幂等的（窗口每次 open 都整份换掉自己的状态）。
//!
//! ## 销毁的回报由宿主负责
//! 前端调 `close()` 之后 webview 就没了，来不及自己发「我关了」，Alt+F4 更是完全没有出口。
//! 于是发起方记录在 [`PanelWindowState::owners`] 里，`WindowEvent::Destroyed` 时由这里补发
//! [`CLOSED_EVENT`] —— 主窗据此解锁内嵌编辑框，不靠前端自觉。
//!
//! ## 本层不认识任何具体窗口
//! label / 标题 / 几何全部由前端在请求里给（各窗口的契约文件持有自己的 `*_WINDOW`），
//! 这里只做**形状校验与配额**；谁销毁后要清哪份订阅，由组合根（`lib.rs`）负责。
//!
//! ## panic-free（release 是 `panic = "abort"`）
//! 不 `unwrap` / 不 `expect` / 不索引：锁拿不到就跳过，建窗失败返回 `Err(String)`。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use tauri::{AppHandle, Emitter, Manager, Runtime, State, WebviewUrl, WebviewWindowBuilder};

/// 主窗 → 面板窗：「这一次要看什么」。
pub const OPEN_EVENT: &str = "ufo-panel-window-open";
/// 面板窗 → 发起方：窗口已经没了（销毁）。
pub const CLOSED_EVENT: &str = "ufo-panel-window-closed";

/// 同时活着的面板窗上限：每扇一个 renderer，放开就是给前端一个把内存吃光的口子。
const MAX_LIVE_PANEL_WINDOWS: usize = 3;
/// 几何夹取：再小装不下标题栏与正文，再大没意义（且容易把 0 当成「不限制」传进来）。
const MIN_INNER: f64 = 320.0;
const MAX_INNER: f64 = 4000.0;
/// label 与标题的长度上限（label 会进窗口系统与事件路由，不给任意字符串的机会）。
const MAX_LABEL_CHARS: usize = 32;
const MAX_TITLE_CHARS: usize = 60;

/// 前端提交的建窗 / 唤起请求。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelWindowRequest {
    /// 窗口 label（`[a-z0-9][a-z0-9-]*`）；同一 label 复用同一扇窗。
    pub label: String,
    /// 任务栏 / Alt+Tab 上显示的名字（窗口本身无边框，标题栏由前端自绘）。
    pub title: String,
    /// 期望宽 / 高（逻辑像素）。
    pub width: f64,
    pub height: f64,
    /// 最小宽 / 高。
    pub min_width: f64,
    pub min_height: f64,
    /// 这一次要展示的数据，形态由那个窗口的契约解释（本层只搬运）。
    pub data: JsonValue,
}

/// 交给窗口的一份数据（也用于事件载荷）。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelPayload {
    /// 是谁开的这扇窗；销毁时回报发给它。
    pub requester: String,
    /// 这一次的数据。
    pub data: JsonValue,
}

/// 销毁回报的载荷。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClosedReport {
    /// 没了的那扇窗 label（接收方按它过滤）。
    window: String,
}

/// 面板窗的宿主状态：待取载荷 + 归属 + 活着的有哪些。
#[derive(Default, Clone)]
pub struct PanelWindowState {
    inner: Arc<Mutex<Live>>,
}

#[derive(Default)]
struct Live {
    /// 等窗口挂载来取的载荷（label → 数据）。
    pending: HashMap<String, PanelPayload>,
    /// 每扇窗的发起方（销毁时回报用）。
    owners: HashMap<String, String>,
}

impl PanelWindowState {
    fn with<T>(&self, run: impl FnOnce(&mut Live) -> T) -> Option<T> {
        let Ok(mut guard) = self.inner.lock() else {
            return None;
        };
        Some(run(&mut guard))
    }

    /// 窗口销毁：摘掉归属并把「它没了」回报给发起方。
    ///
    /// 由 `lib.rs` 的 `WindowEvent::Destroyed` 调用 —— 前端来不及自己发这条。
    pub fn note_destroyed<R: Runtime>(&self, app: &AppHandle<R>, label: &str) {
        // 一次取干净：摘了两次的话第二次拿不到归属，回报就永远发不出去。
        let owner = match self.with(|live| {
            live.pending.remove(label);
            live.owners.remove(label)
        }) {
            Some(Some(owner)) => owner,
            // 锁拿不到，或这扇窗本来不是这里建的：没什么可回报的，跳过就好
            // （宁可少一条通知，也不为了报错去 unwrap）。
            _ => return,
        };
        let report = ClosedReport {
            window: label.to_string(),
        };
        if let Err(error) = app.emit_to(owner.as_str(), CLOSED_EVENT, &report) {
            eprintln!("failed to report panel window {label} closed: {error}");
        }
    }
}

/// 校验并规范化 label：只允许小写字母 / 数字 / 短横线，且必须以字母或数字开头。
///
/// ⚠️ 这条不是形式主义：label 会进窗口系统与事件路由，还会被当作 `emit_to` 的目标。
/// 放过 `../x`、空串或超长串，等于把「前端能给任意窗口发消息」的口子留在这一层。
fn normalize_label(raw: &str) -> Result<String, String> {
    let label = raw.trim();
    if label.is_empty() || label.chars().count() > MAX_LABEL_CHARS {
        return Err("窗口 label 为空或过长".to_string());
    }
    let mut chars = label.chars();
    let first = chars.next().unwrap_or(' ');
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return Err("窗口 label 必须以小写字母或数字开头".to_string());
    }
    if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        return Err("窗口 label 只能含小写字母、数字与短横线".to_string());
    }
    Ok(label.to_string())
}

/// 规范化标题（只限长度与控制字符：它是给人看的，不做语义解释）。
fn normalize_title(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_TITLE_CHARS)
        .collect();
    if cleaned.trim().is_empty() {
        "UFO".to_string()
    } else {
        cleaned
    }
}

/// 把尺寸夹进合理区间（前端传 0 / NaN / 负数都不该建出一扇用不了的窗）。
fn clamp_size(value: f64, fallback: f64) -> f64 {
    if !value.is_finite() {
        return fallback;
    }
    value.clamp(MIN_INNER, MAX_INNER)
}

/// 打开（或唤起）一扇独立面板窗。
///
/// 返回 `true` = 这次真的新建了一扇（窗口挂载后会来取载荷）；`false` = 已存在，只是唤起并直接发数据。
///
/// ⚠️ 发起方身份取**调用方窗口**的 label（`window` 由 tauri 注入），不信请求里写的字段 ——
/// 否则任何窗口都能谎报「我是 main」，把销毁回报拐到自己那儿。
#[tauri::command]
pub async fn open_panel_window<R: Runtime>(
    app: AppHandle<R>,
    window: tauri::WebviewWindow<R>,
    state: State<'_, PanelWindowState>,
    request: PanelWindowRequest,
) -> Result<bool, String> {
    let label = normalize_label(&request.label)?;
    let requester = window.label().to_string();
    let title = normalize_title(&request.title);
    let width = clamp_size(request.width, 900.0);
    let height = clamp_size(request.height, 640.0);
    let min_width = clamp_size(request.min_width, MIN_INNER).min(width);
    let min_height = clamp_size(request.min_height, MIN_INNER).min(height);
    let payload = PanelPayload {
        requester: requester.clone(),
        data: request.data,
    };

    // 已经活着：唤起 + 直接把这一次的数据发过去（它的监听器在挂着）。
    if let Some(existing) = app.get_webview_window(&label) {
        let _ = existing.unminimize();
        existing.show().map_err(|error| error.to_string())?;
        // 聚焦失败不该让整次打开报错：窗口已经出来了，抢不到前台只是体验瑕疵。
        let _ = existing.set_focus();
        // 顺手留一份待取载荷：极小概率用户在建窗完成前又点了一次，此时监听器可能还没挂上。
        let _ = state.with(|live| {
            live.pending.insert(label.clone(), payload.clone());
            live.owners.insert(label.clone(), requester);
        });
        if let Err(error) = app.emit_to(label.as_str(), OPEN_EVENT, &payload) {
            eprintln!("failed to deliver panel payload to {label}: {error}");
        }
        return Ok(false);
    }

    // 配额：超过上限就拒绝，并把原因说清楚（前端有行内反馈的位置）。
    let over_quota = state
        .with(|live| live.owners.len() >= MAX_LIVE_PANEL_WINDOWS)
        .unwrap_or(false);
    if over_quota {
        return Err(format!(
            "同时打开的独立窗口已达上限（{MAX_LIVE_PANEL_WINDOWS} 扇），先关掉一个"
        ));
    }

    // 先记账再建窗：建窗失败要能回滚，别让归属与载荷留成孤儿。
    let _ = state.with(|live| {
        live.pending.insert(label.clone(), payload.clone());
        live.owners.insert(label.clone(), requester);
    });

    let built = WebviewWindowBuilder::new(
        &app,
        &label,
        // 与配置文件里的窗口同一入口：按 label 分派视图（见 `src/App.vue`）。
        WebviewUrl::App("index.html".into()),
    )
    .title(&title)
    .inner_size(width, height)
    .min_inner_size(min_width, min_height)
    .center()
    .resizable(true)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .skip_taskbar(false)
    .visible(true)
    .build();

    match built {
        Ok(window) => {
            // 面板窗是运行时才建的，`setup` 里那一轮遍历不到它：按同一个 `UFO_DEVTOOLS` 在这里补开。
            #[cfg(debug_assertions)]
            if app
                .try_state::<crate::devtools::DevtoolsTargets>()
                .is_some_and(|targets| targets.wants(&label))
            {
                window.open_devtools();
            }
            Ok(true)
        }
        Err(error) => {
            let _ = state.with(|live| {
                live.pending.remove(&label);
                live.owners.remove(&label);
            });
            Err(format!("窗口开不起来：{error}"))
        }
    }
}

/// 窗口侧：取走属于自己这一次的数据（取完即清，重复调用返回 None）。
///
/// 新建的窗口靠它拿到初始载荷 —— 那一刻它还没来得及 `listen`，事件会丢。
#[tauri::command]
pub fn take_panel_payload<R: Runtime>(
    window: tauri::WebviewWindow<R>,
    state: State<'_, PanelWindowState>,
) -> Option<PanelPayload> {
    let label = window.label();
    state.with(|live| live.pending.remove(label))?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// label 是「发给哪个窗口」的判据，放过路径片段或超长串就等于让前端能定向发消息。
    #[test]
    fn label_accepts_only_window_name_shape() {
        assert_eq!(normalize_label("script-editor").ok().as_deref(), Some("script-editor"));
        assert_eq!(normalize_label(" status-log ").ok().as_deref(), Some("status-log"));
        let too_long = "a".repeat(MAX_LABEL_CHARS + 1);
        for bad in ["", "   ", "Main", "-main", "a/b", "a\\b", "a b", "a.b", "a\nb", too_long.as_str()] {
            assert!(normalize_label(bad).is_err(), "不该放过 {bad:?}");
        }
    }

    /// 标题只用于任务栏：控制字符要剥掉（它能伪造出一行看不见的东西），超长要截。
    #[test]
    fn title_is_cleaned_not_rejected() {
        assert_eq!(normalize_title("UFO · 编辑脚本"), "UFO · 编辑脚本");
        assert_eq!(normalize_title("a\u{7}b\u{0}c"), "abc");
        assert_eq!(normalize_title("   "), "UFO");
        assert_eq!(normalize_title(&"长".repeat(80)).chars().count(), MAX_TITLE_CHARS);
    }

    /// 0 / 负数 / NaN / 无穷都不该建出一扇用不了的窗：非有限值回落到默认，越界的夹而不是报错。
    #[test]
    fn sizes_are_clamped_into_the_usable_band() {
        assert_eq!(clamp_size(0.0, 900.0), MIN_INNER);
        assert_eq!(clamp_size(-20.0, 900.0), MIN_INNER);
        assert_eq!(clamp_size(f64::NAN, 900.0), 900.0);
        assert_eq!(clamp_size(f64::INFINITY, 900.0), 900.0);
        assert_eq!(clamp_size(99_999.0, 900.0), MAX_INNER);
        assert_eq!(clamp_size(1160.0, 900.0), 1160.0);
    }
}
