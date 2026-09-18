<script setup lang="ts">
/**
 * 三形态实时预览：把运行时快照画成悬浮窗在「方形·四角 / 横条 / 竖条」下的真实比例。
 *
 * 尺寸口径与真窗口完全一致（`floatingWindowSize` + 与 snap.ts 同源的格子倍率），
 * 只是整体等比缩放到卡片里显示；每个色块的宽度 / 高度按模块需求比例分配，
 * 与宿主分槽用的是同一份 `demands`。
 */
import { computed } from 'vue'
import {
  FLOATING_BASE_CELL_PX,
  cellSizeOf,
  floatingWindowSize,
  type FloatingLayout,
} from '@/lib/floating-layout'
import type { FloatingModuleDefinition, FloatingRuntimeSnapshot } from '@/lib/floating-modules'

const props = withDefaults(
  defineProps<{
    /** 悬浮窗广播来的运行时快照；还没收到时为 null（显示占位）。 */
    snapshot: FloatingRuntimeSnapshot | null
    /** 注册表里的模块定义，用于给色块取名字。 */
    definitions: readonly FloatingModuleDefinition[]
    /** 配置里的一格边长（物理像素）；预览按它换算真实尺寸，默认即兜底值。 */
    cellPx?: number
  }>(),
  { cellPx: FLOATING_BASE_CELL_PX }
)

/** 每个分组在卡片里的最大绘制尺寸（等比缩放前）。 */
const MAX_WIDTH = 250
const MAX_HEIGHT = 132

/** 一个预览分组：分组标题、说明、用哪个形态的几何、以及贴边后的真实像素尺寸。 */
interface ShapeGroup {
  /** 分组标题 */
  title: string
  /** 几何说明（如 side = ⌈√总格数⌉） */
  formula: string
  /** 用哪种形态的窗口几何来画 */
  layout: FloatingLayout
  /** 该分组代表的形态（用于判断「当前形态」） */
  layouts: FloatingLayout[]
}

const groups: ShapeGroup[] = [
  { title: '方形 · 四角', formula: 'side = ⌈√总格数⌉', layout: 'corner', layouts: ['square', 'corner'] },
  { title: '横条', formula: '上下贴边 · 宽 = 总格数', layout: 'horizontal', layouts: ['horizontal'] },
  { title: '竖条', formula: '左右贴边 · 高 = 总格数', layout: 'vertical', layouts: ['vertical'] },
]

/** 某个分组要画的东西：真实尺寸 + 缩放系数 + 各模块色块。 */
interface RenderedGroup extends ShapeGroup {
  /** 真实像素尺寸（未缩放） */
  size: { width: number; height: number }
  /** 等比缩放到卡片里的系数（1 = 不缩放） */
  scale: number
  /** 窗口总格子数 */
  cells: number
  /** 是否就是悬浮窗此刻的形态 */
  current: boolean
  /** 尺寸说明（方形分组要额外标出未贴边时的 1 格尺寸，如 45×45） */
  sizeNote: string
  /** 各模块色块：标签 + 空间权重 + 名字放不放得下 */
  blocks: { id: string; label: string; weight: number; showLabel: boolean }[]
}

const rendered = computed<RenderedGroup[]>(() => {
  const snapshot = props.snapshot
  // 一格的实际像素尺寸（已夹取范围），三个分组共用同一份口径。
  const cell = cellSizeOf(props.cellPx)
  return groups.map(group => {
    const runtime = snapshot?.byLayout[group.layout]
    const cells = runtime?.windowCells ?? 0
    // 尺寸口径与真窗口一致：同一格边长 + 同一倍率，只是这里整体等比缩放到卡片里。
    const size = floatingWindowSize(group.layout, Math.max(cells, 1), cell)
    const scale = Math.min(1, MAX_WIDTH / size.width, MAX_HEIGHT / size.height)
    // 缩放后画出来的窗口尺寸；色块尺寸由它按权重切出来。
    const drawnWidth = size.width * scale
    const drawnHeight = size.height * scale
    const entries = Object.entries(runtime?.demands ?? {}).map(([id, demand]) => ({
      id,
      weight: Math.max(1, demand),
    }))
    const totalWeight = entries.reduce((sum, entry) => sum + entry.weight, 0) || 1
    const blocks = entries.map(entry => {
      // 分槽方向与宿主一致：横条左右并排，方形 / 四角 / 竖条上下堆叠。
      const share = entry.weight / totalWeight
      const boxWidth = group.layout === 'horizontal' ? drawnWidth * share : drawnWidth
      const boxHeight = group.layout === 'horizontal' ? drawnHeight : drawnHeight * share
      return {
        id: entry.id,
        label: props.definitions.find(definition => definition.id === entry.id)?.label ?? entry.id,
        weight: entry.weight,
        // 太窄 / 太扁的色块只留一条色带：放名字会溢出成糊字，名字改由 title 提供。
        showLabel: boxWidth >= 40 && boxHeight >= 16,
      }
    })
    return {
      ...group,
      size,
      scale,
      cells,
      current: snapshot?.layout ? group.layouts.includes(snapshot.layout) : false,
      sizeNote:
        group.layout === 'corner'
          ? `${cells} 格 · 未贴边 ${cell.width}×${cell.height} · 贴角 ${size.width}×${size.height}px`
          : `${cells} 格 · ${size.width}×${size.height}px`,
      blocks,
    }
  })
})
</script>

<template>
  <div class="shape-preview">
    <p v-if="!snapshot" class="shape-preview__empty">正在读取悬浮窗状态…</p>
    <template v-else>
      <article v-for="group in rendered" :key="group.title" class="shape-preview__group">
        <header class="shape-preview__head">
          <h3 class="shape-preview__title">{{ group.title }}</h3>
          <span v-if="group.current" class="shape-preview__badge">当前</span>
          <span class="shape-preview__formula">{{ group.formula }}</span>
        </header>

        <div class="shape-preview__stage">
          <div
            class="shape-preview__window"
            :class="`shape-preview__window--${group.layout}`"
            :style="{
              width: `${Math.round(group.size.width * group.scale)}px`,
              height: `${Math.round(group.size.height * group.scale)}px`,
            }"
            :aria-label="`${group.title}：共 ${group.cells} 格`"
          >
            <span
              v-for="block in group.blocks"
              :key="block.id"
              class="shape-preview__block"
              :title="`${block.label} · ${block.weight} 格`"
              :style="{ flexGrow: block.weight }"
            >
              <span v-if="block.showLabel" class="shape-preview__block-label">{{ block.label }}</span>
            </span>
          </div>
        </div>

        <p class="shape-preview__size">{{ group.sizeNote }}</p>
      </article>
    </template>
  </div>
</template>