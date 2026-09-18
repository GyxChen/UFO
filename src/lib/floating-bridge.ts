import type { FloatingRuntimeSnapshot } from './floating-modules'

/**
 * 悬浮窗 ↔ 主窗口之间的窗口级消息契约。
 *
 * 两个窗口各自跑一份前端，跨窗口只能靠 `emitTo`；这里集中定义事件名与载荷，
 * 避免事件字符串散落在组件里。事件名带 `ufo-` 前缀以免与 Tauri 内置事件撞名。
 */

/** 悬浮窗 → 主窗口：运行时快照（各形态下每个模块的格子需求 + 窗口总格子数）。 */
export const FLOATING_RUNTIME_EVENT = 'ufo-floating-runtime'
/** 主窗口 → 悬浮窗：请补发一次运行时快照（主窗口挂载时可能已经错过广播）。 */
export const FLOATING_RUNTIME_REQUEST_EVENT = 'ufo-floating-runtime-request'
/** 悬浮窗 → 主窗口：打开某个组件的设置页（`moduleId` 为 null 时打开组件列表）。 */
export const OPEN_MODULE_SETTINGS_EVENT = 'ufo-open-module-settings'

/** 「打开组件设置」的载荷。 */
export interface OpenModuleSettingsPayload {
  /** 要打开的组件 id；null / 缺省 = 主窗口默认的组件列表页。 */
  moduleId: string | null
}

/** 运行时快照事件的载荷类型别名，供监听方标注。 */
export type FloatingRuntimePayload = FloatingRuntimeSnapshot