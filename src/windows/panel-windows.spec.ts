/**
 * 独立面板窗的「三处同源」护栏：`tauri.conf.json` 的声明、`capabilities/*.json` 的 windows 名单、
 * 前端 label 常量与 `PANEL_WINDOW_VIEWS` 的登记，四处必须对齐。
 *
 * ⚠️ 这四样里少任何一处，症状都不是编译错误而是「点了没反应」：
 * - 配置里没声明 ⇒ `openPanelWindow` 找不到那个 label，界面只剩一行失败说明；
 * - label 没进 `capabilities/default.json` ⇒ tauri 直接拒掉跨窗 show / hide 与事件，
 *   连窗口都叫不醒（AGENTS.md 的「新增窗口必须把 label 加进 capability」）；
 * - 登记了视图但配置里没有（或反过来）⇒ 一处改名的漏项，永远要跑到那一扇窗才发现。
 */
import { describe, expect, it } from 'vitest'
import confRaw from '../../src-tauri/tauri.conf.json?raw'
import defaultCapabilityRaw from '../../src-tauri/capabilities/default.json?raw'
import settingsCapabilityRaw from '../../src-tauri/capabilities/main-module-settings.json?raw'
import { SCRIPT_EDITOR_WINDOW_LABEL } from '@/lib/script-editor-window'
import { STATUS_LOG_WINDOW_LABEL } from '@/modules/agent-status/lib/status-log-window'
import { PANEL_WINDOW_VIEWS } from './index'

/** 只用到的那几个字段：窗口声明的其余项与本护栏无关。 */
interface DeclaredWindow {
  label: string
  visible: boolean
  decorations: boolean
}

const declaredWindows = (JSON.parse(confRaw) as { app: { windows: DeclaredWindow[] } }).app.windows
const declared = new Map(declaredWindows.map(window => [window.label, window]))
const capabilityWindows = (JSON.parse(defaultCapabilityRaw) as { windows: string[] }).windows
const settingsCapabilityWindows = (JSON.parse(settingsCapabilityRaw) as { windows: string[] }).windows

describe('独立面板窗的声明与权限', () => {
  it('每个登记过的视图都在配置里声明了一扇隐藏窗', () => {
    for (const label of Object.keys(PANEL_WINDOW_VIEWS)) {
      const window = declared.get(label)
      expect(window, `「${label}」登记了视图，但 tauri.conf.json 里没有这个窗口`).toBeDefined()
      // 按需显示：启动就可见的窗会凭空多出一块没人要的屏。
      expect(window?.visible, `「${label}」必须 visible: false，由主窗按需叫醒`).toBe(false)
      // 外壳自绘（`.panel-shell` / `.panel-bar`）：系统给了标题栏就两套标题栏。
      expect(window?.decorations, `「${label}」必须 decorations: false，标题栏由 PanelWindow 自绘`).toBe(false)
    }
  })

  it('每个独立窗口的 label 都在 default capability 的名单里', () => {
    for (const label of Object.keys(PANEL_WINDOW_VIEWS)) {
      expect(capabilityWindows, `「${label}」没进 capabilities/default.json：show / hide / 事件都会被拒`).toContain(
        label
      )
    }
  })

  it('契约里的 label 常量与配置里的声明一一对应', () => {
    // 常量写错一个字母 = 开不出一扇窗，所以两头都要比。
    expect(declared.has(SCRIPT_EDITOR_WINDOW_LABEL), '脚本窗常量与配置声明不一致').toBe(true)
    expect(declared.has(STATUS_LOG_WINDOW_LABEL), '日志窗常量与配置声明不一致').toBe(true)
    expect(Object.keys(PANEL_WINDOW_VIEWS).sort()).toEqual([SCRIPT_EDITOR_WINDOW_LABEL, STATUS_LOG_WINDOW_LABEL].sort())
  })

  /**
   * 日志窗带着「清空 / 导出 / 计数 / attach」这四枚按钮，命令写在
   * `allow-manage-status-log` 那一组里（只给设置侧的窗口），所以它的 label 必须出现在
   * `main-module-settings.json` —— 漏掉的话窗口能开、能翻页，只有那两个动作报权限错。
   */
  it('转灯日志窗拿到了设置侧的写权限', () => {
    expect(settingsCapabilityWindows).toContain(STATUS_LOG_WINDOW_LABEL)
    // 脚本窗只搬正文，不碰任何写命令：它不该出现在这份 capability 里（权限面不扩大）。
    expect(settingsCapabilityWindows).not.toContain(SCRIPT_EDITOR_WINDOW_LABEL)
  })
})
