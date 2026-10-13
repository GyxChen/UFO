<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { emitTo, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow, monitorFromPoint, PhysicalPosition, PhysicalSize } from '@tauri-apps/api/window'
import {
  clampTooltipSize,
  extendTooltipSizeForPlugs,
  placeTooltip,
  resolveTooltipDefinition,
  resolveTooltipExtensions,
  resolveTooltipSize,
  TOOLTIP_HOVER_EVENT,
  TOOLTIP_INTERACTION_EVENT,
  type TooltipHoverPayload,
} from '@/lib/tooltip'
import type { FloatingModuleDefinition } from '@/lib/floating-modules'
import { FLOATING_MODULE_DEFINITIONS } from '@/modules'
import { pluginExtensions } from '@/lib/plugin-loader'
import { pluginTooltipSizeOf, pluginTooltipSizes } from '@/lib/plugin-tooltip-size'
import { useFloatingModulesStore } from '@/stores/floatingModules'

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
/** 最新收到的 hover 序号；小于等于它的迟到 payload 直接忽略。 */
let latestSequence = 0
/** 显隐请求代次；新事件到达后，队列里的旧任务会自动放弃。 */
let transitionGeneration = 0
/** 串联 present/hide，保证窗口操作按请求顺序执行。 */
let transitionTail: Promise<void> = Promise.resolve()
/**
 * 最近一次「该展示哪张卡」的请求。
 *
 * 组件包在运行时改卡尺寸时要**按同一份**重新量窗与定位 —— 只改尺寸会把卡留在旧位置，
 * 贴边时可能一下飞出屏幕。
 */
let lastRequest: PresentRequest | null = null

/**
 * 该不该有一张卡在屏幕上（present 一登记就置真，hide 一收就置假）。
 *
 * ⚠️ 这是打断「悬浮窗 ↔ 提示窗互相点着空转」的那道闸：`hide()` 会向悬浮窗回报「指针不在卡上了」，
 * 悬浮窗收到后又报一次「离开了灯」，于是收起 → 回报 → 再收起……实测约 8 Hz 跑一整晚，
 * 每轮两趟跨窗事件 + 一次 `window.hide()` + 一条 Rust 命令（实测占掉约 20% 单核）。
 * 本来就没有卡要收的时候，就别再说话。
 */
let shown = false

/**
 * 此刻该展示哪个模块的信息卡：有悬停目标 + 该模块声明了 `tooltip` + 模块认为内容已就绪。
 * 内容未就绪时返回 null（不弹窗），避免先在旧位置闪一个空框。
 */
const activeDefinition = computed(() => {
  const payload = hover.value
  const target = payload?.target
  if (!payload?.visible || !target) return null
  const definition = resolveTooltipDefinition(FLOATING_MODULE_DEFINITIONS.value, target)
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

const modules = useFloatingModulesStore()

/**
 * 该往这张卡尾部追加的插入块（组件包声明的 `surfaces.extension`）。
 *
 * 外壳只按「目标模块是谁 + 那个包有没有启用 + 块自己有没有内容」筛，
 * 不认识里面写的是什么字段 —— 提示窗因此仍然不认识任何模块。
 */
const plugSlots = computed(() =>
  resolveTooltipExtensions(pluginExtensions.value, hover.value?.target, pluginId =>
    modules.moduleEnabled(pluginId, true)
  )
)

/**
 * 决定显隐的标识（`模块 id:目标 key`）。
 * 用标识而非布尔值做 watch 源，保证从 A 对象移到 B 对象、或换到另一个模块时也重新定位。
 * 数据晚到导致 `hasContent` 从 false 变为 true 时，它也负责重新触发 present。
 */
const displayKey = computed(() => (content.value ? `${activeDefinition.value?.id}:${content.value.target.key}` : null))

interface PresentRequest {
  /** 触发本次显示的完整 hover payload。 */
  payload: TooltipHoverPayload
  /** 本次要显示的悬停目标，已从 payload 中快照出来。 */
  target: NonNullable<TooltipHoverPayload['target']>
  /** 本次要显示的信息卡定义，避免异步期间全局状态被替换。 */
  definition: FloatingModuleDefinition
  /** 发起本次显示时的请求代次。 */
  generation: number
}

/** 比较两个悬停目标是否仍指向同一个模块对象。 */
function sameTarget(left: TooltipHoverPayload['target'], right: TooltipHoverPayload['target']): boolean {
  if (!left || !right) return left === right
  return left.moduleId === right.moduleId && left.key === right.key
}

/** 判断模块当前是否仍声明了可展示且有内容的信息卡。 */
function hasTooltipContent(definition: FloatingModuleDefinition, target: TooltipHoverPayload['target']): boolean {
  if (!target || !definition.tooltip?.component) return false
  return definition.tooltip.hasContent?.(target) !== false
}

/** 判断一次 present 是否仍是当前最新请求；旧请求不得再提交任何窗口状态。 */
function isPresentRequestCurrent(request: PresentRequest): boolean {
  const payload = hover.value
  return (
    request.generation === transitionGeneration &&
    latestSequence === request.payload.sequence &&
    payload?.sequence === request.payload.sequence &&
    payload?.visible === true &&
    sameTarget(payload.target, request.target) &&
    hasTooltipContent(request.definition, request.target)
  )
}

/** 把一次窗口 transition 排到队尾；单个任务失败也不能阻断后续 hide/show。 */
function enqueueTransition(task: () => Promise<void>) {
  transitionTail = transitionTail.then(task).catch(error => {
    console.warn('failed to transition tooltip window', error)
  })
}

/**
 * 按内容组件声明的尺寸调整提示窗（`tooltip.size`）；没声明就沿用 tauri.conf.json 的默认值。
 * 必须在量尺寸定位之前做，否则会按旧高度算出位置再挪动。
 *
 * `size` 可以是「按悬停目标算」的函数：折叠清单要按行数长高才装得下全部行，
 * 所以这里先把目标交给模块算出想要的尺寸，再夹进工作区（比屏幕还高的窗口没法看）。
 * 未声明尺寸时把窗口还原成默认尺寸 —— 上一个内容可能把它撑大过。
 *
 * ⚠️ 插入块是外壳追加的，模块不知道会被追加几块，所以那部分预算在这里加完再一起夹。
 */
async function applyTooltipSize(
  definition: FloatingModuleDefinition,
  target: TooltipHoverPayload['target'],
  workArea: PhysicalSize
) {
  // 运行时申请过的尺寸优先于清单初值（作者按内容量要地方），没申请过才用声明的那份。
  const declared = pluginTooltipSizeOf(definition.id) ?? resolveTooltipSize(definition.tooltip, target)
  const base = declared ?? defaultSize
  if (!base) return
  const size = clampTooltipSize(extendTooltipSizeForPlugs(base, plugSlots.value), workArea)
  try {
    await window.setSize(new PhysicalSize(size.width, size.height))
  } catch (error) {
    console.warn('failed to resize tooltip window', error)
  }
}

/**
 * 组件包在运行时改这张卡的尺寸：只让**正在显示的那张**立刻重新量窗，
 * 别的包的卡不该被一次调用带偏（所以 watch 源按当前模块 id 取）。
 */
watch(
  () => (activeDefinition.value ? pluginTooltipSizes.value[activeDefinition.value.id] : undefined),
  () => {
    const request = lastRequest
    if (!request || request.definition !== activeDefinition.value) return
    enqueueTransition(() => present(request))
  }
)

/** 定位并显示提示窗；先 setPosition 再 show，避免在旧位置闪一下。 */
async function present(request: PresentRequest) {
  if (!isPresentRequestCurrent(request)) return
  // 一登记这次展示就算「该有卡」：中途被更新的请求顶掉时，收起那一路照样要把窗口与穿透状态复位。
  shown = true

  const { payload, definition, target } = request
  const centerX = Math.round(payload.anchor.x + payload.anchor.width / 2)
  const centerY = Math.round(payload.anchor.y + payload.anchor.height / 2)
  // 以 anchor 中心取屏：多屏下提示窗自身所在屏未必与悬浮窗同屏。
  // 必须先拿到工作区，才能把模块要的尺寸夹进屏幕再 setSize。
  const monitor = await monitorFromPoint(centerX, centerY)
  if (!isPresentRequestCurrent(request) || !monitor) return

  const workArea = {
    position: { x: monitor.workArea.position.x, y: monitor.workArea.position.y },
    size: { width: monitor.workArea.size.width, height: monitor.workArea.size.height },
  }

  // 夹尺寸要用监视器给的那份原物：自己按字面量重拼一份 `{width,height}` 在类型上不是
  // Tauri 的 `PhysicalSize`（那是个类），运行时凑巧一样而已。
  await applyTooltipSize(definition, target, monitor.workArea.size)
  if (!isPresentRequestCurrent(request)) return

  const size = await window.outerSize()
  if (!isPresentRequestCurrent(request)) return

  const { position } = placeTooltip(payload.anchor, { width: size.width, height: size.height }, workArea, payload.docked)
  if (!isPresentRequestCurrent(request)) return

  await invoke('set_tooltip_interactive', { interactive: true })
  if (!isPresentRequestCurrent(request)) return

  await window.setPosition(new PhysicalPosition(position.x, position.y))
  if (!isPresentRequestCurrent(request)) return

  await window.show()
}

async function hide(generation?: number) {
  if (generation !== undefined && generation !== transitionGeneration) return
  // 卡已经不在了：运行时改尺寸的回调不该再拿旧请求去量窗。
  lastRequest = null
  // 见 `shown` 那条：没在显示就不必再回报交互，否则两头会互相触发着空转下去。
  if (!shown) return
  shown = false
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

watch(
  [displayKey, () => hover.value?.sequence],
  () => {
    const payload = hover.value
    if (!payload) return

    const generation = ++transitionGeneration
    const target = payload.target
    const definition =
      displayKey.value && payload.visible ? resolveTooltipDefinition(FLOATING_MODULE_DEFINITIONS.value, target) : null

    if (!payload.visible || !target || !definition || !hasTooltipContent(definition, target)) {
      enqueueTransition(() => hide(generation))
      return
    }

    const request: PresentRequest = { payload, target, definition, generation }
    // 请求一登记就记下来（不是等 present 真的跑到量窗那一步）：
    // 卡片的组件可能在那之前就 mounted 并申请尺寸，晚记会让第一次申请落空、卡按初值弹出来。
    lastRequest = request
    enqueueTransition(() => present(request))
  }
)

onMounted(async () => {
  // 先记默认尺寸再开始跟随内容改尺寸，否则「没声明 size 的模块」就没有可还原的基准。
  defaultSize = await window.outerSize()
  unlisten = await listen<TooltipHoverPayload>(TOOLTIP_HOVER_EVENT, event => {
    const payload = event.payload
    if (payload.sequence <= latestSequence) return
    latestSequence = payload.sequence
    hover.value = payload
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

    <!--
      插入块：组件包往这张卡尾部追加的一块（清单里的 `surfaces.extension`）。
      外壳只管三件事 —— 挂到谁的卡上、那个包启用没有、这块此刻有没有内容；
      里面渲染什么字段不认识。
      ⚠️ 宿主模块自己得声明了信息卡，这块才有地方落：没有卡的模块不会弹提示窗（这是刻意的，
      否则「装个包就能让任何模块都弹窗」会变成一条谁都能开的口子）。
    -->
    <section v-if="content && plugSlots.length" class="tooltip-plugs">
      <div v-for="slot in plugSlots" :key="slot.pluginId" class="tooltip-plug">
        <p class="tooltip-plug__source">
          <span class="tooltip-plug__badge">插件 · {{ slot.label }}</span>
        </p>
        <component :is="slot.component" :target="content.target" :max-rows="slot.maxRows" />
      </div>
    </section>
  </div>
</template>
