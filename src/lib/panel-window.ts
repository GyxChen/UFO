import { invoke } from '@tauri-apps/api/core'
import { emitTo, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getAllWindows, getCurrentWindow } from '@tauri-apps/api/window'

/**
 * 独立面板窗的公开契约：主窗与「脚本编辑 / 转灯日志 / 插件日志」这类窗口之间只走这一条协议。
 *
 * ## 窗口是**按需建、关掉即销毁**的
 * 与常驻三窗（悬浮窗 / 主窗 / 提示窗）不同：面板窗平时不显示，但「声明即建窗」会让 WebView2
 * 给它分配一个 renderer 进程（实测每扇 85~105 MB），三扇隐藏窗就是 ~270 MB 常驻。
 * 所以它们不在 `tauri.conf.json` 里声明，而是由 `open_panel_window` 现建、关窗即销毁。
 *
 * ## 于是数据要走两段
 * 新窗的 JS 挂上监听器需要时间，「先 show 再 emit」会把数据丢在空气里：
 * 1. 建窗时宿主把这一份数据**暂存**，窗口挂载后调 `take_panel_payload` 取一次；
 * 2. 窗口已经活着时宿主直接把它 emit 过去（监听器在挂着）。
 * {@link usePanelWindowOpen} 把两条都接上：先 listen 再取暂存 —— 同一份数据重复应用是幂等的
 * （窗口每次 open 都整份换掉自己的状态），所以中一条就够、中两条也不坏。
 *
 * ## 「我关了」由宿主回报
 * 销毁中的 webview 来不及自己发消息，Alt+F4 更是没有出口，所以发起方身份记在宿主侧，
 * `WindowEvent::Destroyed` 时补发 {@link PANEL_WINDOW_CLOSED_EVENT}。窗口自己只管关。
 *
 * ⚠️ 这里是通用层：**不认识任何具体窗口的数据形状与几何**。每扇窗的 payload 类型、校验函数
 * 与 `*_WINDOW` 几何都收在它自己的契约文件里（`lib/script-editor-window.ts`、
 * 各模块与插件的 `*-window.ts`）。
 */

/** 宿主 → 面板窗：这次打开要展示的数据。 */
export const PANEL_WINDOW_OPEN_EVENT = 'ufo-panel-window-open'

/** 面板窗 → 发起方：窗口已经没了（被关闭 / Alt+F4 / 主窗强制关）。 */
export const PANEL_WINDOW_CLOSED_EVENT = 'ufo-panel-window-closed'

/** 一扇面板窗的身份与几何：由它的契约文件持有，宿主只照这个建窗。 */
export interface PanelWindowSpec {
  /** 窗口 label；同一 label 复用同一扇窗（也是 `PANEL_WINDOW_VIEWS` 的登记键）。 */
  label: string
  /** 任务栏 / Alt+Tab 上的名字（窗口无边框，屏幕内的标题栏由 `PanelWindow.vue` 自绘）。 */
  title: string
  /** 初始宽 / 高（逻辑像素）。 */
  width: number
  height: number
  /** 最小宽 / 高：再小就装不下两栏正文。 */
  minWidth: number
  minHeight: number
}

/** 打开请求的载荷（与 Rust `PanelPayload` 同形）。 */
export interface PanelWindowOpen<T = unknown> {
  /** 发起方窗口 label；宿主据此回报关闭，主窗以外的发起方因此也能收到。 */
  requester: string
  /** 这一窗口的数据，形态由窗口自己解释与校验。 */
  data: T
}

/** 关闭回报的载荷。 */
export interface PanelWindowClosed {
  /** 没了的那个窗口 label：接收方按它过滤，别的窗口广播过来的直接忽略。 */
  window: string
}

/**
 * 打开（或唤起）一扇独立面板窗，并把这次要用的数据递进去。
 *
 * 返回 `true` = 这次真的新建了一扇（窗口随后会来自取数据）；`false` = 已存在，只是唤到前台。
 * 宿主会拒掉非法 label 与超额请求（每扇窗一个 renderer，放开就是给内存开个口子），
 * 失败一律抛出 —— 调用方要把它落到行内反馈，不许安静地什么都不发生。
 */
export async function openPanelWindow<T>(spec: PanelWindowSpec, data: T): Promise<boolean> {
  return await invoke<boolean>('open_panel_window', { request: { ...spec, data } })
}

/**
 * 主窗侧：强制关掉某个面板窗（销毁）。
 *
 * 用于「发起方那一页要卸载了，没人再接收同步」这类场景 —— 不等窗口自己的关闭按钮。
 * 回报照常由宿主补发，所以调用方不必自己清理状态。
 */
export async function closePanelWindow(label: string): Promise<void> {
  const target = (await getAllWindows()).find(({ label: name }) => name === label)
  if (target) await target.close()
}

/**
 * 窗口侧：关掉自己（销毁，释放它那个 renderer 进程）。
 *
 * ⚠️ 先做完本窗口的收尾（停订阅、摘 attach）再调这里：销毁之后没有第二次机会。
 * 「我关了」这条通知不用发 —— 宿主在 `Destroyed` 时代为回报。
 */
export async function closeSelfPanelWindow(): Promise<void> {
  await getCurrentWindow().close()
}

/**
 * 面板窗侧：订阅打开请求，并把校验过的数据与发起方一起交给 `handler`。
 *
 * ⚠️ 这是**只接了事件那一条路**的下层件，只给 {@link usePanelWindowOpen} 用：
 * 新建那一次的载荷在宿主那里暂存着，不走自取就永远拿不到。
 * 载荷形状不对就整份丢掉（与提示窗同一做法）：半个对象进不了界面，
 * 否则会用 `undefined` 顶掉上一次的读数。
 */
function listenPanelWindowOpen<T>(
  guard: (value: unknown) => value is T,
  handler: (data: T, requester: string) => void
): Promise<UnlistenFn> {
  return listen<PanelWindowOpen<unknown>>(PANEL_WINDOW_OPEN_EVENT, event => {
    const payload = event.payload
    if (!payload || typeof payload.requester !== 'string' || !guard(payload.data)) return
    handler(payload.data, payload.requester)
  })
}

/** 面板窗侧：取走建窗时暂存的那一份数据（取完即清；没有则 null）。 */
async function takePanelPayload<T>(
  guard: (value: unknown) => value is T
): Promise<PanelWindowOpen<T> | null> {
  try {
    const payload = await invoke<PanelWindowOpen<unknown> | null>('take_panel_payload')
    if (!payload || typeof payload.requester !== 'string' || !guard(payload.data)) return null
    return { requester: payload.requester, data: payload.data }
  } catch (error) {
    // 取不到初始载荷不算致命：后续每次 open 宿主还会 emit 一份过来。
    console.warn('panel window payload unavailable', error)
    return null
  }
}

/**
 * 面板窗侧：一条命令接上「新建时的暂存数据」与「后续 open 事件」两条路。
 *
 * ⚠️ 顺序是硬要求：**先 listen 再取暂存**。反过来会在两步之间丢掉宿主发来的那一次 open，
 * 窗口就永远停在「还没人告诉我看什么」的空壳上。
 * 返回卸载函数（两个来源共用一个 handler，重复应用同一份数据是幂等的）。
 */
export async function usePanelWindowOpen<T>(
  guard: (value: unknown) => value is T,
  handler: (data: T, requester: string) => void
): Promise<UnlistenFn> {
  const unlisten = await listenPanelWindowOpen<T>(guard, handler)
  const staged = await takePanelPayload<T>(guard)
  if (staged) handler(staged.data, staged.requester)
  return unlisten
}

/** 发起方侧：订阅某个面板窗的关闭回报（其它窗口发来的按 label 忽略）。 */
export function listenPanelWindowClosed(label: string, handler: () => void): Promise<UnlistenFn> {
  return listen<PanelWindowClosed>(PANEL_WINDOW_CLOSED_EVENT, event => {
    if (event.payload?.window === label) handler()
  })
}
