//! 转灯信号全链路日志。
//!
//! 一句话：**把「一条信号从平台进来，到最终改变灯色（或者没改变）」的每一步裁决落成一行 JSONL**，
//! 让「灯为什么不动 / 为什么乱跳」从「只能复现」变成「可以回查」。
//!
//! 为什么需要它：这条链路原先**一切拒绝都不留痕迹**。内核有四条 `return false`
//! （去重命中、序号过旧、完成保留窗内、schema 不符），hook 侧有适配器的静默忽略，
//! 巡检器有六类丢弃 —— 它们在外部与「灯本来就不该动」完全同形，用户只能看到灯没变。
//!
//! # 结构
//!
//! | 文件 | 职责 | 依赖 tauri |
//! |---|---|---|
//! | [`record`] | 全链路词汇表：阶段 / 理由 / 条目 / 内核裁决 | ❌（hook 也要用） |
//! | [`paths`] | 落点与文件名，纯函数 | ❌ |
//! | [`sink`] | 追加写：一次 open + 一次 `write_all` | ❌ |
//! | [`config`] | `module_settings['agent-status'].log` 子块 | ❌ |
//! | [`runtime`] | GUI 侧运行态：配置热更新、tail、环形缓冲、回收 | ✅ |
//! | [`commands`] | 面板读日志 / 清空 / 导出 / 开关跟随 | ✅ |
//!
//! # 三条不可破的规矩
//!
//! 1. **日志不得改变灯态**。埋点一律吞掉结果（[`sink::Sink::report`] 返回 `()` 就是为这个），
//!    内核裁决化只加返回值、不改任何 `return` 分支的判定。
//! 2. **默认档不落用户文本**。`detail` 只放白名单标量；提问 / 审批理由只在 `verbose` 档
//!    经 [`record::excerpt`] 截断后写入。凭据（`secrets.get` 的值、`auth.json`、
//!    `settings_config`）任何档都不写。
//! 3. **理由必须是具名枚举**。自由文本没法聚合、没法解释，也就没法变成验收清单。
//!
//! 完整黑洞清单与验收见 `reference/doc/agent-status-log-guide.md`。

mod commands;
mod config;
mod paths;
mod record;
mod runtime;
mod sink;

/// 实时广播的事件名：monitor 每轮把新增的几条推给面板（载荷是一页日志）。
///
/// ⚠️ 前端同名常量在 `src/modules/agent-status/lib/status-log.ts`，两边必须一致
/// （由 `lib/status-log.spec.ts` 锁住）。
pub const STATUS_LOG_EVENT: &str = "agent-status-log";

pub(crate) use commands::{
    clear_status_log, export_status_log, get_status_log, set_status_log_view,
};
pub(crate) use config::{LogConfig, LogLevel};
pub(crate) use paths::log_dir;
pub(crate) use record::{
    excerpt, length_of, wire_name, CleanupReport, EventVerdict, LogEntry, LogReason, LogStage,
    LogWriter,
};
pub(crate) use runtime::{ExportedLog, LogState, StatusLogEntry, StatusLogPage, StatusLogQuery};
pub(crate) use sink::Sink;
