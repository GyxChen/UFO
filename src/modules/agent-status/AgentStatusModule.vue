<script setup lang="ts">
import { computed, inject } from 'vue'
import { colorForSession, type SessionSnapshot } from './lib/agent-status'
import { partitionIndicatorSessions } from './lib/agent-status-layout'
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
// 超出容量时留出最后一格显示 +N。
const partitioned = computed(() => partitionIndicatorSessions(store.sessions, props.layoutMode, cellBudget.value))
const gridStyle = computed(() => ({
  gridTemplateColumns: `repeat(${grid.value.cols}, minmax(0, 1fr))`,
  gridTemplateRows: `repeat(${grid.value.rows}, minmax(0, 1fr))`,
}))


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
    <span
      v-if="partitioned.overflow > 0"
      class="agent-status-module__item agent-status-module__item--overflow"
      :aria-label="`还有 ${partitioned.overflow} 个会话`"
      @pointerenter="moduleApi?.reportHover(null)"
      @pointerleave="moduleApi?.reportHover(null)"
    >
      <span class="agent-status-module__overflow">+{{ partitioned.overflow }}</span>
    </span>
  </div>
</template>
