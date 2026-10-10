/**
 * 合并注册表：台账里的包 → 外壳认识的 `FloatingModuleDefinition`。
 *
 * 三条规矩：
 * 1. **装载失败也要留下读数**：某一条挂不上只让它自己降级（记进 `pluginLoadErrors`），
 *    其余包照常挂 —— 一个坏包不能让悬浮窗整块空掉。
 * 2. **清单没声明的挂载点不去取**：作者没声明 `surfaces.cell` 就不会占格子，
 *    哪怕入口导出了 `cell` 也忽略（声明是承诺，导出是实现，外壳只按承诺分配空间）。
 * 3. **顺序 = 台账顺序**，与内置模块的先后由配置的 `order` 统一排（外壳负责）。
 */

import { defineComponent, h, provide, shallowRef, type Component } from 'vue'
import {
  FLOATING_LAYOUTS,
  type FloatingModuleDefinition,
  type FloatingModuleSection,
} from '@/lib/floating-modules'
import type { PluginRecordJson } from '@/lib/plugin-types'
import type { FloatingHoverTarget } from '@/lib/floating-module-context'
import { createPluginHost, PLUGIN_HOST_KEY, type PluginHost } from '@/lib/plugin-host'
import { importPluginEntry, type PluginEntryResult, type PluginUiEntry } from '@/lib/plugin-runtime'

/** 一条「插进别人信息卡」的挂载位（这条通道不在 FloatingModuleDefinition 里，见 guide 的插入块一节）。 */
export interface PluginExtensionSlot {
  /** 目标组件 id（清单 `surfaces.extension.into`）。 */
  hostId: string
  /** 插件自己的 id（来源标注与卸载时摘掉自己那一块要用）。 */
  pluginId: string
  /** 来源标注上显示的名字。 */
  label: string
  /** 追加那块的内容组件。 */
  component: Component
  /** 最多贡献几行（宿主夹到 3）。 */
  maxRows: number
  /** 此刻有没有内容可展示；不给 = 有悬停目标就出现（与独立信息卡同一条规矩）。 */
  hasContent?: (target: unknown) => boolean
}

/** 此刻该挂进窗口的插件定义。 */
export const pluginDefinitions = shallowRef<FloatingModuleDefinition[]>([])

/** 此刻该往各宿主信息卡里追加的块。 */
export const pluginExtensions = shallowRef<PluginExtensionSlot[]>([])

/** 每个插件最近一次装载失败的原因（管理区那一行直接渲染它）。 */
export const pluginLoadErrors = shallowRef<Record<string, string>>({})

/** 本窗口里同步过几次（开发期热重载的缓存串就从它来，见 `syncPluginDefinitions`）。 */
let devSyncSequence = 0

/** 注入点：单测与 bench 走不同的取入口实现。 */
export interface PluginLoaderDeps {
  /** 取并执行入口（`version` 是开发期热重载的缓存击穿串，正式包为 undefined）。 */
  importEntry?: (id: string, entry: string, version?: string) => Promise<PluginEntryResult>
  /** 上报装载耗时（读台账用，不参与渲染）。 */
  reportLoadMs?: (id: string, loadMs: number) => void
}

/** 没声明格子的插件用的占位视图：不画任何东西，但必须是个组件（外壳按 `component` 挂载）。 */
const HiddenSurface = defineComponent({
  name: 'PluginHiddenSurface',
  setup: () => () => null,
})

/**
 * 给组件套一层「这个包的 host」作用域。
 *
 * 组件里用 `inject(PLUGIN_HOST_KEY)` 取；外壳的三个挂载点（格子 / 信息卡 / 设置页）
 * 都只按注册表渲染，不知道也不需要知道 host 是谁的 —— 所以绑定必须发生在这一层。
 */
function withPluginHost(component: Component, host: PluginHost): Component {
  return defineComponent({
    name: 'PluginHostScope',
    // 关掉自动透传：下面手动把 attrs 交给内层组件，避免出现「外层一份、内层一份」。
    inheritAttrs: false,
    setup(_props, { attrs, slots }) {
      provide(PLUGIN_HOST_KEY, host)
      return () => h(component, attrs, slots)
    },
  })
}

/** 声明了格子却没导出 `cell` 时的降级视图：一格里只放一个灰点，点击仍可开主窗。 */
const MissingCell = defineComponent({
  name: 'PluginMissingCell',
  setup: () => () => h('span', { class: 'plugin-cell plugin-cell--missing', 'aria-hidden': 'true' }, '·'),
})

/** 该包申请占几格；没声明格子就是 1（它总要占地方，`demand` 返回 0 不等于不占）。 */
function demandOf(cells: number) {
  return () => cells
}

/**
 * 把「台账记录 + 入口导出」拼成一条外壳定义。
 *
 * 返回 `null` = 这个包一个可用挂载点都没有（不挂进任何窗口，原因写进 `pluginLoadErrors`）。
 *
 * `host` 在这一层绑到每个挂载点的组件上：外壳只按注册表渲染，不知道 host 的存在，
 * 所以「这个组件属于哪个包」这件事必须由这里决定 —— 决定了它调命令时能用到哪些权限。
 */
export function buildPluginDefinition(
  record: PluginRecordJson,
  entry: PluginUiEntry,
  notes: string[],
  host: PluginHost
): FloatingModuleDefinition | null {
  const manifest = record.manifest
  if (!manifest) return null
  const surfaces = manifest.surfaces

  const cells = surfaces.cell ?? null
  const wantsCell = cells != null
  const wantsTooltip = surfaces.tooltip?.own === true
  const wantsExtension = surfaces.extension != null
  const wantsSettings = surfaces.settings === true

  const component = wantsCell ? withPluginHost(entry.cell ?? MissingCell, host) : HiddenSurface
  if (wantsCell && !entry.cell) {
    notes.push('清单申请了格子，但入口没导出 cell 组件，这一格只画占位点')
  }

  const tooltip =
    wantsTooltip && entry.tooltip
      ? {
          component: withPluginHost(entry.tooltip, host),
          size: entry.tooltipSize,
          // 没给 `hasContent` 时按「有悬停目标就有内容」处理（与内置模块同一口径）。
          hasContent: entry.hasContent ? (target: FloatingHoverTarget) => entry.hasContent!(target) : undefined,
        }
      : undefined
  if (wantsTooltip && !entry.tooltip) {
    notes.push('清单声明了独立信息卡，但入口没导出 tooltip 组件，悬停不弹卡')
  }

  const settings = buildSections(entry, wantsSettings, notes, host)

  if (!component || (!tooltip && !settings && !wantsCell && !wantsExtension)) {
    return null
  }

  return {
    id: record.id,
    label: record.name || record.id,
    description: manifest.description || record.version,
    version: 1,
    defaultEnabled: true,
    supportedLayouts: [...FLOATING_LAYOUTS],
    component,
    demand: wantsCell ? demandOf(cells) : undefined,
    settings,
    tooltip,
    // 生命周期挂在定义上：外壳在每个挂模块的窗口调一次，插件自己订阅自己的数据。
    // host 由这里传进去 —— 作者不必 inject 就能拿到，订阅与日志在挂载前就要能用。
    activate: entry.activate ? () => entry.activate!(host) : undefined,
  }
}

/** 设置页分区：只在清单声明了 `settings` 时采纳，并滤掉缺组件的脏条目。 */
function buildSections(
  entry: PluginUiEntry,
  declared: boolean,
  notes: string[],
  host: PluginHost
): FloatingModuleDefinition['settings'] | undefined {
  if (!declared) return undefined
  const given = entry.settings ?? []
  if (given.length === 0) {
    notes.push('清单声明了设置页，但入口没导出 settings 分区')
    return undefined
  }
  const sections = given.filter(
    (section: FloatingModuleSection) => section && typeof section.id === 'string' && !!section.component
  )
  if (sections.length !== given.length) {
    notes.push('settings 里有缺 id 或缺组件的分区，已跳过那几条')
  }
  if (sections.length === 0) return undefined
  // 展开保留作者给的其余字段（label / fill），只换掉 component 那一层绑定。
  return {
    sections: sections.map(section => ({ ...section, component: withPluginHost(section.component, host) })),
  }
}

/** 从记录里取插入块挂载位（入口没导出 `extension` 就一条也不加）。 */
function collectExtension(
  record: PluginRecordJson,
  entry: PluginUiEntry,
  slots: PluginExtensionSlot[],
  notes: string[],
  host: PluginHost
) {
  const manifest = record.manifest
  const declared = manifest?.surfaces.extension
  if (!declared) return
  if (!entry.extension) {
    notes.push('清单声明了插入块，但入口没导出 extension 组件')
    return
  }
  slots.push({
    hostId: declared.into,
    pluginId: record.id,
    label: record.name || record.id,
    component: withPluginHost(entry.extension, host),
    maxRows: declared.maxRows,
    hasContent: entry.extensionHasContent ? target => entry.extensionHasContent!(target) : undefined,
  })
}

/**
 * 重算全部插件定义（安装 / 卸载 / 开关 / 热重载之后都要调一次）。
 *
 * ⚠️ 入口是异步的：本函数会 await 全部完成再整体替换数组，
 * 因此界面在「换血」那一刻只看到一份完整清单，不会出现半新半旧的格子账。
 */
export async function syncPluginDefinitions(
  records: readonly PluginRecordJson[],
  deps: PluginLoaderDeps = {}
): Promise<void> {
  const importEntry = deps.importEntry ?? importPluginEntry
  const candidates = records.filter(record => record.manifest)
  const definitions: FloatingModuleDefinition[] = []
  const slots: PluginExtensionSlot[] = []
  const errors: Record<string, string> = {}
  // 开发期热重载的缓存击穿串：一次同步换一个值，正式包恒为 undefined。
  // ESM 按 URL 缓存模块，不带 `?v=` 重 import 拿回来的还是旧的那份；
  // 用「第几次同步」而不是时间戳，是因为同一次点两下重载可能落在同一毫秒里，
  // 而 URL 只要在本窗口内不重样就够（协议那头本来就是 no-store）。
  const bust = (++devSyncSequence).toString(36)
  const versionOf = (record: PluginRecordJson) => (record.devReload ? bust : undefined)

  for (const record of candidates) {
    const notes: string[] = []
    const result = await importEntry(record.id, record.manifest!.entry, versionOf(record))
    if (!result.ok) {
      errors[record.id] = result.reason
      deps.reportLoadMs?.(record.id, result.loadMs)
      continue
    }
    deps.reportLoadMs?.(record.id, result.loadMs)
    // 一个包一个 host：权限闸门按 pluginId 判，所以 host 绝不能跨包复用。
    const host = createPluginHost(record)
    collectExtension(record, result.entry, slots, notes, host)
    const definition = buildPluginDefinition(record, result.entry, notes, host)
    if (!definition) {
      errors[record.id] = notes[0] ?? '没有任何可用挂载点'
      continue
    }
    if (notes.length > 0) errors[record.id] = notes.join('；')
    definitions.push(definition)
  }

  // 校验失败（manifest 为 null）的包不尝试装载：Rust 已经给出了中文原因，直接沿用。
  for (const record of records) {
    if (!record.manifest && !errors[record.id]) errors[record.id] = record.error ?? '清单校验失败'
  }

  pluginDefinitions.value = definitions
  pluginExtensions.value = slots
  pluginLoadErrors.value = errors
}

/** 清干净（所有包都卸掉之后）。 */
export function clearPluginDefinitions() {
  pluginDefinitions.value = []
  pluginExtensions.value = []
  pluginLoadErrors.value = {}
}
