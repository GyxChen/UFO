<script setup lang="ts">
import { computed, watch } from 'vue'
import FloatingModuleSlot from './FloatingModuleSlot.vue'
import { FLOATING_MODULE_DEFINITIONS } from '@/modules'
import type { FloatingHoverTarget } from '@/lib/floating-module-context'
import { FLOATING_BASE_CELL_PX, ABS_MAX_CELLS, cellsGrid, type FloatingLayout } from '@/lib/floating-layout'
import {
  allocatedCells,
  enabledModuleDefinitions,
  moduleCellShares,
  moduleDemand,
  resolveWindowCells,
  runtimeSnapshot,
  type FloatingModuleDefinition,
  type FloatingRuntimeSnapshot,
} from '@/lib/floating-modules'
import { useFloatingModulesStore } from '@/stores/floatingModules'

/**
 * 悬浮窗模块宿主：挂载所有启用的模块，并把它们的格子需求汇总成窗口尺寸。
 *
 * 各模块同级共享同一块窗口：宿主按需求比例用 grid 的 fr 分槽
 * （横条左右并排，竖条 / 方形 / 四角上下堆叠），窗口总格子数 = 各模块需求之和，
 * 夹在默认格子数与该形态上限之间；模块之间不需要知道彼此存在。
 *
 * 模块的格子需求声明在定义里（`demand(layout)`），因此宿主既能算当前形态的窗口尺寸，
 * 也能算出全部形态的快照（`runtime`）交给外壳广播给主窗口做三形态预览 ——
 * 后者不需要挂载模块就能求值，被 `supportedLayouts` 排除的形态也算得准。
 */
const props = withDefaults(
  defineProps<{
    /** 当前悬浮窗形态，由外壳按贴边状态推出 */
    layoutMode: FloatingLayout
    /** 可挂载的模块清单；不给就取注册表，测试可注入替身 */
    definitions?: FloatingModuleDefinition[]
  }>(),
  { definitions: undefined }
)

/**
 * 此刻有哪些模块可挂：不给替身就取注册表。
 *
 * ⚠️ 这里必须是 `computed`，不能写成 prop 的默认值工厂：Vue 只在 setup 里求一次默认值，
 * 而组件包是**异步**装载的 —— 悬浮窗挂载那一刻注册表里只有内置模块，
 * 之后整份换掉的清单再也进不来（表现：包装上了、徽标也齐，但悬浮窗就是少一格）。
 */
const definitions = computed(() => props.definitions ?? FLOATING_MODULE_DEFINITIONS.value)

const emit = defineEmits<{
  /** 汇总后的窗口格子数（当前形态） */
  cells: [cells: number]
  /** 各形态的运行时快照（各模块需求 + 窗口总格子数） */
  runtime: [snapshot: FloatingRuntimeSnapshot]
  /** 模块上报的悬停目标；null 表示离开 */
  hover: [target: FloatingHoverTarget | null]
}>()

const modules = useFloatingModulesStore()

/** 此刻该挂载的模块：已启用 + 支持当前形态，按配置里的 order 排列。 */
const activeModules = computed(() => enabledModuleDefinitions(modules.config, definitions.value, props.layoutMode))
/** 当前形态下各模块的需求，供分槽与窗口尺寸共用（一次求值，避免重复调用需求函数）。 */
const currentDemands = computed(() =>
  activeModules.value.map(definition => moduleDemand(definition, props.layoutMode))
)
/** 每个模块的空间权重（整数，至少 1 格），用于 grid 分槽。 */
const weights = computed(() => moduleCellShares(currentDemands.value))
/** 窗口格子数：所有模块需求之和，夹在 [1, 绝对上限 ABS_MAX_CELLS] 之间（各模块自行按配置上限需求）。 */
const windowCells = computed(() => resolveWindowCells(currentDemands.value, ABS_MAX_CELLS))
/** 每个模块实际分到的格子数，按权重切分窗口总数。 */
const allocations = computed(() => weights.value.map((_, index) => allocatedCells(windowCells.value, weights.value, index)))
/** 全部形态的运行时快照：给主窗口做组件徽标与三形态预览（含当前形态，便于列表标注）。 */
const runtime = computed(() =>
  runtimeSnapshot(props.layoutMode, modules.config, definitions.value, () => ABS_MAX_CELLS)
)

/**
 * 一格尺寸的 CSS 变量：宿主声明在根节点上，模块继承使用。
 *
 * 「格子大小」是用户可调的（配置 `cell_px`），所以模块里写死的**设计像素**（字号、描边、间隙、
 * 图标上限……）不能就那么冻着 —— 格子调大后槽位变大、百分比尺寸跟着长，写死的那部分却停住，
 * 比例就散了。约定是模块写成 `calc(<设计像素> * var(--floating-cell-scale, 1))`：
 *
 * - `--floating-cell-px`：一格实际多少像素（原始值，便于将来按格数排版）
 * - `--floating-cell-scale`：相对设计基准 `FLOATING_BASE_CELL_PX` 的倍率，1 = 设计尺寸
 *
 * 变量缺失时模块回落 1，即设计尺寸（与「没有槽位容器时 `cqh` 退化、clamp 取到设计上限」一致）。
 */
const cellVars = computed(() => ({
  '--floating-cell-px': `${modules.cellPx}px`,
  // 保留三位小数就够了：CSS 里只用来乘设计像素，多余的精度只会把 DOM 撑长。
  '--floating-cell-scale': String(Math.round((modules.cellPx / FLOATING_BASE_CELL_PX) * 1000) / 1000),
}))

/**
 * 分槽权重：按权重铺 fr 轨道。
 * 横条沿宽度分槽（模块左右并排）；竖条 / 方形 / 四角沿高度分槽（模块上下堆叠）。
 *
 * ⚠️ 四角逐个模块按**它自己占几行**分槽（`cellsGrid('corner', demand).rows`），而不是原始
 * 需求（面积）：四角里每个模块各占满一整行，行高应正比于它要几行。需求 = 行数 × 列数，
 * 只有一个模块更宽（列更多）时用需求才会把行高算胖（常见情形各模块同宽，两者等价）。
 */
const slotShares = computed(() =>
  props.layoutMode === 'corner'
    ? currentDemands.value.map(demand => cellsGrid('corner', demand).rows)
    : weights.value
)

const slotStyle = computed(() => {
  const tracks = slotShares.value.map(share => `${share}fr`).join(' ')
  return props.layoutMode === 'horizontal' ? { gridTemplateColumns: tracks } : { gridTemplateRows: tracks }
})


// 窗口尺寸变化时通知外壳，由外壳负责 clamp 与贴边重算。
watch(windowCells, cells => emit('cells', cells), { immediate: true })
// 需求 / 形态 / 配置变化时重算快照，交给外壳广播（主窗口可能没开着，广播方不关心）。
watch(runtime, snapshot => emit('runtime', snapshot), { immediate: true })
</script>

<template>
  <div class="floating-module-host" :style="[slotStyle, cellVars]">
    <FloatingModuleSlot
      v-for="(definition, index) in activeModules"
      :key="definition.id"
      :definition="definition"
      :layout-mode="props.layoutMode"
      :allocated="allocations[index] ?? 1"

      @hover="emit('hover', $event)"
    />
  </div>
</template>