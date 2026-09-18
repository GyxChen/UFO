<script setup lang="ts">
import { computed, watch } from 'vue'
import FloatingModuleSlot from './FloatingModuleSlot.vue'
import { FLOATING_MODULE_DEFINITIONS } from './modules'
import type { FloatingHoverTarget } from '@/lib/floating-module-context'
import type { FloatingLayout } from '@/lib/floating-layout'
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
    /** 可挂载的模块清单；默认取注册表，测试可注入替身 */
    definitions?: FloatingModuleDefinition[]
  }>(),
  { definitions: () => FLOATING_MODULE_DEFINITIONS }
)

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
const activeModules = computed(() => enabledModuleDefinitions(modules.config, props.definitions, props.layoutMode))
/** 当前形态下各模块的需求，供分槽与窗口尺寸共用（一次求值，避免重复调用需求函数）。 */
const currentDemands = computed(() =>
  activeModules.value.map(definition => moduleDemand(definition, props.layoutMode))
)
/** 每个模块的空间权重（整数，至少 1 格），用于 grid 分槽。 */
const weights = computed(() => moduleCellShares(currentDemands.value))
/** 窗口格子数：所有模块需求之和，夹在默认格子数与该形态上限之间。 */
const windowCells = computed(() => resolveWindowCells(currentDemands.value, modules.maxCellsFor(props.layoutMode)))
/** 每个模块实际分到的格子数，按权重切分窗口总数。 */
const allocations = computed(() => weights.value.map((_, index) => allocatedCells(windowCells.value, weights.value, index)))
/** 全部形态的运行时快照：给主窗口做组件徽标与三形态预览（含当前形态，便于列表标注）。 */
const runtime = computed(() =>
  runtimeSnapshot(props.layoutMode, modules.config, props.definitions, layout => modules.maxCellsFor(layout))
)

/**
 * 分槽样式：按权重铺 fr 轨道。
 * 横条沿宽度分槽（模块左右并排）；竖条 / 方形 / 四角沿高度分槽（模块上下堆叠）。
 */
const slotStyle = computed(() => {
  const tracks = weights.value.map(weight => `${weight}fr`).join(' ')
  return props.layoutMode === 'horizontal' ? { gridTemplateColumns: tracks } : { gridTemplateRows: tracks }
})


// 窗口尺寸变化时通知外壳，由外壳负责 clamp 与贴边重算。
watch(windowCells, cells => emit('cells', cells), { immediate: true })
// 需求 / 形态 / 配置变化时重算快照，交给外壳广播（主窗口可能没开着，广播方不关心）。
watch(runtime, snapshot => emit('runtime', snapshot), { immediate: true })
</script>

<template>
  <div class="floating-module-host" :style="slotStyle">
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