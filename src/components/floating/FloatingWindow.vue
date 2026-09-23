<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { emitTo, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/core'
import { currentMonitor, getCurrentWindow, PhysicalPosition, PhysicalSize } from '@tauri-apps/api/window'
import {
  ABS_MAX_CELLS,
  ABS_MIN_CELLS,
  DEFAULT_WINDOW_CELLS,
  cellSizeOf,
  layoutFromDocked,
  type FloatingLayout,
} from '@/lib/floating-layout'
import type { FloatingHoverTarget } from '@/lib/floating-module-context'
import {
  FLOATING_RUNTIME_EVENT,
  FLOATING_RUNTIME_REQUEST_EVENT,
  OPEN_MODULE_SETTINGS_EVENT,
  type OpenModuleSettingsPayload,
} from '@/lib/floating-bridge'
import type { FloatingRuntimeSnapshot } from '@/lib/floating-modules'
import { snapToWorkArea, type DockedEdges } from '@/lib/snap'
import {
  TOOLTIP_HOVER_EVENT,
  TOOLTIP_INTERACTION_EVENT,
  TOOLTIP_WINDOW_LABEL,
  type TooltipHoverPayload,
} from '@/lib/tooltip'
import { showMainWindow } from '@/lib/window'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import FloatingModuleHost from './FloatingModuleHost.vue'

const window = getCurrentWindow()
const pointerStart = ref<{ x: number; y: number } | null>(null)
const dragging = ref(false)
const layoutMode = ref<FloatingLayout>('square')
let snapTimer: ReturnType<typeof setTimeout> | undefined
let unlistenMoved: (() => void) | undefined
let unlistenScaleChanged: (() => void) | undefined
let unlistenTooltipInteraction: UnlistenFn | undefined
// 悬浮窗配置（窗口级数据）由外壳 store 持有：这里只读一格边长，不关心谁改的。
const modules = useFloatingModulesStore()
// 一格的实际尺寸：未贴边时窗口恢复为它，贴边时按格数成倍放大。
// 用户在「悬浮窗设置」里调的就是它（配置 `cell_px`），没配过时回落常量。
const baseSize = computed(() => cellSizeOf(modules.cellPx))
// 模块申请的格子数；不低于默认，不超过绝对上限。
let requestedCells = DEFAULT_WINDOW_CELLS
// 最近一次吸附结果：提示窗定位直接复用，避免 hover 时重复查询窗口几何。
let lastRect: { x: number; y: number; width: number; height: number } | null = null
let lastDocked: DockedEdges = { left: false, top: false, right: false, bottom: false }

// 悬停显示延迟：够长以免鼠标扫过时闪一下。
const SHOW_DELAY_MS = 250
// 离开后的隐藏延迟：短一些，但避免灯之间移动时抖动。
const HIDE_DELAY_MS = 120
const hoverTarget = ref<FloatingHoverTarget | null>(null)
const tooltipHovered = ref(false)
let showTimer: ReturnType<typeof setTimeout> | undefined
let hideTimer: ReturnType<typeof setTimeout> | undefined
// 提示窗请求序号：每次发送 hover 状态都递增，提示窗用它丢弃跨窗口迟到事件。
let tooltipHoverSequence = 0

function setWindowCells(cells: number) {
  const next = Math.min(Math.max(Math.floor(cells), ABS_MIN_CELLS), ABS_MAX_CELLS)
  if (next === requestedCells) return
  requestedCells = next
  scheduleSnap()
}

/** 最近一次运行时快照：主窗口可能后开，所以缓存一份给它要。 */
let lastRuntime: FloatingRuntimeSnapshot | null = null
let unlistenRuntimeRequest: UnlistenFn | undefined

/**
 * 宿主算出各形态的运行时快照 → 广播给主窗口（组件徽标 + 三形态预览）。
 * 主窗口没开也没关系，事件没人收而已。
 */
function onModuleRuntime(snapshot: FloatingRuntimeSnapshot) {
  lastRuntime = snapshot
  void broadcastRuntime()
}

/** 把缓存的运行时快照发给主窗口；主窗口刚挂载时会主动来要一次。 */
async function broadcastRuntime() {
  if (!lastRuntime) {
    return
  }
  try {
    await emitTo('main', FLOATING_RUNTIME_EVENT, lastRuntime)
  } catch (error) {
    console.warn('failed to broadcast floating runtime', error)
  }
}

/**
 * 取某形态下各模块的格子需求，供贴边算窗口尺寸用。
 *
 * 四角必须知道「有哪些模块、各要几格」才能按模块数分行（只给总数会退化成方格，
 * 见 floating-layout 的 windowGrid）。快照里四种形态的需求都算好了，直接按目标形态取；
 * 快照还没到（首帧）时返回空数组，snap 会退回按 `requestedCells` 单模块算。
 */
function demandsForLayout(layout: FloatingLayout): number[] {
  const runtime = lastRuntime?.byLayout[layout]
  return runtime ? Object.values(runtime.demands) : []
}

/** 某个模块上报悬停到了它的哪个对象（null 表示离开），由外壳转成提示窗事件。 */
function onModuleHover(target: FloatingHoverTarget | null) {
  clearTimeout(showTimer)
  clearTimeout(hideTimer)
  showTimer = undefined
  hideTimer = undefined

  if (target === null) {
    hoverTarget.value = null
    if (tooltipHovered.value) return
    // 稍作延迟再隐藏，避免在相邻指示灯之间移动时反复开关。
    hideTimer = setTimeout(() => {
      hideTimer = undefined
      void emitHover(false)
    }, HIDE_DELAY_MS)
    return
  }

  hoverTarget.value = target
  showTimer = setTimeout(() => {
    showTimer = undefined
    void emitHover(true)
  }, SHOW_DELAY_MS)
}

/** 通知提示窗显示 / 隐藏；显示时附带悬浮窗外框与贴边状态供其定位。 */
/** Keeps tooltip visible while the pointer is over its interactive window. */
function onTooltipInteraction(hovered: boolean) {
  tooltipHovered.value = hovered
  if (hovered) {
    clearTimeout(hideTimer)
    hideTimer = undefined
    return
  }
  if (hoverTarget.value === null) {
    onModuleHover(null)
  }
}

async function emitHover(visible: boolean) {
  const payload: TooltipHoverPayload = {
    sequence: ++tooltipHoverSequence,
    visible,
    target: visible ? hoverTarget.value : null,
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
  const wasActive = showTimer !== undefined || hideTimer !== undefined || hoverTarget.value !== null
  clearTimeout(showTimer)
  clearTimeout(hideTimer)
  showTimer = undefined
  hideTimer = undefined
  hoverTarget.value = null
  tooltipHovered.value = false
  if (!wasActive) return
  void emitHover(false)
}

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
    // 原生拖拽循环会吞掉松手的 pointerup，改由轮询物理按键状态补做贴边。
    startDragReleasePolling()
  } catch (error) {
    dragging.value = false
    console.error('failed to start dragging floating window', error)
  }
}

/**
 * 点在某块组件上就打开那个组件的设置页，点空白处打开组件列表页。
 * 模块槽位的根节点带 `data-module-id`，所以这里不需要认识任何具体模块。
 */
function moduleIdAt(target: EventTarget | null): string | null {
  const element = target instanceof HTMLElement ? target : null
  return element?.closest('[data-module-id]')?.getAttribute('data-module-id') ?? null
}

/** 显示主窗口并让它切到某个组件的设置页（null = 组件列表页）。 */
async function openMainWindow(moduleId: string | null) {
  try {
    await showMainWindow()
    await emitTo('main', OPEN_MODULE_SETTINGS_EVENT, { moduleId } satisfies OpenModuleSettingsPayload)
  } catch (error) {
    console.error('failed to open module settings', error)
  }
}

function onPointerUp(event: PointerEvent) {
  if (!pointerStart.value) {
    return
  }

  const wasDragging = dragging.value
  pointerStart.value = null
  dragging.value = false
  // 有些平台 / 时机下 pointerup 仍会正常到达；停止轮询避免重复贴边判定。
  stopDragReleasePolling()

  if (!wasDragging) {
    // 不是拖拽 = 一次点击：落到哪个组件上就开哪个组件的设置。
    void openMainWindow(moduleIdAt(event.target))
    return
  }

  scheduleSnap()
}

/** 触摸 / 系统取消：只清理状态，不当成点击（否则会莫名弹主窗口）。 */
function onPointerCancel() {
  pointerStart.value = null
  dragging.value = false
  stopDragReleasePolling()
}

/**
 * 拖拽释放轮询。
 *
 * Windows 上 startDragging() 进入原生模态拖拽循环：松手时 tao 只向根窗口 post
 * 合成的 WM_LBUTTONUP，WebView2 子窗口收不到，DOM 层永远等不到 pointerup；
 * 而循环期间的 invoke 响应也会被主线程模态循环滞留到松手后才送达。
 * 因此拖拽开始后以固定间隔查询系统级物理左键状态（GetAsyncKeyState，与捕获/
 * 焦点无关），一旦确认松开就补做「松手贴边」。按住不动时绝不吸附，
 * 修复「拖到位后停手未松键就提前贴边」的原问题。
 */
const DRAG_POLL_INTERVAL_MS = 80
let dragPollTimer: ReturnType<typeof setInterval> | undefined
// 是否有一次按键状态查询在途：模态循环滞留响应时防止 IPC 请求排队堆积。
let dragPollInFlight = false

function startDragReleasePolling() {
  stopDragReleasePolling()
  dragPollTimer = setInterval(() => {
    // dragging 已被 pointerup 路径清掉（其他平台 / 未来 tao 版本会送达）则停止轮询。
    if (!dragging.value) {
      stopDragReleasePolling()
      return
    }
    if (dragPollInFlight) {
      return
    }
    dragPollInFlight = true
    // 命令可用时返回 boolean；平台不支持时返回 null（invoke 得到 null/undefined）。
    void invoke('is_left_mouse_pressed')
      .then(pressed => {
        dragPollInFlight = false
        // 响应可能滞后送达：处理前先确认仍处于「未收到 pointerup 的拖拽」状态。
        if (!dragging.value) {
          stopDragReleasePolling()
          return
        }
        if (pressed === null || pressed === undefined) {
          // 该平台无此命令：退回 pointerup / onMoved 路径，停用轮询。
          stopDragReleasePolling()
          return
        }
        if (pressed) {
          return
        }
        // 物理左键已松开而 DOM pointerup 没到达：就地补做松手处理（等效 onPointerUp 的拖拽分支）。
        stopDragReleasePolling()
        pointerStart.value = null
        dragging.value = false
        scheduleSnap()
      })
      .catch(error => {
        dragPollInFlight = false
        // 命令不可用时退回原行为：由 pointerup / onMoved 防抖负责贴边，
        // 宁可偶发提前贴边，也不能永远不贴边。
        console.warn('failed to poll mouse button state', error)
        stopDragReleasePolling()
      })
  }, DRAG_POLL_INTERVAL_MS)
}

function stopDragReleasePolling() {
  dragPollInFlight = false
  if (dragPollTimer !== undefined) {
    clearInterval(dragPollTimer)
    dragPollTimer = undefined
  }
}

function scheduleSnap() {
  if (snapTimer) {
    clearTimeout(snapTimer)
  }
  // 原生窗口移动事件会连续触发；等待事件稳定后再读取最终位置并吸附。
  snapTimer = setTimeout(() => {
    snapTimer = undefined
    // 触发时再次判定：拖拽进行中（鼠标仍按住）不吸附，避免用户「停手不移动但未松手」
    // 时因防抖到点而提前贴边。此时可能已有一次由移动过程 armed 的定时器，故守卫放在这里。
    if (dragging.value) {
      return
    }
    void snapWindow().catch(error => {
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
    baseSize.value,
    {
      position: { x: monitor.workArea.position.x, y: monitor.workArea.position.y },
      size: { width: monitor.workArea.size.width, height: monitor.workArea.size.height },
    },
    { cells: requestedCells, demandsFor: demandsForLayout }
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
  // 拖拽进行中不吸附：startDragging 后窗口跟随光标会持续触发 onMoved，
  // 若此时保持 scheduleSnap 的 140ms 防抖，用户「按住不动」也会让计时器到点、
  // 悬浮窗在鼠标未松开时提前贴边。真正的吸附改由 onPointerUp 在松手时触发。
  if (dragging.value) {
    return
  }
  scheduleSnap()
}

function onScaleChanged() {
  scheduleSnap()
}

// 主窗改了格子大小：按新尺寸重新吸附（配置由外壳 store 广播过来，这里不关心是谁改的）。
watch(
  () => modules.cellPx,
  () => {
    scheduleSnap()
  }
)

onMounted(async () => {
  // 先同步当前窗口的布局，避免首次显示内容时仍使用未知形态。
  scheduleSnap()
  unlistenMoved = await window.onMoved(onMoved)
  unlistenScaleChanged = await window.onScaleChanged(onScaleChanged)
  unlistenTooltipInteraction = await listen<boolean>(TOOLTIP_INTERACTION_EVENT, event =>
    onTooltipInteraction(event.payload)
  )
  // 主窗口挂载时会来要一次快照（它可能错过了我们启动时那次广播）。
  unlistenRuntimeRequest = await listen(FLOATING_RUNTIME_REQUEST_EVENT, () => {
    void broadcastRuntime()
  })
})

onBeforeUnmount(() => {
  if (snapTimer) {
    clearTimeout(snapTimer)
  }
  stopDragReleasePolling()
  clearTimeout(showTimer)
  clearTimeout(hideTimer)
  showTimer = undefined
  hideTimer = undefined
  unlistenMoved?.()
  unlistenScaleChanged?.()
  unlistenTooltipInteraction?.()
  unlistenRuntimeRequest?.()
  unlistenRuntimeRequest = undefined
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
      @pointercancel="onPointerCancel"
    >
      <FloatingModuleHost
        :layout-mode="layoutMode"
        @cells="setWindowCells"
        @hover="onModuleHover"
        @runtime="onModuleRuntime"
      />
    </button>
  </div>
</template>
