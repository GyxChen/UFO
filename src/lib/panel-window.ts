import { emitTo, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getAllWindows, getCurrentWindow } from '@tauri-apps/api/window'

/**
 * 独立面板窗的公开契约：主窗与「脚本编辑 / 转灯日志」这类独立窗口之间只走这一条协议。
 *
 * 窗口一律在 `tauri.conf.json` 里**声明好、平时隐藏、按需显示**（与信息提示窗同一口径，
 * 不在运行时建窗），所以窗口侧的监听器在启动时就挂上了 —— 主窗「先 show 再 emit 数据」
 * 不存在时序竞争。窗口关掉也不销毁（Rust 侧统一把 CloseRequested 改成隐藏），因此每次
 * 打开都必须把这一次要用的数据重新递进去，窗侧不许留着上一次的真相自己往下跑。
 *
 * ⚠️ 这里是通用层：**不认识任何具体窗口的数据形状**。每个窗口自己的 payload 类型与
 * 校验函数收在它的调用方那一侧（见 `lib/script-editor-window.ts` 与各模块的 `lib/*-window.ts`）。
 */

/** 主窗 → 独立窗口：这次打开要展示的数据。 */
export const PANEL_WINDOW_OPEN_EVENT = 'ufo-panel-window-open'

/** 独立窗口 → 主窗：窗口已经收起（完成 / Esc / 系统关窗被改成隐藏）。 */
export const PANEL_WINDOW_CLOSED_EVENT = 'ufo-panel-window-closed'

/** 打开请求的载荷。 */
export interface PanelWindowOpen<T = unknown> {
  /** 发起方窗口 label；窗口收起时照着它回报，主窗以外的发起方因此也能收到。 */
  requester: string
  /** 这一窗口的数据，形态由窗口自己解释与校验。 */
  data: T
}

/** 收起回报的载荷。 */
export interface PanelWindowClosed {
  /** 收起的那个窗口 label：接收方按它过滤，别的窗口广播过来的直接忽略。 */
  window: string
}

/**
 * 显示并聚焦某个独立窗口，同时把这次要用的数据递进去。
 *
 * 返回是否真的找到那个窗口：`false` = 配置里没声明这个 label（或被改名），
 * 调用方据此给出可见的失败反馈，而不是让界面安静地什么都不发生。
 */
export async function openPanelWindow<T>(label: string, data: T): Promise<boolean> {
  const target = (await getAllWindows()).find(({ label: name }) => name === label)
  if (!target) return false
  await target.unminimize()
  await target.show()
  await target.setFocus()
  const payload: PanelWindowOpen<T> = { requester: getCurrentWindow().label, data }
  await emitTo(label, PANEL_WINDOW_OPEN_EVENT, payload)
  return true
}

/** 直接收起某个独立窗口（不走它自己的「完成」）：发起方那一页要卸载时用它，不需要回报。 */
export async function hidePanelWindow(label: string): Promise<void> {
  const target = (await getAllWindows()).find(({ label: name }) => name === label)
  if (target) await target.hide()
}

/**
 * 窗口侧：收起自己并回报发起方。
 *
 * 先隐藏再回报：接收方看到的顺序就是「窗口确实没了」，而不是先收到通知、屏幕上还挂着。
 */
export async function closeSelfPanelWindow(requester: string): Promise<void> {
  const label = getCurrentWindow().label
  await getCurrentWindow().hide()
  const payload: PanelWindowClosed = { window: label }
  await emitTo(requester, PANEL_WINDOW_CLOSED_EVENT, payload)
}

/**
 * 窗口侧：订阅打开请求，并把校验过的数据与发起方一起交给 `handler`。
 *
 * 载荷形状不对就整份丢掉（与提示窗那条同一做法）：半个对象进不了界面，
 * 否则会用 `undefined` 顶掉上一次的读数。
 */
export function listenPanelWindowOpen<T>(
  guard: (value: unknown) => value is T,
  handler: (data: T, requester: string) => void
): Promise<UnlistenFn> {
  return listen<PanelWindowOpen<unknown>>(PANEL_WINDOW_OPEN_EVENT, event => {
    const payload = event.payload
    if (!payload || typeof payload.requester !== 'string' || !guard(payload.data)) return
    handler(payload.data, payload.requester)
  })
}

/** 主窗侧：订阅某个独立窗口的收起回报（其它窗口发来的按 label 忽略）。 */
export function listenPanelWindowClosed(label: string, handler: () => void): Promise<UnlistenFn> {
  return listen<PanelWindowClosed>(PANEL_WINDOW_CLOSED_EVENT, event => {
    if (event.payload?.window === label) handler()
  })
}

/**
 * 窗口侧：注册「本窗口变得不可见」的兜底回调。
 *
 * 完成 / Esc 之外还有 Alt+F4 与托盘那条路：Rust 把关窗统一改成隐藏，进程不退，
 * 窗口只是不见了，于是这一条把「看不见」也当成收起去收尾。
 * ⚠️ 只是兜底：隐藏是否真会触发 `visibilitychange` 取决于 webview 实现，
 * 所以调用方仍要在自己的「完成」里显式收尾，不许把正确性压在这条上。
 */
export function onPanelWindowHidden(handler: () => void): () => void {
  const listener = () => {
    if (document.visibilityState === 'hidden') handler()
  }
  document.addEventListener('visibilitychange', listener)
  return () => document.removeEventListener('visibilitychange', listener)
}
