import type { UnlistenFn } from '@tauri-apps/api/event'
import { closeSelfPanelWindow, listenPanelWindowOpen, openPanelWindow } from '@/lib/panel-window'

/**
 * 「转灯日志」独立窗口的契约：主窗「记录详情」里那条折叠条目 ↔ 完整链路那一整窗。
 *
 * 窗口只认这一条会话（`provider:session_id`），过滤条件（环节 / 只看被拒 / 暂停跟随）
 * 是窗口自己的界面状态，不由主窗传 —— 传过来反而会出现「两处各记一套过滤」的真相。
 */

/** 窗口 label；⚠️ 与 `src-tauri/tauri.conf.json` 里声明的那一份同源，由 `panel-windows.spec.ts` 锁住。 */
export const STATUS_LOG_WINDOW_LABEL = 'status-log'

/** 主窗递进来的那一次查看。 */
export interface StatusLogWindowView {
  /** 要看的那条会话 key（`provider:session_id`）；空串没有意义，校验挡在外面。 */
  sessionKey: string
}

/** 主窗侧：打开（或重新聚焦）日志窗并告诉它要看哪条会话。窗口没声明时返回 false。 */
export function openStatusLogWindow(view: StatusLogWindowView): Promise<boolean> {
  return openPanelWindow(STATUS_LOG_WINDOW_LABEL, view)
}

/** 窗口侧：收起自己并回报发起方。 */
export function closeStatusLogWindow(requester: string): Promise<void> {
  return closeSelfPanelWindow(requester)
}

/** 窗口侧：订阅「要看哪一条会话」，载荷形状不对由 `listenPanelWindowOpen` 整份丢掉。 */
export function listenStatusLogOpen(
  handler: (view: StatusLogWindowView, requester: string) => void
): Promise<UnlistenFn> {
  return listenPanelWindowOpen<StatusLogWindowView>(isStatusLogWindowView, handler)
}

/**
 * 载荷形状校验。
 *
 * `sessionKey` 会被拼进后端那条按盘分页的查询（`StatusLogQuery.key`），所以这里不认
 * 非字符串与空串；长度按会话 key 的实际口径给一个宽上限，超了就是不认识的东西。
 */
export function isStatusLogWindowView(value: unknown): value is StatusLogWindowView {
  const view = value as StatusLogWindowView | null
  return !!view && typeof view.sessionKey === 'string' && view.sessionKey.length > 0 && view.sessionKey.length <= 200
}
