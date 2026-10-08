<script setup lang="ts">
/**
 * 三形态预览（方案 C）：把**当前形态**按真实比例放大画在画布里，配一份「颜色 ↔ 组件」图例，
 * 其余两态降成缩略索引。
 *
 * 为什么不是平铺三张图：旧版把方形 / 横条 / 竖条各自等比缩进同样大的框，
 * 1 格和 16 格看着一样大（比例不可比），三块同色也读不出谁是谁。现在只回答一个问题 ——
 * 「我现在这块窗长什么样」，并把各模块的需求格数与整窗像素直接标在旁边。
 *
 * 尺寸口径与真窗口同源：`windowGrid` / `floatingWindowSize` + 配置里的 `cell_px`，
 * 色块按各模块 `demand` 的比例分槽（与宿主分槽用的是同一份 demands），只是整体等比缩进画布。
 * ⚠️ 色块的名字只在块内折行，绝不参与分槽 —— 否则长名字的块看着比它实际占的格数更宽，
 * 「按真实比例画」这个前提就没了（防线是 main.css 里 `.shape-preview__block` 的 `flex-basis: 0`）。
 */
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import {
  FLOATING_BASE_CELL_PX,
  cellSizeOf,
  floatingWindowSize,
  windowGrid,
  type FloatingLayout,
} from '@/lib/floating-layout'
import type { FloatingModuleDefinition, FloatingRuntimeSnapshot } from '@/lib/floating-modules'

const props = withDefaults(
  defineProps<{
    /** 悬浮窗广播来的运行时快照；还没收到时为 null（显示占位）。 */
    snapshot: FloatingRuntimeSnapshot | null
    /** 注册表里的模块定义，用于给色块取名字与身份色。 */
    definitions: readonly FloatingModuleDefinition[]
    /** 配置里的一格边长（物理像素）；预览按它换算真实尺寸，默认即兜底值。 */
    cellPx?: number
  }>(),
  { cellPx: FLOATING_BASE_CELL_PX }
)

/** 一个分组的展示信息：名字、副标注（怎么才会变成它），以及用哪种窗口几何。 */
interface ShapeMeta {
  /** 分组的名义形态（决定用 `byLayout` 里哪一份需求来画） */
  layout: FloatingLayout
  /** 这一组代表哪些运行时形态；未贴边的方形折进四角那一组 */
  covers: readonly FloatingLayout[]
  /** 显示名 */
  name: string
  /** 一句话说明「什么情况下会是这个形状」 */
  sub: string
  /** 是否就是悬浮窗此刻的形态（决定用哪块画布的尺寸去缩放） */
  current: boolean
}

/**
 * 只列三组：未贴边的方形不单独占一格 —— 它就是「四角」那一组最短的形态（整窗只有一格），
 * 多一张缩略图只会多出一个没有信息量的小点；未贴边时主图按实际形态画（见 {@link shapeView}）。
 */
const SHAPE_METAS: Omit<ShapeMeta, 'current'>[] = [
  { layout: 'corner', covers: ['square', 'corner'], name: '四角', sub: '贴到角' },
  { layout: 'horizontal', covers: ['horizontal'], name: '横条', sub: '上下贴边' },
  { layout: 'vertical', covers: ['vertical'], name: '竖条', sub: '左右贴边' },
]

/** 色块：模块身份 + 占几格 + 放不放得下名字。 */
interface PreviewBlock {
  id: string
  /** 模块显示名 */
  label: string
  /** 身份色（来自模块定义的 `tint`，省略时回落强调色） */
  tint: string
  /** 本形态下它要占的格数（= 分槽权重） */
  weight: number
  /** 缩放后色块够不够放下名字（放不下就只留一条色带，名字改由 title 提供） */
  showLabel: boolean
  /** 色块的高度容得下名字折几行（交给 CSS 的 `line-clamp`，超出就省略号收掉） */
  labelLines: number
}

/** 一个形态的完整绘制结果：真实像素 + 缩放系数 + 色块。 */
interface ShapeView extends ShapeMeta {
  /** 窗口网格（行×列），与宿主分槽同一份口径 */
  grid: { rows: number; cols: number }
  /** 真实像素尺寸（未缩放） */
  size: { width: number; height: number }
  /** 缩放后的绘制尺寸 */
  drawn: { width: number; height: number }
  /** 该形态下的总格子数（各模块需求之和） */
  cells: number
  /** 是否就是悬浮窗此刻的形态 */
  current: boolean
  blocks: PreviewBlock[]
}

/** 画布量不到尺寸时的兜底上限（happy-dom 没有布局；真机上由 ResizeObserver 接管）。 */
const FALLBACK_CANVAS = { width: 196, height: 150 }
/** 缩略排里每个小图的盒子。 */
const THUMB_BOX = { width: 62, height: 44 }
/**
 * 名字的折行尺寸账（与 `main.css` 里 `.shape-preview__block-label` 的 9px × 1.15 行高同源）：
 * 一行按 11px 算，色块上下各留 1px 呼吸位；宽度至少要放得下两个汉字。
 */
const LABEL_LINE_PX = 11
const LABEL_MIN_WIDTH_PX = 22

const canvasEl = ref<HTMLElement | null>(null)
/** 画布的实际内容盒尺寸：两列等高后卡片会变高，画布要跟着长，不能写死。 */
const canvasBox = ref(FALLBACK_CANVAS)

let observer: ResizeObserver | undefined
/** 开始观察画布尺寸；环境没有 ResizeObserver 时静默用兜底值。 */
function observeCanvas() {
  observer?.disconnect()
  const element = canvasEl.value
  if (!element || typeof ResizeObserver === 'undefined') return
  observer = new ResizeObserver(entries => {
    const box = entries[0]?.contentRect
    if (box && box.width > 0 && box.height > 0) {
      canvasBox.value = { width: box.width, height: box.height }
    }
  })
  observer.observe(element)
}
watch(canvasEl, observeCanvas)
onBeforeUnmount(() => observer?.disconnect())

/**
 * 把窗口等比塞进给定盒子：返回缩放系数。
 *
 * 与旧版的差别是不再夹在 ≤1 —— 画布比窗口大时**允许放大**，
 * 否则一格 45px 的方形在放大后的画布里会缩成一个小点；绝对尺寸由旁边的读数表达。
 */
function fitScale(size: { width: number; height: number }, box: { width: number; height: number }) {
  if (!size.width || !size.height) return 1
  return Math.min(box.width / size.width, box.height / size.height)
}

/** 某个分组在当前快照下的绘制结果。 */
function shapeView(meta: Omit<ShapeMeta, 'current'>, cell: { width: number; height: number }, snapshot: FloatingRuntimeSnapshot | null): ShapeView {
  const current = snapshot ? meta.covers.includes(snapshot.layout) : false
  // 当前这一组按**实际形态**画：未贴边的窗就画一格，不拿贴角的比例冒充现状。
  const layout = current && snapshot ? snapshot.layout : meta.layout
  const runtime = snapshot?.byLayout[layout]
  const demands = runtime?.demands ?? {}
  const entries = Object.entries(demands)
  // 没有任何需求数据时按 1 格画，避免整块空白（快照晚于组件挂载到达是正常路径）。
  const weights = entries.length ? entries.map(([, demand]) => Math.max(1, demand)) : [1]
  const grid = windowGrid(layout, weights)
  const size = floatingWindowSize(layout, weights, cell)
  const box = current ? canvasBox.value : THUMB_BOX
  const scale = fitScale(size, box)
  const total = weights.reduce((sum, weight) => sum + weight, 0) || 1
  const blocks: PreviewBlock[] = entries.map(([id, demand]) => {
    // 分槽方向与宿主一致：横条左右并排，方形 / 四角 / 竖条上下堆叠。
    const weight = Math.max(1, demand)
    const share = weight / total
    const drawnWidth = layout === 'horizontal' ? size.width * scale * share : size.width * scale
    const drawnHeight = layout === 'horizontal' ? size.height * scale : size.height * scale * share
    // 名字允许在色块里折行（宽度被格数钉死，见 main.css 的 `flex-basis: 0`），
    // 所以「放得下」只看**宽度够两个字、高度够一行**，能折几行由高度算。
    const lines = Math.floor((drawnHeight - 2) / LABEL_LINE_PX)
    return {
      id,
      label: props.definitions.find(definition => definition.id === id)?.label ?? id,
      tint: props.definitions.find(definition => definition.id === id)?.tint ?? 'var(--ufo-accent)',
      weight,
      showLabel: drawnWidth >= LABEL_MIN_WIDTH_PX && lines >= 1,
      labelLines: Math.max(1, lines),
    }
  })
  return {
    ...meta,
    layout,
    // 折进来的未贴边形态要老实说自己是「未贴边」，否则标题会读成「贴到角却是 1×1」。
    sub: layout === 'square' ? '未贴边' : meta.sub,
    grid,
    size,
    drawn: { width: Math.round(size.width * scale), height: Math.round(size.height * scale) },
    cells: weights.reduce((sum, weight) => sum + weight, 0),
    current,
    blocks,
  }
}

/** 三个分组的绘制结果（当前那个进 hero，其余进缩略排）。 */
const shapes = computed<ShapeView[]>(() => {
  const snapshot = props.snapshot
  const cell = cellSizeOf(props.cellPx)
  return SHAPE_METAS.map(meta => shapeView(meta, cell, snapshot))
})

/** 一格的实际像素：读数用它，配置改了跟着改，绝不写死 45。 */
const cellSide = computed(() => cellSizeOf(props.cellPx).width)

/** 主图：当前那一组；快照还没来时画四角（未贴边也归它）。 */
const hero = computed(() => shapes.value.find(view => view.current) ?? shapes.value[0])
/** 缩略排：除当前分组外的其余两组。 */
const thumbs = computed(() => shapes.value.filter(view => !view.current))

/** 窗口元素的内联样式：绘制尺寸 + 格子辅助线的行列数。 */
function windowStyle(view: ShapeView) {
  return {
    width: `${view.drawn.width}px`,
    height: `${view.drawn.height}px`,
    '--grid-cols': String(view.grid.cols),
    '--grid-rows': String(view.grid.rows),
  }
}

/** 图例行：当前形态下每个模块要几格。 */
const legend = computed(() => hero.value.blocks)
</script>

<template>
  <div class="shape-preview">
    <p v-if="!snapshot" class="shape-preview__empty">正在读取悬浮窗状态…</p>
    <template v-else>
      <div class="shape-preview__hero">
        <div ref="canvasEl" class="shape-preview__canvas">
          <div
            class="shape-preview__window"
            :class="`shape-preview__window--${hero.layout}`"
            :style="windowStyle(hero)"
            :aria-label="`${hero.name} · ${hero.sub}：共 ${hero.cells} 格，${hero.size.width}×${hero.size.height} 像素`"
          >
            <span
              v-for="block in hero.blocks"
              :key="block.id"
              class="shape-preview__block"
              :style="{ flexGrow: block.weight, '--block-tint': block.tint, '--label-lines': String(block.labelLines) }"
              :title="`${block.label} · ${block.weight} 格`"
            >
              <span v-if="block.showLabel" class="shape-preview__block-label">{{ block.label }}</span>
            </span>
          </div>
        </div>

        <div class="shape-preview__legend">
          <header class="shape-preview__head">
            <h3 class="shape-preview__title">{{ hero.name }} · {{ hero.sub }}</h3>
            <span class="shape-preview__badge">当前</span>
          </header>
          <ul class="shape-preview__rows">
            <li v-for="row in legend" :key="row.id" class="shape-preview__row">
              <i class="shape-preview__dot" :style="{ '--block-tint': row.tint }" aria-hidden="true" />
              <span class="shape-preview__row-name">{{ row.label }}</span>
              <span class="shape-preview__cells">{{ row.weight }} 格</span>
            </li>
          </ul>
          <p class="shape-preview__readout">
            整窗 {{ hero.grid.rows }}×{{ hero.grid.cols }} · {{ hero.size.width }}×{{ hero.size.height }}px
          </p>
          <p class="shape-preview__cell-note">一格 {{ cellSide }}px · 需求之和 {{ hero.cells }} 格</p>
        </div>
      </div>

      <ul class="shape-preview__thumbs">
        <li v-for="view in thumbs" :key="view.layout" class="shape-preview__thumb">
          <span class="shape-preview__thumb-name">{{ view.name }}</span>
          <span
            class="shape-preview__window shape-preview__window--thumb"
            :class="`shape-preview__window--${view.layout}`"
            :style="windowStyle(view)"
            aria-hidden="true"
          >
            <span
              v-for="block in view.blocks"
              :key="block.id"
              class="shape-preview__block"
              :style="{ flexGrow: block.weight, '--block-tint': block.tint }"
            />
          </span>
          <span class="shape-preview__thumb-size">{{ view.size.width }}×{{ view.size.height }}</span>
        </li>
      </ul>
    </template>
  </div>
</template>
