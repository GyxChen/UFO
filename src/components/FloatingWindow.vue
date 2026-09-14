<script setup lang="ts">
import { onBeforeUnmount, onMounted, provide, ref } from 'vue'
import { emitTo } from '@tauri-apps/api/event'
import { currentMonitor, getCurrentWindow, PhysicalPosition, PhysicalSize } from '@tauri-apps/api/window'
import {
  ABS_MAX_CELLS,
  ABS_MIN_CELLS,
  DEFAULT_WINDOW_CELLS,
  layoutFromDocked,
} from '@/lib/agent-status-layout'
import { floatingWindowKey } from '@/lib/floating-window-context'
import { snapToWorkArea, type DockedEdges } from '@/lib/snap'
import { TOOLTIP_HOVER_EVENT, TOOLTIP_WINDOW_LABEL, type TooltipHoverPayload } from '@/lib/tooltip'
import { showMainWindow } from '@/lib/window'
import FloatingModuleHost from './FloatingModuleHost.vue'

const window = getCurrentWindow()
const pointerStart = ref<{ x: number; y: number } | null>(null)
const dragging = ref(false)
const layoutMode = ref<'square' | 'corner' | 'horizontal' | 'vertical'>('square')
let snapTimer: ReturnType<typeof setTimeout> | undefined
let unlistenMoved: (() => void) | undefined
let unlistenScaleChanged: (() => void) | undefined
// 悬浮窗的基础尺寸,未贴边时恢复为该尺寸;挂载时读取。
let baseSize: { width: number; height: number } = { width: 56, height: 56 }
// 模块申请的格子数；不低于默认，不超过绝对上限。
let requestedCells = DEFAULT_WINDOW_CELLS
// 最近一次吸附结果：提示窗定位直接复用，避免 hover 时重复查询窗口几何。
let lastRect: { x: number; y: number; width: number; height: number } | null = null
let lastDocked: DockedEdges = { left: false, top: false, right: false, bottom: false }

// 悬停显示延迟：够长以免鼠标扫过时闪一下。
const SHOW_DELAY_MS = 250
// 离开后的隐藏延迟：短一些，但避免灯之间移动时抖动。
const HIDE_DELAY_MS = 120
const hoverKey = ref<string | null>(null)
let showTimer: ReturnType<typeof setTimeout> | undefined
let hideTimer: ReturnType<typeof setTimeout> | undefined

function setWindowCells(cells: number) {
  const next = Math.min(Math.max(Math.floor(cells), ABS_MIN_CELLS), ABS_MAX_CELLS)
  if (next === requestedCells) return
  requestedCells = next
  scheduleSnap()
}

/** 悬停到某个指示灯（key 非 null）或离开（null）。 */
function onSessionHover(key: string | null) {
  clearTimeout(showTimer)
  clearTimeout(hideTimer)
  showTimer = undefined
  hideTimer = undefined

  if (key === null) {
    hoverKey.value = null
    // 稍作延迟再隐藏，避免在相邻指示灯之间移动时反复开关。
    hideTimer = setTimeout(() => {
      hideTimer = undefined
      void emitHover(false)
    }, HIDE_DELAY_MS)
    return
  }

  hoverKey.value = key
  showTimer = setTimeout(() => {
    showTimer = undefined
    void emitHover(true)
  }, SHOW_DELAY_MS)
}

/** 通知提示窗显示 / 隐藏；显示时附带悬浮窗外框与贴边状态供其定位。 */
async function emitHover(visible: boolean) {
  const payload: TooltipHoverPayload = {
    visible,
    sessionKey: visible ? hoverKey.value : null,
    anchor: lastRect ?? { x: 0, y: 0, width: 0, height: 0 },
    docked: lastDocked,
  }
  try {
    await emitTo(TOOLTIP_WINDOW_LABEL, TOOLTIP_HOVER_EVENT, payload)
  } catch (error) {
    console.warn('failed to emit tooltip hover state', error)
  }
}

/** 立即取消待显示的提示窗并隐藏已显示的（拖拽 / 点击 / 贴边重算时调用）。 */
function dismissTooltip() {
  const wasActive = showTimer !== undefined || hideTimer !== undefined || hoverKey.value !== null
  clearTimeout(showTimer)
  clearTimeout(hideTimer)
  showTimer = undefined
  hideTimer = undefined
  hoverKey.value = null
  if (!wasActive) return
  void emitHover(false)
}

provide(floatingWindowKey, { layoutMode, setWindowCells, onSessionHover })

function onPointerDown(event: PointerEvent) {
  if (event.button !== 0) {
    return
  }

  // 点击 / 拖拽期间不应有弹窗；startDragging 后指针交给系统，pointerleave 可能不再触发。
  dismissTooltip()
  pointerStart.value = { x: event.clientX, y: event.clientY }
  dragging.value = false
}

async function onPointerMove(event: PointerEvent) {
  if (!pointerStart.value || dragging.value) {
    return
  }

  const movedX = Math.abs(event.clientX - pointerStart.value.x)
  const movedY = Math.abs(event.clientY - pointerStart.value.y)
  if (Math.max(movedX, movedY) < 4) {
    return
  }

  dragging.value = true
  dismissTooltip()
  try {
    await window.startDragging()
  } catch (error) {
    dragging.value = false
    console.error('failed to start dragging floating window', error)
  }
}

function onPointerUp() {
  if (!pointerStart.value) {
    return
  }

  const wasDragging = dragging.value
  pointerStart.value = null
  dragging.value = false

  if (!wasDragging) {
    void showMainWindow().catch((error) => {
      console.error('failed to show main window', error)
    })
    return
  }

  scheduleSnap()
}

function scheduleSnap() {
  if (snapTimer) {
    clearTimeout(snapTimer)
  }
  // 原生窗口移动事件会连续触发；等待事件稳定后再读取最终位置并吸附。
  snapTimer = setTimeout(() => {
    snapTimer = undefined
    void snapWindow().catch((error) => {
      console.error('failed to snap floating window', error)
    })
  }, 140)
}

async function snapWindow() {
  const monitor = await currentMonitor()
  if (!monitor) {
    return
  }
  // 使用窗口外框的物理坐标和尺寸进行吸附，避免受浏览器 CSS 像素和 DPI 缩放影响。
  const [position, size] = await Promise.all([window.outerPosition(), window.outerSize()])
  const result = snapToWorkArea(
    { x: position.x, y: position.y },
    { width: size.width, height: size.height },
    baseSize,
    {
      position: { x: monitor.workArea.position.x, y: monitor.workArea.position.y },
      size: { width: monitor.workArea.size.width, height: monitor.workArea.size.height },
    },
    { cells: requestedCells },
  )

  if (result.position.x !== position.x || result.position.y !== position.y) {
    await window.setPosition(new PhysicalPosition(result.position.x, result.position.y))
  }
  if (result.size.width !== size.width || result.size.height !== size.height) {
    await window.setSize(new PhysicalSize(result.size.width, result.size.height))
  }
  // 记录吸附后的几何：提示窗定位复用，避免 hover 时再次查询窗口。
  lastRect = {
    x: result.position.x,
    y: result.position.y,
    width: result.size.width,
    height: result.size.height,
  }
  lastDocked = result.docked
  layoutMode.value = layoutFromDocked(result.docked)
  // 贴边形态变化会改变 anchor，已显示的提示窗需要按新位置重算。
  dismissTooltip()
}

function onMoved() {
  scheduleSnap()
}

function onScaleChanged() {
  scheduleSnap()
}

onMounted(async () => {
  // 先同步当前窗口的布局，避免首次显示内容时仍使用未知形态。
  scheduleSnap()
  unlistenMoved = await window.onMoved(onMoved)
  unlistenScaleChanged = await window.onScaleChanged(onScaleChanged)
})

onBeforeUnmount(() => {
  if (snapTimer) {
    clearTimeout(snapTimer)
  }
  clearTimeout(showTimer)
  clearTimeout(hideTimer)
  showTimer = undefined
  hideTimer = undefined
  unlistenMoved?.()
  unlistenScaleChanged?.()
})
</script>

<template>
  <div class="floating-shell">
    <button
      class="floating-launcher"
      type="button"
      aria-label="打开应用"
      @pointerdown="onPointerDown"
      @pointermove="onPointerMove"
      @pointerup="onPointerUp"
      @pointercancel="onPointerUp"
    >
      <FloatingModuleHost :layout-mode="layoutMode" />
    </button>
  </div>
</template>
