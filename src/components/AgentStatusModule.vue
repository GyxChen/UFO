<script setup lang="ts">
import { computed, inject, watch } from 'vue'
import { colorForSession, type SessionSnapshot } from '@/lib/agent-status'
import {
  indicatorGrid,
  partitionIndicatorSessions,
  windowCellsForSessions,
} from '@/lib/agent-status-layout'
import type { FloatingLayout } from '@/lib/floating-modules'
import { floatingWindowKey } from '@/lib/floating-window-context'
import { useAgentStatusStore } from '@/stores/agentStatus'
import { useFloatingModulesStore } from '@/stores/floatingModules'
import claudeIcon from '@/assets/claude.svg'
import codexIcon from '@/assets/codex.svg'

const props = defineProps<{ layoutMode: FloatingLayout }>()
const store = useAgentStatusStore()
const modules = useFloatingModulesStore()
const floatingWindow = inject(floatingWindowKey, null)

// {
//   const testSession = new Array(9).fill({
//     key: `claude`,
//     provider: 'claude',
//     session_id: Math.random().toFixed(32),
//     phase:"123",
//     source: 'hook',
//     confidence: 100,
//     started_at_ms: 0,
//     last_seen_at_ms: 0,
//     completed_at_ms: 123,
//     remove_at_ms: 123,
//     is_flashing: 123,
//     pid: null,
//     error_code: null,
//     last_sequence: null,
//     termination_confirmed: 123,
//   }) 
//   const windowCells = computed(() => windowCellsForSessions(testSession.length, layoutMax.value))
//   const partitioned = computed(() =>
//     partitionIndicatorSessions(testSession, props.layoutMode, layoutMax.value, windowCells.value),
//   )
// }
const layoutMax = computed(() => modules.maxCellsFor(props.layoutMode))
const windowCells = computed(() => windowCellsForSessions(store.sessions.length, layoutMax.value))
const grid = computed(() => indicatorGrid(props.layoutMode, windowCells.value))
// 边条按上限切分；方形按网格 side²，溢出时 +N 占满最后一格，避免 3×3 空一格。
const partitioned = computed(() =>
  partitionIndicatorSessions(store.sessions, props.layoutMode, layoutMax.value, windowCells.value),
)
const gridStyle = computed(() => ({
  gridTemplateColumns: `repeat(${grid.value.cols}, minmax(0, 1fr))`,
  gridTemplateRows: `repeat(${grid.value.rows}, minmax(0, 1fr))`,
}))

watch(
  windowCells,
  (cells) => {
    floatingWindow?.setWindowCells(cells)
  },
  { immediate: true },
)

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
    <span
      v-for="session in partitioned.visible"
      :key="session.key"
      class="agent-status-module__item"
      :class="[`agent-status-module__item--${colorForSession(session)}`, { 'is-flashing': session.is_flashing }]"
      :title="labelFor(session)"
      :aria-label="labelFor(session)"
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
      :title="`还有 ${partitioned.overflow} 个会话`"
      :aria-label="`还有 ${partitioned.overflow} 个会话`"
    >
      <span class="agent-status-module__overflow">+{{ partitioned.overflow }}</span>
    </span>
  </div>
</template>
