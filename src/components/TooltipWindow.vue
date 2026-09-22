<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { emitTo, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow, monitorFromPoint, PhysicalPosition, PhysicalSize } from '@tauri-apps/api/window'
import {
  clampTooltipSize,
  placeTooltip,
  resolveTooltipDefinition,
  resolveTooltipSize,
  TOOLTIP_HOVER_EVENT,
  TOOLTIP_INTERACTION_EVENT,
  type TooltipHoverPayload,
} from '@/lib/tooltip'
import { FLOATING_MODULE_DEFINITIONS } from '@/modules'

/**
 * 悬停信息提示窗（三个窗口里的第三个）。
 *
 * 它只当容器：定位 / 显隐 / 指针进出转发；**内容按悬停目标所属模块从注册表分派**
 * （模块定义里的 `tooltip`）。提示窗因此不认识任何模块语义 —— 新增模块自带信息卡即可，
 * 这里零改动；模块没声明 `tooltip` 就什么都不弹。
 */
const window = getCurrentWindow()
const hover = ref<TooltipHoverPayload | null>(null)
let unlisten: UnlistenFn | undefined
/**
 * 窗口的默认尺寸（tauri.conf.json 里声明的那份），挂载时量一次。
 * 模块没声明 `tooltip.size` 时把窗口还原成它 —— 否则会沿用上一个内容撑大的尺寸。
 */
let defaultSize: PhysicalSize | null = null

/**
 * 此刻该展示哪个模块的信息卡：有悬停目标 + 该模块声明了 `tooltip` + 模块认为内容已就绪。
 * 内容未就绪时返回 null（不弹窗），避免先在旧位置闪一个空框。
 */
const activeDefinition = computed(() => {
  const payload = hover.value
  const target = payload?.target
  if (!payload?.visible || !target) return null
  const definition = resolveTooltipDefinition(FLOATING_MODULE_DEFINITIONS, target)
  if (!definition) return null
  return definition.tooltip?.hasContent?.(target) === false ? null : definition
})

/** 要渲染的内容组件与它的悬停目标；无可展示内容时为 null。 */
const content = computed(() => {
  const tooltip = activeDefinition.value?.tooltip
  const target = hover.value?.target
  if (!tooltip || !target) return null
  return { component: tooltip.component, target }
})

/**
 * 决定显隐的标识（`模块 id:目标 key`）。
 * 用标识而非布尔值做 watch 源，保证从 A 对象移到 B 对象、或换到另一个模块时也重新定位。
 */
const displayKey = computed(() => (content.value ? `${activeDefinition.value?.id}:${content.value.target.key}` : null))

/**
 * 按内容组件声明的尺寸调整提示窗（`tooltip.size`）；没声明就沿用 tauri.conf.json 的默认值。
 * 必须在量尺寸定位之前做，否则会按旧高度算出位置再挪动。
 *
 * `size` 可以是「按悬停目标算」的函数：折叠清单要按行数长高才装得下全部行，
 * 所以这里先把目标交给模块算出想要的尺寸，再夹进工作区（比屏幕还高的窗口没法看）。
 * 未声明尺寸时把窗口还原成默认尺寸 —— 上一个内容可能把它撑大过。
 */
async function applyTooltipSize(target: TooltipHoverPayload['target'], workArea: PhysicalSize) {
  const declared = resolveTooltipSize(activeDefinition.value?.tooltip, target)
  const size = declared ? clampTooltipSize(declared, workArea) : defaultSize
  if (!size) return
  try {
    await window.setSize(new PhysicalSize(size.width, size.height))
  } catch (error) {
    console.warn('failed to resize tooltip window', error)
  }
}

/** 定位并显示提示窗；先 setPosition 再 show，避免在旧位置闪一下。 */
async function present(payload: TooltipHoverPayload) {
  const centerX = Math.round(payload.anchor.x + payload.anchor.width / 2)
  const centerY = Math.round(payload.anchor.y + payload.anchor.height / 2)
  // 以 anchor 中心取屏：多屏下提示窗自身所在屏未必与悬浮窗同屏。
  // 必须先拿到工作区，才能把模块要的尺寸夹进屏幕再 setSize。
  const monitor = await monitorFromPoint(centerX, centerY)
  if (!monitor) return

  const workArea = {
    position: { x: monitor.workArea.position.x, y: monitor.workArea.position.y },
    size: { width: monitor.workArea.size.width, height: monitor.workArea.size.height },
  }

  await applyTooltipSize(payload.target, workArea.size)
  const size = await window.outerSize()

  const { position } = placeTooltip(payload.anchor, { width: size.width, height: size.height }, workArea, payload.docked)

  await invoke('set_tooltip_interactive', { interactive: true })
  await window.setPosition(new PhysicalPosition(position.x, position.y))
  await window.show()
}

async function hide() {
  try {
    notifyTooltipInteraction(false)
    await window.hide()
    await invoke('set_tooltip_interactive', { interactive: false })
  } catch (error) {
    console.warn('failed to hide tooltip window', error)
  }
}

/** Sends tooltip pointer state back to the floating window. */
function notifyTooltipInteraction(hovered: boolean) {
  void emitTo('floating', TOOLTIP_INTERACTION_EVENT, hovered)
}

watch(displayKey, key => {
  if (key && hover.value) {
    void present(hover.value).catch(error => {
      console.warn('failed to present tooltip window', error)
    })
    return
  }
  void hide()
})

onMounted(async () => {
  // 先记默认尺寸再开始跟随内容改尺寸，否则「没声明 size 的模块」就没有可还原的基准。
  defaultSize = await window.outerSize()
  unlisten = await listen<TooltipHoverPayload>(TOOLTIP_HOVER_EVENT, event => {
    hover.value = event.payload
  })
})

onBeforeUnmount(() => {
  unlisten?.()
  unlisten = undefined
})
</script>

<template>
  <div
    class="tooltip-shell"
    @pointerenter="notifyTooltipInteraction(true)"
    @pointerleave="notifyTooltipInteraction(false)"
  >
    <!-- 内容由模块提供（每个组件自带的信息卡）；多根节点渲染，外壳样式不用为谁调整。 -->
    <component :is="content.component" v-if="content" :target="content.target" />
  </div>
</template>
