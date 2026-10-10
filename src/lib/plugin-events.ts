/** 插件相关的事件名（与 Rust 侧各写一份，靠 spec 对撞）。 */

/**
 * 台账变更广播：安装 / 卸载 / 开关 / 重扫之后 Rust 把它发给所有窗口。
 * payload 是 `{ revision, records }`（见 `plugin-types.ts` 的 `LedgerBroadcastJson`）。
 */
export const PLUGINS_CHANGED_EVENT = 'plugins-changed'

/**
 * 插件日志的实时批次（调试卡与独立日志窗都订阅它）。
 * payload 是 `{ pluginId, lines }`；只有该包挂着跟随者时 Rust 才会发。
 */
export const PLUGIN_LOG_EVENT = 'plugin-log'
