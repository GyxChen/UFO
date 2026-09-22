import type { Component } from 'vue'
import type { FloatingHoverTarget } from '@/lib/floating-module-context'
import {
  ABS_MAX_CELLS,
  ABS_MIN_CELLS,
  FLOATING_BASE_CELL_PX,
  clampCellPx,
  clampCells,
  type FloatingLayout,
} from '@/lib/floating-layout'

/**
 * 模块 id：只允许字母 / 数字 / 短横线（后端会校验），在注册表里登记即生效。
 * 刻意不写成「已知 id 的联合类型」—— 外壳的类型层不该点名任何具体模块。
 */
export type FloatingModuleId = string

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
  /**
   * 模块的生命周期：外壳在每个窗口挂载时调用一次（悬浮窗 / 主窗 / 提示窗都调）。
   *
   * 模块自己在这里订阅跨窗口数据（`listen`）、启动它自己的 store —— **外壳不该知道**
   * 任何模块的数据源，所以「谁需要先 start」这件事必须写在模块自己身上。
   * 返回一个清理函数（可选），窗口卸载时调用（外壳按逆序执行）。
   *
   * ⚠️ 必须挂在这里而不是组件 `onMounted`：提示窗的 `tooltip.hasContent` 在内容组件挂载
   * **之前**就要问「有没有内容」，数据晚一步到就永远弹不出来（鸡生蛋死锁）。
   * 需要按窗口区分行为时，模块自己去读 `getCurrentWindow().label`。
   */
  activate?: () => void | (() => void)
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
   * 列表 / 编辑器这类分区（左右两列各自撑满、各自滚动）要为 true：
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

/** 信息卡窗口的尺寸（物理像素）。 */
export interface FloatingTooltipSize {
  width: number
  height: number
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
   *
   * 也可以给**按悬停目标算的函数**：内容行数随目标变化时（如折叠清单里有几个会话），
   * 窗口要跟着长高才装得下全部行。外壳只负责取值、并把结果夹进工作区，
   * 不认识里面是按什么算的（见 `resolveTooltipSize` / `clampTooltipSize`）。
   */
  size?: FloatingTooltipSize | ((target: FloatingHoverTarget) => FloatingTooltipSize)
  /**
   * 该悬停目标此刻是否有内容可展示（如该模块的数据已到达本窗口）。
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

/**
 * 启动注册表里所有模块的生命周期（外壳在每个窗口挂载时调一次）。
 *
 * 返回值是「停掉全部」的函数：按启动的**逆序**调用各模块交回来的清理函数。
 * 单个模块抛错不影响其它模块（外壳只警告，不让一个坏模块带崩整窗）。
 */
export function activateModules(definitions: readonly FloatingModuleDefinition[]): () => void {
  const disposers: (() => void)[] = []
  for (const definition of definitions) {
    try {
      const dispose = definition.activate?.()
      if (typeof dispose === 'function') {
        disposers.push(dispose)
      }
    } catch (error) {
      console.warn(`failed to activate floating module ${definition.id}`, error)
    }
  }
  return () => {
    for (const dispose of disposers.reverse()) {
      try {
        dispose()
      } catch (error) {
        console.warn('failed to dispose floating module', error)
      }
    }
  }
}

/** 四种形态的固定顺序；遍历形态（预览 / 汇总）时用它，避免各处各写一遍。 */
export const FLOATING_LAYOUTS: readonly FloatingLayout[] = ['square', 'corner', 'horizontal', 'vertical']

export interface FloatingModuleConfig {
  schema_version: number
  revision: number
  modules: FloatingModuleConfigEntry[]
  /**
   * 一格的边长（物理像素）：整个悬浮窗的尺寸基准，同级组件共享，因此由外壳解释。
   *
   * 可选是有意的 —— 旧配置文件里没有这个键（Rust 侧 `#[serde(default)]` 兜底），
   * 所以读的时候一律走 `configCellPx()`，不要直接取字段。
   */
  cell_px?: number
  /**
   * 各组件自己的配置块，键 = 组件 id，值 = 该组件自己解释的对象。
   *
   * 外壳**只负责原样存取**，不认识里面任何字段 —— 所以新增组件的配置项
   * 不需要改外壳（也不需要迁移配置：读不到键就回落组件自己的默认值）。
   */
  module_settings: Record<string, unknown>
}

/**
 * 配置里的一格边长（物理像素）；没配过或取值非法时回落到 `FLOATING_BASE_CELL_PX`。
 * 外壳只在这里把配置翻译成能用的数。
 */
export function configCellPx(config: FloatingModuleConfig | null): number {
  return clampCellPx(config?.cell_px ?? FLOATING_BASE_CELL_PX)
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
 * 结果夹在 [1, 该形态上限]；到顶后窗口不再变大，各模块按需求比例等分收缩
 * （模块要自行处理「分到的比需求的少」，如退化成「+N」溢出格）。
 */
export function resolveWindowCells(demands: readonly number[], layoutMax: number): number {
  const max = clampCells(layoutMax, ABS_MIN_CELLS, ABS_MAX_CELLS)
  const total = demands.reduce((sum, demand) => sum + moduleCellWeight(demand), 0)
  // 地板用 ABS_MIN_CELLS(1) 而非默认容量：边条要能按会话个数缩到 1 格（用户要「按个数」）。
  return clampCells(total, ABS_MIN_CELLS, max)
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
