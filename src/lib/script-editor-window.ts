import { emitTo, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { closeSelfPanelWindow, listenPanelWindowOpen, openPanelWindow } from './panel-window'
import type { EditorCaret } from './js-editor'

/**
 * 「整窗编辑脚本」独立窗口的契约：主窗的内嵌编辑框 ↔ 独立窗口里的编辑框。
 *
 * 这个文件只说「这一次编辑会话」要传什么，不认识 endpoint、也不认识脚本该怎么写 ——
 * 草稿归属由调用方用一个不透明的 `token` 表达（见 `ScriptEditorSession.token`）。
 */

/** 窗口 label；⚠️ 与 `src-tauri/tauri.conf.json` 里声明的那一份同源，由 `panel-windows.spec.ts` 锁住。 */
export const SCRIPT_EDITOR_WINDOW_LABEL = 'script-editor'

/** 窗口 → 发起方：正文每变一次同步一条（发起方因此随时能拿最新文本去保存 / 运行测试）。 */
export const SCRIPT_EDITOR_SYNC_EVENT = 'ufo-script-editor-sync'

/** 主窗递进来的一次编辑会话。 */
export interface ScriptEditorSession {
  /**
   * 这一次编辑的身份令牌（由发起方每次打开时新造一枚）。
   *
   * 窗口把它原样带回，发起方只接受令牌还有效的那一条 —— 切换草稿或关掉那一页之后，
   * 迟到的正文会被丢掉，不会写进另一份草稿。
   */
  token: string
  /** 标题栏文案（带这份草稿的上下文）。 */
  title: string
  /** 当前草稿。 */
  script: string
  /** 空内容时的灰字提示。 */
  placeholder: string
  /** textarea 的无障碍名称。 */
  ariaLabel: string
  /** 内嵌框里光标与滚动的位置：窗口接手后从这一处继续写。 */
  caret: EditorCaret
}

/** 窗口回给发起方的一条正文。 */
export interface ScriptEditorSync {
  /** 原样带回的令牌。 */
  token: string
  /** 窗口里的全部正文（整份，不是增量）。 */
  script: string
}

/** 主窗侧：打开（或重新聚焦）脚本窗并递进这一次的草稿。窗口没声明时返回 false。 */
export function openScriptEditorWindow(session: ScriptEditorSession): Promise<boolean> {
  return openPanelWindow(SCRIPT_EDITOR_WINDOW_LABEL, session)
}

/** 窗口侧：收起自己并回报发起方（完成 / Esc / 被系统关窗改成隐藏）。 */
export function closeScriptEditorWindow(requester: string): Promise<void> {
  return closeSelfPanelWindow(requester)
}

/** 窗口侧：把当前正文同步给发起方。 */
export function syncScriptEditorTo(requester: string, sync: ScriptEditorSync): Promise<void> {
  return emitTo(requester, SCRIPT_EDITOR_SYNC_EVENT, sync)
}

/** 主窗侧：订阅脚本窗回传的正文。 */
export function listenScriptEditorSync(handler: (sync: ScriptEditorSync) => void): Promise<UnlistenFn> {
  return listen<ScriptEditorSync>(SCRIPT_EDITOR_SYNC_EVENT, event => {
    if (isScriptEditorSync(event.payload)) handler(event.payload)
  })
}

/**
 * 会话载荷的形状校验。
 *
 * ⚠️ 事件可能被别的窗口或旧二进制发歪：结构不对就整份丢掉（与提示窗、日志页同一做法），
 * 半个对象进界面会显示成「空脚本」，那比空白更误导。
 */
export function isScriptEditorSession(value: unknown): value is ScriptEditorSession {
  const session = value as ScriptEditorSession | null
  return (
    !!session &&
    typeof session.token === 'string' &&
    session.token !== '' &&
    typeof session.title === 'string' &&
    typeof session.script === 'string' &&
    typeof session.placeholder === 'string' &&
    typeof session.ariaLabel === 'string' &&
    isEditorCaret(session.caret)
  )
}

/** 正文回包的形状校验（同上：令牌或正文缺一个就不认）。 */
export function isScriptEditorSync(value: unknown): value is ScriptEditorSync {
  const sync = value as ScriptEditorSync | null
  return !!sync && typeof sync.token === 'string' && typeof sync.script === 'string'
}

/** 光标交接的四项读数：全是数字才敢直接喂给 DOM。 */
function isEditorCaret(value: unknown): value is EditorCaret {
  const caret = value as EditorCaret | null
  return (
    !!caret &&
    typeof caret.selectionStart === 'number' &&
    typeof caret.selectionEnd === 'number' &&
    typeof caret.scrollTop === 'number' &&
    typeof caret.scrollLeft === 'number'
  )
}
