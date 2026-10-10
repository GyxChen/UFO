import type { UnlistenFn } from '@tauri-apps/api/event'
import { closeSelfPanelWindow, listenPanelWindowOpen, openPanelWindow } from '@/lib/panel-window'
import { PLUGIN_ID_MAX_CHARS, PLUGIN_ID_PATTERN } from '@/lib/plugin-limits'

/**
 * 「插件日志」独立窗口的契约：管理区调试卡上的「拓展」↔ 完整流水那一整窗。
 *
 * 窗口只认一个包（`pluginId`）。级别过滤与「暂停跟随」是窗口自己的界面状态，
 * 不由主窗传 —— 传过来就变成「两处各记一套过滤」，用户看到的不是同一份历史。
 */

/** 窗口 label；⚠️ 与 `tauri.conf.json` 里声明的那一份同源，由 `panel-windows.spec.ts` 锁住。 */
export const PLUGIN_LOG_WINDOW_LABEL = 'plugin-log'

/** 主窗递进来的那一次查看。 */
export interface PluginLogWindowView {
  /** 要看哪个包的日志（清单 id）。 */
  pluginId: string
  /** 展示名：标题栏要说「插件日志 · 余额看板」，不是光秃秃的 id。 */
  pluginName: string
}

/** 主窗侧：打开（或重新聚焦）日志窗并告诉它看哪个包。窗口没声明时返回 false。 */
export function openPluginLogWindow(view: PluginLogWindowView): Promise<boolean> {
  return openPanelWindow(PLUGIN_LOG_WINDOW_LABEL, view)
}

/** 窗口侧：收起自己并回报发起方。 */
export function closePluginLogWindow(requester: string): Promise<void> {
  return closeSelfPanelWindow(requester)
}

/** 窗口侧：订阅「要看哪个包」，载荷形状不对由 `listenPanelWindowOpen` 整份丢掉。 */
export function listenPluginLogOpen(
  handler: (view: PluginLogWindowView, requester: string) => void
): Promise<UnlistenFn> {
  return listenPanelWindowOpen<PluginLogWindowView>(isPluginLogWindowView, handler)
}

/**
 * 载荷校验：`pluginId` 会直接进后端那条「按包取日志目录」的命令，
 * 所以必须过与清单同一套 id 判据；名字只用于显示，但也要挡掉控制字符。
 */
export function isPluginLogWindowView(value: unknown): value is PluginLogWindowView {
  const view = value as PluginLogWindowView | null
  if (!view) return false
  if (typeof view.pluginId !== 'string') return false
  if (view.pluginId.length > PLUGIN_ID_MAX_CHARS || !PLUGIN_ID_PATTERN.test(view.pluginId)) return false
  // eslint-disable-next-line no-control-regex
  if (typeof view.pluginName !== 'string' || /[\x00-\x1f]/.test(view.pluginName)) return false
  return true
}
