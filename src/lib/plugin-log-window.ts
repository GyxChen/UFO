import type { UnlistenFn } from '@tauri-apps/api/event'
import {
  closeSelfPanelWindow,
  openPanelWindow,
  usePanelWindowOpen,
  type PanelWindowSpec
} from '@/lib/panel-window'
import { PLUGIN_ID_MAX_CHARS, PLUGIN_ID_PATTERN } from '@/lib/plugin-limits'

/**
 * 「插件日志」独立窗口的契约：管理区调试卡上的「拓展」↔ 完整流水那一整窗。
 *
 * 窗口只认一个包（`pluginId`）。级别过滤与「暂停跟随」是窗口自己的界面状态，
 * 不由主窗传 —— 传过来就变成「两处各记一套过滤」，用户看到的不是同一份历史。
 */

/** 窗口 label；⚠️ 与 `capabilities/default.json` 的 windows 名单、`src/windows/index.ts` 的登记键同源，由 `panel-windows.spec.ts` 锁住。 */
export const PLUGIN_LOG_WINDOW_LABEL = 'plugin-log'

/** 这扇窗的身份与几何；窗口不在 `tauri.conf.json` 里声明，这里就是尺寸的唯一真相。 */
export const PLUGIN_LOG_WINDOW: PanelWindowSpec = {
  label: PLUGIN_LOG_WINDOW_LABEL,
  title: 'UFO · 插件日志',
  width: 1160,
  height: 720,
  minWidth: 760,
  minHeight: 420
}

/** 主窗递进来的那一次查看。 */
export interface PluginLogWindowView {
  /** 要看哪个包的日志（清单 id）。 */
  pluginId: string
  /** 展示名：标题栏要说「插件日志 · 余额看板」，不是光秃秃的 id。 */
  pluginName: string
}

/** 主窗侧：打开（或唤起）日志窗并告诉它看哪个包。返回 true = 这次真的新建了一扇。 */
export function openPluginLogWindow(view: PluginLogWindowView): Promise<boolean> {
  return openPanelWindow(PLUGIN_LOG_WINDOW, view)
}

/** 窗口侧：关掉自己（「我关了」由宿主代为回报发起方）。 */
export function closePluginLogWindow(): Promise<void> {
  return closeSelfPanelWindow()
}

/** 窗口侧：接上「新建时暂存的载荷」与「后续 open 事件」两条路。 */
export function usePluginLogOpen(
  handler: (view: PluginLogWindowView, requester: string) => void
): Promise<UnlistenFn> {
  return usePanelWindowOpen<PluginLogWindowView>(isPluginLogWindowView, handler)
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
