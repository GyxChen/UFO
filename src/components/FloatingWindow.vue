<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { currentMonitor, getCurrentWindow, PhysicalPosition } from '@tauri-apps/api/window'
import { snapToWorkArea } from '@/lib/snap'
import { showMainWindow } from '@/lib/window'

const window = getCurrentWindow()
const pointerStart = ref<{ x: number; y: number } | null>(null)
const dragging = ref(false)
let snapTimer: ReturnType<typeof setTimeout> | undefined
let unlistenMoved: (() => void) | undefined
let unlistenScaleChanged: (() => void) | undefined

function onPointerDown(event: PointerEvent) {
  if (event.button !== 0) {
    return
  }

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

  const [position, size] = await Promise.all([window.outerPosition(), window.outerSize()])
  const snapped = snapToWorkArea(
    { x: position.x, y: position.y },
    { width: size.width, height: size.height },
    {
      position: { x: monitor.workArea.position.x, y: monitor.workArea.position.y },
      size: { width: monitor.workArea.size.width, height: monitor.workArea.size.height },
    },
  )

  if (snapped.x !== position.x || snapped.y !== position.y) {
    await window.setPosition(new PhysicalPosition(snapped.x, snapped.y))
  }
}

function onMoved() {
  scheduleSnap()
}

function onScaleChanged() {
  scheduleSnap()
}

onMounted(async () => {
  unlistenMoved = await window.onMoved(onMoved)
  unlistenScaleChanged = await window.onScaleChanged(onScaleChanged)
})

onBeforeUnmount(() => {
  if (snapTimer) {
    clearTimeout(snapTimer)
  }
  unlistenMoved?.()
  unlistenScaleChanged?.()
})
</script>

<template>
  <button
    class="floating-launcher"
    type="button"
    aria-label="打开应用"
    @pointerdown="onPointerDown"
    @pointermove="onPointerMove"
    @pointerup="onPointerUp"
    @pointercancel="onPointerUp"
  >
    <span class="floating-launcher__mark" aria-hidden="true">U</span>
  </button>
</template>
