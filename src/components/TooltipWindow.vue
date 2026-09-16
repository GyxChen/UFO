<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow, monitorFromPoint, PhysicalPosition } from '@tauri-apps/api/window'
import {
  colorForSession,
  displayEndpoint,
  formatTokenCount,
  formatTokenMillions,
  tokenTotal,
  type SessionSnapshot,
} from '@/lib/agent-status'
import { placeTooltip, TOOLTIP_HOVER_EVENT, type TooltipHoverPayload } from '@/lib/tooltip'
import {
  formatFlowInterval,
  formatFlowUpdatedAt,
  flowColorClass,
  type FlowField,
} from '@/lib/flow'
import { useAgentStatusStore } from '@/stores/agentStatus'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import { useFlowResultsStore } from '@/stores/flowResults'
import claudeIcon from '@/assets/claude.svg'
import codexIcon from '@/assets/codex.svg'

const window = getCurrentWindow()
const store = useAgentStatusStore()
const modules = useFloatingModulesStore()
const flowResults = useFlowResultsStore()
const hover = ref<TooltipHoverPayload | null>(null)
let unlisten: UnlistenFn | undefined

/** 当前悬停的会话；目标已从快照消失（retention 移除）时为 null。 */
const session = computed<SessionSnapshot | null>(() => {
  const key = hover.value?.sessionKey
  if (!key) return null
  return store.sessions.find((item) => item.key === key) ?? null
})

/**
 * 决定显示内容的会话 key：只有悬停中且会话数据已就绪才非 null。
 * 用 key 而非布尔值做 watch 源，保证从 A 灯移到 B 灯时也会重新定位。
 */
const displayKey = computed(() => (hover.value?.visible && session.value ? session.value.key : null))

const providerLabel = computed(() => (session.value?.provider === 'codex' ? 'Codex' : 'Claude'))
const providerIcon = computed(() => (session.value?.provider === 'codex' ? codexIcon : claudeIcon))
const statusColor = computed(() => (session.value ? colorForSession(session.value) : 'green'))

/**
 * Endpoint 展示值：命中代理解析的上游时优先显示上游，否则回落 base_url。
 */
const endpointDisplay = computed(() =>
  session.value ? displayEndpoint(session.value) : null,
)

/** 解析出上游且与原地址不同时，把原本地地址作为副标注展示（如 127.0.0.1:15721）。 */
const endpointLocalNote = computed(() => {
  const s = session.value
  if (!s?.upstream_url || !s.base_url || s.upstream_url === s.base_url) return null
  return s.base_url
})

/** 该会话累计 token 总量；未统计到（tokens 为 null）时为 0，此时不渲染该行。 */
const tokenCount = computed(() => tokenTotal(session.value?.tokens))

/** token 明细：四项分列在 title 上，避免挤占提示窗的固定高度。 */
const tokenTitle = computed(() => {
  const tokens = session.value?.tokens
  if (!tokens) return ''
  return [
    `输入 ${formatTokenCount(tokens.input_tokens)}`,
    `缓存写 ${formatTokenCount(tokens.cache_creation_input_tokens)}`,
    `缓存读 ${formatTokenCount(tokens.cache_read_input_tokens)}`,
    `输出 ${formatTokenCount(tokens.output_tokens)}`,
  ].join(' · ')
})

/**
 * 按主窗口选定的单位渲染总量：`tokens` 原始计数、`m_tokens` 百万。
 * 明细（`tokenTitle`）始终用原始计数，保证任何单位下都能看到精确值。
 */
const tokenDisplay = computed(() =>
  modules.tokenUnit === 'm_tokens'
    ? formatTokenMillions(tokenCount.value)
    : formatTokenCount(tokenCount.value),
)

/**
 * 该会话的拓展结果（流程脚本返回的字段 + 新鲜度）。
 *
 * 数据按会话 key 索引，由后端 worker 跑脚本后广播（见 `stores/flowResults`）；
 * 无对应 endpoint / 脚本未跑或失败时为 null，整块不渲染、不占位。
 */
const extensionResult = computed(() => flowResults.resultFor(session.value?.key))

/** 拓展字段列表（null 结果当空数组，方便 v-for 与占位判断）。 */
const extensionFields = computed<FlowField[]>(() => extensionResult.value?.fields ?? [])

/**
 * 刷新标注：`每 Ns 刷新 · 更新于 HH:MM:SS`。让用户分辨屏上字段是刚跑的还是旧值
 * （脚本挂掉时 tooltip 直接不显字段，更需要这个时间锚点）。缺元信息时回落空串不渲染。
 */
const extensionStamp = computed(() => {
  const result = extensionResult.value
  if (!result) return ''
  const interval = formatFlowInterval(result.interval_sec)
  const updated = formatFlowUpdatedAt(result.updated_at_ms)
  if (!interval && !updated) return ''
  if (!updated) return `每 ${interval} 刷新`
  if (!interval) return `更新于 ${updated}`
  return `每 ${interval} 刷新 · 更新于 ${updated}`
})

/** 有拓展字段时，提问行压缩为单行，把高度让给字段。 */
const compactPrompt = computed(() => extensionFields.value.length > 0)

/** 定位并显示提示窗；先 setPosition 再 show，避免在旧位置闪一下。 */
async function present(payload: TooltipHoverPayload) {
  const centerX = Math.round(payload.anchor.x + payload.anchor.width / 2)
  const centerY = Math.round(payload.anchor.y + payload.anchor.height / 2)
  const [size, monitor] = await Promise.all([
    window.outerSize(),
    // 以 anchor 中心取屏：多屏下提示窗自身所在屏未必与悬浮窗同屏。
    monitorFromPoint(centerX, centerY),
  ])
  if (!monitor) return

  const { position } = placeTooltip(
    payload.anchor,
    { width: size.width, height: size.height },
    {
      position: { x: monitor.workArea.position.x, y: monitor.workArea.position.y },
      size: { width: monitor.workArea.size.width, height: monitor.workArea.size.height },
    },
    payload.docked,
  )

  await window.setPosition(new PhysicalPosition(position.x, position.y))
  await window.show()
}

async function hide() {
  try {
    await window.hide()
  } catch (error) {
    console.warn('failed to hide tooltip window', error)
  }
}

watch(displayKey, (key) => {
  if (key && hover.value) {
    void present(hover.value).catch((error) => {
      console.warn('failed to present tooltip window', error)
    })
    return
  }
  void hide()
})

onMounted(async () => {
  unlisten = await listen<TooltipHoverPayload>(TOOLTIP_HOVER_EVENT, (event) => {
    hover.value = event.payload
  })
})

onBeforeUnmount(() => {
  unlisten?.()
  unlisten = undefined
})
</script>

<template>
  <div class="tooltip-shell">
    <template v-if="session">
      <div class="tooltip-row tooltip-row--head">
        <span class="tooltip-dot" :class="`tooltip-dot--${statusColor}`" aria-hidden="true" />
        <img :src="providerIcon" :alt="providerLabel" class="tooltip-provider-icon" aria-hidden="true" />
        <span class="tooltip-row__value">{{ providerLabel }}</span>
        <span class="tooltip-row__badge">{{ session.phase }}</span>
      </div>

      <div class="tooltip-row">
        <span class="tooltip-row__label">Endpoint</span>
        <span class="tooltip-row__value tooltip-row__value--mono" :title="endpointLocalNote ? `经本地代理 ${endpointLocalNote}` : (session.base_url ?? '未上报')">
          {{ endpointDisplay ?? '—' }}
        </span>
      </div>

      <!-- token 消耗：一行显示「已使用 token: 总量」（单位由主窗口决定），四项明细放在 title；未统计到时不占位。 -->
      <div v-if="tokenCount > 0" class="tooltip-row">
        <span class="tooltip-row__label">已使用 token:</span>
        <span class="tooltip-row__value tooltip-row__value--mono" :title="tokenTitle">
          {{ tokenDisplay }}
        </span>
      </div>

      <!--
        拓展字段：紧跟 token 行之后（语义同属「用量类」），来自该 endpoint 的流程脚本。
        空则整块不渲染；末尾一行小字标刷新周期与更新时间。
      -->
      <template v-if="extensionFields.length">
        <div
          v-for="(field, index) in extensionFields"
          :key="index"
          class="tooltip-row"
        >
          <span class="tooltip-row__label">{{ field.label }}</span>
          <span
            class="tooltip-row__value tooltip-row__value--mono"
            :class="`tooltip-row__value--${flowColorClass(field.color)}`"
            :title="field.value"
          >
            {{ field.value }}
          </span>
        </div>
        <p v-if="extensionStamp" class="tooltip-stamp">{{ extensionStamp }}</p>
      </template>

      <!-- 用户提问内容：有拓展字段时压到单行，完整内容在 title。 -->
      <div v-if="session.prompt" class="tooltip-prompt" :class="{ 'tooltip-prompt--compact': compactPrompt }">
        <span class="tooltip-row__label">提问</span>
        <p class="tooltip-prompt__text" :title="session.prompt">{{ session.prompt }}</p>
      </div>
    </template>
  </div>
</template>
