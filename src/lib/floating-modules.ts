import type { Component } from 'vue'
import type { FloatingHoverTarget } from '@/lib/floating-module-context'
import { invoke } from '@tauri-apps/api/core'
import {
  ABS_MAX_CELLS,
  ABS_MIN_CELLS,
  DEFAULT_WINDOW_CELLS,
  clampCells,
  type FloatingLayout,
} from '@/lib/floating-layout'

/** 模块 id；除内置 `agent-status` 外都可自由扩展，只需在注册表里登记。 */
export type FloatingModuleId = 'agent-status' | (string & {})

/**
 * 悬浮窗模块定义：新增一个组件就是在注册表里加一条这个结构。
 * 它同时供悬浮窗（挂载 / 排序）与主窗口（组件选择列表）使用。
 */
export interface FloatingModuleDefinition {
  /** 模块唯一 id，同时是配置里的键；只允许字母 / 数字 / 短横线（后端会校验）。 */
  id: FloatingModuleId
  /** 主窗口组件选择列表里的名字。 */
  label: string
  /** 主窗口组件选择列表里的一句话说明。 */
  description: string
  /** 模块自身版本号，供将来做配置迁移。 */
  version: number
  /** 配置里没有该模块时是否默认启用（新模块首次出现按此兜底）。 */
  defaultEnabled: boolean
  /** 模块能正常显示的形态；宿主只在命中的形态下挂载它。 */
  supportedLayouts: FloatingLayout[]
  /** 模块视图组件；按 `layout-mode` prop 自行适配四种形态。 */
  component: Component
  /**
   * 该模块在给定形态下要占几格（纯函数，可以读 store；省略 = 恒占 1 格）。
   *
   * ⚠️ 只能依赖模块自己的数据与传入的形态，**不得**依赖宿主分给它多少格（会形成回路）。
   * 声明在定义里而不是组件里上报：宿主不挂载它也能算任意形态的窗口大小（三形态预览要用）。
   */
  demand?: (layout: FloatingLayout) => number
  /**
   * 组件自己的设置页（主窗口里的一页）：组件列表的齿轮、悬浮窗里点该组件都打开它。
   * 不给 = 该组件没有可配置项，列表里不显示齿轮。
   */
  settings?: FloatingModuleSettings
  /** 悬停信息卡；不给 = 悬停该模块不弹提示窗（如纯展示型模块）。 */
  tooltip?: FloatingModuleTooltip
}

/**
 * 组件设置页的一个分区：主窗口在组件页顶部的二级 tabbar 里列出来，点一下换内容。
 *
 * 分区是「一类配置一块」（如行为 / 窗口格子 / 信息卡拓展），不是子页面路由：
 * 只有组件自己知道该怎么切，主窗口只负责把 `sections` 渲染成 tab + 内容。
 */
export interface FloatingModuleSection {
  /** 分区 id，在所属组件内唯一即可（用作二级 tab 的 key） */
  id: string
  /** 二级 tab 上的文字 */
  label: string
  /** 分区内容组件 */
  component: Component
  /**
   * 该分区是否要**占满内容区高度**（默认 false = 内容纵向流动，滚外层）。
   *
   * 列表 / 编辑器这类分区（信息卡拓展、会话记录）要为 true：左右两列各自撑满、各自滚动，
   * 外层不滚 —— 否则高度链断掉，面板会退化成「有多高算多高 + 整页滚动」。
   */
  fill?: boolean
}

/**
 * 组件的设置页：由一个或多个分区组成。
 *
 * ⚠️ 只给一个分区时**不渲染二级 tabbar**（只有一个 tab 的 tabbar 是纯噪音）；
 * 分区顺序即 tab 顺序，第一个是打开该组件时的默认分区。
 */
export interface FloatingModuleSettings {
  /** 分区清单，顺序 = 二级 tab 顺序 */
  sections: FloatingModuleSection[]
}

/**
 * 模块的悬停信息卡：提示窗是独立窗口，由外壳按模块分派内容。
 * 宿主（提示窗）不认识任何模块语义，只负责「谁悬停了 → 渲染谁的组件」。
 */
export interface FloatingModuleTooltip {
  /**
   * 信息卡内容组件；容器（毛玻璃外壳 / 定位 / 显隐 / 尺寸）由提示窗提供，
   * 组件只渲染自己的行，用 flex + 省略号自适应给定尺寸。
   */
  component: Component
  /**
   * 该组件希望的提示窗尺寸（物理像素）；省略 = 用 tauri.conf.json 里的默认尺寸。
   * 提示窗在显示前按它 `setSize`，所以不同组件可以有不同高度的信息卡。
   */
  size?: { width: number; height: number }
  /**
   * 该悬停目标此刻是否有内容可展示（如会话数据已到达本窗口）。
   * 省略 = 只要有悬停目标就弹窗；返回 false 时不弹，避免先弹一个空框再填内容。
   */
  hasContent?: (target: FloatingHoverTarget) => boolean
}

/** 模块在配置里的开关与排序。 */
export interface FloatingModuleConfigEntry {
  id: string
  enabled: boolean
  order: number
}

/** 该组件是否声明了设置页（主窗口据此决定要不要在列表里给个齿轮）。 */
export function hasModuleSettings(definition: FloatingModuleDefinition): boolean {
  return (definition.settings?.sections.length ?? 0) > 0
}

/** 四种形态的固定顺序；遍历形态（预览 / 汇总）时用它，避免各处各写一遍。 */
export const FLOATING_LAYOUTS: readonly FloatingLayout[] = ['square', 'corner', 'horizontal', 'vertical']

/** 各贴边形态的格子上限（4..=8，用户可配）。 */
export interface MaxCellsByLayout {
  square: number
  corner: number
  horizontal: number
  vertical: number
}

/**
 * token 展示单位，与 Rust 侧 `TokenUnit` 的序列化值一一对应。
 * - `tokens`：原始计数，如 `1,261,847`
 * - `m_tokens`：以百万为单位，如 `1.26M`
 */
export type TokenUnit = 'tokens' | 'm_tokens'

/** token 单位默认值：保持原始计数，与加入该设置之前的行为一致。 */
export const DEFAULT_TOKEN_UNIT: TokenUnit = 'tokens'

/** 代理解析器下拉项：id 存进配置并作 value，label 供 UI 展示。与 Rust `ProxyResolverInfo` 对应。 */
export interface ProxyResolverInfo {
  id: string
  label: string
}

/** 不解析（直连）的固定 id，与 Rust `NONE_RESOLVER` 一致。 */
export const NONE_PROXY_RESOLVER = 'none'

export interface FloatingModuleConfig {
  schema_version: number
  revision: number
  modules: FloatingModuleConfigEntry[]
  completed_retention_ms: number
  /** Running 无新 hook 时的超时（毫秒）；超时后移除绿灯。 */
  running_timeout_ms: number
  max_cells_by_layout: MaxCellsByLayout
  /** token 展示单位；主窗口可切换。 */
  token_unit: TokenUnit
  /** 选中的代理解析器 id；`none` = 不解析。 */
  proxy_resolver: string
}

/** 四形态同值的格子上限；非法取值夹到 [ABS_MIN_CELLS, ABS_MAX_CELLS]。 */
export function defaultMaxCellsByLayout(value: number = ABS_MAX_CELLS): MaxCellsByLayout {
  const cells = clampCells(value, ABS_MIN_CELLS, ABS_MAX_CELLS)
  return {
    square: cells,
    corner: cells,
    horizontal: cells,
    vertical: cells,
  }
}

/** 单个模块的格子权重：至少 1 格；非有限值（NaN / Infinity）按 1 格算。 */
export function moduleCellWeight(demand: number): number {
  return Number.isFinite(demand) ? Math.max(1, Math.floor(demand)) : 1
}

/** 问某个模块在给定形态下要几格；定义里没写 `demand` 时按 1 格算（它总要占地方）。 */
export function moduleDemand(definition: FloatingModuleDefinition, layout: FloatingLayout): number {
  return moduleCellWeight(definition.demand?.(layout) ?? 1)
}

/**
 * 汇总各模块的格子需求 → 悬浮窗应采用的格子数。
 *
 * 各模块同级共享同一个窗口，所以需求相加：横条按宽度、竖条按高度、方形/四角按网格面积。
 * 结果夹在 [默认格子数, 该形态上限]；到顶后窗口不再变大，各模块按需求比例等分收缩
 * （模块要自行处理「分到的比需求的少」，如状态灯退化成 +N 溢出格）。
 */
export function resolveWindowCells(demands: readonly number[], layoutMax: number): number {
  const max = clampCells(layoutMax, ABS_MIN_CELLS, ABS_MAX_CELLS)
  const total = demands.reduce((sum, demand) => sum + moduleCellWeight(demand), 0)
  return clampCells(total, DEFAULT_WINDOW_CELLS, max)
}

/** 各模块的空间权重（与需求等比的整数），供宿主用 grid 的 fr 单位分槽。 */
export function moduleCellShares(demands: readonly number[]): number[] {
  return demands.map(moduleCellWeight)
}

/**
 * 某个模块实际分到的格子数：按权重切分窗口总数，四舍五入且至少 1 格。
 * 无权重信息时退回「整个窗口都给它」。
 */
export function allocatedCells(windowCells: number, shares: readonly number[], index: number): number {
  const total = shares.reduce((sum, share) => sum + share, 0)
  if (total <= 0) return Math.max(1, Math.floor(windowCells))
  const share = shares[index] ?? 0
  return Math.max(1, Math.round((windowCells * share) / total))
}

/**
 * 判断模块是否启用；配置里没有该模块时回落到定义里的默认值，
 * 这样新增模块不需要迁移配置就能按默认状态出现。
 */
export function isModuleEnabled(config: FloatingModuleConfig | null, id: string, defaultEnabled = false): boolean {
  return config?.modules.find(module => module.id === id)?.enabled ?? defaultEnabled
}

/** 向后端拉取可用的代理解析器列表（含 `none`）。 */
export function fetchProxyResolvers(): Promise<ProxyResolverInfo[]> {
  return invoke<ProxyResolverInfo[]>('get_proxy_resolvers')
}

/** 把 id 列表里的 `from` 项移动到 `to` 位置，返回新数组（越界自动夹住）。 */
export function reorderIds(ids: readonly string[], from: number, to: number): string[] {
  const next = [...ids]
  if (from < 0 || from >= next.length) return next
  const target = Math.min(Math.max(to, 0), next.length - 1)
  if (target === from) return next
  const [moved] = next.splice(from, 1)
  next.splice(target, 0, moved)
  return next
}

/**
 * 按给定 id 顺序重排配置项：列表里的按新序号写 order，
 * 未列出的（配置里有、注册表里没有的旧模块）保持相对顺序排到后面。
 */
export function applyModuleOrder(
  entries: readonly FloatingModuleConfigEntry[],
  orderedIds: readonly string[]
): FloatingModuleConfigEntry[] {
  const orderOf = new Map(orderedIds.map((id, index) => [id, index]))
  return entries.map((entry, index) => ({
    ...entry,
    order: orderOf.get(entry.id) ?? orderedIds.length + index,
  }))
}

/** 某个形态下的运行时快照。 */
export interface FloatingModuleLayoutRuntime {
  /** moduleId → 该形态下这个模块要的格子数 */
  demands: Record<string, number>
  /** 该形态下窗口的总格子数（各模块需求之和，夹在默认值与形态上限之间） */
  windowCells: number
}

/** 各形态的运行时快照：悬浮窗广播给主窗口，供组件列表徽标与三形态预览使用。 */
export interface FloatingRuntimeSnapshot {
  /** 悬浮窗此刻的形态（列表徽标按它显示各组件当下占几格）。 */
  layout: FloatingLayout
  byLayout: Record<FloatingLayout, FloatingModuleLayoutRuntime>
}

/**
 * 算出各形态下的运行时快照（纯函数，主窗口预览与悬浮窗广播共用同一份口径）。
 *
 * 与宿主挂载时的算法一致：先按形态筛出该挂载的模块，再取各自的格子需求求和、夹进上限。
 * 需求按形态分别求值（`demandFor(id, layout)`），因此各形态上限不同时预览也是准的。
 */
export function runtimeSnapshot(
  current: FloatingLayout,
  config: FloatingModuleConfig | null,
  definitions: readonly FloatingModuleDefinition[],
  layoutMaxFor: (layout: FloatingLayout) => number
): FloatingRuntimeSnapshot {
  const byLayout = {} as Record<FloatingLayout, FloatingModuleLayoutRuntime>
  for (const layout of FLOATING_LAYOUTS) {
    const active = enabledModuleDefinitions(config, definitions, layout)
    const demands: Record<string, number> = {}
    const values: number[] = []
    for (const definition of active) {
      const cells = moduleDemand(definition, layout)
      demands[definition.id] = cells
      values.push(cells)
    }
    byLayout[layout] = { demands, windowCells: resolveWindowCells(values, layoutMaxFor(layout)) }
  }
  return { layout: current, byLayout }
}

/**
 * 按配置里的 order 排序模块（order 相同按 id 字典序）；配置里还没有该模块的排到最后。
 * 组件列表（含未启用的）与宿主都用它，保证「列表里的先后」与「悬浮窗里的先后」一致。
 */
export function orderedModuleDefinitions(
  config: FloatingModuleConfig | null,
  definitions: readonly FloatingModuleDefinition[]
): FloatingModuleDefinition[] {
  const orderOf = (id: string) =>
    config?.modules.find(module => module.id === id)?.order ?? Number.MAX_SAFE_INTEGER
  return [...definitions].sort(
    (left, right) => orderOf(left.id) - orderOf(right.id) || left.id.localeCompare(right.id)
  )
}

/**
 * 过滤出此刻该挂载的模块：已启用、且支持当前形态（给了 layout 时），并按配置里的 order 排序。
 * order 相同时按 id 字典序，保证顺序稳定。
 */
export function enabledModuleDefinitions(
  config: FloatingModuleConfig | null,
  definitions: readonly FloatingModuleDefinition[],
  layout?: FloatingLayout
): FloatingModuleDefinition[] {
  return orderedModuleDefinitions(config, definitions)
    .filter(definition => isModuleEnabled(config, definition.id, definition.defaultEnabled))
    .filter(definition => !layout || definition.supportedLayouts.includes(layout))
}
