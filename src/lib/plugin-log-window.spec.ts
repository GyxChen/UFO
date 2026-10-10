// @vitest-environment happy-dom

/**
 * 插件日志通道的三处同源护栏：事件名、载荷判据、命令的授权落点。
 *
 * 手法照 `panel-windows.spec.ts` / `push-providers.spec.ts`：`?raw` 读源、按字面量比对。
 * 期望值全部手写 —— 从被测那边反推等于 `x === x`，是一条永远绿的假护栏。
 */
import { describe, expect, it } from 'vitest'
import logRsRaw from '../../src-tauri/src/plugin/log.rs?raw'
import permissionsRaw from '../../src-tauri/permissions/plugin.toml?raw'
import defaultCapabilityRaw from '../../src-tauri/capabilities/default.json?raw'
import settingsCapabilityRaw from '../../src-tauri/capabilities/main-module-settings.json?raw'
import { PLUGIN_LOG_EVENT } from './plugin-events'
import { PLUGIN_LOG_WINDOW_LABEL, isPluginLogWindowView } from './plugin-log-window'

/** 归一行尾（CRLF 会藏进捕获组）。 */
const logSource = logRsRaw.replace(/\r\n/g, '\n')

describe('日志事件名与 Rust 同源', () => {
  it('Rust 里的 PLUGIN_LOG_EVENT 与前端那份是同一个串', () => {
    const at = logSource.indexOf('pub const PLUGIN_LOG_EVENT')
    expect(at, 'Rust 侧少了这个常量').toBeGreaterThanOrEqual(0)
    const literal = logSource.slice(at).match(/"([^"]+)"/)
    expect(literal?.[1]).toBe(PLUGIN_LOG_EVENT)
  })

  it('日志窗 label 在 Rust 与前端是同一个串', () => {
    // 窗口 label 只在配置与 capability 里出现，Rust 侧不点名 —— 这里比的是前端与配置那一头。
    expect(PLUGIN_LOG_WINDOW_LABEL).toBe('plugin-log')
  })
})

describe('打开载荷的判据', () => {
  it('认合法的包', () => {
    expect(isPluginLogWindowView({ pluginId: 'balance-board', pluginName: '余额看板' })).toBe(true)
  })

  it('拒掉会拼进后端路径或污染标题栏的东西', () => {
    // 逐条断言形状：`pluginId` 会决定读哪个日志目录，绝不能拿未经校验的输入拼路径。
    expect(isPluginLogWindowView(null)).toBe(false)
    expect(isPluginLogWindowView({ pluginId: '', pluginName: 'x' })).toBe(false)
    expect(isPluginLogWindowView({ pluginId: '../evil', pluginName: 'x' })).toBe(false)
    expect(isPluginLogWindowView({ pluginId: 'Bad_Id', pluginName: 'x' })).toBe(false)
    expect(isPluginLogWindowView({ pluginId: 'a'.repeat(41), pluginName: 'x' })).toBe(false)
    expect(isPluginLogWindowView({ pluginId: 'ok', pluginName: '坏\t名字' })).toBe(false)
    expect(isPluginLogWindowView({ pluginId: 'ok' })).toBe(false)
  })
})

describe('日志命令的授权落点', () => {
  /** 从 toml 里抠某个 identifier 授权了哪些命令（按字面量分块，不拼正则）。 */
  function commandsOf(identifier: string): string[] {
    const at = permissionsRaw.indexOf(`identifier = "${identifier}"`)
    expect(at, `permissions/plugin.toml 里没有 ${identifier}`).toBeGreaterThanOrEqual(0)
    const block = permissionsRaw.slice(at, at + 600)
    const list = block.slice(block.indexOf('commands.allow'), block.indexOf(']', block.indexOf('commands.allow')) + 1)
    return [...list.matchAll(/"([a-z0-9_]+)"/g)].map(match => match[1]).filter(name => name !== 'commands.allow')
  }

  const permissions = (JSON.parse(defaultCapabilityRaw) as { permissions: string[] }).permissions
  const settings = (JSON.parse(settingsCapabilityRaw) as { permissions: string[] }).permissions

  it('写与「最近几行」给所有窗口（挂载插件的不止主窗）', () => {
    expect(permissions).toContain('allow-plugin-log-writes')
    expect(commandsOf('allow-plugin-log-writes').sort()).toEqual(['get_recent_plugin_logs', 'plugin_log_write'])
  })

  it('按盘翻页、清空、挂跟随只给设置侧', () => {
    expect(settings).toContain('allow-manage-plugin-logs')
    expect(commandsOf('allow-manage-plugin-logs').sort()).toEqual([
      'clear_plugin_logs',
      'get_plugin_logs',
      'set_plugin_log_view',
    ])
  })

  it('灯态事件的三条只给所有窗口的读侧组', () => {
    expect(permissions).toContain('allow-plugin-events')
    expect(commandsOf('allow-plugin-events').sort()).toEqual([
      'close_plugin_events',
      'open_plugin_events',
      'poll_plugin_events',
    ])
    // 事件不进设置侧那两组：闸在命令里按 pluginId 查清单，跟哪个窗口无关。
    expect(settings).not.toContain('allow-plugin-events')
  })

  it('各组命令名不重复（跨文件撞名会让前一份授权静默失权）', () => {
    const groups = [...commandsOf('allow-get-plugins'), ...commandsOf('allow-manage-plugins'),
      ...commandsOf('allow-plugin-log-writes'), ...commandsOf('allow-manage-plugin-logs'),
      ...commandsOf('allow-plugin-events')]
    expect(new Set(groups).size).toBe(groups.length)
  })
})
