<script setup lang="ts">
import { computed } from 'vue'
import { emitTo } from '@tauri-apps/api/event'
import {
  colorForSession,
  displayEndpoint,
  formatTokenCount,
  formatTokenMillions,
  tokenTotal,
  type SessionSnapshot,
} from './lib/agent-status'
import type { FloatingHoverTarget } from '@/lib/floating-module-context'
import { TOOLTIP_INTERACTION_EVENT } from '@/lib/tooltip'
import { formatFlowUpdatedAt, flowColorClass, type FlowField } from './lib/flow'
import TooltipRow from '@/components/floating/tooltip/TooltipRow.vue'
import TooltipSection from '@/components/floating/tooltip/TooltipSection.vue'
import { useAgentStatusStore } from './stores/agentStatus'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import { useFlowResultsStore } from './stores/flowResults'
import { agentStatusSettings } from './settings'
import claudeIcon from '@/assets/claude.svg'
import codexIcon from '@/assets/codex.svg'

/**
 * Agent 状态灯的悬停信息卡：一个 CLI 会话的完整画像
 * （provider / phase、Endpoint、token 用量、流程脚本拓展字段、提问）。
 *
 * 提示窗提供容器（毛玻璃外壳、定位、显隐），本组件只渲染内容行；
 * 多根节点渲染，保证提示窗里的 DOM 结构与从前一致（CSS 不用改）。
 */
// 内容根节点不接收透传属性：提示窗只传 target，避免属性落到 fragment 根上报警告。
defineOptions({ inheritAttrs: false })

const props = defineProps<{
  /** 悬停目标；`key` 在本模块里是会话 key。 */
  target: FloatingHoverTarget
}>()

const store = useAgentStatusStore()
const modules = useFloatingModulesStore()
const flowResults = useFlowResultsStore()

/** 当前悬停的会话；目标已从快照消失（retention 移除）时为 null。 */
const session = computed<SessionSnapshot | null>(
  () => store.sessions.find(item => item.key === props.target.key) ?? null
)

const providerLabel = computed(() => (session.value?.provider === 'codex' ? 'Codex' : 'Claude'))
const providerIcon = computed(() => (session.value?.provider === 'codex' ? codexIcon : claudeIcon))
const statusColor = computed(() => (session.value ? colorForSession(session.value) : 'green'))

/** Endpoint 展示值：命中代理解析的上游时优先显示上游，否则回落 base_url。 */
const endpointDisplay = computed(() => (session.value ? displayEndpoint(session.value) : null))

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
 * 按本组件配置里的单位渲染总量：`tokens` 原始计数、`m_tokens` 百万。
 * 明细（`tokenTitle`）始终用原始计数，保证任何单位下都能看到精确值。
 */
const tokenDisplay = computed(() =>
  agentStatusSettings(modules.config).token_unit === 'm_tokens'
    ? formatTokenMillions(tokenCount.value)
    : formatTokenCount(tokenCount.value)
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
 * 刷新标注：`更新于 HH:MM:SS`。让用户分辨屏上字段是刚跑的还是旧值
 * （脚本挂掉时 tooltip 直接不显字段，更需要这个时间锚点）。缺元信息时回落空串不渲染。
 * 按需求不再展示「每 Ns 刷新」——刷新周期对用户是噪音，只留时间锚点。
 */
const extensionStamp = computed(() => {
  const result = extensionResult.value
  if (!result) return ''
  const updated = formatFlowUpdatedAt(result.updated_at_ms)
  return updated ? `更新于 ${updated}` : ''
})

/** 有拓展字段时，限制提问最多显示三行，避免字段与提问互相挤压。 */
const compactPrompt = computed(() => extensionFields.value.length > 0)

/** Removes the currently displayed status light from the runtime snapshot. */
async function dismissSession() {
  const key = session.value?.key
  if (!key) return
  try {
    // 提示窗即将消失，先通知悬浮窗「指针已不在提示窗上」，否则它会继续等指针离开。
    void emitTo('floating', TOOLTIP_INTERACTION_EVENT, false)
    await store.dismissSession(key)
  } catch (error) {
    console.warn('failed to dismiss status session', error)
  }
}
</script>

<template>
  <template v-if="session">
    <div class="tooltip-row tooltip-row--head">
      <span class="tooltip-dot" :class="`tooltip-dot--${statusColor}`" aria-hidden="true" />
      <img :src="providerIcon" :alt="providerLabel" class="tooltip-provider-icon" aria-hidden="true" />
      <span class="tooltip-row__value">{{ providerLabel }}</span>
      <button
        class="tooltip-close"
        type="button"
        aria-label="移除状态灯"
        @pointerdown.stop
        @pointerup.stop
        @click.stop.prevent="void dismissSession()"
      >
        <img src="@/assets/close.svg" alt="" aria-hidden="true" />
      </button>
      <!--
        「正在等你操作」的原因（Codex 的审批理由）：塞进 head 行而不是单开一行 ——
        tooltip 的一屏预算已经排满（见 AGENTS.md），多一行会把下方内容挤出可视区。
        窄了就省略号，完整原文走 title。
      -->
      <span v-if="session.waiting_reason" class="tooltip-reason" :title="session.waiting_reason">{{
        session.waiting_reason
      }}</span>
      <span class="tooltip-row__badge">{{ session.phase }}</span>
    </div>

    <TooltipRow
      label="Endpoint"
      mono
      :value="endpointDisplay ?? '—'"
      :title="endpointLocalNote ? `经本地代理 ${endpointLocalNote}` : (session.base_url ?? '未上报')"
    />

    <!-- token 消耗：一行显示「已使用 token: 总量」（单位由主窗口决定），四项明细放在 title；未统计到时不占位。 -->
    <TooltipRow
      v-if="tokenCount > 0"
      label="已使用 token:"
      mono
      :value="tokenDisplay"
      :title="tokenTitle"
    />

    <!--
      拓展字段分组：紧跟 token 行之后（语义同属「用量类」），来自该 endpoint 的流程脚本。
      独立分组容器 + 顶部强分隔线，与上方基础信息、下方提问块切分；
      空则整块不渲染；末尾一行小字标更新时间。
    -->
    <TooltipSection v-if="extensionFields.length">
      <TooltipRow
        v-for="(field, index) in extensionFields"
        :key="index"
        :label="field.label"
        mono
        :value="field.value"
        :title="field.value"
        :tone="flowColorClass(field.color)"
      />
      <p v-if="extensionStamp" class="tooltip-stamp">{{ extensionStamp }}</p>
    </TooltipSection>

    <!-- 用户提问内容：有拓展字段时最多显示三行，完整内容在 title。 -->
    <div v-if="session.prompt" class="tooltip-prompt" :class="{ 'tooltip-prompt--compact': compactPrompt }">
      <span class="tooltip-row__label">提问</span>
      <p class="tooltip-prompt__text" :title="session.prompt">{{ session.prompt }}</p>
    </div>
  </template>
</template>
