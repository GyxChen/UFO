// @vitest-environment happy-dom
/**
 * 把模板的**源码**过一遍**真实的装载链路**：`.vue` 编译 → 宿主注入 → 四个挂载点 → activate。
 *
 * 为什么值得单独一档：前面所有测试喂给装载器的是**夹具**（假组件、假入口）。这一档跑的是
 * 作者照抄的那份样本本体，所以它证的是「照这个模板做，真的挂得上四个面」——
 * 也就是需求 3（`.vue` 开发）与需求 0（架构选型）目前能在无真窗口下拿到的最强证据。
 *
 * 不依赖 `dist/`：那份产物被 gitignore，别人 clone 下来没有它。
 * 这里让 vite 直接编译 `reference/plugin-template/src/index.js`，测试进程自己就是「宿主」，
 * 所以 `from 'vue'` 拿到的与组件实例用的是同一份 Vue（这正是「不许有第二个 Vue」要防的事）。
 */

import { beforeAll, describe, expect, it, vi } from 'vitest'
import { createApp, h, nextTick } from 'vue'
import manifestRaw from '../../reference/plugin-template/public/plugin.json?raw'

vi.mock('@tauri-apps/api/core', () => ({
  // 模板的 activate 会订灯态事件；这里只需要「别真的去敲 IPC」，返回值没有意义。
  invoke: vi.fn(async () => null),
}))

const loader = await import('@/lib/plugin-loader')
const { syncPluginDefinitions, clearPluginDefinitions, pluginDefinitions, pluginExtensions } = loader
const types = await import('@/lib/plugin-types')
const template = await import('../../reference/plugin-template/src/index.js')

/** 台账里的那条记录：清单直接用模板自己的 `plugin.json`（同源，不另写一份）。 */
function recordOf() {
  const manifest = JSON.parse(manifestRaw.replace(/\r\n/g, '\n'))
  return {
    id: manifest.id,
    name: manifest.name,
    version: manifest.version,
    manifest,
    dir: 'reference/plugin-template/dist/my-plugin',
    source: 'local',
    devReload: false,
    warnings: [],
    files: 2,
    bytes: 1024,
    loadMs: 0,
    error: null,
  }
}

/** 把一个组件挂进一棵真应用树，返回它渲染出来的文本与容器。 */
function mountComponent(component: unknown) {
  const host = document.createElement('div')
  document.body.appendChild(host)
  const app = createApp({ render: () => h(component as never) })
  const vm = app.mount(host)
  return { app, vm, html: host.innerHTML, host }
}

describe('模板源码过真实装载链路', () => {
  let definition: Record<string, any>
  let dispose: (() => void) | undefined

  beforeAll(async () => {
    clearPluginDefinitions()
    await syncPluginDefinitions([recordOf() as never], {
      // 与 `importPluginEntry` 一字同构：装载的是**默认导出**，不是模块命名空间。
      importEntry: async () => ({
        ok: true as const,
        entry: (template as { default: never }).default,
        loadMs: 2,
      }),
    })
    definition = pluginDefinitions.value[0] as never
    // activate 是外壳在窗口里起步的那一步，这里手动走一次（真实链路由 App.vue 调）。
    dispose = definition?.activate?.()
    await nextTick()
  })

  it('装出来的是一条完整的模块定义（id 与清单一致、四个面都在）', () => {
    expect(definition, '模板没能装出定义：装载失败的原因见下一档').toBeTruthy()
    expect(definition.id).toBe('my-plugin')
    expect(definition.tooltip, '独立信息卡没挂上').toBeTruthy()
    expect(definition.settings?.sections?.length, '设置页一个分区都没有').toBeGreaterThan(0)
    expect(definition.component, '格子组件没挂上').toBeTruthy()
    expect(definition.activate, 'activate 没接进生命周期').toBeTypeOf('function')
  })

  it('清单声明的格子数就是 demand 的读数（每个支持的形态都问一遍）', () => {
    const cells = recordOf().manifest.surfaces.cell as number
    for (const layout of definition.supportedLayouts) {
      expect(definition.demand?.(layout), `${layout} 形态下格子数不对`).toBe(cells)
    }
  })

  it('独立信息卡的尺寸走的是清单那一份声明', () => {
    expect(definition.tooltip.size).toEqual({ width: 320, height: 160 })
    expect(types.tooltipSizeOf(recordOf().manifest)).toEqual({ width: 320, height: 160 })
  })

  it('插入块按清单的宿主名收进扩展槽', () => {
    const slot = pluginExtensions.value.find(item => item.pluginId === 'my-plugin')
    expect(slot, '模板的 extension 没进槽').toBeTruthy()
    expect(slot?.hostId).toBe('agent-status')
    expect(slot?.label, '来源标注的名字该取清单的展示名').toBe(recordOf().name)
    expect(slot?.maxRows, '行数上限被宿主夹过就要如实落在界面上').toBeLessThanOrEqual(3)
  })

  it('四个面的组件都能真的渲染（host 绑到了每一个面上）', () => {
    const surfaces = [
      ['格子', definition.component],
      ['信息卡', definition.tooltip.component],
      ['插入块', pluginExtensions.value[0]?.component],
      ['设置页', definition.settings.sections[0].component],
    ] as const
    for (const [name, component] of surfaces) {
      const { app, html, host } = mountComponent(component)
      expect(html, `${name} 渲染出的是空的`).toContain('tpl-')
      app.unmount()
      host.remove()
    }
  })

  /** 样式没有 `styles` 这个口子：它必须由 JS 带进来并挂到文档上，否则「挂上了但没样式」。 */
  it('activate 之后文档里出现了样式，且重复激活不会重复插', () => {
    const count = document.querySelectorAll('style').length
    expect(count, '模板的样式没被挂进文档（?inline 那条链路断了）').toBeGreaterThan(0)
    const again = definition.activate?.()
    expect(document.querySelectorAll('style').length).toBe(count)
    again?.()
  })

  it('卸载尾巴不抛（dispose 是作者自己返回的那一句）', () => {
    expect(dispose, '模板的 activate 没回 dispose').toBeTypeOf('function')
    expect(() => dispose?.()).not.toThrow()
  })

  it('装载过程没被记成坏包', () => {
    expect(loader.pluginLoadErrors.value['my-plugin'], '装载带了降级说明').toBeUndefined()
  })
})
