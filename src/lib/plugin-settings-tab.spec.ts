/**
 * 需求「在主窗口可以有自己的一级 tabbar 做设置页」的边界护栏。
 *
 * 主窗不是按「这是个插件」给页签，而是走外壳的公开判据 `hasModuleSettings`
 * （`src/components/MainWindow.vue` 的 `tabs = FLOATING_MODULE_DEFINITIONS.filter(hasModuleSettings)`）。
 * 所以这条需求真正要钉的是两件事，方向相反：
 * 1. 声明了 `surfaces.settings` 的包，产出的定义必须让 `hasModuleSettings` 为真
 *    —— 不然表现是「包装上了、界面也有组件，但主窗就是不给它页签」；
 * 2. **没**声明的包必须为假 —— 不然每个包都白占一条一级页签，点进去是空的。
 * 顺带钉分区的形状字段（`label` / `fill`）是作者给的那一份：`fill` 丢了就破坏主窗的高度链规矩。
 */

import { describe, expect, it } from 'vitest'
import type { Component } from 'vue'
import { clearPluginDefinitions, pluginDefinitions, syncPluginDefinitions } from './plugin-loader'
import { hasModuleSettings, type FloatingModuleSection } from './floating-modules'
import type { PluginRecordJson } from './plugin-types'
import type { PluginEntryResult, PluginUiEntry } from './plugin-runtime'

const Stub: Component = { name: 'SettingsTabStub', template: '<div />' }

/** 造一条已装记录；`settings` 决定清单里给不给那个挂载点。 */
function record(settings: boolean): PluginRecordJson {
  return {
    id: 'balance-board',
    name: '余额看板',
    version: '1.2.0',
    manifest: {
      manifestVersion: 1,
      apiVersion: 1,
      id: 'balance-board',
      name: '余额看板',
      version: '1.2.0',
      description: '把余额画进一格',
      entry: 'main.js',
      surfaces: { cell: 1, settings },
      permissions: [],
      hosts: [],
    },
    dir: 'C:/work/balance-board',
    source: 'local',
    devReload: false,
    warnings: [],
    files: 1,
    bytes: 512,
    loadMs: 0,
    error: null,
  }
}

/** 用给定的入口导出跑一次装载。 */
async function load(entry: Partial<PluginUiEntry>, settings: boolean) {
  clearPluginDefinitions()
  const result: PluginEntryResult = { ok: true, entry: entry as PluginUiEntry, loadMs: 1 }
  await syncPluginDefinitions([record(settings)], { importEntry: async () => result })
  return pluginDefinitions.value[0]
}

describe('插件的设置页能不能拿到主窗那一格一级页签', () => {
  it('声明了 settings 的包，外壳判据为真（主窗据此给一级页签）', async () => {
    const definition = await load({ cell: Stub, settings: [{ id: 'general', label: '常规', component: Stub }] }, true)
    expect(definition, '包装不出来').toBeTruthy()
    expect(hasModuleSettings(definition!), '主窗不会给这个包页签').toBe(true)
    expect(definition!.settings?.sections.map(item => item.label)).toEqual(['常规'])
  })

  /** 反方向：白占页签的坏法比少一条页签更常见（清单抄来的、导出留着没删）。 */
  it('没声明 settings 的包，外壳判据必须为假', async () => {
    const definition = await load({ cell: Stub, settings: [{ id: 'general', label: '常规', component: Stub }] }, false)
    expect(hasModuleSettings(definition!), '没声明也该有页签 —— 那会白占一条一级 tab').toBe(false)
  })

  it('分区自己的 `fill` 与 id 是作者给的那一份（丢了会破坏主窗的高度链）', async () => {
    const definition = await load(
      {
        cell: Stub,
        settings: [
          { id: 'tall', label: '要占满', component: Stub, fill: true },
          { id: 'short', label: '自然高', component: Stub },
        ],
      },
      true
    )
    const sections = definition!.settings?.sections ?? []
    expect(sections.map(item => item.id)).toEqual(['tall', 'short'])
    expect(sections[0]?.fill).toBe(true)
    expect(sections[1]?.fill).not.toBe(true)
  })

  /** 缺 component 的那种脏条目：整条跳过，但不能把这一页拖成「有页签却没内容」。 */
  it('脏分区被跳过时，剩下的那页仍然让判据为真', async () => {
    const definition = await load(
      {
        cell: Stub,
        settings: [
          { id: 'broken', label: '坏的那页' } as unknown as FloatingModuleSection,
          { id: 'good', label: '好的那页', component: Stub },
        ],
      },
      true
    )
    expect(definition!.settings?.sections.map(item => item.id)).toEqual(['good'])
    expect(hasModuleSettings(definition!)).toBe(true)
  })
})
