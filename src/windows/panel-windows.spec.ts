/**
 * 独立面板窗的「同源」护栏：前端契约（label + 几何）、`capabilities/*.json` 的 windows 名单、
 * `PANEL_WINDOW_VIEWS` 的登记，以及宿主侧那份配额常量，四处必须对齐。
 *
 * ⚠️ 这里少任何一处，症状都不是编译错误而是「点了没反应 / 窗口用不了」：
 * - label 没进 `capabilities/default.json` ⇒ 建窗与跨窗事件都会被 tauri 拒掉
 *   （AGENTS.md 的「新增窗口必须把 label 加进 capability」）；
 * - 几何越出宿主那对夹取边界 ⇒ 界面以为开了 1160 宽，实际被悄悄夹成别的尺寸；
 * - 面板窗出现在 `tauri.conf.json` 里 ⇒ 启动就各占一个 WebView2 renderer（实测每扇 85~105 MB），
 *   这正是改成按需建窗要省掉的那笔常驻内存，所以这里反向锁死：**不许再声明回去**。
 */
import { describe, expect, it } from 'vitest'
import confRaw from '../../src-tauri/tauri.conf.json?raw'
import panelWindowRs from '../../src-tauri/src/panel_window.rs?raw'
import defaultCapabilityRaw from '../../src-tauri/capabilities/default.json?raw'
import settingsCapabilityRaw from '../../src-tauri/capabilities/main-module-settings.json?raw'
import { SCRIPT_EDITOR_WINDOW, SCRIPT_EDITOR_WINDOW_LABEL } from '@/lib/script-editor-window'
import {
  STATUS_LOG_WINDOW,
  STATUS_LOG_WINDOW_LABEL,
} from '@/modules/agent-status/lib/status-log-window'
import { PLUGIN_LOG_WINDOW, PLUGIN_LOG_WINDOW_LABEL } from '@/lib/plugin-log-window'
import { PANEL_WINDOW_VIEWS } from './index'

/** 三扇窗的契约（label / 标题 / 几何）：宿主按这份现建窗口。 */
const PANEL_WINDOW_SPECS = [SCRIPT_EDITOR_WINDOW, STATUS_LOG_WINDOW, PLUGIN_LOG_WINDOW]
const panelLabels = Object.keys(PANEL_WINDOW_VIEWS)

/** 常驻三窗：它们才应该在配置文件里声明。 */
const PERSISTENT_LABELS = ['main', 'floating', 'tooltip']

const configuredWindows = (JSON.parse(confRaw) as { app: { windows: { label: string }[] } }).app.windows
  .map(window => window.label)
  .sort()
const capabilityWindows = (JSON.parse(defaultCapabilityRaw) as { windows: string[] }).windows
const settingsCapabilityWindows = (JSON.parse(settingsCapabilityRaw) as { windows: string[] }).windows

/** 从 Rust 那份源里读一个 `const NAME: 类型 = 值;`；读不到就说明护栏该跟着改了。 */
function rustConst(name: string): number {
  const matched = new RegExp(`const ${name}:\\s*[^=]+=\\s*([\\d.]+);`).exec(panelWindowRs)
  if (!matched) throw new Error(`panel_window.rs 里找不到常量 ${name}（护栏要跟着改）`)
  return Number(matched[1])
}

describe('独立面板窗的登记与权限', () => {
  it('配置文件里只剩常驻三窗：面板窗一律按需现建', () => {
    // 声明即建窗：隐藏着的面板窗也各占一个 renderer 进程，这是本次改造要省掉的常驻内存。
    for (const label of panelLabels) {
      expect(configuredWindows, `「${label}」被写回 tauri.conf.json 了，它会在启动时就占一整个进程`).not.toContain(
        label
      )
    }
    expect(configuredWindows).toEqual([...PERSISTENT_LABELS].sort())
  })

  it('每个面板窗的 label 都在 default capability 的名单里', () => {
    for (const label of panelLabels) {
      expect(capabilityWindows, `「${label}」没进 capabilities/default.json：建窗与事件都会被拒`).toContain(label)
    }
  })

  it('契约里的 label 常量与视图登记一一对应', () => {
    // 常量写错一个字母 = 开不出一扇窗，所以两头都要比。
    expect([...PANEL_WINDOW_SPECS].map(spec => spec.label)).toEqual([
      SCRIPT_EDITOR_WINDOW_LABEL,
      STATUS_LOG_WINDOW_LABEL,
      PLUGIN_LOG_WINDOW_LABEL,
    ])
    expect(panelLabels.sort()).toEqual(
      [SCRIPT_EDITOR_WINDOW_LABEL, STATUS_LOG_WINDOW_LABEL, PLUGIN_LOG_WINDOW_LABEL].sort()
    )
  })

  /**
   * 宿主的 `normalize_label` / `clamp_size` 都会**夹或拒**，不报错的那一半是静默降级：
   * label 不合法直接开不出来，尺寸越界则被夹成另一个值 —— 两种都得在改契约时当场拦住。
   */
  it('每扇窗的几何与 label 都落在宿主认可的范围里', () => {
    const minInner = rustConst('MIN_INNER')
    const maxInner = rustConst('MAX_INNER')
    const maxLabelChars = rustConst('MAX_LABEL_CHARS')
    const maxTitleChars = rustConst('MAX_TITLE_CHARS')
    for (const spec of PANEL_WINDOW_SPECS) {
      expect(spec.label.length, `「${spec.label}」超出宿主允许的 label 长度`).toBeLessThanOrEqual(maxLabelChars)
      // 与 Rust `normalize_label` 同一条判据：小写字母 / 数字开头，只含小写字母、数字与短横线。
      expect(spec.label, `「${spec.label}」过不了宿主的 label 校验，窗口开不出来`).toMatch(/^[a-z0-9][a-z0-9-]*$/)
      // 标题会被截到上限并剥掉控制字符：写了个更长的名字只会在任务栏上被砍一截。
      expect([...spec.title].length, `「${spec.label}」的标题超出宿主上限`).toBeLessThanOrEqual(maxTitleChars)
      for (const [field, value] of Object.entries(spec)) {
        if (field === 'label' || field === 'title') continue
        expect(value, `「${spec.label}」的 ${field} 必须是有限正数`).toBeGreaterThan(0)
        if (field.startsWith('min')) {
          expect(value, `「${spec.label}」的 ${field} 小于宿主下限，会被夹住`).toBeGreaterThanOrEqual(minInner)
        } else {
          expect(value, `「${spec.label}」的 ${field} 超出宿主上限，会被夹住`).toBeLessThanOrEqual(maxInner)
        }
      }
      expect(spec.minWidth, `「${spec.label}」最小宽大于初始宽`).toBeLessThanOrEqual(spec.width)
      expect(spec.minHeight, `「${spec.label}」最小高大于初始高`).toBeLessThanOrEqual(spec.height)
    }
  })

  /**
   * 配额挡的是「同时在开几扇窗」，而每扇窗按 label 复用，所以登记数一旦超过配额，
   * 用户就会遇到「三扇都开着，第四扇点了没反应」。加窗口时这一条会把 Rust 那个常量顶出来。
   */
  it('宿主配额容得下所有登记过的面板窗', () => {
    expect(rustConst('MAX_LIVE_PANEL_WINDOWS')).toBeGreaterThanOrEqual(panelLabels.length)
  })

  /**
   * 日志窗带着「清空 / 导出 / 计数 / attach」这些写命令（只给设置侧的窗口），所以它的 label
   * 必须出现在 `main-module-settings.json` —— 漏掉的话窗口能开、能翻页，只有那几个动作报权限错。
   */
  it('转灯日志窗拿到了设置侧的写权限', () => {
    expect(settingsCapabilityWindows).toContain(STATUS_LOG_WINDOW_LABEL)
    // 插件日志窗同理：它有「清空 / 翻页 / 挂跟随」三枚写动作，没进这份 capability 就只有这三个报权限错。
    expect(settingsCapabilityWindows).toContain(PLUGIN_LOG_WINDOW_LABEL)
    // 脚本窗只搬正文，不碰任何写命令：它不该出现在这份 capability 里（权限面不扩大）。
    expect(settingsCapabilityWindows).not.toContain(SCRIPT_EDITOR_WINDOW_LABEL)
  })
})
