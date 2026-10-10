// @vitest-environment happy-dom

import { mount } from '@vue/test-utils'
import type { FloatingModuleSection } from '@/lib/floating-modules'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h, inject, type Component } from 'vue'
import * as Vue from 'vue'
import {
  clearPluginDefinitions,
  pluginDefinitions,
  pluginExtensions,
  pluginLoadErrors,
  syncPluginDefinitions,
} from './plugin-loader'
import { PLUGIN_HOST_KEY, type PluginHost } from './plugin-host'
import { PLUGIN_ORIGIN, installPluginGlobals, pluginAssetUrl } from './plugin-runtime'
import type { PluginEntryResult, PluginUiEntry } from './plugin-runtime'
import type { PluginManifestJson, PluginRecordJson } from './plugin-types'

// host 只在装载时构造，不调命令；mock 掉是为了让 `@tauri-apps/api/core` 不在测试环境里做真初始化。
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => undefined) }))

/** 一个够用的组件替身（loader 只把它原样挂到定义上，不渲染）。 */
const Stub = defineComponent({ name: 'Stub', setup: () => () => null })

/**
 * 探针组件：挂载时把自己 inject 到的 host 记进 `seen`。
 *
 * 外壳套的那一层（`withPluginHost`）是看不见的，唯一的判据是真的挂一次 ——
 * 「忘了套」的表现是组件里 inject 到 undefined，只有跑起来才看得见。
 */
const seen = new Map<string, unknown>()
function probe(name: string): Component {
  return defineComponent({
    name,
    // 渲染一个真空元素：attrs 透传要有个元素才落得下去（渲染 null 时属性会被丢掉，测不出真相）。
    setup: () => {
      seen.set(name, inject(PLUGIN_HOST_KEY, undefined))
      return () => h('i')
    },
  })
}

/** 取探针看到的那份 host（没挂过或没包装就是 undefined）。 */
function hostSeen(name: string): PluginHost | undefined {
  return seen.get(name) as PluginHost | undefined
}

/** 造清单：只填测试关心的字段。 */
function manifest(patch: Partial<PluginManifestJson> = {}): PluginManifestJson {
  return {
    id: 'balance-board',
    name: '余额看板',
    version: '1.2.0',
    description: '把余额画进一格',
    entry: 'main.js',
    surfaces: { cell: 2 },
    permissions: [],
    hosts: [],
    ...patch,
  }
}

/** 造安装记录。 */
function record(patch: Partial<PluginRecordJson> = {}): PluginRecordJson {
  return {
    id: 'balance-board',
    name: '余额看板',
    version: '1.2.0',
    manifest: manifest(),
    dir: 'C:/plugins/balance-board',
    source: 'local',
    devReload: false,
    warnings: [],
    files: 2,
    bytes: 2048,
    loadMs: 0,
    error: null,
    ...patch,
  }
}

/** 一次成功的入口结果。 */
function ok(entry: PluginUiEntry, loadMs = 12): PluginEntryResult {
  return { ok: true, entry, loadMs }
}

/** 按 id 分派的假装载器（第三个参数是开发期的缓存击穿串）。 */
function importer(byId: Record<string, PluginEntryResult>) {
  return vi.fn(async (id: string, _entry: string, _version?: string) => byId[id])
}

describe('入口 URL 的拼接', () => {
  it('保留包内目录结构，只编码每一段', () => {
    // 整串编码会把 `/` 变成 %2F，浏览器就解析不回包内路径。
    expect(pluginAssetUrl('balance-board', 'assets/chunk.js')).toBe(
      `${PLUGIN_ORIGIN}/balance-board/assets/chunk.js`
    )
  })

  it('编码需要转义的段名', () => {
    expect(pluginAssetUrl('a-b', 'my chunk.js')).toBe(`${PLUGIN_ORIGIN}/a-b/my%20chunk.js`)
  })

  it('空路径拼不出 URL（返回空串而不是一个悬空斜杠）', () => {
    expect(pluginAssetUrl('a-b', '')).toBe('')
    expect(pluginAssetUrl('a-b', '/')).toBe('')
  })

  it('协议基址按平台取（Windows 是 host 形式）', () => {
    expect(PLUGIN_ORIGIN === 'http://ufo-plugin.localhost' || PLUGIN_ORIGIN === 'ufo-plugin://').toBe(true)
  })

  /** 版本串只加在末尾，不动路径结构（Rust 的 split_uri 会先把 query 摘掉）。 */
  it('带版本串时拼出 ?v=，不带时 URL 干净', () => {
    expect(pluginAssetUrl('a-b', 'main.js', 'm3k2')).toBe(`${PLUGIN_ORIGIN}/a-b/main.js?v=m3k2`)
    expect(pluginAssetUrl('a-b', 'assets/chunk.js', 'x y')).toBe(
      `${PLUGIN_ORIGIN}/a-b/assets/chunk.js?v=x%20y`
    )
    expect(pluginAssetUrl('a-b', 'main.js')).toBe(`${PLUGIN_ORIGIN}/a-b/main.js`)
  })
})

describe('宿主全局注入', () => {
  it('插件产物取到的 Vue 与宿主是同一份引用', () => {
    // 「全场只有一个 Vue 实例」这条不变量的机器判据：多实例会让 provide-inject 静默错乱。
    const target: Record<string, unknown> = {}
    installPluginGlobals(target)
    const injected = target.__UFO_VUE__ as typeof Vue
    expect(injected.ref).toBe(Vue.ref)
    expect(target.__UFO_PINIA__).toBeTruthy()
  })
})

describe('合并注册表', () => {
  it('声明了格子的包产出一条定义，demand 取自清单', async () => {
    await syncPluginDefinitions([record()], { importEntry: importer({ 'balance-board': ok({ cell: Stub }) }) })
    expect(pluginDefinitions.value).toHaveLength(1)
    const definition = pluginDefinitions.value[0]
    expect(definition.id).toBe('balance-board')
    expect(definition.demand?.('square')).toBe(2)
    expect(definition.supportedLayouts).toEqual(['square', 'corner', 'horizontal', 'vertical'])
    clearPluginDefinitions()
  })

  it('没声明格子的包不占地方，但仍能挂设置页', async () => {
    await syncPluginDefinitions(
      [record({ manifest: manifest({ surfaces: { settings: true } }) })],
      { importEntry: importer({ 'balance-board': ok({ settings: [{ id: 'general', label: '常规', component: Stub }] }) }) }
    )
    const definition = pluginDefinitions.value[0]
    expect(definition.demand).toBeUndefined()
    expect(definition.settings?.sections).toHaveLength(1)
    clearPluginDefinitions()
  })

  it('独立信息卡把尺寸与 hasContent 一起交给外壳', async () => {
    await syncPluginDefinitions(
      [record({ manifest: manifest({ surfaces: { cell: 1, tooltip: { own: true, size: [340, 260] } } }) })],
      {
        importEntry: importer({
          'balance-board': ok({
            cell: Stub,
            tooltip: Stub,
            tooltipSize: { width: 340, height: 260 },
            hasContent: target => (target as { key: string }).key !== 'empty',
          }),
        }),
      }
    )
    const tooltip = pluginDefinitions.value[0].tooltip
    // 组件被外壳包了一层（用来绑 host），所以这里只看卡还在，绑定的判据在下面的专测。
    expect(tooltip?.component).toBeTruthy()
    expect(tooltip?.size).toEqual({ width: 340, height: 260 })
    expect(tooltip?.hasContent?.({ moduleId: 'agent-status', key: 'empty' })).toBe(false)
    clearPluginDefinitions()
  })

  it('入口取不到时这个包降级、其余照常挂', async () => {
    const importEntry = importer({
      'balance-board': { ok: false, kind: 'missing', reason: '取不到入口 main.js', loadMs: 3 },
      'ci-timing': ok({ cell: Stub }),
    })
    await syncPluginDefinitions(
      [record(), record({ id: 'ci-timing', name: '构建计时', manifest: manifest({ id: 'ci-timing' }) })],
      { importEntry }
    )
    expect(pluginDefinitions.value.map(item => item.id)).toEqual(['ci-timing'])
    expect(pluginLoadErrors.value['balance-board']).toContain('取不到入口')
    clearPluginDefinitions()
  })

  it('清单声明了挂载点但入口没导出，降级而不是整包消失', async () => {
    await syncPluginDefinitions([record()], { importEntry: importer({ 'balance-board': ok({}) }) })
    // 格子声明还在 → 仍然产出一条定义（画占位点），但把「没导出 cell」写进原因里。
    expect(pluginDefinitions.value).toHaveLength(1)
    expect(pluginLoadErrors.value['balance-board']).toContain('没导出 cell')
    clearPluginDefinitions()
  })

  it('清单校验失败的包沿用 Rust 给的中文原因', async () => {
    await syncPluginDefinitions(
      [record({ manifest: null, error: '清单缺少 entry（ESM 入口的包内路径，如 "main.js"）' })],
      { importEntry: importer({}) }
    )
    expect(pluginDefinitions.value).toEqual([])
    expect(pluginLoadErrors.value['balance-board']).toContain('清单缺少 entry')
  })

  it('插入块只在清单声明时收，未声明的导出被忽略', async () => {
    await syncPluginDefinitions(
      [
        record({ id: 'ci-timing', manifest: manifest({ id: 'ci-timing', surfaces: { extension: { into: 'agent-status', maxRows: 2 } } }) }),
        record({ id: 'quiet', manifest: manifest({ id: 'quiet', surfaces: {} }) }),
      ],
      {
        importEntry: importer({
          'ci-timing': ok({ extension: Stub }),
          quiet: ok({ extension: Stub }),
        }),
      }
    )
    expect(pluginExtensions.value).toHaveLength(1)
    expect(pluginExtensions.value[0]).toMatchObject({ hostId: 'agent-status', pluginId: 'ci-timing', maxRows: 2 })
    clearPluginDefinitions()
  })

  it('settings 里缺组件的脏条目只丢那一条', async () => {
    await syncPluginDefinitions(
      [record({ manifest: manifest({ surfaces: { settings: true } }) })],
      {
        importEntry: importer({
          'balance-board': ok({
            settings: [
              { id: 'general', label: '常规', component: Stub },
              // 脏条目：没有 component（作者从别处抄来的分区列表常见错法）。
              { id: 'broken', label: '坏的那页' } as unknown as FloatingModuleSection,
            ],
          }),
        }),
      }
    )
    expect(pluginDefinitions.value[0].settings?.sections.map(item => item.id)).toEqual(['general'])
    expect(pluginLoadErrors.value['balance-board']).toContain('已跳过那几条')
    clearPluginDefinitions()
  })

  it('重算会整体替换：卸掉的包不会留在表里', async () => {
    const importEntry = importer({ a: ok({ cell: Stub }), b: ok({ cell: Stub }) })
    await syncPluginDefinitions(
      [record({ id: 'a', manifest: manifest({ id: 'a' }) }), record({ id: 'b', manifest: manifest({ id: 'b' }) })],
      { importEntry }
    )
    expect(pluginDefinitions.value).toHaveLength(2)
    await syncPluginDefinitions([record({ id: 'a', manifest: manifest({ id: 'a' }) })], { importEntry })
    expect(pluginDefinitions.value.map(item => item.id)).toEqual(['a'])
    clearPluginDefinitions()
  })

  /**
   * ESM 按 URL 缓存模块：同一个入口 URL 第二次 `import()` 拿回的是同一份实例。
   * 所以「热重载」必须换串，而正式包必须不换（换了就每次都重新取一遍产物）。
   */
  it('开了热重载的包每次重挂都换缓存串，正式包不带串', async () => {
    const importEntry = importer({ 'balance-board': ok({ cell: Stub }) })
    await syncPluginDefinitions([record({ devReload: true })], { importEntry })
    const first = importEntry.mock.calls[0]?.[2]
    expect(first, 'dev 包要带缓存串').toBeTruthy()
    await syncPluginDefinitions([record({ devReload: true })], { importEntry })
    expect(importEntry.mock.calls[1]?.[2]).toBeTruthy()
    expect(importEntry.mock.calls[1]?.[2]).not.toBe(first)
    await syncPluginDefinitions([record()], { importEntry })
    expect(importEntry.mock.calls[2]?.[2]).toBeUndefined()
    clearPluginDefinitions()
  })

  it('装载耗时逐包上报（管理区那行的读数来源）', async () => {
    const reported: Array<[string, number]> = []
    await syncPluginDefinitions([record()], {
      importEntry: importer({ 'balance-board': ok({ cell: Stub }, 42) }),
      reportLoadMs: (id, loadMs) => reported.push([id, loadMs]),
    })
    expect(reported).toEqual([['balance-board', 42]])
    clearPluginDefinitions()
  })
})

/**
 * `host` 的绑定。
 *
 * 这是权限模型里最容易「看起来没事」的一环：漏包一层的症状不是报错，而是组件里
 * `inject` 到 undefined（作者写成 `host!.log()` 才炸），或者两个包共用一份 host
 * （A 包声明了 storage、B 包没有，B 却能用 A 的门禁 → 声明形同虚设）。
 */
describe('host 绑定到每个挂载点', () => {
  /** activate 收到的 host（每次装载重置）。 */
  let activated: PluginHost[] = []

  /** 一次同步里挂 N 条包，每包都声明四个挂载点 + activate。 */
  async function loadPackages(...specs: Array<{ id: string; permissions: string[] }>) {
    const records = specs.map(spec =>
      record({
        id: spec.id,
        manifest: manifest({
          id: spec.id,
          surfaces: {
            cell: 1,
            tooltip: { own: true, size: [300, 200] },
            extension: { into: 'agent-status', maxRows: 2 },
            settings: true,
          },
          permissions: spec.permissions,
        }),
      })
    )
    const byId: Record<string, PluginEntryResult> = {}
    for (const spec of specs) {
      byId[spec.id] = ok({
        cell: probe(`${spec.id}-cell`),
        tooltip: probe(`${spec.id}-tooltip`),
        extension: probe(`${spec.id}-extension`),
        settings: [{ id: 'general', label: '常规', component: probe(`${spec.id}-settings`) }],
        activate: host => {
          activated.push(host)
        },
      })
    }
    await syncPluginDefinitions(records, { importEntry: importer(byId) })
  }

  /** 按包 id 取出这条包的四个挂载点组件（`activate` 单独调）。 */
  function surfacesOf(id: string) {
    const definition = pluginDefinitions.value.find(item => item.id === id)
    return {
      cell: definition?.component,
      tooltip: definition?.tooltip?.component,
      extension: pluginExtensions.value.find(slot => slot.pluginId === id)?.component,
      settings: definition?.settings?.sections[0]?.component,
      activate: definition?.activate,
    }
  }

  beforeEach(() => {
    seen.clear()
    activated = []
  })

  it('格子 / 信息卡 / 插入块 / 设置页 / activate 都拿到自己那份 host', async () => {
    await loadPackages({ id: 'balance-board', permissions: ['storage'] })
    const surfaces = surfacesOf('balance-board')
    mount(surfaces.cell!)
    mount(surfaces.tooltip!)
    mount(surfaces.extension!)
    mount(surfaces.settings!)
    surfaces.activate?.()

    for (const name of ['cell', 'tooltip', 'extension', 'settings']) {
      expect(hostSeen(`balance-board-${name}`), `${name} 没拿到 host（外壳漏包了这一层）`).toBeTruthy()
      expect(hostSeen(`balance-board-${name}`)!.plugin.id).toBe('balance-board')
    }
    expect(activated).toHaveLength(1)
    expect(activated[0].plugin.permissions).toEqual(['storage'])
    clearPluginDefinitions()
  })

  it('一次同步里的两个包各拿自己的 host：权限不串门', async () => {
    await loadPackages({ id: 'balance-board', permissions: ['storage'] }, { id: 'ci-timing', permissions: ['http'] })
    mount(surfacesOf('balance-board').cell!)
    mount(surfacesOf('ci-timing').cell!)
    surfacesOf('balance-board').activate?.()
    surfacesOf('ci-timing').activate?.()

    expect(hostSeen('balance-board-cell')!.plugin.id).toBe('balance-board')
    expect(hostSeen('ci-timing-cell')!.plugin.id).toBe('ci-timing')
    // 各自的能力面来自各自的清单，不是「上一份的残留」。
    expect(hostSeen('balance-board-cell')!.plugin.permissions).toEqual(['storage'])
    expect(hostSeen('ci-timing-cell')!.plugin.permissions).toEqual(['http'])
    expect(activated.map(host => host.plugin.id)).toEqual(['balance-board', 'ci-timing'])
    clearPluginDefinitions()
  })

  it('组件透传外壳给的 attrs（包一层不能把 class 一类的属性吃掉）', async () => {
    await loadPackages({ id: 'balance-board', permissions: [] })
    // 未在组件上声明的属性会落进 attrs：包装层必须把它们交给内层，否则外壳的格子样式全丢。
    const wrapper = mount(surfacesOf('balance-board').cell!, { props: { 'data-cell': '1' } })
    expect(wrapper.attributes('data-cell')).toBe('1')
    clearPluginDefinitions()
  })
})
