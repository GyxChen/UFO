<script setup lang="ts">
import { computed, inject } from 'vue'
import { colorForSession, colorLabel, type SessionSnapshot } from './lib/agent-status'
import {
  overflowHoverKey,
  overflowRingGradient,
  overflowSegments,
  partitionIndicatorSessions,
} from './lib/agent-status-layout'
import { cellsGrid, type FloatingLayout } from '@/lib/floating-layout'
import { floatingModuleKey } from '@/lib/floating-module-context'
import { useAgentStatusStore } from './stores/agentStatus'
import { agentStatusDemand } from './demand'
import claudeIcon from '@/assets/claude.svg'
import codexIcon from '@/assets/codex.svg'

const props = defineProps<{ layoutMode: FloatingLayout }>()
const store = useAgentStatusStore()
// 宿主注入的模块上下文：没有时（单独渲染组件）退化成「独占窗口」，不影响显示。
const moduleApi = inject(floatingModuleKey, null)

/** 本模块在当前形态下要的格子数：与定义里声明的需求同源（见 `demand.ts`）。 */
const demand = computed(() => agentStatusDemand(props.layoutMode))
/**
 * 实际可用格子 = 自身需求与宿主分配取小。
 * 窗口被同级模块挤小时（或配置上限调小），灯会自动减到 `+N` 溢出格。
 */
const cellBudget = computed(() => Math.min(demand.value, moduleApi?.allocatedCells.value ?? demand.value))
const grid = computed(() => cellsGrid(props.layoutMode, cellBudget.value))
// 超出容量时留出最后一格显示 +N（`hidden` = 被折叠的会话，占比环与悬停清单共用同一份）。
const partitioned = computed(() => partitionIndicatorSessions(store.sessions, props.layoutMode, cellBudget.value))
const gridStyle = computed(() => ({
  gridTemplateColumns: `repeat(${grid.value.cols}, minmax(0, 1fr))`,
  gridTemplateRows: `repeat(${grid.value.rows}, minmax(0, 1fr))`,
}))

/**
 * 溢出格的占比环：按折叠会话的灯色占比切开的 `conic-gradient` 字符串
 * （红 → 黄 → 绿，从 12 点顺时针），挂成 CSS 变量给环与两圈光晕共用。
 */
const ringStyle = computed(() => ({
  '--agent-overflow-ring': overflowRingGradient(overflowSegments(partitioned.value.hidden)),
}))

/**
 * 溢出格的合成悬停 key：把当前形态与格子数一起带过去。
 * 提示窗（另一个窗口，不知道悬浮窗的形态与格数）据此用同一个纯函数重算折叠清单，
 * 屏上的环与清单里的行因此必然一致。
 */
const overflowKey = computed(() => overflowHoverKey(props.layoutMode, cellBudget.value))

/** 溢出格的读屏文案：折叠数量 + 按灯色分组的状态（环本身是纯装饰，aria-hidden）。 */
const overflowLabel = computed(() => {
  const parts = overflowSegments(partitioned.value.hidden).map(
    segment => `${segment.count} 个${colorLabel(segment.color)}`
  )
  return `还有 ${partitioned.value.overflow} 个会话：${parts.join('、')}`
})

/** 指示灯的读屏文案：provider + 会话 id 前 8 位 + 阶段。 */
function labelFor(session: SessionSnapshot) {
  const provider = session.provider === 'claude' ? 'Claude' : 'Codex'
  return `${provider} ${session.session_id.slice(0, 8)}: ${session.phase}`
}
</script>

<template>
  <div
    class="agent-status-module"
    :class="`agent-status-module--${props.layoutMode}`"
    :style="gridStyle"
    aria-label="Agent 状态"
  >
    <!--
      指示灯刻意不设 title：title 会触发系统原生文字提示，与悬浮窗自己的 tooltip 重复且更简陋。
      无障碍信息只走 aria-label（不产生可见提示），保证读屏仍能朗读 provider / phase。
    -->
    <span
      v-for="session in partitioned.visible"
      :key="session.key"
      class="agent-status-module__item"
      :class="[`agent-status-module__item--${colorForSession(session)}`, { 'is-flashing': session.is_flashing }]"
      :aria-label="labelFor(session)"
      @pointerenter="moduleApi?.reportHover(session.key)"
      @pointerleave="moduleApi?.reportHover(null)"
    >
      <span class="agent-status-module__dot" aria-hidden="true" />
      <span class="agent-status-module__identity" aria-hidden="true">
        <img
          :src="session.provider === 'claude' ? claudeIcon : codexIcon"
          :alt="session.provider"
          class="agent-status-module__icon"
        />
      </span>
    </span>

    <!--
      溢出格（「分片环」）：外圈按折叠会话的状态占比分色，环外两圈同色模糊光晕做层次，
      中间深色核心托住 +N。悬停上报的是**合成 key** —— 提示窗据此渲染整张折叠清单，
      而不是单个会话（单选一盏灯时仍走上面那套）。
    -->
    <span
      v-if="partitioned.overflow > 0"
      class="agent-status-module__item agent-status-module__item--overflow"
      :style="ringStyle"
      :aria-label="overflowLabel"
      @pointerenter="moduleApi?.reportHover(overflowKey)"
      @pointerleave="moduleApi?.reportHover(null)"
    >
      <span class="agent-status-module__halo agent-status-module__halo--far" aria-hidden="true" />
      <span class="agent-status-module__halo agent-status-module__halo--near" aria-hidden="true" />
      <span class="agent-status-module__ring" aria-hidden="true" />
      <span class="agent-status-module__overflow-core">
        <!-- 三位数的字宽会顶到核心圆边，收窄一档（见样式里的 --wide）。 -->
        <span
          class="agent-status-module__overflow"
          :class="{ 'agent-status-module__overflow--wide': partitioned.overflow >= 10 }"
          >+{{ partitioned.overflow }}</span
        >
      </span>
    </span>
  </div>
</template>
